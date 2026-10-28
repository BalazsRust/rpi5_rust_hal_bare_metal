//! GPIO on the RP1 (the 40-pin header), plus the pin-mux used by the other
//! RP1 drivers.
//!
//! Registers: [`pi5_pac::rp1_gpio`] (`pinctrl-rp1.c`). The short version:
//! every pin is touched through three blocks, IO_BANK (which peripheral owns
//! the pad: `CTRL.FUNCSEL`), SYS_RIO (software GPIO: `OUT`/`OE`/`IN`, one bit
//! per pin) and PADS (pull resistors, drive strength, input buffer). To use a
//! pin as software GPIO: configure the pad, set level and direction in
//! SYS_RIO, then write `FUNCSEL = 5` so SYS_RIO actually drives the pad.
//!
//! Writes to `OUT`/`OE` go through RP1's SET/CLR/XOR alias addresses
//! ([`pi5_pac::rp1::SET_OFFSET`] etc.): one store changes only our pin's bit,
//! so `toggle()` is a single instruction and two cores can drive two pins of
//! the same bank without a race.
//!
//! # Header (GPIO -> physical pin)
//!
//! ```text
//!              3V3  (1) (2)  5V
//!      GPIO2  SDA1  (3) (4)  5V
//!      GPIO3  SCL1  (5) (6)  GND
//!      GPIO4        (7) (8)  GPIO14 TXD0
//!              GND  (9) (10) GPIO15 RXD0
//!      GPIO17      (11) (12) GPIO18 PWM0.2
//!      GPIO27      (13) (14) GND
//!      GPIO22      (15) (16) GPIO23
//!              3V3 (17) (18) GPIO24
//!      GPIO10 MOSI (19) (20) GND
//!      GPIO9  MISO (21) (22) GPIO25
//!      GPIO11 SCLK (23) (24) GPIO8  CE0
//!              GND (25) (26) GPIO7  CE1
//!      GPIO0 ID_SD (27) (28) GPIO1  ID_SC
//!      GPIO5       (29) (30) GND
//!      GPIO6       (31) (32) GPIO12 PWM0.0
//!      GPIO13 PWM0.1 (33) (34) GND
//!      GPIO19 PWM0.3 (35) (36) GPIO16
//!      GPIO26      (37) (38) GPIO20
//!              GND (39) (40) GPIO21
//! ```

use crate::asyncutil::wait_until;
use core::convert::Infallible;
use embedded_hal::digital::{ErrorType, InputPin, OutputPin, PinState, StatefulOutputPin};
use pi5_pac::mmio::{modify32, read32, write32};
use pi5_pac::rp1::{CLR_OFFSET, SET_OFFSET, XOR_OFFSET};
use pi5_pac::rp1_gpio as regs;
use pi5_pac::rp1_pins::{self, Function};

// ---------------------------------------------------------------------------
// Value types
// ---------------------------------------------------------------------------

/// Logic level of a pin.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Level {
    /// 0 V.
    Low,
    /// 3.3 V.
    High,
}

impl From<bool> for Level {
    fn from(high: bool) -> Self {
        if high { Level::High } else { Level::Low }
    }
}
impl From<Level> for bool {
    fn from(level: Level) -> bool {
        level == Level::High
    }
}
impl From<PinState> for Level {
    fn from(state: PinState) -> Self {
        match state {
            PinState::Low => Level::Low,
            PinState::High => Level::High,
        }
    }
}
impl core::ops::Not for Level {
    type Output = Level;
    fn not(self) -> Level {
        match self {
            Level::Low => Level::High,
            Level::High => Level::Low,
        }
    }
}

/// Internal pull resistor of a pad (about 50 kOhm).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Pull {
    /// Floating. Only when something external defines the level.
    #[default]
    None,
    /// Towards 3.3 V. A button to GND then reads High released, Low pressed.
    Up,
    /// Towards GND.
    Down,
}

/// Output driver strength (`pinctrl-rp1.c:140-143`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
#[allow(non_camel_case_types)]
pub enum DriveStrength {
    /// 2 mA
    _2mA = 0,
    /// 4 mA
    _4mA = 1,
    /// 8 mA (default; plenty for an LED with a series resistor)
    #[default]
    _8mA = 2,
    /// 12 mA
    _12mA = 3,
}

/// Options for [`Output::new`] (esp-hal style builder).
#[derive(Debug, Clone, Copy, Default)]
pub struct OutputConfig {
    /// Driver strength.
    pub drive_strength: DriveStrength,
    /// Fast slew rate (sharper edges, more ringing). Off by default.
    pub slew_fast: bool,
}

impl OutputConfig {
    /// Set the drive strength.
    pub const fn with_drive_strength(mut self, drive_strength: DriveStrength) -> Self {
        self.drive_strength = drive_strength;
        self
    }
    /// Set the slew rate.
    pub const fn with_slew_fast(mut self, slew_fast: bool) -> Self {
        self.slew_fast = slew_fast;
        self
    }
}

/// Options for [`Input::new`].
#[derive(Debug, Clone, Copy)]
pub struct InputConfig {
    /// Pull resistor.
    pub pull: Pull,
    /// Schmitt trigger (hysteresis) on the input buffer. On by default: a
    /// slowly bouncing button edge becomes one clean transition.
    pub schmitt: bool,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self { pull: Pull::None, schmitt: true }
    }
}

impl InputConfig {
    /// Set the pull resistor.
    pub const fn with_pull(mut self, pull: Pull) -> Self {
        self.pull = pull;
        self
    }
    /// Enable/disable the Schmitt trigger.
    pub const fn with_schmitt(mut self, schmitt: bool) -> Self {
        self.schmitt = schmitt;
        self
    }
}

/// Why a pin could not be given to a peripheral.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PinError {
    /// This pad has no FUNCSEL value for that peripheral (see [`pi5_pac::rp1_pins::FUNCTIONS`]).
    NoSuchFunction {
        /// The GPIO number.
        gpio: u8,
        /// The function that was asked for.
        function: Function,
    },
    /// The pad reaches the peripheral, but as a different signal (e.g. you
    /// passed SCL where SDA was expected).
    WrongRole {
        /// The GPIO number.
        gpio: u8,
    },
    /// This peripheral's pins are fixed in silicon (the BCM2712 debug UART).
    DedicatedPins,
}

impl core::fmt::Display for PinError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            PinError::NoSuchFunction { gpio, function } => write!(f, "GPIO{gpio} cannot be routed to {function:?}"),
            PinError::WrongRole { gpio } => write!(f, "GPIO{gpio} carries a different signal of this peripheral"),
            PinError::DedicatedPins => write!(f, "this peripheral has dedicated pins; do not pass GPIOs"),
        }
    }
}

// ---------------------------------------------------------------------------
// Pin identity
// ---------------------------------------------------------------------------

mod sealed {
    pub trait Sealed {}
}

/// Something that identifies one GPIO: a typed pin like `GPIO21`, or an
/// [`AnyPin`]. Owning such a value is the proof that nobody else uses that
/// pin; that is why [`crate::Peripherals`] hands out zero-sized pin structs
/// instead of plain numbers.
pub trait Pin: sealed::Sealed + Sized {
    /// The GPIO number (0..=53).
    fn number(&self) -> u8;

    /// Forget the compile-time type and keep only the number.
    fn degrade(self) -> AnyPin {
        AnyPin { number: self.number() }
    }
}

/// A GPIO identified at run time by its number.
#[derive(Debug)]
pub struct AnyPin {
    number: u8,
}

impl AnyPin {
    /// Conjure a pin handle out of thin air.
    ///
    /// # Safety
    /// The pin must not be in use anywhere else. `number` must be < 54.
    pub unsafe fn steal(number: u8) -> Self {
        assert!(number < regs::NUM_GPIOS, "RP1 has GPIO0..=GPIO53");
        Self { number }
    }
}

impl sealed::Sealed for AnyPin {}
impl Pin for AnyPin {
    fn number(&self) -> u8 {
        self.number
    }
    fn degrade(self) -> AnyPin {
        self
    }
}

macro_rules! declare_pins {
    ($($name:ident = $num:literal;)*) => {
        $(
            #[doc = concat!("RP1 GPIO", stringify!($num), " (header pin ",
                            stringify!($num), " -> see `pi5_pac::rp1_pins::HEADER_PIN`).")]
            #[derive(Debug)]
            pub struct $name {
                _private: (),
            }
            impl $name {
                pub(crate) const fn new() -> Self {
                    Self { _private: () }
                }
            }
            impl sealed::Sealed for $name {}
            impl Pin for $name {
                #[inline]
                fn number(&self) -> u8 {
                    $num
                }
            }
        )*
    };
}

declare_pins! {
    GPIO0 = 0; GPIO1 = 1; GPIO2 = 2; GPIO3 = 3; GPIO4 = 4; GPIO5 = 5; GPIO6 = 6;
    GPIO7 = 7; GPIO8 = 8; GPIO9 = 9; GPIO10 = 10; GPIO11 = 11; GPIO12 = 12;
    GPIO13 = 13; GPIO14 = 14; GPIO15 = 15; GPIO16 = 16; GPIO17 = 17; GPIO18 = 18;
    GPIO19 = 19; GPIO20 = 20; GPIO21 = 21; GPIO22 = 22; GPIO23 = 23; GPIO24 = 24;
    GPIO25 = 25; GPIO26 = 26; GPIO27 = 27;
}

// ---------------------------------------------------------------------------
// Register plumbing shared with the other RP1 drivers
// ---------------------------------------------------------------------------

/// The addresses that belong to one pin.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PinRegs {
    /// IO_BANK `CTRL` of this pin.
    ctrl: usize,
    /// SYS_RIO base of this pin's bank (`OUT`/`OE`/`IN` relative to it).
    rio: usize,
    /// PADS register of this pin.
    pad: usize,
    /// This pin's bit in the bank-wide SYS_RIO words.
    mask: u32,
}

pub(crate) fn pin_regs(number: u8) -> PinRegs {
    PinRegs {
        ctrl: regs::ctrl_addr(number),
        rio: regs::rio_base(number),
        pad: regs::pad_addr(number),
        mask: regs::rio_mask(number),
    }
}

/// Point the pad at a function. Read-modify-write of CTRL touching only the
/// mux fields: FUNCSEL picks the function, the three *OVER fields are reset
/// to 0 = "follow the peripheral" (a previous owner may have left OEOVER =
/// "force disabled", which would silently ignore our OE bit; Linux's
/// `rp1_set_fsel` does the same, `pinctrl-rp1.c:676-694`).
unsafe fn set_function(r: PinRegs, fsel: u32) {
    // SAFETY: forwarded.
    unsafe {
        modify32(
            r.ctrl,
            regs::CTRL_FUNCSEL_MASK | regs::CTRL_OUTOVER_MASK | regs::CTRL_OEOVER_MASK | regs::CTRL_INOVER_MASK,
            fsel & regs::CTRL_FUNCSEL_MASK,
        );
    }
}

const fn pull_bits(pull: Pull) -> u32 {
    match pull {
        Pull::None => 0,
        Pull::Up => regs::PAD_PULL_UP,
        Pull::Down => regs::PAD_PULL_DOWN,
    }
}

/// Hand a pin to a peripheral: find the FUNCSEL for `function` on this pad,
/// configure the pad (input buffer on, 4 mA, Schmitt, `pull`) and switch the
/// mux. Used by the UART/I2C/SPI/PWM drivers' `with_*` methods.
pub(crate) fn claim_pin(pin: &AnyPin, function: Function, pull: Pull) -> Result<u8, PinError> {
    let gpio = pin.number;
    let fsel = rp1_pins::fsel_for(gpio, function).ok_or(PinError::NoSuchFunction { gpio, function })?;
    let r = pin_regs(gpio);
    // SAFETY: the caller owns `pin`.
    unsafe {
        write32(r.pad, regs::PAD_IN_ENABLE | regs::PAD_SCHMITT | (1 << regs::PAD_DRIVE_SHIFT) | pull_bits(pull));
        set_function(r, u32::from(fsel));
    }
    Ok(fsel)
}

// ---------------------------------------------------------------------------
// Output
// ---------------------------------------------------------------------------

/// A push-pull output pin driven by software through SYS_RIO.
#[derive(Debug)]
pub struct Output {
    pin: AnyPin,
    regs: PinRegs,
}

impl Output {
    /// Configure `pin` as an output driving `initial` from the first moment
    /// it becomes an output (no glitch).
    pub fn new(pin: impl Pin, initial: Level, config: OutputConfig) -> Self {
        let pin = pin.degrade();
        let r = pin_regs(pin.number);

        // SAFETY: we own `pin`, so nothing else touches these registers.
        unsafe {
            // 1. Pad: driver enabled (OD = 0), input buffer on too (harmless,
            //    lets you read the real pad level through RIO_IN), no pull
            //    (pointless on a driven pin), chosen drive strength / slew.
            let mut pad = regs::PAD_IN_ENABLE | (((config.drive_strength as u32) << regs::PAD_DRIVE_SHIFT) & regs::PAD_DRIVE_MASK);
            if config.slew_fast {
                pad |= regs::PAD_SLEWFAST;
            }
            write32(r.pad, pad);
            // 2. Level FIRST, while still an input, so the first thing the pad
            //    drives is what we asked for (Linux: value, dir, fsel).
            let alias = if initial == Level::High { SET_OFFSET } else { CLR_OFFSET };
            write32(r.rio + regs::RIO_OUT + alias, r.mask);
            // 3. Output enable through the SET alias: other pins untouched.
            write32(r.rio + regs::RIO_OE + SET_OFFSET, r.mask);
            // 4. Connect the pad to SYS_RIO.
            set_function(r, regs::FSEL_SYS_RIO);
        }
        Self { pin, regs: r }
    }

    /// Drive the pin high (one store to the SET alias).
    #[inline]
    pub fn set_high(&mut self) {
        // SAFETY: we own the pin.
        unsafe { write32(self.regs.rio + regs::RIO_OUT + SET_OFFSET, self.regs.mask) }
    }

    /// Drive the pin low (one store to the CLR alias).
    #[inline]
    pub fn set_low(&mut self) {
        // SAFETY: we own the pin.
        unsafe { write32(self.regs.rio + regs::RIO_OUT + CLR_OFFSET, self.regs.mask) }
    }

    /// Drive the pin to `level`.
    #[inline]
    pub fn set_level(&mut self, level: Level) {
        match level {
            Level::High => self.set_high(),
            Level::Low => self.set_low(),
        }
    }

    /// Invert the output (one store to the XOR alias).
    #[inline]
    pub fn toggle(&mut self) {
        // SAFETY: we own the pin.
        unsafe { write32(self.regs.rio + regs::RIO_OUT + XOR_OFFSET, self.regs.mask) }
    }

    /// The level we are driving (read back from `OUT`, not from the pad).
    #[inline]
    pub fn output_level(&self) -> Level {
        // SAFETY: reading OUT has no side effects.
        let out = unsafe { read32(self.regs.rio + regs::RIO_OUT) };
        Level::from(out & self.regs.mask != 0)
    }

    /// Is the pin driven high?
    #[inline]
    pub fn is_set_high(&self) -> bool {
        self.output_level() == Level::High
    }

    /// Is the pin driven low?
    #[inline]
    pub fn is_set_low(&self) -> bool {
        self.output_level() == Level::Low
    }

    /// The GPIO number.
    pub fn number(&self) -> u8 {
        self.pin.number
    }

    /// Turn this output into an input.
    pub fn into_input(self, config: InputConfig) -> Input {
        Input::new(self.free(), config)
    }

    /// Stop driving the pin and disconnect the pad (FUNCSEL = none),
    /// returning the bare pin.
    pub fn free(self) -> AnyPin {
        // SAFETY: we own the pin.
        unsafe {
            write32(self.regs.rio + regs::RIO_OE + CLR_OFFSET, self.regs.mask);
            set_function(self.regs, regs::FSEL_NONE);
        }
        self.pin
    }
}

// embedded-hal 1.0. Register stores cannot fail, so the error type is
// `Infallible`, the type with no values.
impl ErrorType for Output {
    type Error = Infallible;
}
impl OutputPin for Output {
    #[inline]
    fn set_low(&mut self) -> Result<(), Infallible> {
        Output::set_low(self);
        Ok(())
    }
    #[inline]
    fn set_high(&mut self) -> Result<(), Infallible> {
        Output::set_high(self);
        Ok(())
    }
}
impl StatefulOutputPin for Output {
    #[inline]
    fn is_set_high(&mut self) -> Result<bool, Infallible> {
        Ok(Output::is_set_high(self))
    }
    #[inline]
    fn is_set_low(&mut self) -> Result<bool, Infallible> {
        Ok(Output::is_set_low(self))
    }
    #[inline]
    fn toggle(&mut self) -> Result<(), Infallible> {
        Output::toggle(self);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Input
// ---------------------------------------------------------------------------

/// An input pin read by software through SYS_RIO's `IN` register.
#[derive(Debug)]
pub struct Input {
    pin: AnyPin,
    regs: PinRegs,
}

impl Input {
    /// Configure `pin` as an input.
    pub fn new(pin: impl Pin, config: InputConfig) -> Self {
        let pin = pin.degrade();
        let r = pin_regs(pin.number);
        // SAFETY: we own `pin`.
        unsafe {
            // 1. Make sure we are not driving: clear OE.
            write32(r.rio + regs::RIO_OE + CLR_OFFSET, r.mask);
            // 2. Pad: IE = 1 is the important one (without the input buffer
            //    RIO_IN reads 0 whatever is on the pin). OD = 1 forces the
            //    driver off as a second line of defence. Then Schmitt + pull.
            let mut pad = regs::PAD_IN_ENABLE | regs::PAD_OUT_DISABLE | pull_bits(config.pull);
            if config.schmitt {
                pad |= regs::PAD_SCHMITT;
            }
            write32(r.pad, pad);
            // 3. Connect the pad to SYS_RIO so IN reflects it.
            set_function(r, regs::FSEL_SYS_RIO);
        }
        Self { pin, regs: r }
    }

    /// Current level on the pin.
    #[inline]
    pub fn level(&self) -> Level {
        Level::from(read_in(self.regs))
    }

    /// Is the pin high?
    #[inline]
    pub fn is_high(&self) -> bool {
        self.level() == Level::High
    }

    /// Is the pin low?
    #[inline]
    pub fn is_low(&self) -> bool {
        self.level() == Level::Low
    }

    /// Change the pull resistor only.
    pub fn set_pull(&mut self, pull: Pull) {
        // SAFETY: we own the pin.
        unsafe { modify32(self.regs.pad, regs::PAD_PULL_MASK, pull_bits(pull)) }
    }

    /// The GPIO number.
    pub fn number(&self) -> u8 {
        self.pin.number
    }

    /// Turn this input into an output.
    pub fn into_output(self, initial: Level, config: OutputConfig) -> Output {
        Output::new(self.free(), initial, config)
    }

    /// Disconnect the pad and return the bare pin.
    pub fn free(self) -> AnyPin {
        // SAFETY: we own the pin.
        unsafe { set_function(self.regs, regs::FSEL_NONE) }
        self.pin
    }

    // --- async (embassy-style). These poll the pin, yielding to other tasks
    // between reads; there is no GPIO interrupt plumbing in this crate. ---

    /// Resolve when the pin is high (immediately if it already is).
    pub async fn wait_for_high(&mut self) {
        let r = self.regs;
        wait_until(move || read_in(r)).await
    }

    /// Resolve when the pin is low.
    pub async fn wait_for_low(&mut self) {
        let r = self.regs;
        wait_until(move || !read_in(r)).await
    }

    /// Resolve after a low-to-high transition.
    pub async fn wait_for_rising_edge(&mut self) {
        self.wait_for_low().await;
        self.wait_for_high().await;
    }

    /// Resolve after a high-to-low transition.
    pub async fn wait_for_falling_edge(&mut self) {
        self.wait_for_high().await;
        self.wait_for_low().await;
    }

    /// Resolve after any transition.
    pub async fn wait_for_any_edge(&mut self) {
        let r = self.regs;
        let start = read_in(r);
        wait_until(move || read_in(r) != start).await
    }
}

#[inline]
/// Current input level of GPIO `number` (true = high), without owning it.
pub(crate) fn input_level(number: u8) -> bool {
    read_in(pin_regs(number))
}

fn read_in(r: PinRegs) -> bool {
    // SAFETY: reading IN has no side effects.
    unsafe { read32(r.rio + regs::RIO_IN) & r.mask != 0 }
}

impl ErrorType for Input {
    type Error = Infallible;
}
impl InputPin for Input {
    #[inline]
    fn is_high(&mut self) -> Result<bool, Infallible> {
        Ok(Input::is_high(self))
    }
    #[inline]
    fn is_low(&mut self) -> Result<bool, Infallible> {
        Ok(Input::is_low(self))
    }
}

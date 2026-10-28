//! PWM on RP1's `PWM0` block: GPIO12 (ch0), GPIO13 (ch1), GPIO18 (ch2),
//! GPIO19 (ch3); also GPIO14/15 for ch2/ch3 if you do not need UART0 there.
//!
//! Registers: [`pi5_pac::rp1_pwm`] (`pwm-rp1.c`). The block's clock is
//! switched to the 50 MHz crystal by [`crate::clocks::enable_pwm0_clock`].
//!
//! ```no_run
//! use pi5_hal::pwm::{ChannelConfig, Pwm};
//! use pi5_hal::time::Rate;
//! use embedded_hal::pwm::SetDutyCycle;
//! # fn demo(p: pi5_hal::Peripherals) -> Result<(), pi5_hal::pwm::Error> {
//! let mut pwm = Pwm::new(p.PWM0);
//! let mut led = pwm.channel(p.GPIO12, ChannelConfig::default().with_frequency(Rate::from_khz(1)))?;
//! led.set_duty_cycle_percent(25)?;
//! # Ok(()) }
//! ```
//!
//! # How a channel works
//!
//! A counter runs from 0 to `RANGE - 1` at the block clock; the output is
//! high while the counter is below `DUTY` ("trailing-edge mark/space"). So
//! `frequency = clock / RANGE` and `duty% = DUTY / RANGE`. New values are
//! latched when `SET_UPDATE` is written to `GLOBAL_CTRL`.
//!
//! Other waveforms are picked with [`ChannelConfig::with_mode`] (see
//! [`Mode`]): centre-aligned, leading-edge, pulse-density, pulse-position.
//! [`PwmChannel::set_phase_ticks`] shifts a channel against the others.
//! For motor control (shared timer, complementary pairs with dead time)
//! use [`crate::mcpwm`] instead.

use crate::clocks;
use crate::gpio::{AnyPin, Pin, Pull, claim_pin};
use crate::sync::SpinLock;
use crate::time::Rate;
use core::fmt;
use embedded_hal::pwm::{ErrorKind, ErrorType, SetDutyCycle};
use pi5_pac::mmio::{read32, write32};
use pi5_pac::rp1_pins::{Function, pwm0_channel};
use pi5_pac::rp1_pwm as r;

/// Ownership token for the PWM0 block; lives in [`crate::Peripherals`].
#[derive(Debug)]
pub struct PWM0 {
    _private: (),
}
impl PWM0 {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }
}

/// Serialises read-modify-writes of `GLOBAL_CTRL`, which all four channels share.
static GLOBAL_LOCK: SpinLock<()> = SpinLock::new(());

/// Errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// This GPIO is not routed to any PWM0 channel.
    PinNotPwm {
        /// The GPIO number.
        gpio: u8,
    },
    /// The channel this GPIO maps to was already taken (two pins share ch2/ch3).
    ChannelInUse {
        /// The channel number.
        channel: u8,
    },
    /// The frequency needs a `RANGE` of 0 or more than 2^32-1 ticks.
    FrequencyOutOfRange,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::PinNotPwm { gpio } => write!(f, "GPIO{gpio} is not a PWM0 output (use 12, 13, 18, 19, 14 or 15)"),
            Error::ChannelInUse { channel } => write!(f, "PWM0 channel {channel} is already in use"),
            Error::FrequencyOutOfRange => write!(f, "PWM frequency out of range"),
        }
    }
}

impl embedded_hal::pwm::Error for Error {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

/// Waveform a channel produces (`CHANx_CTRL.MODE`). In every mode
/// `duty% = DUTY / RANGE`; only where the high time sits differs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mode {
    /// Normal PWM: the pulse starts at the beginning of each period.
    #[default]
    TrailingEdge,
    /// The pulse ends at the end of each period.
    LeadingEdge,
    /// Centre-aligned ("phase-correct"): the pulse sits in the middle of the
    /// period and grows to both sides. The counter goes down and back up, so
    /// a period is 2 x RANGE ticks; the frequency you ask for is still what
    /// you get.
    CenterAligned,
    /// Pulse-density: the high ticks are spread out as evenly as possible
    /// (good for a DAC through an RC filter: much less ripple).
    PulseDensity,
    /// Pulse-position: one clock tick high per period, at tick DUTY.
    PulsePosition,
}

impl Mode {
    pub(crate) const fn ctrl_bits(self) -> u32 {
        match self {
            Mode::TrailingEdge => r::CTRL_MODE_TRAILING_EDGE_MS,
            Mode::LeadingEdge => r::CTRL_MODE_LEADING_EDGE_MS,
            Mode::CenterAligned => r::CTRL_MODE_PHASE_CORRECT,
            Mode::PulseDensity => r::CTRL_MODE_PDM,
            Mode::PulsePosition => r::CTRL_MODE_PPM,
        }
    }

    /// Clock ticks per output period for a given RANGE.
    pub(crate) const fn ticks_per_range(self) -> u32 {
        match self {
            Mode::CenterAligned => 2,
            _ => 1,
        }
    }
}

/// Per-channel settings.
#[derive(Debug, Clone, Copy)]
pub struct ChannelConfig {
    /// Output frequency (default 1 kHz).
    pub frequency: Rate,
    /// Invert the output (high while the counter is *above* DUTY).
    pub inverted: bool,
    /// Waveform (default [`Mode::TrailingEdge`]).
    pub mode: Mode,
}

impl Default for ChannelConfig {
    fn default() -> Self {
        Self { frequency: Rate::from_khz(1), inverted: false, mode: Mode::TrailingEdge }
    }
}

impl ChannelConfig {
    /// Set the frequency.
    pub const fn with_frequency(mut self, frequency: Rate) -> Self {
        self.frequency = frequency;
        self
    }
    /// Set polarity.
    pub const fn with_inverted(mut self, inverted: bool) -> Self {
        self.inverted = inverted;
        self
    }
    /// Set the waveform.
    pub const fn with_mode(mut self, mode: Mode) -> Self {
        self.mode = mode;
        self
    }
}

/// The PWM0 block. Hands out channels.
#[derive(Debug)]
pub struct Pwm {
    base: usize,
    clock_hz: u32,
    taken: u8,
}

impl Pwm {
    /// Take the block and turn on its clock (50 MHz from the crystal).
    pub fn new(_pwm: PWM0) -> Self {
        let clock_hz = clocks::enable_pwm0_clock();
        Self { base: pi5_pac::rp1::PWM0_BASE, clock_hz, taken: 0 }
    }

    /// Block clock in Hz.
    pub fn clock_hz(&self) -> u32 {
        self.clock_hz
    }

    /// Route `pin` to its PWM0 channel and start it at 0 % duty.
    pub fn channel(&mut self, pin: impl Pin, config: ChannelConfig) -> Result<PwmChannel, Error> {
        let pin: AnyPin = pin.degrade();
        let gpio = pin.number();
        let channel = pwm0_channel(gpio).ok_or(Error::PinNotPwm { gpio })?;
        if self.taken & (1 << channel) != 0 {
            return Err(Error::ChannelInUse { channel });
        }
        let range = range_for(self.clock_hz, config.frequency, config.mode)?;

        // Program the channel before connecting the pad, so the pad never
        // shows a half-configured waveform.
        let mut ch = PwmChannel {
            base: self.base,
            channel,
            clock_hz: self.clock_hz,
            mode: config.mode,
            range,
            duty: 0,
            enabled: false,
        };
        // SAFETY: we own the block and this channel is unused.
        unsafe {
            let mut ctrl = r::CTRL_FIFO_POP_MASK | config.mode.ctrl_bits();
            if config.inverted {
                ctrl |= r::CTRL_POLARITY_INVERTED;
            }
            write32(self.base + r::channel_ctrl(channel), ctrl);
            write32(self.base + r::range(channel), range);
            write32(self.base + r::phase(channel), 0);
            write32(self.base + r::duty(channel), 0);
        }
        claim_pin(&pin, Function::Pwm0, Pull::None).map_err(|_| Error::PinNotPwm { gpio })?;
        ch.enable();
        self.taken |= 1 << channel;
        Ok(ch)
    }
}

/// RANGE for `frequency` in `mode` (centre-aligned needs half as much).
pub(crate) fn range_for(clock_hz: u32, frequency: Rate, mode: Mode) -> Result<u32, Error> {
    let hz = u64::from(frequency.as_hz()) * u64::from(mode.ticks_per_range());
    if hz == 0 {
        return Err(Error::FrequencyOutOfRange);
    }
    let range = (u64::from(clock_hz) + hz / 2) / hz;
    if range < 2 || range > u64::from(u32::MAX) {
        return Err(Error::FrequencyOutOfRange);
    }
    Ok(range as u32)
}

/// One PWM output.
#[derive(Debug)]
pub struct PwmChannel {
    base: usize,
    channel: u8,
    clock_hz: u32,
    mode: Mode,
    range: u32,
    duty: u32,
    enabled: bool,
}

impl PwmChannel {
    /// Channel number 0..=3.
    pub fn channel(&self) -> u8 {
        self.channel
    }

    /// Period in clock ticks (`RANGE`).
    pub fn range_ticks(&self) -> u32 {
        self.range
    }

    /// Current high time in clock ticks (`DUTY`).
    pub fn duty_ticks(&self) -> u32 {
        self.duty
    }

    /// Actual output frequency.
    pub fn frequency(&self) -> Rate {
        Rate::from_hz(self.clock_hz / (self.range.max(1) * self.mode.ticks_per_range()))
    }

    /// The waveform this channel was set up with.
    pub fn mode(&self) -> Mode {
        self.mode
    }

    /// Shift this channel's waveform: its counter starts `ticks` further on
    /// (clamped to RANGE). Two channels with the same frequency and phases
    /// 0 and RANGE/2 are half a period apart. Applied with a restart of the
    /// counter at the next settings update.
    pub fn set_phase_ticks(&mut self, ticks: u32) {
        // SAFETY: we own the channel.
        unsafe { write32(self.base + r::phase(self.channel), ticks.min(self.range)) };
        self.latch();
    }

    /// Change the frequency, keeping the duty *fraction*.
    pub fn set_frequency(&mut self, frequency: Rate) -> Result<(), Error> {
        let range = range_for(self.clock_hz, frequency, self.mode)?;
        let duty = ((u64::from(self.duty) * u64::from(range)) / u64::from(self.range.max(1))) as u32;
        self.range = range;
        self.duty = duty;
        // SAFETY: we own the channel.
        unsafe {
            write32(self.base + r::range(self.channel), range);
            write32(self.base + r::duty(self.channel), duty);
        }
        self.latch();
        Ok(())
    }

    /// Set the high time in clock ticks (clamped to `RANGE`).
    pub fn set_duty_ticks(&mut self, ticks: u32) {
        self.duty = ticks.min(self.range);
        // SAFETY: we own the channel.
        unsafe { write32(self.base + r::duty(self.channel), self.duty) };
        self.latch();
    }

    /// Start the output.
    pub fn enable(&mut self) {
        self.set_enabled(true);
    }

    /// Stop the output (pad goes to the inactive level).
    pub fn disable(&mut self) {
        self.set_enabled(false);
    }

    /// Is the output running?
    pub fn is_enabled(&self) -> bool {
        self.enabled
    }

    fn set_enabled(&mut self, on: bool) {
        self.enabled = on;
        let _guard = GLOBAL_LOCK.lock();
        // SAFETY: GLOBAL_CTRL RMW serialised by the lock.
        unsafe {
            let mut v = read32(self.base + r::GLOBAL_CTRL);
            if on {
                v |= r::global_ctrl_enable(self.channel);
            } else {
                v &= !r::global_ctrl_enable(self.channel);
            }
            write32(self.base + r::GLOBAL_CTRL, v | r::GLOBAL_CTRL_SET_UPDATE);
        }
    }

    /// Apply pending RANGE/DUTY/CTRL changes (`SET_UPDATE`).
    fn latch(&self) {
        let _guard = GLOBAL_LOCK.lock();
        // SAFETY: GLOBAL_CTRL RMW serialised by the lock.
        unsafe {
            let v = read32(self.base + r::GLOBAL_CTRL);
            write32(self.base + r::GLOBAL_CTRL, v | r::GLOBAL_CTRL_SET_UPDATE);
        }
    }
}

impl ErrorType for PwmChannel {
    type Error = Error;
}

/// `embedded-hal` duty cycle control. The trait works in `0..=max_duty_cycle()`
/// units; we expose 16-bit resolution and scale to `RANGE` internally.
impl SetDutyCycle for PwmChannel {
    fn max_duty_cycle(&self) -> u16 {
        u16::MAX
    }

    fn set_duty_cycle(&mut self, duty: u16) -> Result<(), Error> {
        let ticks = ((u64::from(duty) * u64::from(self.range)) / u64::from(u16::MAX)) as u32;
        self.set_duty_ticks(ticks);
        Ok(())
    }
}

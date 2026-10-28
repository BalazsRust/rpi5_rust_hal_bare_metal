//! Motor-control PWM in the style of the ESP32's MCPWM, built on RP1 `PWM0`.
//!
//! What it adds over [`crate::pwm`]:
//!
//! - **One shared timer.** Every output runs at the same frequency and all
//!   counters are started in the same clock tick ([`McPwm::start`]), so the
//!   edges of different outputs line up exactly.
//! - **Centre-aligned ("up-down") counting** ([`CountMode::UpDown`]), the
//!   usual choice for motors: the pulse grows from the middle of the period.
//! - **Complementary pairs with dead time** ([`McPwm::complementary`]): a
//!   high-side and a low-side output that are never on together, with a gap
//!   of a few hundred ns between one turning off and the other turning on.
//!   That is what an H-bridge or half-bridge driver needs (otherwise both
//!   transistors conduct for a moment and short the supply).
//!
//! ```no_run
//! use pi5_hal::mcpwm::{CountMode, McPwm, OutputConfig, TimerConfig};
//! use pi5_hal::time::Rate;
//! # fn demo(p: pi5_hal::Peripherals) -> Result<(), pi5_hal::mcpwm::Error> {
//! let mut mcpwm = McPwm::new(p.PWM0, TimerConfig::new(Rate::from_khz(20)).with_mode(CountMode::UpDown))?;
//! // Half-bridge: high side on GPIO12, low side on GPIO13, 500 ns dead time.
//! let mut bridge = mcpwm.complementary(p.GPIO12, p.GPIO13, 500)?;
//! let mut fan = mcpwm.output(p.GPIO18, OutputConfig::default())?;
//! mcpwm.start();
//! bridge.set_duty_percent(30);
//! fan.set_duty_percent(75);
//! # Ok(()) }
//! ```
//!
//! # Outputs
//!
//! Only PWM0 is on the 40-pin header, so there are four outputs:
//!
//! | channel | GPIO   | header         |
//! |---------|--------|----------------|
//! | 0       | GPIO12 | physical pin 32 |
//! | 1       | GPIO13 | physical pin 33 |
//! | 2       | GPIO18 | physical pin 12 |
//! | 3       | GPIO19 | physical pin 35 |
//!
//! (GPIO14/15 also reach channels 2/3, but they are UART0 to the ESP32 and
//! the reset watch, so leave them alone.) Two complementary pairs, or one
//! pair plus two single outputs, or four single outputs.
//!
//! # Differences from the ESP32
//!
//! RP1's PWM has no fault inputs, no capture unit and no hardware dead-time
//! generator. Dead time is made by giving the low side a slightly larger
//! compare value with the output inverted; in up-down counting that leaves
//! the same gap before and after each pulse. It therefore needs
//! [`CountMode::UpDown`]: in [`CountMode::Up`] the counter jumps from the
//! end back to 0 and one of the two gaps would be missing.
//!
//! # How it maps to the hardware
//!
//! All used channels get the same `RANGE`. In up-down mode the counter goes
//! `RANGE -> 0 -> RANGE` (2 x RANGE ticks per period) and an output is high
//! while the count is below its `DUTY`. Inverting an output makes it high
//! while the count is at or above `DUTY`.

use crate::clocks;
use crate::gpio::{AnyPin, Pin, Pull, claim_pin};
use crate::pwm::{Mode, PWM0, range_for};
use crate::sync::SpinLock;
use crate::time::Rate;
use core::fmt;
use embedded_hal::pwm::{ErrorKind, ErrorType, SetDutyCycle};
use pi5_pac::mmio::{read32, write32};
use pi5_pac::rp1_pins::{Function, pwm0_channel};
use pi5_pac::rp1_pwm as r;

const BASE: usize = pi5_pac::rp1::PWM0_BASE;

/// Errors.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// This GPIO is not a PWM0 output.
    PinNotPwm {
        /// The GPIO number.
        gpio: u8,
    },
    /// The channel this GPIO maps to is already used (GPIO12/13/18/19 are
    /// channels 0/1/2/3; GPIO14/15 share channels 2/3).
    ChannelInUse {
        /// The channel number.
        channel: u8,
    },
    /// The frequency is 0 or too high for the 50 MHz clock.
    FrequencyOutOfRange,
    /// Dead time needs [`CountMode::UpDown`].
    DeadTimeNeedsUpDown,
    /// The dead time is longer than half the period.
    DeadTimeTooLong,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::PinNotPwm { gpio } => write!(f, "GPIO{gpio} is not a PWM0 output (use 12, 13, 18 or 19)"),
            Error::ChannelInUse { channel } => write!(f, "PWM0 channel {channel} is already in use"),
            Error::FrequencyOutOfRange => write!(f, "MCPWM frequency out of range"),
            Error::DeadTimeNeedsUpDown => write!(f, "dead time needs CountMode::UpDown"),
            Error::DeadTimeTooLong => write!(f, "dead time is longer than half the PWM period"),
        }
    }
}

impl embedded_hal::pwm::Error for Error {
    fn kind(&self) -> ErrorKind {
        ErrorKind::Other
    }
}

/// How the shared timer counts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CountMode {
    /// Count up and wrap (edge-aligned, like normal PWM). ESP32: `Increase`.
    Up,
    /// Count down and back up (centre-aligned). ESP32: `UpDown`. Needed for
    /// dead time; the default because it is what motor drivers want.
    #[default]
    UpDown,
}

impl CountMode {
    const fn pwm_mode(self) -> Mode {
        match self {
            CountMode::Up => Mode::TrailingEdge,
            CountMode::UpDown => Mode::CenterAligned,
        }
    }
}

/// Settings of the shared timer.
#[derive(Debug, Clone, Copy)]
pub struct TimerConfig {
    /// PWM frequency of every output (e.g. 20 kHz for motors: above hearing).
    pub frequency: Rate,
    /// Counting mode.
    pub mode: CountMode,
}

impl TimerConfig {
    /// `frequency`, up-down counting.
    pub const fn new(frequency: Rate) -> Self {
        Self { frequency, mode: CountMode::UpDown }
    }
    /// Set the counting mode.
    pub const fn with_mode(mut self, mode: CountMode) -> Self {
        self.mode = mode;
        self
    }
}

/// Settings of a single output.
#[derive(Debug, Clone, Copy, Default)]
pub struct OutputConfig {
    /// Active low: the pin is LOW for the duty part of the period.
    pub inverted: bool,
    /// Phase offset in timer ticks: preloads this output's counter when
    /// [`McPwm::start`] runs, shifting it against the others. Clamped to
    /// [`McPwm::period_ticks`].
    pub phase_ticks: u32,
}

impl OutputConfig {
    /// Active-low output.
    pub const fn with_inverted(mut self, inverted: bool) -> Self {
        self.inverted = inverted;
        self
    }
    /// Phase offset in timer ticks.
    pub const fn with_phase_ticks(mut self, ticks: u32) -> Self {
        self.phase_ticks = ticks;
        self
    }
}

/// Shared between [`McPwm`] and its outputs (PWM0 exists once, so a static
/// is the natural home).
struct State {
    range: u32,
    used: u8,
    /// Duty of each channel in ticks, 0..=range (range = always on).
    duty: [u32; 4],
    /// For the high side of a pair: (low-side channel, dead time in ticks).
    pair: [Option<(u8, u32)>; 4],
}

static STATE: SpinLock<State> = SpinLock::new(State { range: 0, used: 0, duty: [0; 4], pair: [None; 4] });

/// Write one channel's DUTY register (and its partner's if it is a pair).
fn write_duty(s: &State, channel: u8) {
    let d = s.duty[channel as usize];
    // 100 % must stay high even at the counter's top value: compare with
    // something it never reaches.
    let hw = if d >= s.range { u32::MAX } else { d };
    // SAFETY: the channel belongs to this driver; the lock serialises us.
    unsafe { write32(BASE + r::duty(channel), hw) };
    if let Some((low, dead)) = s.pair[channel as usize] {
        // Low side: inverted, high while count >= d + dead.
        // SAFETY: as above.
        unsafe { write32(BASE + r::duty(low), d.saturating_add(dead)) };
    }
}

fn set_update() {
    // SAFETY: GLOBAL_CTRL belongs to this driver; callers hold STATE.
    unsafe {
        let v = read32(BASE + r::GLOBAL_CTRL);
        write32(BASE + r::GLOBAL_CTRL, v | r::GLOBAL_CTRL_SET_UPDATE);
    }
}

/// The MCPWM driver: a shared timer that hands out outputs.
#[derive(Debug)]
pub struct McPwm {
    _pwm: PWM0,
    clock_hz: u32,
    mode: CountMode,
}

impl McPwm {
    /// Take PWM0 and set up the shared timer. Outputs stay off until
    /// [`McPwm::start`].
    pub fn new(pwm: PWM0, config: TimerConfig) -> Result<Self, Error> {
        let clock_hz = clocks::enable_pwm0_clock();
        let range = range_for(clock_hz, config.frequency, config.mode.pwm_mode()).map_err(|_| Error::FrequencyOutOfRange)?;
        let mut s = STATE.lock();
        *s = State { range, used: 0, duty: [0; 4], pair: [None; 4] };
        // SAFETY: we own PWM0: stop every channel.
        unsafe { write32(BASE + r::GLOBAL_CTRL, r::GLOBAL_CTRL_SET_UPDATE) };
        Ok(Self { _pwm: pwm, clock_hz, mode: config.mode })
    }

    /// Timer clock in Hz (50 MHz).
    pub fn clock_hz(&self) -> u32 {
        self.clock_hz
    }

    /// Ticks per period that duty values are measured in (`RANGE`). A duty
    /// of `period_ticks()` is 100 %.
    pub fn period_ticks(&self) -> u32 {
        STATE.lock().range
    }

    /// Actual PWM frequency.
    pub fn frequency(&self) -> Rate {
        let per_range = self.mode.pwm_mode().ticks_per_range();
        Rate::from_hz(self.clock_hz / (STATE.lock().range.max(1) * per_range))
    }

    /// Convert nanoseconds to timer ticks (20 ns each at 50 MHz), rounded.
    pub fn ns_to_ticks(&self, ns: u32) -> u32 {
        ((u64::from(ns) * u64::from(self.clock_hz) + 500_000_000) / 1_000_000_000) as u32
    }

    fn claim(&mut self, pin: AnyPin) -> Result<(AnyPin, u8), Error> {
        let gpio = pin.number();
        let channel = pwm0_channel(gpio).ok_or(Error::PinNotPwm { gpio })?;
        if STATE.lock().used & (1 << channel) != 0 {
            return Err(Error::ChannelInUse { channel });
        }
        Ok((pin, channel))
    }

    /// Program a channel (still disabled), then connect its pad.
    fn setup(&mut self, pin: &AnyPin, channel: u8, inverted: bool, phase: u32) -> Result<(), Error> {
        let mut s = STATE.lock();
        let mut ctrl = r::CTRL_FIFO_POP_MASK | self.mode.pwm_mode().ctrl_bits();
        if inverted {
            ctrl |= r::CTRL_POLARITY_INVERTED;
        }
        // SAFETY: we own PWM0 and the channel is unused.
        unsafe {
            write32(BASE + r::channel_ctrl(channel), ctrl);
            write32(BASE + r::range(channel), s.range);
            write32(BASE + r::phase(channel), phase.min(s.range));
        }
        s.duty[channel as usize] = 0;
        s.pair[channel as usize] = None;
        write_duty(&s, channel);
        set_update();
        claim_pin(pin, Function::Pwm0, Pull::None).map_err(|_| Error::PinNotPwm { gpio: pin.number() })?;
        s.used |= 1 << channel;
        Ok(())
    }

    /// One PWM output on `pin` (GPIO12, 13, 18 or 19), at 0 % duty.
    pub fn output(&mut self, pin: impl Pin, config: OutputConfig) -> Result<McPwmOutput, Error> {
        let (pin, channel) = self.claim(pin.degrade())?;
        self.setup(&pin, channel, config.inverted, config.phase_ticks)?;
        Ok(McPwmOutput { channel })
    }

    /// A complementary pair: `high` is on for the duty, `low` is on for the
    /// rest, and neither comes on until the other has been off for
    /// `dead_time_ns` (rounded to 20 ns). Starts at 0 % (low side on).
    ///
    /// Needs [`CountMode::UpDown`].
    pub fn complementary(&mut self, high: impl Pin, low: impl Pin, dead_time_ns: u32) -> Result<Complementary, Error> {
        if self.mode != CountMode::UpDown {
            return Err(Error::DeadTimeNeedsUpDown);
        }
        let dead = self.ns_to_ticks(dead_time_ns);
        if dead > self.period_ticks() / 2 {
            return Err(Error::DeadTimeTooLong);
        }
        let (high, hi_ch) = self.claim(high.degrade())?;
        let (low, lo_ch) = self.claim(low.degrade())?;
        if hi_ch == lo_ch {
            return Err(Error::ChannelInUse { channel: lo_ch });
        }
        self.setup(&low, lo_ch, true, 0)?;
        self.setup(&high, hi_ch, false, 0)?;
        let mut s = STATE.lock();
        s.pair[hi_ch as usize] = Some((lo_ch, dead));
        write_duty(&s, hi_ch);
        set_update();
        Ok(Complementary { high: hi_ch, low: lo_ch, dead })
    }

    /// Start (or restart) every output taken so far, all in the same clock
    /// tick, so their timing lines up. Call it after creating the outputs;
    /// an output created later stays off until `start` runs again.
    pub fn start(&mut self) {
        let s = STATE.lock();
        let mut enable = 0;
        for ch in 0..4 {
            if s.used & (1 << ch) != 0 {
                enable |= r::global_ctrl_enable(ch);
            }
        }
        // SAFETY: we own PWM0. Stop all, then start all in one write: the
        // counters are preloaded (with PHASE) when a channel is enabled.
        unsafe {
            write32(BASE + r::GLOBAL_CTRL, r::GLOBAL_CTRL_SET_UPDATE);
            write32(BASE + r::GLOBAL_CTRL, enable | r::GLOBAL_CTRL_SET_UPDATE);
        }
    }

    /// Stop every output (pads go to their inactive level: LOW, or HIGH for
    /// inverted outputs and the low side of a pair).
    pub fn stop(&mut self) {
        let _s = STATE.lock();
        // SAFETY: we own PWM0.
        unsafe { write32(BASE + r::GLOBAL_CTRL, r::GLOBAL_CTRL_SET_UPDATE) };
    }

    /// Change the frequency of all outputs, keeping their duty *fractions*
    /// and dead times. Takes effect at the end of the current period.
    pub fn set_frequency(&mut self, frequency: Rate) -> Result<(), Error> {
        let range = range_for(self.clock_hz, frequency, self.mode.pwm_mode()).map_err(|_| Error::FrequencyOutOfRange)?;
        let mut s = STATE.lock();
        if s.pair.iter().flatten().any(|&(_, dead)| dead > range / 2) {
            return Err(Error::DeadTimeTooLong);
        }
        let old = s.range.max(1);
        for ch in 0..4u8 {
            if s.used & (1 << ch) != 0 {
                let i = ch as usize;
                s.duty[i] = ((u64::from(s.duty[i]) * u64::from(range)) / u64::from(old)) as u32;
            }
        }
        s.range = range;
        for ch in 0..4u8 {
            if s.used & (1 << ch) != 0 {
                // SAFETY: we own PWM0.
                unsafe { write32(BASE + r::range(ch), range) };
                write_duty(&s, ch);
            }
        }
        Ok(())
    }
}

fn set_duty(channel: u8, ticks: u32) {
    let mut s = STATE.lock();
    s.duty[channel as usize] = ticks.min(s.range);
    write_duty(&s, channel);
}

fn duty_from_percent(percent: u8) -> u32 {
    let range = STATE.lock().range;
    ((u64::from(percent.min(100)) * u64::from(range)) / 100) as u32
}

fn duty_from_u16(duty: u16) -> u32 {
    let range = STATE.lock().range;
    ((u64::from(duty) * u64::from(range)) / u64::from(u16::MAX)) as u32
}

/// A single output of the shared timer.
#[derive(Debug)]
pub struct McPwmOutput {
    channel: u8,
}

impl McPwmOutput {
    /// PWM0 channel number.
    pub fn channel(&self) -> u8 {
        self.channel
    }
    /// Duty in timer ticks, `0..=period_ticks()` (clamped).
    pub fn set_duty_ticks(&mut self, ticks: u32) {
        set_duty(self.channel, ticks);
    }
    /// Duty in percent (0..=100).
    pub fn set_duty_percent(&mut self, percent: u8) {
        set_duty(self.channel, duty_from_percent(percent));
    }
    /// Current duty in ticks.
    pub fn duty_ticks(&self) -> u32 {
        STATE.lock().duty[self.channel as usize]
    }
}

impl ErrorType for McPwmOutput {
    type Error = Error;
}

impl SetDutyCycle for McPwmOutput {
    fn max_duty_cycle(&self) -> u16 {
        u16::MAX
    }
    fn set_duty_cycle(&mut self, duty: u16) -> Result<(), Error> {
        set_duty(self.channel, duty_from_u16(duty));
        Ok(())
    }
}

/// A high-side / low-side output pair with dead time (ESP32: linked pins
/// with a dead-time generator). The duty is the high side's on time; the
/// low side is on for the rest minus the dead time on both sides.
#[derive(Debug)]
pub struct Complementary {
    high: u8,
    low: u8,
    dead: u32,
}

impl Complementary {
    /// PWM0 channels (high side, low side).
    pub fn channels(&self) -> (u8, u8) {
        (self.high, self.low)
    }
    /// Dead time in timer ticks.
    pub fn dead_time_ticks(&self) -> u32 {
        self.dead
    }
    /// High-side on time in timer ticks, `0..=period_ticks()` (clamped).
    pub fn set_duty_ticks(&mut self, ticks: u32) {
        set_duty(self.high, ticks);
    }
    /// High-side duty in percent (0..=100).
    pub fn set_duty_percent(&mut self, percent: u8) {
        set_duty(self.high, duty_from_percent(percent));
    }
    /// Current high-side duty in ticks.
    pub fn duty_ticks(&self) -> u32 {
        STATE.lock().duty[self.high as usize]
    }
}

impl ErrorType for Complementary {
    type Error = Error;
}

impl SetDutyCycle for Complementary {
    fn max_duty_cycle(&self) -> u16 {
        u16::MAX
    }
    fn set_duty_cycle(&mut self, duty: u16) -> Result<(), Error> {
        set_duty(self.high, duty_from_u16(duty));
        Ok(())
    }
}

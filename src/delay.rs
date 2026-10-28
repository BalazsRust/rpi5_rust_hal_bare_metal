//! Busy-wait delays based on the ARM Generic Timer.
//!
//! Every ARMv8 core has a built-in 64-bit counter ticking at a fixed
//! frequency, independent of the CPU clock. Two system registers:
//!
//! * `CNTFRQ_EL0` - the counter frequency in Hz, programmed by the firmware
//!   (54 MHz expected on the BCM2712, `pi5_pac::bcm2712::EXPECTED_CNTFRQ_HZ`).
//!   We read it rather than assume it.
//! * `CNTVCT_EL0` - the *virtual* count = physical count minus `CNTVOFF_EL2`,
//!   which the runtime sets to 0 on every core, so all cores read the same
//!   system-wide counter. (The virtual counter is used because it is readable
//!   from EL1 unconditionally; at EL2 both are.)
//!
//! A delay is: remember the count now, spin until `now - start >= ticks`.
//! `wrapping_sub` keeps that correct across a counter wrap (10,000 years at
//! 54 MHz, but it is the right habit).

use core::arch::asm;
use embedded_hal::delay::DelayNs;

/// A blocking delay provider. Zero-sized and `Copy`.
#[derive(Debug, Clone, Copy, Default)]
pub struct Delay {
    _private: (),
}

impl Delay {
    /// Create a delay provider. Needs no peripheral: the timer is in the core.
    pub const fn new() -> Self {
        Self { _private: () }
    }

    /// Counter frequency in Hz (`CNTFRQ_EL0`).
    #[inline]
    pub fn frequency() -> u64 {
        let f: u64;
        // SAFETY: side-effect free read.
        unsafe { asm!("mrs {}, cntfrq_el0", out(reg) f, options(nomem, nostack, preserves_flags)) };
        f
    }

    /// Current counter value (`CNTVCT_EL0`).
    ///
    /// The `isb` matters: the CPU may execute an `mrs` of the counter
    /// speculatively, earlier than program order suggests. The barrier makes
    /// every earlier instruction finish first, so the timestamp really is
    /// "now" relative to the code before it.
    #[inline]
    pub fn now() -> u64 {
        let t: u64;
        // SAFETY: no memory effects. `nomem` deliberately omitted so the
        // compiler keeps register writes before this call ordered before it.
        unsafe { asm!("isb", "mrs {}, cntvct_el0", out(reg) t, options(nostack, preserves_flags)) };
        t
    }

    /// Microseconds since the counter started (roughly: since power-on).
    pub fn uptime_us() -> u64 {
        Self::now() / (Self::frequency() / 1_000_000).max(1)
    }

    /// Spin for `ticks` counter ticks.
    #[inline]
    pub fn delay_ticks(ticks: u64) {
        let start = Self::now();
        while Self::now().wrapping_sub(start) < ticks {
            core::hint::spin_loop();
        }
    }

    /// Spin for `ns` nanoseconds (rounded up to whole ticks).
    pub fn delay_nanos(&self, ns: u32) {
        // ticks = ns * f / 1e9 in u64: 4.29e9 * 54e6 = 2.3e17 fits.
        Self::delay_ticks((u64::from(ns) * Self::frequency()).div_ceil(1_000_000_000));
    }

    /// Spin for `us` microseconds.
    pub fn delay_micros(&self, us: u32) {
        Self::delay_ticks((u64::from(us) * Self::frequency()).div_ceil(1_000_000));
    }

    /// Spin for `ms` milliseconds.
    pub fn delay_millis(&self, ms: u32) {
        Self::delay_ticks((u64::from(ms) * Self::frequency()).div_ceil(1_000));
    }
}

/// `embedded-hal` 1.0 delay trait, so driver crates can `delay.delay_ms(10)`.
impl DelayNs for Delay {
    #[inline]
    fn delay_ns(&mut self, ns: u32) {
        Delay::delay_nanos(self, ns);
    }
    #[inline]
    fn delay_us(&mut self, us: u32) {
        Delay::delay_micros(self, us);
    }
    #[inline]
    fn delay_ms(&mut self, ms: u32) {
        Delay::delay_millis(self, ms);
    }
}

/// A deadline for bounded waits inside drivers ("give up after 100 ms").
#[derive(Debug, Clone, Copy)]
pub struct Timeout {
    start: u64,
    ticks: u64,
}

impl Timeout {
    /// Start counting now.
    pub fn new(duration: core::time::Duration) -> Self {
        let ticks = (duration.as_nanos() as u64).saturating_mul(Delay::frequency()) / 1_000_000_000;
        Self { start: Delay::now(), ticks }
    }

    /// Has the time run out?
    #[inline]
    pub fn expired(&self) -> bool {
        Delay::now().wrapping_sub(self.start) >= self.ticks
    }
}

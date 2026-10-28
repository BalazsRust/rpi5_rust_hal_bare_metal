//! Frequency type for driver configuration (`esp_hal::time::Rate` style).
//!
//! Durations for timeouts use `core::time::Duration`; the async layer's
//! `pi5_time::{Duration, Instant}` are a different, tick-based pair.

/// A frequency in Hz.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Rate {
    hz: u32,
}

impl Rate {
    /// From hertz.
    pub const fn from_hz(hz: u32) -> Self {
        Self { hz }
    }
    /// From kilohertz.
    pub const fn from_khz(khz: u32) -> Self {
        Self { hz: khz * 1_000 }
    }
    /// From megahertz.
    pub const fn from_mhz(mhz: u32) -> Self {
        Self { hz: mhz * 1_000_000 }
    }
    /// In hertz.
    pub const fn as_hz(&self) -> u32 {
        self.hz
    }
    /// In kilohertz (truncated).
    pub const fn as_khz(&self) -> u32 {
        self.hz / 1_000
    }
    /// In megahertz (truncated).
    pub const fn as_mhz(&self) -> u32 {
        self.hz / 1_000_000
    }
}

impl core::fmt::Display for Rate {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        if self.hz.is_multiple_of(1_000_000) {
            write!(f, "{} MHz", self.hz / 1_000_000)
        } else if self.hz.is_multiple_of(1_000) {
            write!(f, "{} kHz", self.hz / 1_000)
        } else {
            write!(f, "{} Hz", self.hz)
        }
    }
}

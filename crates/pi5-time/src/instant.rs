//! A point in time, counted in ticks since the counter started.

use super::{Duration, GCD_1K, GCD_1M, TICK_HZ};
use core::fmt;
use core::ops::{Add, AddAssign, Sub, SubAssign};

/// A moment, in 1 us ticks since boot (strictly: since the counter started).
#[derive(Debug, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Instant {
    ticks: u64,
}

impl Instant {
    /// The earliest representable instant.
    pub const MIN: Instant = Instant { ticks: u64::MIN };
    /// The latest representable instant.
    pub const MAX: Instant = Instant { ticks: u64::MAX };

    /// The current time.
    pub fn now() -> Instant {
        Instant { ticks: pi5_executor::time_driver::now() }
    }

    /// From ticks since boot.
    pub const fn from_ticks(ticks: u64) -> Self {
        Self { ticks }
    }
    /// From microseconds since boot.
    pub const fn from_micros(micros: u64) -> Self {
        Self { ticks: micros * (TICK_HZ / GCD_1M) / (1_000_000 / GCD_1M) }
    }
    /// From milliseconds since boot.
    pub const fn from_millis(millis: u64) -> Self {
        Self { ticks: millis * (TICK_HZ / GCD_1K) / (1000 / GCD_1K) }
    }
    /// From seconds since boot.
    pub const fn from_secs(seconds: u64) -> Self {
        Self { ticks: seconds * TICK_HZ }
    }

    /// Ticks since boot.
    pub const fn as_ticks(&self) -> u64 {
        self.ticks
    }
    /// Seconds since boot.
    pub const fn as_secs(&self) -> u64 {
        self.ticks / TICK_HZ
    }
    /// Milliseconds since boot.
    pub const fn as_millis(&self) -> u64 {
        self.ticks * (1000 / GCD_1K) / (TICK_HZ / GCD_1K)
    }
    /// Microseconds since boot.
    pub const fn as_micros(&self) -> u64 {
        self.ticks * (1_000_000 / GCD_1M) / (TICK_HZ / GCD_1M)
    }

    /// Time since `earlier`. Panics if `earlier` is later than `self`.
    pub fn duration_since(&self, earlier: Instant) -> Duration {
        Duration { ticks: self.ticks.checked_sub(earlier.ticks).expect("`earlier` is later than `self`") }
    }
    /// Time since `earlier`, `None` if it is later than `self`.
    pub fn checked_duration_since(&self, earlier: Instant) -> Option<Duration> {
        self.ticks.checked_sub(earlier.ticks).map(|ticks| Duration { ticks })
    }
    /// Time since `earlier`, zero if it is later than `self`.
    pub fn saturating_duration_since(&self, earlier: Instant) -> Duration {
        Duration { ticks: self.ticks.saturating_sub(earlier.ticks) }
    }
    /// Time since this instant.
    pub fn elapsed(&self) -> Duration {
        Instant::now().saturating_duration_since(*self)
    }

    /// Add, `None` on overflow.
    pub fn checked_add(&self, duration: Duration) -> Option<Instant> {
        self.ticks.checked_add(duration.ticks).map(|ticks| Instant { ticks })
    }
    /// Subtract, `None` on underflow.
    pub fn checked_sub(&self, duration: Duration) -> Option<Instant> {
        self.ticks.checked_sub(duration.ticks).map(|ticks| Instant { ticks })
    }
    /// Add, saturating at `MAX`.
    pub fn saturating_add(self, duration: Duration) -> Self {
        Instant { ticks: self.ticks.saturating_add(duration.ticks) }
    }
    /// Subtract, saturating at `MIN`.
    pub fn saturating_sub(self, duration: Duration) -> Self {
        Instant { ticks: self.ticks.saturating_sub(duration.ticks) }
    }
}

impl Add<Duration> for Instant {
    type Output = Instant;
    fn add(self, other: Duration) -> Instant {
        self.checked_add(other).expect("overflow when adding duration to instant")
    }
}
impl AddAssign<Duration> for Instant {
    fn add_assign(&mut self, other: Duration) {
        *self = *self + other;
    }
}
impl Sub<Duration> for Instant {
    type Output = Instant;
    fn sub(self, other: Duration) -> Instant {
        self.checked_sub(other).expect("overflow when subtracting duration from instant")
    }
}
impl SubAssign<Duration> for Instant {
    fn sub_assign(&mut self, other: Duration) {
        *self = *self - other;
    }
}
impl Sub<Instant> for Instant {
    type Output = Duration;
    fn sub(self, other: Instant) -> Duration {
        self.duration_since(other)
    }
}

impl fmt::Display for Instant {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ticks", self.ticks)
    }
}

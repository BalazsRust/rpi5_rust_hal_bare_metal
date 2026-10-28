//! A span of time, counted in ticks.

use super::{GCD_1G, GCD_1K, GCD_1M, TICK_HZ};
use core::fmt;
use core::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Sub, SubAssign};

/// A length of time, in 1 us ticks.
#[derive(Debug, Default, Copy, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Duration {
    pub(crate) ticks: u64,
}

impl Duration {
    /// Zero.
    pub const MIN: Duration = Duration { ticks: 0 };
    /// The largest representable duration.
    pub const MAX: Duration = Duration { ticks: u64::MAX };

    /// In ticks.
    pub const fn as_ticks(&self) -> u64 {
        self.ticks
    }
    /// In whole seconds.
    pub const fn as_secs(&self) -> u64 {
        self.ticks / TICK_HZ
    }
    /// In whole milliseconds.
    pub const fn as_millis(&self) -> u64 {
        self.ticks * (1000 / GCD_1K) / (TICK_HZ / GCD_1K)
    }
    /// In whole microseconds.
    pub const fn as_micros(&self) -> u64 {
        self.ticks * (1_000_000 / GCD_1M) / (TICK_HZ / GCD_1M)
    }
    /// In nanoseconds.
    pub const fn as_nanos(&self) -> u64 {
        self.ticks * (1_000_000_000 / GCD_1G) / (TICK_HZ / GCD_1G)
    }

    /// From ticks.
    pub const fn from_ticks(ticks: u64) -> Duration {
        Duration { ticks }
    }
    /// From seconds (rounded up to a whole tick).
    pub const fn from_secs(secs: u64) -> Duration {
        Duration { ticks: secs * TICK_HZ }
    }
    /// From milliseconds (rounded up).
    pub const fn from_millis(millis: u64) -> Duration {
        Duration { ticks: div_ceil(millis * (TICK_HZ / GCD_1K), 1000 / GCD_1K) }
    }
    /// From microseconds (rounded up).
    pub const fn from_micros(micros: u64) -> Duration {
        Duration { ticks: div_ceil(micros * (TICK_HZ / GCD_1M), 1_000_000 / GCD_1M) }
    }
    /// From nanoseconds (rounded up).
    pub const fn from_nanos(nanos: u64) -> Duration {
        Duration { ticks: div_ceil(nanos * (TICK_HZ / GCD_1G), 1_000_000_000 / GCD_1G) }
    }
    /// From milliseconds, rounded down.
    pub const fn from_millis_floor(millis: u64) -> Duration {
        Duration { ticks: millis * (TICK_HZ / GCD_1K) / (1000 / GCD_1K) }
    }
    /// From microseconds, rounded down.
    pub const fn from_micros_floor(micros: u64) -> Duration {
        Duration { ticks: micros * (TICK_HZ / GCD_1M) / (1_000_000 / GCD_1M) }
    }
    /// The period of a frequency: `from_hz(10)` = 100 ms.
    pub const fn from_hz(hz: u64) -> Duration {
        Duration { ticks: if hz >= TICK_HZ { 1 } else { div_ceil(TICK_HZ, hz) } }
    }

    /// Add, `None` on overflow.
    pub const fn checked_add(self, rhs: Duration) -> Option<Duration> {
        match self.ticks.checked_add(rhs.ticks) {
            Some(ticks) => Some(Duration { ticks }),
            None => None,
        }
    }
    /// Subtract, `None` if the result would be negative.
    pub const fn checked_sub(self, rhs: Duration) -> Option<Duration> {
        match self.ticks.checked_sub(rhs.ticks) {
            Some(ticks) => Some(Duration { ticks }),
            None => None,
        }
    }
    /// Multiply, `None` on overflow.
    pub const fn checked_mul(self, rhs: u32) -> Option<Duration> {
        match self.ticks.checked_mul(rhs as u64) {
            Some(ticks) => Some(Duration { ticks }),
            None => None,
        }
    }
    /// Divide, `None` if `rhs` is 0.
    pub const fn checked_div(self, rhs: u32) -> Option<Duration> {
        match self.ticks.checked_div(rhs as u64) {
            Some(ticks) => Some(Duration { ticks }),
            None => None,
        }
    }
}

const fn div_ceil(a: u64, b: u64) -> u64 {
    a.div_ceil(b)
}

impl Add for Duration {
    type Output = Duration;
    fn add(self, rhs: Duration) -> Duration {
        self.checked_add(rhs).expect("overflow when adding durations")
    }
}
impl AddAssign for Duration {
    fn add_assign(&mut self, rhs: Duration) {
        *self = *self + rhs;
    }
}
impl Sub for Duration {
    type Output = Duration;
    fn sub(self, rhs: Duration) -> Duration {
        self.checked_sub(rhs).expect("overflow when subtracting durations")
    }
}
impl SubAssign for Duration {
    fn sub_assign(&mut self, rhs: Duration) {
        *self = *self - rhs;
    }
}
impl Mul<u32> for Duration {
    type Output = Duration;
    fn mul(self, rhs: u32) -> Duration {
        self.checked_mul(rhs).expect("overflow when multiplying duration by scalar")
    }
}
impl Mul<Duration> for u32 {
    type Output = Duration;
    fn mul(self, rhs: Duration) -> Duration {
        rhs * self
    }
}
impl MulAssign<u32> for Duration {
    fn mul_assign(&mut self, rhs: u32) {
        *self = *self * rhs;
    }
}
impl Div<u32> for Duration {
    type Output = Duration;
    fn div(self, rhs: u32) -> Duration {
        self.checked_div(rhs).expect("divide by zero error when dividing duration by scalar")
    }
}
impl DivAssign<u32> for Duration {
    fn div_assign(&mut self, rhs: u32) {
        *self = *self / rhs;
    }
}

impl fmt::Display for Duration {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} ticks", self.ticks)
    }
}

impl TryFrom<core::time::Duration> for Duration {
    type Error = ();
    fn try_from(value: core::time::Duration) -> Result<Self, ()> {
        let nanos = u64::try_from(value.as_nanos()).map_err(|_| ())?;
        Ok(Duration::from_nanos(nanos))
    }
}

impl From<Duration> for core::time::Duration {
    fn from(value: Duration) -> Self {
        core::time::Duration::from_micros(value.as_micros())
    }
}

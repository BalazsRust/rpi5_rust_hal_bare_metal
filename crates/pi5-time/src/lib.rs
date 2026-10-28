//! # pi5-time: `Duration`, `Instant`, `Timer`, `Ticker`
//!
//! Shaped like `embassy-time`, with a fixed tick of 1 us
//! ([`TICK_HZ`] = 1,000,000) coming from the ARM Generic Timer through
//! [`pi5_executor::time_driver`].
//!
//! ```no_run
//! use pi5_time::{Duration, Instant, Ticker, Timer};
//! # async fn demo() {
//! Timer::after(Duration::from_millis(500)).await;
//! Timer::after_millis(500).await;              // same thing
//! let t0 = Instant::now();
//! let mut ticker = Ticker::every(Duration::from_hz(10));
//! loop {
//!     ticker.next().await;                     // 10 times per second, no drift
//!     let _ = t0.elapsed();
//! }
//! # }
//! ```
//!
//! `Timer` and `Ticker` only work inside executor tasks. For a plain busy
//! wait use [`block_for`] or [`Delay`].

#![no_std]
#![warn(missing_docs)]

mod delay;
mod duration;
mod instant;
mod timer;

pub use delay::{Delay, block_for};
pub use duration::Duration;
pub use instant::Instant;
pub use timer::{Ticker, TimeoutError, TimeoutFuture, Timer, with_deadline, with_timeout};

/// Ticks per second.
pub const TICK_HZ: u64 = pi5_executor::time_driver::TICK_HZ;

const fn gcd(a: u64, b: u64) -> u64 {
    if b == 0 { a } else { gcd(b, a % b) }
}

pub(crate) const GCD_1K: u64 = gcd(TICK_HZ, 1_000);
pub(crate) const GCD_1M: u64 = gcd(TICK_HZ, 1_000_000);
pub(crate) const GCD_1G: u64 = gcd(TICK_HZ, 1_000_000_000);

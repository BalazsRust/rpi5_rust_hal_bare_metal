//! Blocking waits, for code that is not inside a task.

use super::{Duration, Instant};
use embedded_hal::delay::DelayNs;

/// Spin until `duration` has passed. Works anywhere, needs no executor.
pub fn block_for(duration: Duration) {
    let deadline = Instant::now() + duration;
    while Instant::now() < deadline {
        core::hint::spin_loop();
    }
}

/// `embedded-hal` `DelayNs` implemented with [`block_for`]
/// (`embassy_time::Delay` equivalent).
#[derive(Debug, Clone, Copy, Default)]
pub struct Delay;

impl DelayNs for Delay {
    fn delay_ns(&mut self, ns: u32) {
        block_for(Duration::from_nanos(u64::from(ns)));
    }
    fn delay_us(&mut self, us: u32) {
        block_for(Duration::from_micros(u64::from(us)));
    }
    fn delay_ms(&mut self, ms: u32) {
        block_for(Duration::from_millis(u64::from(ms)));
    }
}

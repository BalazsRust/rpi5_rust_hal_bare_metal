//! The clock behind `pi5_time`: the ARM Generic Timer, scaled to 1 MHz ticks.
//!
//! In embassy this is the `embassy-time-driver` trait that the HAL
//! implements; here it is simply two functions. `now()` reads `CNTVCT_EL0`
//! (the virtual counter; pi5-hal sets every core's offset to 0, so all cores
//! agree) and divides by `CNTFRQ_EL0 / 1_000_000`, which is exact for the
//! 54 MHz counter of the BCM2712. `schedule_wake` hands a deadline to the
//! timer queue of the executor that owns the calling task.

use core::arch::asm;
use core::sync::atomic::{AtomicU64, Ordering};
use core::task::Waker;

/// Ticks per second of [`now`].
pub const TICK_HZ: u64 = 1_000_000;

static TICKS_PER_US: AtomicU64 = AtomicU64::new(0);

fn ticks_per_us() -> u64 {
    let cached = TICKS_PER_US.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    let f: u64;
    // SAFETY: side-effect free read.
    unsafe { asm!("mrs {}, cntfrq_el0", out(reg) f, options(nomem, nostack, preserves_flags)) };
    let v = (f / TICK_HZ).max(1);
    TICKS_PER_US.store(v, Ordering::Relaxed);
    v
}

/// Current time in microseconds since the counter started.
#[inline]
pub fn now() -> u64 {
    let t: u64;
    // SAFETY: `isb` keeps the read from being speculated early.
    unsafe { asm!("isb", "mrs {}, cntvct_el0", out(reg) t, options(nostack, preserves_flags)) };
    t / ticks_per_us()
}

/// Ask the executor to wake the task behind `waker` at time `at` (ticks).
///
/// # Panics
/// If `waker` does not belong to a pi5-executor task.
pub fn schedule_wake(at: u64, waker: &Waker) {
    let task = crate::raw::waker::task_from_waker(waker);
    let executor = task.executor().expect("task without executor");
    // SAFETY: called from the task's own poll, i.e. on its executor's core.
    unsafe { executor.schedule_wake(task, at) }
}

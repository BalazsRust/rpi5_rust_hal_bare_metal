//! # pi5-executor: an embassy-shaped async executor
//!
//! ```no_run
//! use pi5_executor::Spawner;
//!
//! #[pi5_executor::main]
//! async fn main(spawner: Spawner) {
//!     spawner.spawn(worker(1).unwrap());
//! }
//!
//! #[pi5_executor::task(pool_size = 4)]
//! async fn worker(id: u32) {
//!     let _ = id;
//! }
//! ```
//!
//! # How it works, in five sentences
//!
//! A **task** is a future stored in static memory ([`raw::TaskStorage`]), with
//! a small header in front of it. The **executor** owns a run queue (an atomic
//! linked list of headers) and polls every task on it. Polling hands the task
//! a **waker** that is just a pointer to its header; when something calls
//! `wake()`, the header is pushed back onto its executor's run queue and the
//! executor's core is kicked with `sev`. A task waiting on a `Timer` instead
//! registers a deadline in the executor's **timer queue**; the executor's idle
//! loop sleeps with `wfe`, which the CPU's timer event stream wakes every
//! ~38 us, and wakes the task once the deadline has passed. Each CPU core
//! runs its own executor ([`Executor::run`]); tasks never migrate, but any
//! core may wake a task on any other.
//!
//! # Differences from embassy-executor
//!
//! * No interrupt-driven executors; there are no interrupts here.
//! * `#[task]` storage comes from a fixed arena ([`raw::TASK_ARENA_SIZE`]) like
//!   embassy's older stable mode, because an `async fn`'s future type cannot
//!   be named in a `static`.
//! * The time driver (`CNTVCT_EL0`, 1 MHz ticks) lives in this crate
//!   ([`time_driver`]) rather than in a separate `embassy-time-driver`.
//!
//! Requires the MMU to be on (atomics); pi5-hal's runtime does that.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

pub mod raw;
mod spawner;
mod thread;
pub mod time_driver;

pub use pi5_executor_macros::{main, task};
pub use spawner::{SendSpawner, SpawnError, SpawnToken, Spawner};
pub use thread::Executor;

/// Used by the macros.
#[doc(hidden)]
pub mod _export {
    /// Extend a reference to `'static`. Sound only if the referent really
    /// does live forever: the `#[main]` macro calls it on an executor whose
    /// `run()` never returns.
    ///
    /// # Safety
    /// `t` must never be dropped or moved.
    pub unsafe fn make_static<T>(t: &mut T) -> &'static mut T {
        // SAFETY: forwarded to the caller.
        unsafe { core::mem::transmute(t) }
    }
}

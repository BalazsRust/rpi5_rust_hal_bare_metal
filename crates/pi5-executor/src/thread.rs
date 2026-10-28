//! The "thread-mode" executor: one per core, runs forever.

use crate::raw;
use crate::spawner::Spawner;
use core::marker::PhantomData;

/// An executor that owns its core's main loop.
///
/// ```no_run
/// # use pi5_executor::{Executor, Spawner};
/// # fn demo() -> ! {
/// let mut executor = Executor::new();
/// let executor = unsafe { pi5_executor::_export::make_static(&mut executor) };
/// executor.run(|spawner: Spawner| {
///     // spawn the first tasks here
///     let _ = spawner;
/// })
/// # }
/// ```
///
/// (`#[pi5_executor::main]` writes exactly that for core 0; for other cores
/// put the same inside a `pi5_hal::multicore::spawn_core1` closure.)
pub struct Executor {
    inner: raw::Executor,
    not_send: PhantomData<*mut ()>,
}

impl Executor {
    /// A new executor. It must end up with a `'static` lifetime before
    /// `run`; a `static` cell or the pattern shown above both work.
    pub const fn new() -> Self {
        Self { inner: raw::Executor::new(), not_send: PhantomData }
    }

    /// Run forever on the current core. `init` is called once with the
    /// spawner to start the first tasks.
    ///
    /// The loop: poll everything runnable; service timers; if nothing is
    /// runnable and no deadline has passed, `wfe`. The CPU's timer event
    /// stream (enabled by pi5-hal's runtime) wakes `wfe` every ~38 us, and
    /// any wake from another core does so with `sev`, so the loop never
    /// sleeps through a deadline or a wake-up.
    pub fn run(&'static mut self, init: impl FnOnce(Spawner)) -> ! {
        init(self.inner.spawner());
        loop {
            // SAFETY: this core owns the executor.
            let next_deadline = unsafe { self.inner.poll() };
            if self.inner.has_runnable() {
                continue;
            }
            if next_deadline != u64::MAX && crate::time_driver::now() >= next_deadline {
                continue;
            }
            // SAFETY: wfe only sleeps the core.
            unsafe { core::arch::asm!("wfe", options(nomem, nostack, preserves_flags)) };
        }
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

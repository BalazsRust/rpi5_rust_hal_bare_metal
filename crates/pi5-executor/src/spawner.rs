//! `Spawner`, `SpawnToken`, `SpawnError`: the user-facing half of spawning.

use crate::raw::{self, TaskRef};
use core::future::{Future, poll_fn};
use core::marker::PhantomData;
use core::task::Poll;

/// A task that has been created but not yet put on an executor. Produced by
/// calling a `#[task]` function; consumed by [`Spawner::spawn`].
///
/// `S` records the type of the task's argument closure: `SendSpawner` only
/// accepts tokens whose `S: Send`, i.e. whose arguments can cross cores.
#[must_use = "a SpawnToken must be passed to Spawner::spawn"]
pub struct SpawnToken<S> {
    raw_task: Option<TaskRef>,
    phantom: PhantomData<*mut S>,
}

impl<S> SpawnToken<S> {
    pub(crate) unsafe fn new(raw_task: TaskRef) -> Self {
        Self { raw_task: Some(raw_task), phantom: PhantomData }
    }

    /// An identifier for the task (its header address), handy for logging.
    pub fn id(&self) -> usize {
        self.raw_task.map(|t| t.as_ptr() as usize).unwrap_or(0)
    }
}

impl<S> Drop for SpawnToken<S> {
    fn drop(&mut self) {
        if self.raw_task.is_some() {
            panic!("SpawnToken instances may not be dropped. You must pass them to Spawner::spawn()");
        }
    }
}

/// Why spawning failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpawnError {
    /// Every slot of the task's pool is in use (`#[task(pool_size = N)]`).
    Busy,
}

impl core::fmt::Display for SpawnError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            SpawnError::Busy => write!(f, "task pool is full (increase pool_size)"),
        }
    }
}

/// Handle to spawn tasks onto one executor. Not `Send`: it belongs to the
/// core that runs the executor. Use [`Spawner::make_send`] to get a handle
/// you can pass to other cores.
#[derive(Clone, Copy)]
pub struct Spawner {
    executor: &'static raw::Executor,
    not_send: PhantomData<*mut ()>,
}

impl Spawner {
    pub(crate) fn new(executor: &'static raw::Executor) -> Self {
        Self { executor, not_send: PhantomData }
    }

    /// Get a spawner for the executor running the current task.
    ///
    /// # Safety
    /// Must be called from inside a task polled by a pi5 executor (that is
    /// what the waker check verifies; it panics otherwise).
    pub unsafe fn for_current_executor() -> impl Future<Output = Self> {
        poll_fn(|cx| {
            let task = raw::waker::task_from_waker(cx.waker());
            let executor = task.executor().expect("task without executor");
            Poll::Ready(Self::new(executor))
        })
    }

    /// Put the task on the executor's run queue. It runs the next time the
    /// executor polls; this never blocks and never fails (pool exhaustion
    /// was already reported when the token was made).
    pub fn spawn<S>(&self, token: SpawnToken<S>) {
        let task = take_task(token);
        // SAFETY: the token proves the task is initialised and unspawned.
        unsafe { self.executor.spawn(task) }
    }

    /// Alias of [`spawn`](Self::spawn) kept for embassy compatibility.
    pub fn must_spawn<S>(&self, token: SpawnToken<S>) {
        self.spawn(token)
    }

    /// A `Send` handle for spawning onto this executor from another core.
    pub fn make_send(&self) -> SendSpawner {
        SendSpawner { executor: self.executor }
    }
}

/// A spawner that can be moved to another core (e.g. in a closure given to
/// `pi5_hal::multicore::spawn_core1`). Only accepts tasks whose arguments
/// are `Send`.
#[derive(Clone, Copy)]
pub struct SendSpawner {
    executor: &'static raw::Executor,
}

// SAFETY: the executor's queues are designed for cross-core enqueue.
unsafe impl Send for SendSpawner {}
unsafe impl Sync for SendSpawner {}

impl SendSpawner {
    /// Spawn onto the executor this handle belongs to, from any core.
    pub fn spawn<S: Send>(&self, token: SpawnToken<S>) {
        let task = take_task(token);
        // SAFETY: as in `Spawner::spawn`.
        unsafe { self.executor.spawn(task) }
    }
}

fn take_task<S>(mut token: SpawnToken<S>) -> TaskRef {
    let task = token.raw_task.take().expect("token already consumed");
    core::mem::forget(token);
    task
}

//! The machinery underneath [`crate::Spawner`] and [`crate::Executor`].
//!
//! You do not need this module to *use* the executor; read it to understand
//! what a task physically is. Layout of one task in memory:
//!
//! ```text
//! TaskStorage<F>  (#[repr(C)], lives in the task arena)
//! +--------------------------------------------------------------+
//! | TaskHeader                                                   |
//! |   state           SPAWNED | RUN_QUEUED | TIMER_QUEUED flags   |
//! |   run_queue_next  link in the executor's run queue            |
//! |   executor        which executor polls this task              |
//! |   poll_fn         TaskStorage::<F>::poll, erases the type F   |
//! |   expires_at      earliest Timer deadline, u64::MAX if none   |
//! |   timer_queue_next link in the executor's timer queue         |
//! +--------------------------------------------------------------+
//! | future: F        the async fn's state machine                 |
//! +--------------------------------------------------------------+
//! ```
//!
//! A `TaskRef` is a pointer to the header. Because the header is the first
//! field, `poll_fn` can cast it back to the full `TaskStorage<F>`.

use core::cell::UnsafeCell;
use core::future::Future;
use core::mem::MaybeUninit;
use core::pin::Pin;
use core::ptr::{self, NonNull};
use core::sync::atomic::{AtomicPtr, AtomicU32, AtomicU64, AtomicUsize, Ordering};
use core::task::Context;

use crate::spawner::{SpawnError, SpawnToken};

pub mod waker;

// ---------------------------------------------------------------------------
// Task state
// ---------------------------------------------------------------------------

/// The slot holds a live future.
const STATE_SPAWNED: u32 = 1 << 0;
/// The task is linked into a run queue.
const STATE_RUN_QUEUED: u32 = 1 << 1;
/// The task is linked into a timer queue.
const STATE_TIMER_QUEUED: u32 = 1 << 2;

/// A cell that is `Sync` because the executor design guarantees the access
/// pattern is safe (single writer, or writer before publication).
struct SyncUnsafeCell<T>(UnsafeCell<T>);
// SAFETY: see each use site.
unsafe impl<T> Sync for SyncUnsafeCell<T> {}
impl<T> SyncUnsafeCell<T> {
    const fn new(v: T) -> Self {
        Self(UnsafeCell::new(v))
    }
    fn get(&self) -> *mut T {
        self.0.get()
    }
}

/// The part of a task that the executor works with.
pub struct TaskHeader {
    state: AtomicU32,
    run_queue_next: AtomicPtr<TaskHeader>,
    executor: AtomicPtr<Executor>,
    /// Set once at spawn, before the task is published; never changed after.
    poll_fn: SyncUnsafeCell<Option<unsafe fn(TaskRef)>>,
    /// Only touched by the task's own executor core.
    expires_at: AtomicU64,
    /// Only touched by the task's own executor core.
    timer_queue_next: SyncUnsafeCell<Option<TaskRef>>,
}

impl TaskHeader {
    const fn new() -> Self {
        Self {
            state: AtomicU32::new(0),
            run_queue_next: AtomicPtr::new(ptr::null_mut()),
            executor: AtomicPtr::new(ptr::null_mut()),
            poll_fn: SyncUnsafeCell::new(None),
            expires_at: AtomicU64::new(u64::MAX),
            timer_queue_next: SyncUnsafeCell::new(None),
        }
    }
}

/// A pointer to a task's header.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct TaskRef {
    ptr: NonNull<TaskHeader>,
}

// SAFETY: TaskRef is a pointer; the header is designed for cross-core use.
unsafe impl Send for TaskRef {}
unsafe impl Sync for TaskRef {}

impl TaskRef {
    fn new<F: Future + 'static>(task: &'static TaskStorage<F>) -> Self {
        Self { ptr: NonNull::from(&task.raw) }
    }

    /// Rebuild from a raw pointer (used by wakers).
    ///
    /// # Safety
    /// `ptr` must come from [`TaskRef::as_ptr`].
    pub unsafe fn from_ptr(ptr: *const TaskHeader) -> Self {
        // SAFETY: the caller guarantees validity.
        Self { ptr: unsafe { NonNull::new_unchecked(ptr as *mut TaskHeader) } }
    }

    /// The header pointer.
    pub fn as_ptr(self) -> *const TaskHeader {
        self.ptr.as_ptr()
    }

    pub(crate) fn header(self) -> &'static TaskHeader {
        // SAFETY: tasks live in static storage and are never freed.
        unsafe { self.ptr.as_ref() }
    }

    /// The executor this task was spawned on, if any.
    pub fn executor(self) -> Option<&'static Executor> {
        let p = self.header().executor.load(Ordering::Acquire);
        // SAFETY: executors are `'static` (see `Executor::spawn`).
        unsafe { p.as_ref() }
    }
}

// ---------------------------------------------------------------------------
// Task storage
// ---------------------------------------------------------------------------

/// Storage for one task: header + future.
#[repr(C)]
pub struct TaskStorage<F: Future + 'static> {
    raw: TaskHeader,
    future: UnsafeCell<MaybeUninit<F>>,
}

// SAFETY: the future is only ever touched by the executor that owns the task,
// one poll at a time; the header is built for concurrent use.
unsafe impl<F: Future + 'static> Sync for TaskStorage<F> {}

impl<F: Future + 'static> TaskStorage<F> {
    /// An empty slot.
    pub const fn new() -> Self {
        Self { raw: TaskHeader::new(), future: UnsafeCell::new(MaybeUninit::uninit()) }
    }

    /// Put a future in this slot. `Err(Busy)` if it is occupied.
    pub fn spawn(&'static self, future: impl FnOnce() -> F) -> Result<SpawnToken<F>, SpawnError> {
        if !self.claim() {
            return Err(SpawnError::Busy);
        }
        // SAFETY: claimed.
        Ok(unsafe { self.initialize(future) })
    }

    /// Atomically set SPAWNED if it was clear. The other flags are kept: a
    /// stale RUN_QUEUED/TIMER_QUEUED means the task is still linked in a
    /// queue from its previous life, and the queue will sort that out.
    fn claim(&'static self) -> bool {
        self.raw
            .state
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |s| if s & STATE_SPAWNED == 0 { Some(s | STATE_SPAWNED) } else { None })
            .is_ok()
    }

    /// # Safety
    /// Must be called exactly once after a successful [`claim`](Self::claim).
    unsafe fn initialize<S>(&'static self, future: impl FnOnce() -> F) -> SpawnToken<S> {
        // SAFETY: we own the slot until the token is spawned.
        unsafe {
            (*self.future.get()).write(future());
            *self.raw.poll_fn.get() = Some(Self::poll);
        }
        // SAFETY: the slot is initialised.
        unsafe { SpawnToken::new(TaskRef::new(self)) }
    }

    /// Poll the future once. Called by the executor through `poll_fn`.
    unsafe fn poll(p: TaskRef) {
        // SAFETY: `p` points at the header of a `TaskStorage<F>` (repr(C),
        // header first) and the executor is the only one polling it.
        let this = unsafe { &*(p.as_ptr() as *const TaskStorage<F>) };
        let future = unsafe { Pin::new_unchecked((*this.future.get()).assume_init_mut()) };
        let waker = waker::from_task(p);
        let mut cx = Context::from_waker(&waker);
        if future.poll(&mut cx).is_ready() {
            // SAFETY: the future is done; drop it and free the slot.
            unsafe { (*this.future.get()).assume_init_drop() };
            this.raw.state.fetch_and(!STATE_SPAWNED, Ordering::AcqRel);
        }
    }
}

impl<F: Future + 'static> Default for TaskStorage<F> {
    fn default() -> Self {
        Self::new()
    }
}

/// `N` slots of the same task type.
pub struct TaskPool<F: Future + 'static, const N: usize> {
    pool: [TaskStorage<F>; N],
}

impl<F: Future + 'static, const N: usize> TaskPool<F, N> {
    #[allow(clippy::declare_interior_mutable_const)] // used only as an array initialiser
    const NEW_TASK: TaskStorage<F> = TaskStorage::new();

    /// An empty pool.
    pub const fn new() -> Self {
        Self { pool: [Self::NEW_TASK; N] }
    }

    /// Put a future in a free slot.
    pub fn spawn(&'static self, future: impl FnOnce() -> F) -> Result<SpawnToken<F>, SpawnError> {
        self.spawn_impl(future)
    }

    /// Like [`spawn`](Self::spawn), but the token's type parameter is the
    /// closure's type, so `SendSpawner` can require the *arguments* to be
    /// `Send` even if the future is not. Used by `#[task]`.
    ///
    /// # Safety
    /// The closure must only capture the task's arguments.
    pub unsafe fn _spawn_async_fn<FutFn: FnOnce() -> F>(&'static self, future: FutFn) -> Result<SpawnToken<FutFn>, SpawnError> {
        self.spawn_impl(future)
    }

    fn spawn_impl<S>(&'static self, future: impl FnOnce() -> F) -> Result<SpawnToken<S>, SpawnError> {
        for slot in &self.pool {
            if slot.claim() {
                // SAFETY: claimed.
                return Ok(unsafe { slot.initialize(future) });
            }
        }
        Err(SpawnError::Busy)
    }
}

impl<F: Future + 'static, const N: usize> Default for TaskPool<F, N> {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Arena-backed pools for #[task]
// ---------------------------------------------------------------------------

/// Bytes reserved for all `#[task]` pools together. A task costs its header
/// (~56 bytes) plus the size of its future (all locals that live across an
/// `.await`). Increase if you see "task arena full".
pub const TASK_ARENA_SIZE: usize = 32 * 1024;

struct Arena {
    buf: UnsafeCell<[MaybeUninit<u8>; TASK_ARENA_SIZE]>,
    used: AtomicUsize,
}
// SAFETY: allocation is a bump pointer advanced with CAS; handed-out regions
// never overlap.
unsafe impl Sync for Arena {}

static ARENA: Arena = Arena { buf: UnsafeCell::new([MaybeUninit::uninit(); TASK_ARENA_SIZE]), used: AtomicUsize::new(0) };

impl Arena {
    /// Reserve room for one `T`; returns a raw pointer to uninitialised,
    /// exclusively owned memory.
    fn alloc<T>(&'static self) -> *mut MaybeUninit<T> {
        let size = core::mem::size_of::<T>();
        let align = core::mem::align_of::<T>();
        let base = self.buf.get() as usize;
        loop {
            let used = self.used.load(Ordering::Relaxed);
            let start = (base + used + align - 1) & !(align - 1);
            let end = start + size;
            if end > base + TASK_ARENA_SIZE {
                panic!("pi5-executor: task arena full ({TASK_ARENA_SIZE} bytes); raise raw::TASK_ARENA_SIZE");
            }
            if self.used.compare_exchange(used, end - base, Ordering::AcqRel, Ordering::Relaxed).is_ok() {
                return start as *mut MaybeUninit<T>;
            }
        }
    }
}

/// A `static` that lazily becomes a `TaskPool<F, N>` for whatever `F` it is
/// first used with. Needed because `#[task]` cannot name the future type of
/// an `async fn` to declare `static POOL: TaskPool<ThatType, N>`.
pub struct TaskPoolRef {
    ptr: AtomicPtr<()>,
}

impl TaskPoolRef {
    /// Not yet allocated.
    pub const fn new() -> Self {
        Self { ptr: AtomicPtr::new(ptr::null_mut()) }
    }

    /// The pool, allocating it from the arena on first use.
    pub fn get<F: Future + 'static, const N: usize>(&'static self) -> &'static TaskPool<F, N> {
        let p = self.ptr.load(Ordering::Acquire);
        if !p.is_null() {
            // SAFETY: a given TaskPoolRef is only ever used with one F/N
            // (it sits inside one generated function).
            return unsafe { &*(p as *const TaskPool<F, N>) };
        }
        let slot = ARENA.alloc::<TaskPool<F, N>>();
        // SAFETY: `slot` is a fresh, aligned, exclusively owned arena region
        // that lives forever (the arena is a static).
        let pool: &'static TaskPool<F, N> = unsafe { &*(*slot).write(TaskPool::new()) };
        match self.ptr.compare_exchange(ptr::null_mut(), pool as *const _ as *mut (), Ordering::AcqRel, Ordering::Acquire) {
            Ok(_) => pool,
            // Another core raced us; use theirs, ours stays unused.
            // SAFETY: as above.
            Err(existing) => unsafe { &*(existing as *const TaskPool<F, N>) },
        }
    }
}

impl Default for TaskPoolRef {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Run queue: an atomic intrusive stack (same design as embassy's)
// ---------------------------------------------------------------------------

struct RunQueue {
    head: AtomicPtr<TaskHeader>,
}

impl RunQueue {
    const fn new() -> Self {
        Self { head: AtomicPtr::new(ptr::null_mut()) }
    }

    /// Push a task. Safe to call from any core: a compare-and-swap loop
    /// links the task in front of whatever is there.
    fn enqueue(&self, task: TaskRef) {
        let _ = self.head.fetch_update(Ordering::AcqRel, Ordering::Acquire, |prev| {
            task.header().run_queue_next.store(prev, Ordering::Relaxed);
            Some(task.as_ptr() as *mut TaskHeader)
        });
    }

    fn is_empty(&self) -> bool {
        self.head.load(Ordering::Acquire).is_null()
    }

    /// Take the whole list in one atomic swap and process it. Tasks woken
    /// *while* we process land on the new (empty) list for the next round.
    fn dequeue_all(&self, mut on_task: impl FnMut(TaskRef)) {
        let mut p = self.head.swap(ptr::null_mut(), Ordering::AcqRel);
        while !p.is_null() {
            // SAFETY: queue entries are valid headers.
            let task = unsafe { TaskRef::from_ptr(p) };
            p = task.header().run_queue_next.load(Ordering::Relaxed);
            // Clear RUN_QUEUED *before* polling, so a wake during the poll
            // re-queues the task instead of being lost.
            task.header().state.fetch_and(!STATE_RUN_QUEUED, Ordering::AcqRel);
            on_task(task);
        }
    }
}

// ---------------------------------------------------------------------------
// Timer queue: single-core (only the owning executor touches it)
// ---------------------------------------------------------------------------

struct TimerQueue {
    head: SyncUnsafeCell<Option<TaskRef>>,
}

impl TimerQueue {
    const fn new() -> Self {
        Self { head: SyncUnsafeCell::new(None) }
    }

    /// Remember that `task` wants to be woken at `at` (ticks). Called from
    /// the task's own poll, i.e. on its executor's core.
    unsafe fn schedule_wake(&self, task: TaskRef, at: u64) {
        let h = task.header();
        if at < h.expires_at.load(Ordering::Relaxed) {
            h.expires_at.store(at, Ordering::Relaxed);
        }
        if h.state.load(Ordering::Relaxed) & STATE_TIMER_QUEUED == 0 {
            h.state.fetch_or(STATE_TIMER_QUEUED, Ordering::Relaxed);
            // SAFETY: single-core access by design.
            unsafe {
                *h.timer_queue_next.get() = *self.head.get();
                *self.head.get() = Some(task);
            }
        }
    }

    /// Wake every task whose deadline has passed (and drop dead tasks from
    /// the list); return the earliest remaining deadline, `u64::MAX` if none.
    unsafe fn next_expiration(&self, now: u64) -> u64 {
        let mut next = u64::MAX;
        let mut link: *mut Option<TaskRef> = self.head.get();
        // SAFETY: single-core access by design.
        unsafe {
            while let Some(task) = *link {
                let h = task.header();
                let at = h.expires_at.load(Ordering::Relaxed);
                let alive = h.state.load(Ordering::Relaxed) & STATE_SPAWNED != 0;
                if at <= now || !alive {
                    *link = *h.timer_queue_next.get();
                    h.state.fetch_and(!STATE_TIMER_QUEUED, Ordering::Relaxed);
                    h.expires_at.store(u64::MAX, Ordering::Relaxed);
                    if alive {
                        wake_task(task);
                    }
                } else {
                    next = next.min(at);
                    link = h.timer_queue_next.get();
                }
            }
        }
        next
    }
}

// ---------------------------------------------------------------------------
// Executor
// ---------------------------------------------------------------------------

/// The raw executor: a run queue plus a timer queue. [`crate::Executor`]
/// wraps it with the idle loop.
pub struct Executor {
    run_queue: RunQueue,
    timer_queue: TimerQueue,
}

impl Executor {
    /// A new executor with empty queues.
    pub const fn new() -> Self {
        Self { run_queue: RunQueue::new(), timer_queue: TimerQueue::new() }
    }

    /// Bind `task` to this executor and queue it for its first poll.
    ///
    /// # Safety
    /// `task` must have been initialised and not yet spawned.
    pub unsafe fn spawn(&'static self, task: TaskRef) {
        task.header().executor.store(self as *const _ as *mut Executor, Ordering::Release);
        wake_task(task);
    }

    /// Push onto the run queue and kick every sleeping core.
    fn enqueue(&self, task: TaskRef) {
        self.run_queue.enqueue(task);
        // The target core may be in `wfe`; `sev` wakes all cores. The
        // Release ordering of the enqueue makes the queue visible first.
        // SAFETY: `sev` has no side effects besides waking cores.
        unsafe { core::arch::asm!("sev", options(nomem, nostack, preserves_flags)) };
    }

    /// Poll every runnable task once, then service the timer queue.
    /// Returns the earliest pending deadline (`u64::MAX` if none).
    ///
    /// # Safety
    /// Must only be called from the one core that runs this executor.
    pub unsafe fn poll(&'static self) -> u64 {
        self.run_queue.dequeue_all(|task| {
            let h = task.header();
            if h.state.load(Ordering::Acquire) & STATE_SPAWNED != 0 {
                // SAFETY: poll_fn was set at initialisation.
                if let Some(f) = unsafe { *h.poll_fn.get() } {
                    // SAFETY: we are the owning executor.
                    unsafe { f(task) };
                }
            }
        });
        // SAFETY: owning core.
        unsafe { self.timer_queue.next_expiration(crate::time_driver::now()) }
    }

    /// Is anything waiting to be polled?
    pub fn has_runnable(&self) -> bool {
        !self.run_queue.is_empty()
    }

    /// A handle for spawning onto this executor.
    pub fn spawner(&'static self) -> crate::Spawner {
        crate::Spawner::new(self)
    }

    /// Called by the time driver: `task` wants to run at `at`.
    ///
    /// # Safety
    /// Must be called on this executor's core (it is, from a task's poll).
    pub(crate) unsafe fn schedule_wake(&self, task: TaskRef, at: u64) {
        // SAFETY: forwarded.
        unsafe { self.timer_queue.schedule_wake(task, at) }
    }
}

impl Default for Executor {
    fn default() -> Self {
        Self::new()
    }
}

/// Wake a task: mark it queued and push it onto its executor's run queue.
/// Does nothing if the task is not spawned or already queued. Safe from any
/// core.
pub fn wake_task(task: TaskRef) {
    let h = task.header();
    let claimed = h
        .state
        .fetch_update(Ordering::AcqRel, Ordering::Acquire, |s| {
            if s & STATE_SPAWNED == 0 || s & STATE_RUN_QUEUED != 0 { None } else { Some(s | STATE_RUN_QUEUED) }
        })
        .is_ok();
    if !claimed {
        return;
    }
    if let Some(executor) = task.executor() {
        executor.enqueue(task);
    }
}

//! Wakers that are nothing but a pointer to a task header.
//!
//! Rust's `Waker` is a `(data pointer, vtable)` pair. Ours uses the task
//! header as the data pointer and a vtable whose `wake` calls
//! [`wake_task`]. No allocation, no reference counting: cloning copies the
//! pointer, dropping does nothing, because tasks live in static storage.

use super::{TaskRef, wake_task};
use core::task::{RawWaker, RawWakerVTable, Waker};

static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake, drop);

unsafe fn clone(p: *const ()) -> RawWaker {
    RawWaker::new(p, &VTABLE)
}

unsafe fn wake(p: *const ()) {
    // SAFETY: `p` was produced by `from_task`.
    wake_task(unsafe { TaskRef::from_ptr(p as *const super::TaskHeader) })
}

unsafe fn drop(_: *const ()) {}

/// Make the waker for `task`.
pub(crate) fn from_task(task: TaskRef) -> Waker {
    // SAFETY: the vtable functions uphold the RawWaker contract.
    unsafe { Waker::from_raw(RawWaker::new(task.as_ptr() as *const (), &VTABLE)) }
}

/// The task behind a waker handed out by this executor.
///
/// # Panics
/// If the waker was not created by pi5-executor (e.g. the no-op waker of a
/// `block_on`): timers and `Spawner::for_current_executor` only work inside
/// executor tasks.
pub fn task_from_waker(waker: &Waker) -> TaskRef {
    if !core::ptr::eq(waker.vtable(), &VTABLE) {
        panic!("this waker is not from pi5-executor: Timer/Spawner::for_current_executor need an executor task");
    }
    // SAFETY: a waker with our vtable carries a task header pointer.
    unsafe { TaskRef::from_ptr(waker.data() as *const super::TaskHeader) }
}

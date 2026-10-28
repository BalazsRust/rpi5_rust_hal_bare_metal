//! An async mutex: `lock().await` instead of spinning, so other tasks run
//! while you wait. Hold the guard across `.await` points freely.

use crate::blocking_mutex::Mutex as BlockingMutex;
use crate::blocking_mutex::raw::RawMutex;
use crate::waitqueue::MultiWakerRegistration;
use core::cell::{RefCell, UnsafeCell};
use core::future::{Future, poll_fn};
use core::ops::{Deref, DerefMut};
use core::task::Poll;

struct State {
    locked: bool,
    waiters: MultiWakerRegistration<8>,
}

/// Async mutual exclusion.
pub struct Mutex<M: RawMutex, T: ?Sized> {
    state: BlockingMutex<M, RefCell<State>>,
    inner: UnsafeCell<T>,
}

// SAFETY: `inner` is only reachable through a guard, handed out one at a time.
unsafe impl<M: RawMutex + Send, T: ?Sized + Send> Send for Mutex<M, T> {}
unsafe impl<M: RawMutex + Sync, T: ?Sized + Send> Sync for Mutex<M, T> {}

impl<M: RawMutex, T> Mutex<M, T> {
    /// Wrap `value`.
    pub const fn new(value: T) -> Self {
        Self {
            state: BlockingMutex::new(RefCell::new(State { locked: false, waiters: MultiWakerRegistration::new() })),
            inner: UnsafeCell::new(value),
        }
    }

    /// Unwrap.
    pub fn into_inner(self) -> T {
        self.inner.into_inner()
    }
}

/// Could not lock right now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TryLockError;

impl<M: RawMutex, T: ?Sized> Mutex<M, T> {
    /// Lock, waiting (asynchronously) if someone else holds it.
    pub fn lock(&self) -> impl Future<Output = MutexGuard<'_, M, T>> {
        poll_fn(move |cx| {
            let got_it = self.state.lock(|s| {
                let mut s = s.borrow_mut();
                if s.locked {
                    s.waiters.register(cx.waker());
                    false
                } else {
                    s.locked = true;
                    true
                }
            });
            if got_it { Poll::Ready(MutexGuard { mutex: self }) } else { Poll::Pending }
        })
    }

    /// Lock without waiting.
    pub fn try_lock(&self) -> Result<MutexGuard<'_, M, T>, TryLockError> {
        let got_it = self.state.lock(|s| {
            let mut s = s.borrow_mut();
            if s.locked {
                false
            } else {
                s.locked = true;
                true
            }
        });
        if got_it { Ok(MutexGuard { mutex: self }) } else { Err(TryLockError) }
    }

    /// Mutable access without locking: `&mut self` proves exclusivity.
    pub fn get_mut(&mut self) -> &mut T {
        self.inner.get_mut()
    }
}

/// Holds the lock while alive.
pub struct MutexGuard<'a, M: RawMutex, T: ?Sized> {
    mutex: &'a Mutex<M, T>,
}

impl<M: RawMutex, T: ?Sized> Drop for MutexGuard<'_, M, T> {
    fn drop(&mut self) {
        self.mutex.state.lock(|s| {
            let mut s = s.borrow_mut();
            s.locked = false;
            s.waiters.wake();
        })
    }
}

impl<M: RawMutex, T: ?Sized> Deref for MutexGuard<'_, M, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: we hold the lock.
        unsafe { &*self.mutex.inner.get() }
    }
}

impl<M: RawMutex, T: ?Sized> DerefMut for MutexGuard<'_, M, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: we hold the lock.
        unsafe { &mut *self.mutex.inner.get() }
    }
}

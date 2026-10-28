//! A spinlock for the HAL's own shared registers.
//!
//! Some registers are shared by things that may be used from different cores
//! at once: the RP1 clock block, a PWM block's `GLOBAL_CTRL`. A read-modify-
//! write of such a register from two cores could lose one core's update, so
//! those paths take this lock.
//!
//! The lock is an `AtomicBool` and `compare_exchange`, which compiles to
//! `ldaxrb`/`stlxrb` (or `casab` with LSE). Those instructions only work on
//! Normal cacheable memory, i.e. **after the MMU is on** ([`crate::mmu`]).
//! Interrupts are always masked in this crate, so there is no need to
//! disable them while holding the lock.

use core::cell::UnsafeCell;
use core::ops::{Deref, DerefMut};
use core::sync::atomic::{AtomicBool, Ordering};

/// A value protected by a spinlock.
pub struct SpinLock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

// SAFETY: access to `value` is serialised by `locked`.
unsafe impl<T: Send> Sync for SpinLock<T> {}
unsafe impl<T: Send> Send for SpinLock<T> {}

impl<T> SpinLock<T> {
    /// A new, unlocked lock.
    pub const fn new(value: T) -> Self {
        Self { locked: AtomicBool::new(false), value: UnsafeCell::new(value) }
    }

    /// Spin until the lock is free, then take it. Released when the guard drops.
    pub fn lock(&self) -> SpinLockGuard<'_, T> {
        // Acquire: everything the previous holder wrote is visible to us.
        while self.locked.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
            while self.locked.load(Ordering::Relaxed) {
                core::hint::spin_loop();
            }
        }
        SpinLockGuard { lock: self }
    }

    /// Try once; `None` if someone else holds it.
    pub fn try_lock(&self) -> Option<SpinLockGuard<'_, T>> {
        self.locked
            .compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed)
            .ok()
            .map(|_| SpinLockGuard { lock: self })
    }
}

/// Holds the lock while alive.
pub struct SpinLockGuard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<T> Deref for SpinLockGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        // SAFETY: we hold the lock.
        unsafe { &*self.lock.value.get() }
    }
}

impl<T> DerefMut for SpinLockGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        // SAFETY: we hold the lock.
        unsafe { &mut *self.lock.value.get() }
    }
}

impl<T> Drop for SpinLockGuard<'_, T> {
    fn drop(&mut self) {
        // Release: everything we wrote is visible to the next holder.
        self.lock.locked.store(false, Ordering::Release);
    }
}

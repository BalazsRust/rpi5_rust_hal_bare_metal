//! Blocking (non-async) mutexes, and the `RawMutex` strategies.

use core::cell::UnsafeCell;

/// The locking strategies.
pub mod raw {
    use core::arch::asm;
    use core::marker::PhantomData;
    use core::sync::atomic::{AtomicUsize, Ordering};

    /// A way to run a closure with exclusive access.
    pub trait RawMutex {
        /// A fresh, unlocked instance (`const` so it can live in a `static`).
        const INIT: Self;
        /// Run `f` while holding the lock.
        fn lock<R>(&self, f: impl FnOnce() -> R) -> R;
    }

    /// One global spinlock for the whole program, re-entrant per core.
    ///
    /// "Critical section" on a machine without interrupts means "no other
    /// core gets in". The owner is recorded as `core id + 1` so that nested
    /// `lock()` calls on the same core do not deadlock.
    pub struct CriticalSectionRawMutex {
        _not_unpin: PhantomData<()>,
    }

    static CS_OWNER: AtomicUsize = AtomicUsize::new(0);
    static CS_DEPTH: AtomicUsize = AtomicUsize::new(0);

    fn core_id() -> usize {
        let mpidr: u64;
        // SAFETY: MPIDR_EL1 is read-only. Core number = Aff1 on the BCM2712.
        unsafe { asm!("mrs {}, mpidr_el1", out(reg) mpidr, options(nomem, nostack, preserves_flags)) };
        ((mpidr >> 8) & 0xff) as usize
    }

    impl CriticalSectionRawMutex {
        /// A new instance (they all share the one global lock).
        pub const fn new() -> Self {
            Self { _not_unpin: PhantomData }
        }
    }

    impl Default for CriticalSectionRawMutex {
        fn default() -> Self {
            Self::new()
        }
    }

    // SAFETY: the global lock serialises access from all cores.
    unsafe impl Send for CriticalSectionRawMutex {}
    unsafe impl Sync for CriticalSectionRawMutex {}

    impl RawMutex for CriticalSectionRawMutex {
        const INIT: Self = Self::new();

        fn lock<R>(&self, f: impl FnOnce() -> R) -> R {
            let me = core_id() + 1;
            if CS_OWNER.load(Ordering::Acquire) == me {
                // Re-entrant: already ours.
                CS_DEPTH.fetch_add(1, Ordering::Relaxed);
                let r = f();
                CS_DEPTH.fetch_sub(1, Ordering::Relaxed);
                return r;
            }
            while CS_OWNER.compare_exchange_weak(0, me, Ordering::Acquire, Ordering::Relaxed).is_err() {
                core::hint::spin_loop();
            }
            let r = f();
            CS_OWNER.store(0, Ordering::Release);
            r
        }
    }

    /// No locking at all. The resulting primitives are `!Sync`, so they can
    /// only be used from one core; the type system enforces it.
    pub struct NoopRawMutex {
        _not_sync: PhantomData<*mut ()>,
    }

    impl NoopRawMutex {
        /// A new instance.
        pub const fn new() -> Self {
            Self { _not_sync: PhantomData }
        }
    }

    impl Default for NoopRawMutex {
        fn default() -> Self {
            Self::new()
        }
    }

    // SAFETY: Send is fine (moving it is harmless); it is deliberately !Sync.
    unsafe impl Send for NoopRawMutex {}

    impl RawMutex for NoopRawMutex {
        const INIT: Self = Self::new();
        fn lock<R>(&self, f: impl FnOnce() -> R) -> R {
            f()
        }
    }
}

use raw::RawMutex;

/// Data protected by a [`RawMutex`]. Access is through a closure, so the
/// lock can never be forgotten open.
pub struct Mutex<R: RawMutex, T: ?Sized> {
    raw: R,
    data: UnsafeCell<T>,
}

// SAFETY: access to `data` is serialised by `raw`; whether that is enough
// across cores is decided by `R: Sync` (CriticalSection: yes, Noop: no).
unsafe impl<R: RawMutex + Send, T: ?Sized + Send> Send for Mutex<R, T> {}
unsafe impl<R: RawMutex + Sync, T: ?Sized + Send> Sync for Mutex<R, T> {}

impl<R: RawMutex, T> Mutex<R, T> {
    /// Wrap `val`.
    pub const fn new(val: T) -> Self {
        Self { raw: R::INIT, data: UnsafeCell::new(val) }
    }

    /// Run `f` with shared access to the data while holding the lock. Put a
    /// `RefCell` or `Cell` inside if you need to mutate.
    pub fn lock<U>(&self, f: impl FnOnce(&T) -> U) -> U {
        self.raw.lock(|| {
            // SAFETY: the lock is held.
            let data = unsafe { &*self.data.get() };
            f(data)
        })
    }

    /// Mutable access without locking: `&mut self` proves exclusivity.
    pub fn get_mut(&mut self) -> &mut T {
        self.data.get_mut()
    }

    /// Unwrap.
    pub fn into_inner(self) -> T {
        self.data.into_inner()
    }
}

/// `Mutex` with the cross-core strategy.
pub type CriticalSectionMutex<T> = Mutex<raw::CriticalSectionRawMutex, T>;
/// `Mutex` with no locking (single core).
pub type NoopMutex<T> = Mutex<raw::NoopRawMutex, T>;

//! Places to park wakers.

use core::task::Waker;

/// Room for one waker. Registering a new one replaces the old (waking it, so
/// no task is left hanging).
#[derive(Debug, Default)]
pub struct WakerRegistration {
    waker: Option<Waker>,
}

impl WakerRegistration {
    /// Empty.
    pub const fn new() -> Self {
        Self { waker: None }
    }

    /// Store `w`.
    pub fn register(&mut self, w: &Waker) {
        match &self.waker {
            Some(old) if old.will_wake(w) => {}
            _ => {
                if let Some(old) = self.waker.replace(w.clone()) {
                    old.wake();
                }
            }
        }
    }

    /// Wake and clear.
    pub fn wake(&mut self) {
        if let Some(w) = self.waker.take() {
            w.wake();
        }
    }

    /// Is a waker stored?
    pub fn occupied(&self) -> bool {
        self.waker.is_some()
    }
}

/// Room for up to `N` wakers (several tasks waiting on the same thing). If
/// it is full, everyone is woken and the list cleared: a spurious wake-up is
/// harmless, a lost one is not.
#[derive(Debug)]
pub struct MultiWakerRegistration<const N: usize> {
    wakers: [Option<Waker>; N],
}

impl<const N: usize> Default for MultiWakerRegistration<N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<const N: usize> MultiWakerRegistration<N> {
    /// Empty.
    pub const fn new() -> Self {
        Self { wakers: [const { None }; N] }
    }

    /// Store `w` unless an equivalent waker is already stored.
    pub fn register(&mut self, w: &Waker) {
        if self.wakers.iter().flatten().any(|old| old.will_wake(w)) {
            return;
        }
        if let Some(slot) = self.wakers.iter_mut().find(|s| s.is_none()) {
            *slot = Some(w.clone());
        } else {
            self.wake();
            self.wakers[0] = Some(w.clone());
        }
    }

    /// Wake everyone and clear.
    pub fn wake(&mut self) {
        for slot in &mut self.wakers {
            if let Some(w) = slot.take() {
                w.wake();
            }
        }
    }
}

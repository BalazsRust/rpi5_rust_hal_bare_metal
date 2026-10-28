//! A single-slot "latest value wins" notification.

use crate::blocking_mutex::Mutex;
use crate::blocking_mutex::raw::RawMutex;
use core::cell::Cell;
use core::future::{Future, poll_fn};
use core::task::{Poll, Waker};

enum State<T> {
    None,
    Waiting(Waker),
    Signaled(T),
}

/// One value, overwritten by each `signal`, consumed by one `wait`.
pub struct Signal<M: RawMutex, T> {
    state: Mutex<M, Cell<State<T>>>,
}

impl<M: RawMutex, T> Default for Signal<M, T> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: RawMutex, T> Signal<M, T> {
    /// Not signalled.
    pub const fn new() -> Self {
        Self { state: Mutex::new(Cell::new(State::None)) }
    }

    /// Store `val`, waking a waiting task. A previous unconsumed value is replaced.
    pub fn signal(&self, val: T) {
        self.state.lock(|cell| {
            if let State::Waiting(w) = cell.replace(State::Signaled(val)) {
                w.wake();
            }
        })
    }

    /// Forget any stored value.
    pub fn reset(&self) {
        self.state.lock(|cell| cell.set(State::None));
    }

    /// Wait for a value.
    pub fn wait(&self) -> impl Future<Output = T> + '_ {
        poll_fn(move |cx| {
            self.state.lock(|cell| match cell.replace(State::None) {
                State::Signaled(v) => Poll::Ready(v),
                _ => {
                    cell.set(State::Waiting(cx.waker().clone()));
                    Poll::Pending
                }
            })
        })
    }

    /// Take the value if there is one, without waiting.
    pub fn try_take(&self) -> Option<T> {
        self.state.lock(|cell| match cell.replace(State::None) {
            State::Signaled(v) => Some(v),
            other => {
                cell.set(other);
                None
            }
        })
    }

    /// Is a value stored?
    pub fn signaled(&self) -> bool {
        self.state.lock(|cell| {
            let s = cell.replace(State::None);
            let r = matches!(s, State::Signaled(_));
            cell.set(s);
            r
        })
    }
}

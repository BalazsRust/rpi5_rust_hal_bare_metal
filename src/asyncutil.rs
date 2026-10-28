//! Helpers that let every driver be written once, as `async`, and offer a
//! blocking API on top.
//!
//! No interrupts are enabled in this crate, so an `async` driver cannot be
//! woken by hardware. Instead, when a driver has to wait for a FIFO flag it
//! *yields*: it wakes its own task immediately and returns `Pending`, letting
//! the executor run other tasks before polling it again. That is polling,
//! but cooperative polling that keeps every other task alive.
//!
//! The blocking methods simply run the same future to completion with a
//! do-nothing waker ([`block_on`]): each `Pending` becomes one more spin of
//! the loop.

use core::future::{Future, poll_fn};
use core::pin::pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

/// Return `Pending` once (waking ourselves), `Ready` the second time.
pub(crate) fn yield_now() -> impl Future<Output = ()> {
    let mut yielded = false;
    poll_fn(move |cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
}

/// Poll `ready()` until it returns `true`, yielding between attempts.
pub(crate) fn wait_until(mut ready: impl FnMut() -> bool) -> impl Future<Output = ()> {
    poll_fn(move |cx| {
        if ready() {
            Poll::Ready(())
        } else {
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
}

/// Drive a future to completion by polling it in a loop. Used to implement
/// the blocking API on top of the async one.
pub(crate) fn block_on<F: Future>(fut: F) -> F::Output {
    let mut fut = pin!(fut);
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    loop {
        if let Poll::Ready(v) = fut.as_mut().poll(&mut cx) {
            return v;
        }
        core::hint::spin_loop();
    }
}

fn noop_waker() -> Waker {
    const VTABLE: RawWakerVTable = RawWakerVTable::new(|_| RawWaker::new(core::ptr::null(), &VTABLE), |_| {}, |_| {}, |_| {});
    // SAFETY: the vtable functions do nothing and own nothing.
    unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
}

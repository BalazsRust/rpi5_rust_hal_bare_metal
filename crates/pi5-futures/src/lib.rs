//! # pi5-futures: combinators, `embassy-futures` shaped
//!
//! * [`select()`]: race two futures, get whichever finishes first.
//! * [`join()`]: run two futures concurrently, get both results.
//! * [`yield_now`]: let other tasks run, then continue.
//! * [`block_on`]: run a future to completion by spinning (no executor).
//!
//! None of this needs an allocator or the executor; it is plain `Future`
//! plumbing. Worth reading [`select()`]'s implementation: it is the whole idea
//! of async in 20 lines.

#![no_std]
#![warn(missing_docs)]

pub mod join;
pub mod select;

pub use join::{join, join3};
pub use select::{Either, Either3, select, select3};

use core::future::Future;
use core::pin::pin;
use core::task::{Context, Poll, RawWaker, RawWakerVTable, Waker};

/// Return `Pending` once (waking ourselves), `Ready` the second time. Lets
/// other tasks on the same executor run in between.
pub fn yield_now() -> impl Future<Output = ()> {
    let mut yielded = false;
    core::future::poll_fn(move |cx| {
        if yielded {
            Poll::Ready(())
        } else {
            yielded = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        }
    })
}

/// Poll `fut` in a busy loop until it completes. Use outside tasks (e.g. in
/// a plain `pi5_hal::entry!` program). Futures that need an executor
/// (`pi5_time::Timer`) will panic here.
pub fn block_on<F: Future>(fut: F) -> F::Output {
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

/// Poll `fut` exactly once.
pub fn poll_once<F: Future>(fut: F) -> Poll<F::Output> {
    let mut fut = pin!(fut);
    let waker = noop_waker();
    let mut cx = Context::from_waker(&waker);
    fut.as_mut().poll(&mut cx)
}

fn noop_waker() -> Waker {
    const VTABLE: RawWakerVTable = RawWakerVTable::new(|_| RawWaker::new(core::ptr::null(), &VTABLE), |_| {}, |_| {}, |_| {});
    // SAFETY: the vtable does nothing.
    unsafe { Waker::from_raw(RawWaker::new(core::ptr::null(), &VTABLE)) }
}

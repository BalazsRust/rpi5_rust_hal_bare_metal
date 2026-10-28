//! Run futures concurrently and wait for all of them.

use core::future::Future;
use core::mem;
use core::pin::Pin;
use core::task::{Context, Poll};

/// A future that remembers its output once it has completed.
enum MaybeDone<F: Future> {
    Future(F),
    Done(F::Output),
    Gone,
}

impl<F: Future> MaybeDone<F> {
    /// Poll if still running; `true` once the output is available.
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> bool {
        // SAFETY: the `Future` variant is structurally pinned; `Done`/`Gone`
        // hold no pinned data.
        let this = unsafe { self.get_unchecked_mut() };
        match this {
            MaybeDone::Future(f) => {
                let f = unsafe { Pin::new_unchecked(f) };
                match f.poll(cx) {
                    Poll::Ready(out) => {
                        *this = MaybeDone::Done(out);
                        true
                    }
                    Poll::Pending => false,
                }
            }
            MaybeDone::Done(_) => true,
            MaybeDone::Gone => panic!("MaybeDone polled after output was taken"),
        }
    }

    fn take(&mut self) -> F::Output {
        match mem::replace(self, MaybeDone::Gone) {
            MaybeDone::Done(out) => out,
            _ => unreachable!(),
        }
    }
}

/// Wait for both futures; returns both outputs.
pub fn join<A: Future, B: Future>(a: A, b: B) -> Join<A, B> {
    Join { a: MaybeDone::Future(a), b: MaybeDone::Future(b) }
}

/// The future returned by [`join`].
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Join<A: Future, B: Future> {
    a: MaybeDone<A>,
    b: MaybeDone<B>,
}

impl<A: Future, B: Future> Future for Join<A, B> {
    type Output = (A::Output, B::Output);
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: structural pinning.
        let this = unsafe { self.get_unchecked_mut() };
        let mut all_done = true;
        all_done &= unsafe { Pin::new_unchecked(&mut this.a) }.poll(cx);
        all_done &= unsafe { Pin::new_unchecked(&mut this.b) }.poll(cx);
        if all_done { Poll::Ready((this.a.take(), this.b.take())) } else { Poll::Pending }
    }
}

/// Wait for three futures.
pub fn join3<A: Future, B: Future, C: Future>(a: A, b: B, c: C) -> Join3<A, B, C> {
    Join3 { a: MaybeDone::Future(a), b: MaybeDone::Future(b), c: MaybeDone::Future(c) }
}

/// The future returned by [`join3`].
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Join3<A: Future, B: Future, C: Future> {
    a: MaybeDone<A>,
    b: MaybeDone<B>,
    c: MaybeDone<C>,
}

impl<A: Future, B: Future, C: Future> Future for Join3<A, B, C> {
    type Output = (A::Output, B::Output, C::Output);
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: structural pinning.
        let this = unsafe { self.get_unchecked_mut() };
        let mut all_done = true;
        all_done &= unsafe { Pin::new_unchecked(&mut this.a) }.poll(cx);
        all_done &= unsafe { Pin::new_unchecked(&mut this.b) }.poll(cx);
        all_done &= unsafe { Pin::new_unchecked(&mut this.c) }.poll(cx);
        if all_done { Poll::Ready((this.a.take(), this.b.take(), this.c.take())) } else { Poll::Pending }
    }
}

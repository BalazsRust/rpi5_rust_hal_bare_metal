//! Race futures against each other.

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};

/// Result of [`select`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Either<A, B> {
    /// The first future finished first.
    First(A),
    /// The second future finished first.
    Second(B),
}

impl<A, B> Either<A, B> {
    /// Did the first future win?
    pub fn is_first(&self) -> bool {
        matches!(self, Either::First(_))
    }
    /// Did the second future win?
    pub fn is_second(&self) -> bool {
        matches!(self, Either::Second(_))
    }
}

/// Wait for whichever of two futures completes first; the other is dropped
/// (cancelled) when the `Select` is.
///
/// Both futures are polled every time the task is woken; whichever says
/// `Ready` first wins. That is all "racing" means in async Rust.
pub fn select<A: Future, B: Future>(a: A, b: B) -> Select<A, B> {
    Select { a, b }
}

/// The future returned by [`select`].
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Select<A, B> {
    a: A,
    b: B,
}

impl<A: Future, B: Future> Future for Select<A, B> {
    type Output = Either<A::Output, B::Output>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: structural pinning: the fields are never moved out of a
        // pinned Select.
        let this = unsafe { self.get_unchecked_mut() };
        let a = unsafe { Pin::new_unchecked(&mut this.a) };
        let b = unsafe { Pin::new_unchecked(&mut this.b) };
        if let Poll::Ready(x) = a.poll(cx) {
            return Poll::Ready(Either::First(x));
        }
        if let Poll::Ready(x) = b.poll(cx) {
            return Poll::Ready(Either::Second(x));
        }
        Poll::Pending
    }
}

/// Result of [`select3`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Either3<A, B, C> {
    /// First.
    First(A),
    /// Second.
    Second(B),
    /// Third.
    Third(C),
}

/// Three-way [`select`].
pub fn select3<A: Future, B: Future, C: Future>(a: A, b: B, c: C) -> Select3<A, B, C> {
    Select3 { a, b, c }
}

/// The future returned by [`select3`].
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Select3<A, B, C> {
    a: A,
    b: B,
    c: C,
}

impl<A: Future, B: Future, C: Future> Future for Select3<A, B, C> {
    type Output = Either3<A::Output, B::Output, C::Output>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: structural pinning, as in `Select`.
        let this = unsafe { self.get_unchecked_mut() };
        if let Poll::Ready(x) = unsafe { Pin::new_unchecked(&mut this.a) }.poll(cx) {
            return Poll::Ready(Either3::First(x));
        }
        if let Poll::Ready(x) = unsafe { Pin::new_unchecked(&mut this.b) }.poll(cx) {
            return Poll::Ready(Either3::Second(x));
        }
        if let Poll::Ready(x) = unsafe { Pin::new_unchecked(&mut this.c) }.poll(cx) {
            return Poll::Ready(Either3::Third(x));
        }
        Poll::Pending
    }
}

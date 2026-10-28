//! `Timer`, `Ticker`, `with_timeout`.

use super::{Duration, Instant};
use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use pi5_executor::time_driver::schedule_wake;

/// A future that completes at a given instant.
///
/// How it works: on the first poll it registers its deadline with the
/// executor's timer queue (`schedule_wake`) and returns `Pending`. When the
/// executor sees the deadline has passed it wakes the task; the next poll
/// finds `now >= expires_at` and returns `Ready`. A timer whose deadline has
/// already passed still yields once (`yielded_once`), so `Timer::after(0)`
/// behaves like `yield_now()` and never starves other tasks.
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct Timer {
    expires_at: Instant,
    yielded_once: bool,
}

impl Timer {
    /// Complete at `expires_at`.
    pub fn at(expires_at: Instant) -> Self {
        Self { expires_at, yielded_once: false }
    }
    /// Complete after `duration`.
    pub fn after(duration: Duration) -> Self {
        Self { expires_at: Instant::now().saturating_add(duration), yielded_once: false }
    }
    /// Complete after `ticks` ticks.
    pub fn after_ticks(ticks: u64) -> Self {
        Self::after(Duration::from_ticks(ticks))
    }
    /// Complete after `nanos` nanoseconds.
    pub fn after_nanos(nanos: u64) -> Self {
        Self::after(Duration::from_nanos(nanos))
    }
    /// Complete after `micros` microseconds.
    pub fn after_micros(micros: u64) -> Self {
        Self::after(Duration::from_micros(micros))
    }
    /// Complete after `millis` milliseconds.
    pub fn after_millis(millis: u64) -> Self {
        Self::after(Duration::from_millis(millis))
    }
    /// Complete after `secs` seconds.
    pub fn after_secs(secs: u64) -> Self {
        Self::after(Duration::from_secs(secs))
    }
}

impl Unpin for Timer {}

impl Future for Timer {
    type Output = ();
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        if self.yielded_once && self.expires_at <= Instant::now() {
            Poll::Ready(())
        } else {
            schedule_wake(self.expires_at.as_ticks(), cx.waker());
            self.yielded_once = true;
            Poll::Pending
        }
    }
}

/// A periodic timer that does not drift: each tick is scheduled relative to
/// the previous *deadline*, not to when the task got around to running.
pub struct Ticker {
    expires_at: Instant,
    duration: Duration,
}

impl Ticker {
    /// Tick every `duration`, starting one `duration` from now.
    pub fn every(duration: Duration) -> Self {
        Self { expires_at: Instant::now() + duration, duration }
    }
    /// Restart the period from now.
    pub fn reset(&mut self) {
        self.expires_at = Instant::now() + self.duration;
    }
    /// Make the next tick happen at `deadline`.
    pub fn reset_at(&mut self, deadline: Instant) {
        self.expires_at = deadline;
    }
    /// Make the next tick happen `after` from now.
    pub fn reset_after(&mut self, after: Duration) {
        self.expires_at = Instant::now() + after;
    }
    /// Wait for the next tick.
    #[allow(clippy::should_implement_trait)] // same name as embassy's Ticker::next
    pub fn next(&mut self) -> impl Future<Output = ()> + '_ {
        core::future::poll_fn(|cx| {
            if self.expires_at <= Instant::now() {
                let dur = self.duration;
                self.expires_at += dur;
                Poll::Ready(())
            } else {
                schedule_wake(self.expires_at.as_ticks(), cx.waker());
                Poll::Pending
            }
        })
    }
}

/// The future given to [`with_timeout`] did not finish in time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimeoutError;

impl core::fmt::Display for TimeoutError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "timeout")
    }
}

/// Run `fut`, but give up after `timeout`.
pub fn with_timeout<F: Future>(timeout: Duration, fut: F) -> TimeoutFuture<F> {
    TimeoutFuture { fut, timer: Timer::after(timeout) }
}

/// Run `fut`, but give up at `at`.
pub fn with_deadline<F: Future>(at: Instant, fut: F) -> TimeoutFuture<F> {
    TimeoutFuture { fut, timer: Timer::at(at) }
}

/// The future returned by [`with_timeout`] / [`with_deadline`].
#[must_use = "futures do nothing unless you `.await` or poll them"]
pub struct TimeoutFuture<F> {
    fut: F,
    timer: Timer,
}

impl<F: Future> Future for TimeoutFuture<F> {
    type Output = Result<F::Output, TimeoutError>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        // SAFETY: structural pinning; neither field is moved out.
        let this = unsafe { self.get_unchecked_mut() };
        let fut = unsafe { Pin::new_unchecked(&mut this.fut) };
        if let Poll::Ready(v) = fut.poll(cx) {
            return Poll::Ready(Ok(v));
        }
        if Pin::new(&mut this.timer).poll(cx).is_ready() {
            return Poll::Ready(Err(TimeoutError));
        }
        Poll::Pending
    }
}

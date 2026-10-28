//! A bounded multi-producer multi-consumer queue with async `send`/`receive`.

use crate::blocking_mutex::Mutex;
use crate::blocking_mutex::raw::RawMutex;
use crate::waitqueue::MultiWakerRegistration;
use core::cell::RefCell;
use core::future::{Future, poll_fn};
use core::mem::MaybeUninit;
use core::task::Poll;

/// Fixed-capacity ring buffer.
struct Queue<T, const N: usize> {
    buf: [MaybeUninit<T>; N],
    head: usize,
    len: usize,
}

impl<T, const N: usize> Queue<T, N> {
    const fn new() -> Self {
        Self { buf: [const { MaybeUninit::uninit() }; N], head: 0, len: 0 }
    }
    fn push(&mut self, v: T) -> Result<(), T> {
        if self.len == N {
            return Err(v);
        }
        let idx = (self.head + self.len) % N;
        self.buf[idx].write(v);
        self.len += 1;
        Ok(())
    }
    fn pop(&mut self) -> Option<T> {
        if self.len == 0 {
            return None;
        }
        // SAFETY: slot `head` was written by `push` and not yet taken.
        let v = unsafe { self.buf[self.head].assume_init_read() };
        self.head = (self.head + 1) % N;
        self.len -= 1;
        Some(v)
    }
}

impl<T, const N: usize> Drop for Queue<T, N> {
    fn drop(&mut self) {
        while self.pop().is_some() {}
    }
}

struct State<T, const N: usize> {
    queue: Queue<T, N>,
    receivers: MultiWakerRegistration<8>,
    senders: MultiWakerRegistration<8>,
}

/// Returned by [`Channel::try_send`] when the channel is full.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TrySendError<T>(pub T);

/// Returned by [`Channel::try_receive`] when the channel is empty.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TryReceiveError;

/// A channel holding up to `N` messages of type `T`.
pub struct Channel<M: RawMutex, T, const N: usize> {
    inner: Mutex<M, RefCell<State<T, N>>>,
}

impl<M: RawMutex, T, const N: usize> Default for Channel<M, T, N> {
    fn default() -> Self {
        Self::new()
    }
}

impl<M: RawMutex, T, const N: usize> Channel<M, T, N> {
    /// An empty channel.
    pub const fn new() -> Self {
        Self {
            inner: Mutex::new(RefCell::new(State {
                queue: Queue::new(),
                receivers: MultiWakerRegistration::new(),
                senders: MultiWakerRegistration::new(),
            })),
        }
    }

    fn with<R>(&self, f: impl FnOnce(&mut State<T, N>) -> R) -> R {
        self.inner.lock(|s| f(&mut s.borrow_mut()))
    }

    /// Send, waiting while the channel is full.
    pub fn send(&self, message: T) -> impl Future<Output = ()> + '_ {
        let mut message = Some(message);
        poll_fn(move |cx| {
            self.with(|s| match s.queue.push(message.take().expect("polled after completion")) {
                Ok(()) => {
                    s.receivers.wake();
                    Poll::Ready(())
                }
                Err(m) => {
                    message = Some(m);
                    s.senders.register(cx.waker());
                    Poll::Pending
                }
            })
        })
    }

    /// Send without waiting.
    pub fn try_send(&self, message: T) -> Result<(), TrySendError<T>> {
        self.with(|s| match s.queue.push(message) {
            Ok(()) => {
                s.receivers.wake();
                Ok(())
            }
            Err(m) => Err(TrySendError(m)),
        })
    }

    /// Receive, waiting while the channel is empty.
    pub fn receive(&self) -> impl Future<Output = T> + '_ {
        poll_fn(move |cx| {
            self.with(|s| match s.queue.pop() {
                Some(v) => {
                    s.senders.wake();
                    Poll::Ready(v)
                }
                None => {
                    s.receivers.register(cx.waker());
                    Poll::Pending
                }
            })
        })
    }

    /// Receive without waiting.
    pub fn try_receive(&self) -> Result<T, TryReceiveError> {
        self.with(|s| match s.queue.pop() {
            Some(v) => {
                s.senders.wake();
                Ok(v)
            }
            None => Err(TryReceiveError),
        })
    }

    /// Messages currently queued.
    pub fn len(&self) -> usize {
        self.with(|s| s.queue.len)
    }
    /// Is it empty?
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    /// Is it full?
    pub fn is_full(&self) -> bool {
        self.len() == N
    }
    /// Capacity.
    pub const fn capacity(&self) -> usize {
        N
    }
    /// Drop every queued message.
    pub fn clear(&self) {
        self.with(|s| {
            while s.queue.pop().is_some() {}
            s.senders.wake();
        })
    }

    /// A send-only handle.
    pub fn sender(&self) -> Sender<'_, M, T, N> {
        Sender { channel: self }
    }
    /// A receive-only handle.
    pub fn receiver(&self) -> Receiver<'_, M, T, N> {
        Receiver { channel: self }
    }
}

/// Send-only view of a [`Channel`].
pub struct Sender<'ch, M: RawMutex, T, const N: usize> {
    channel: &'ch Channel<M, T, N>,
}

impl<M: RawMutex, T, const N: usize> Clone for Sender<'_, M, T, N> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<M: RawMutex, T, const N: usize> Copy for Sender<'_, M, T, N> {}

impl<'ch, M: RawMutex, T, const N: usize> Sender<'ch, M, T, N> {
    /// See [`Channel::send`].
    pub fn send(&self, message: T) -> impl Future<Output = ()> + 'ch {
        self.channel.send(message)
    }
    /// See [`Channel::try_send`].
    pub fn try_send(&self, message: T) -> Result<(), TrySendError<T>> {
        self.channel.try_send(message)
    }
}

/// Receive-only view of a [`Channel`].
pub struct Receiver<'ch, M: RawMutex, T, const N: usize> {
    channel: &'ch Channel<M, T, N>,
}

impl<M: RawMutex, T, const N: usize> Clone for Receiver<'_, M, T, N> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<M: RawMutex, T, const N: usize> Copy for Receiver<'_, M, T, N> {}

impl<'ch, M: RawMutex, T, const N: usize> Receiver<'ch, M, T, N> {
    /// See [`Channel::receive`].
    pub fn receive(&self) -> impl Future<Output = T> + 'ch {
        self.channel.receive()
    }
    /// See [`Channel::try_receive`].
    pub fn try_receive(&self) -> Result<T, TryReceiveError> {
        self.channel.try_receive()
    }
}

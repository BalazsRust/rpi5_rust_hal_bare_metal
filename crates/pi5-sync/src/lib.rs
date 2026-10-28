//! # pi5-sync: talking between tasks (and cores)
//!
//! Shaped like `embassy-sync`:
//!
//! ```no_run
//! use pi5_sync::blocking_mutex::raw::CriticalSectionRawMutex;
//! use pi5_sync::channel::Channel;
//! use pi5_sync::signal::Signal;
//!
//! static EVENTS: Channel<CriticalSectionRawMutex, u32, 8> = Channel::new();
//! static DONE: Signal<CriticalSectionRawMutex, ()> = Signal::new();
//!
//! # async fn producer() {
//! EVENTS.send(42).await;
//! DONE.signal(());
//! # }
//! # async fn consumer() {
//! let v = EVENTS.receive().await;
//! DONE.wait().await;
//! # let _ = v;
//! # }
//! ```
//!
//! The `RawMutex` type parameter says how the primitive protects its state:
//!
//! * [`blocking_mutex::raw::CriticalSectionRawMutex`]: a global spinlock,
//!   safe between tasks on **different cores**. (There are no interrupts
//!   here, so a "critical section" only has to keep other cores out.)
//! * [`blocking_mutex::raw::NoopRawMutex`]: no locking; for data used by a
//!   single core only. `Channel<NoopRawMutex, ..>` is then `!Sync` and the
//!   compiler stops you from sharing it across cores.
//!
//! All of this needs atomics, i.e. the MMU on; pi5-hal's runtime does that.

#![no_std]
#![warn(missing_docs)]

pub mod blocking_mutex;
pub mod channel;
pub mod mutex;
pub mod signal;
pub mod waitqueue;

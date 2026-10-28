//! defmt / esp-println style logging: `info!`, `warn!`, `error!`, `debug!`,
//! `trace!`, plus `println!` / `print!`.
//!
//! ```ignore
//! use pi5_hal::{info, warn};
//! info!("x = {}, y = {:#x}", x, y);
//! warn!("battery low: {} mV", mv);
//! ```
//!
//! Output goes to RP1 UART0 (GPIO14 = TX, physical pin 8), the line wired to
//! the ESP32 bridge, so it shows up in the `cargo run` terminal:
//!
//! ```text
//! 0.004211 INFO  x = 3, y = 0x1f
//! 2.000937 WARN  battery low: 3300 mV
//! ```
//!
//! The number is seconds since power-on (the ARM generic timer), the level
//! is coloured like defmt/espflash does it.
//!
//! The format strings are ordinary Rust `core::fmt` ones (`{}`, `{:?}`,
//! `{:#x}`, ...), not defmt's `{=u8}` encoding: we send plain text, so no
//! decoder is needed on the PC.
//!
//! If the program has not set UART0 up, the first log line does it (115200
//! 8N1 on GPIO14/15). If it has, logging shares that UART at its baud rate;
//! a line from `info!` is never split by output from another core or an
//! interrupt handler that also logs.
//!
//! # Logging does not hold up async tasks
//!
//! A log line takes ~4 ms to send at 115200 baud. Under the pi5-executor,
//! `info!` copies the line into a 4 KB buffer, puts what fits into the
//! UART's 32-byte FIFO and returns at once; the executor sends the rest
//! while it is idle (before it sleeps). So
//! `loop { info!(..); Timer::after_millis(500).await }` really runs every
//! 500 ms instead of 500 ms + the time to print. Only if the buffer is full
//! does `info!` wait.
//!
//! Programs without the executor (plain `loop {}` with `Delay`) log the old
//! way: each call returns once its text is in the UART FIFO, so nothing is
//! ever left sitting in the buffer. A panic flushes the buffer before it
//! prints its message.

use crate::delay::Delay;
use core::fmt::{self, Write};
use core::sync::atomic::{AtomicBool, AtomicU8, Ordering};

/// Log levels, most important first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u8)]
pub enum Level {
    /// Something failed.
    Error = 1,
    /// Something looks wrong but we carry on.
    Warn = 2,
    /// Normal progress messages.
    Info = 3,
    /// Details for debugging.
    Debug = 4,
    /// Very detailed, high-volume output.
    Trace = 5,
}

impl Level {
    fn label(self) -> &'static str {
        // ANSI colours: red, yellow, green, blue, grey; then reset.
        match self {
            Level::Error => "\x1b[31mERROR\x1b[0m",
            Level::Warn => "\x1b[33mWARN \x1b[0m",
            Level::Info => "\x1b[32mINFO \x1b[0m",
            Level::Debug => "\x1b[34mDEBUG\x1b[0m",
            Level::Trace => "\x1b[90mTRACE\x1b[0m",
        }
    }
}

/// Messages less important than this are dropped. Default: everything.
static MAX_LEVEL: AtomicU8 = AtomicU8::new(Level::Trace as u8);

/// Show only messages at `level` or more important, e.g.
/// `set_max_level(Level::Info)` hides `debug!` and `trace!`.
pub fn set_max_level(level: Level) {
    MAX_LEVEL.store(level as u8, Ordering::Relaxed);
}

#[doc(hidden)]
pub fn enabled(level: Level) -> bool {
    level as u8 <= MAX_LEVEL.load(Ordering::Relaxed)
}

/// One lock for all cores. Interrupts are masked while it is held, so an
/// interrupt handler that logs can't deadlock against the code it
/// interrupted on the same core.
static LOCK: AtomicBool = AtomicBool::new(false);

/// Set once an executor has gone idle: from then on log output may stay in
/// the buffer, because the executor's idle loop sends it.
static BUFFERED: AtomicBool = AtomicBool::new(false);

const BUF_SIZE: usize = 4096;

/// Text waiting to go out on the UART: a ring buffer, only touched with
/// LOCK held.
struct Ring {
    buf: [u8; BUF_SIZE],
    /// Next byte to send.
    head: usize,
    /// Bytes waiting.
    len: usize,
}

static mut RING: Ring = Ring { buf: [0; BUF_SIZE], head: 0, len: 0 };

impl Ring {
    /// Move bytes into the UART FIFO; with `wait`, until the ring is empty.
    fn pump(&mut self, uart: &mut crate::uart::Uart<crate::Blocking>, wait: bool) {
        while self.len > 0 {
            if uart.try_push_byte(self.buf[self.head]) {
                self.head = (self.head + 1) % BUF_SIZE;
                self.len -= 1;
            } else if !wait {
                return;
            } else {
                core::hint::spin_loop();
            }
        }
    }

    fn push(&mut self, uart: &mut crate::uart::Uart<crate::Blocking>, b: u8) {
        if self.len == BUF_SIZE {
            // Full: wait for one FIFO slot (~87 us at 115200).
            while self.len == BUF_SIZE {
                self.pump(uart, false);
                core::hint::spin_loop();
            }
        }
        self.buf[(self.head + self.len) % BUF_SIZE] = b;
        self.len += 1;
    }
}

/// `fmt::Write` into the ring, `\n` -> `\r\n` like the UART does.
struct RingWriter<'a> {
    ring: &'a mut Ring,
    uart: &'a mut crate::uart::Uart<crate::Blocking>,
}

impl Write for RingWriter<'_> {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for &b in s.as_bytes() {
            if b == b'\n' {
                self.ring.push(self.uart, b'\r');
            }
            self.ring.push(self.uart, b);
        }
        Ok(())
    }
}

fn mask_irqs() -> u64 {
    let daif: u64;
    // SAFETY: reading DAIF and masking IRQ/FIQ at EL2 has no other effect.
    unsafe {
        core::arch::asm!("mrs {}, daif", out(reg) daif, options(nomem, nostack, preserves_flags));
        core::arch::asm!("msr daifset, #3", options(nomem, nostack, preserves_flags));
    }
    daif
}

fn restore_irqs(daif: u64) {
    // SAFETY: restores the caller's interrupt mask.
    unsafe { core::arch::asm!("msr daif, {}", in(reg) daif, options(nomem, nostack, preserves_flags)) };
}

fn with_console(f: impl FnOnce(&mut RingWriter<'_>)) {
    let daif = mask_irqs();
    while LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
    // SAFETY: the lock serialises every user of the console handle and RING.
    let mut uart = unsafe { crate::uart::Uart::<crate::Blocking>::console() };
    let ring = unsafe { &mut *core::ptr::addr_of_mut!(RING) };
    f(&mut RingWriter { ring, uart: &mut uart });
    // Under an executor, send what fits now and leave the rest for its idle
    // loop; otherwise everything must be in the FIFO before we return.
    ring.pump(&mut uart, !BUFFERED.load(Ordering::Relaxed));
    LOCK.store(false, Ordering::Release);
    restore_irqs(daif);
}

/// Send buffered log text that fits into the UART FIFO, without waiting.
/// The pi5-executor calls this (as `__pi5_executor_idle`) every time it has
/// nothing to run, right before it sleeps.
pub fn drain() {
    BUFFERED.store(true, Ordering::Relaxed);
    let daif = mask_irqs();
    // Another core is logging: it pumps too, no need to wait for it.
    if LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        // SAFETY: the lock is held.
        let ring = unsafe { &mut *core::ptr::addr_of_mut!(RING) };
        if ring.len > 0 {
            // SAFETY: as above.
            let mut uart = unsafe { crate::uart::Uart::<crate::Blocking>::console() };
            ring.pump(&mut uart, false);
        }
        LOCK.store(false, Ordering::Release);
    }
    restore_irqs(daif);
}

/// Hook called by the pi5-executor's idle loop.
#[doc(hidden)]
#[unsafe(no_mangle)]
pub extern "C" fn __pi5_executor_idle() {
    drain();
}

/// Send everything still buffered, waiting as long as it takes. For the
/// panic handler: takes the lock if it can within ~10 ms, and goes ahead
/// without it otherwise (the core holding it may be the one that panicked).
#[cfg_attr(not(feature = "panic-handler"), allow(dead_code))]
pub(crate) fn flush_for_panic() {
    let t = crate::delay::Timeout::new(core::time::Duration::from_millis(10));
    let mut locked = false;
    while !t.expired() {
        if LOCK.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
            locked = true;
            break;
        }
    }
    // SAFETY: we hold the lock, or gave up waiting for a core that is stuck.
    let ring = unsafe { &mut *core::ptr::addr_of_mut!(RING) };
    if ring.len > 0 {
        let mut uart = unsafe { crate::uart::Uart::<crate::Blocking>::console() };
        ring.pump(&mut uart, true);
    }
    if locked {
        LOCK.store(false, Ordering::Release);
    }
}

#[doc(hidden)]
pub fn log(level: Level, args: fmt::Arguments) {
    if !enabled(level) {
        return;
    }
    let ticks = Delay::now();
    let freq = Delay::frequency().max(1);
    let (secs, micros) = (ticks / freq, (ticks % freq) * 1_000_000 / freq);
    with_console(|w| {
        let _ = writeln!(w, "{secs}.{micros:06} {} {args}", level.label());
    });
}

#[doc(hidden)]
pub fn print(args: fmt::Arguments) {
    with_console(|w| {
        let _ = w.write_fmt(args);
    });
}

/// Log an error: `error!("sensor {} not found", addr)`.
#[macro_export]
macro_rules! error {
    ($($arg:tt)*) => { $crate::log::log($crate::log::Level::Error, format_args!($($arg)*)) };
}

/// Log a warning: `warn!("retrying ({}/3)", n)`.
#[macro_export]
macro_rules! warn {
    ($($arg:tt)*) => { $crate::log::log($crate::log::Level::Warn, format_args!($($arg)*)) };
}

/// Log information: `info!("started, x = {}", x)`.
#[macro_export]
macro_rules! info {
    ($($arg:tt)*) => { $crate::log::log($crate::log::Level::Info, format_args!($($arg)*)) };
}

/// Log debug details: `debug!("reg = {:#010x}", v)`.
#[macro_export]
macro_rules! debug {
    ($($arg:tt)*) => { $crate::log::log($crate::log::Level::Debug, format_args!($($arg)*)) };
}

/// Log very detailed tracing: `trace!("loop {}", i)`.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => { $crate::log::log($crate::log::Level::Trace, format_args!($($arg)*)) };
}

/// Print without level or timestamp, like `std::print!`.
#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => { $crate::log::print(format_args!($($arg)*)) };
}

/// Print a line without level or timestamp, like `std::println!`.
#[macro_export]
macro_rules! println {
    () => { $crate::log::print(format_args!("\n")) };
    ($($arg:tt)*) => { $crate::log::print(format_args!("{}\n", format_args!($($arg)*))) };
}

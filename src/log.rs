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

fn with_console(f: impl FnOnce(&mut crate::uart::Uart<crate::Blocking>)) {
    let daif: u64;
    // SAFETY: reading DAIF and masking IRQ/FIQ at EL2 has no other effect.
    unsafe {
        core::arch::asm!("mrs {}, daif", out(reg) daif, options(nomem, nostack, preserves_flags));
        core::arch::asm!("msr daifset, #3", options(nomem, nostack, preserves_flags));
    }
    while LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        core::hint::spin_loop();
    }
    // SAFETY: the lock serialises every user of the console handle.
    let mut uart = unsafe { crate::uart::Uart::<crate::Blocking>::console() };
    f(&mut uart);
    LOCK.store(false, Ordering::Release);
    // SAFETY: restores the caller's interrupt mask.
    unsafe { core::arch::asm!("msr daif, {}", in(reg) daif, options(nomem, nostack, preserves_flags)) };
}

#[doc(hidden)]
pub fn log(level: Level, args: fmt::Arguments) {
    if !enabled(level) {
        return;
    }
    let ticks = Delay::now();
    let freq = Delay::frequency().max(1);
    let (secs, micros) = (ticks / freq, (ticks % freq) * 1_000_000 / freq);
    with_console(|uart| {
        let _ = writeln!(uart, "{secs}.{micros:06} {} {args}", level.label());
    });
}

#[doc(hidden)]
pub fn print(args: fmt::Arguments) {
    with_console(|uart| {
        let _ = uart.write_fmt(args);
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

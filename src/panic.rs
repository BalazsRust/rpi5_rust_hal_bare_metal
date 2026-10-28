//! Default panic handler (feature `panic-handler`).
//!
//! `#![no_std]` programs must say what happens on `panic!` because there is
//! no OS to unwind to. We print the message (file, line, text) on the debug
//! UART and park the core. The target uses `panic = "abort"`, so there is
//! never any unwinding: the handler simply must not return, hence `-> !`.
//! Other cores keep running; only the panicking one stops.

use core::fmt::Write;
use core::panic::PanicInfo;

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    // Log lines still waiting in the buffer go out first, in order.
    crate::log::flush_for_panic();
    // SAFETY: normal execution on this core is over.
    let mut uart = unsafe { crate::uart::Uart::<crate::Blocking>::emergency() };
    // `PanicInfo`'s Display prints "panicked at <file>:<line>:<col>:\n<msg>".
    let _ = writeln!(uart, "\n!!! PANIC on core {}: {info}", crate::cpu::core_id());
    uart.flush();
    crate::cpu::halt()
}

//! UART chainloader: lives on the boot stick, receives a program over the
//! serial line and runs it. No more swapping the stick for every build.
//!
//! The chainloader itself is linked at 0x2000000 (32 MiB, see build.sh and
//! `kernel_address=0x2000000` in config.txt). Received programs run at
//! 0x200000 (see LOAD_ADDR for why not 0x80000), so they must be the
//! out/load/<name>.img builds, which are linked there.
//!
//! Protocol (PC -> Pi, all integers little-endian), implemented by
//! tools/pi5load.py:
//!   "PI5LOAD!"        8-byte magic; anything before it is ignored
//!                     (Pi answers "GO\r\n"; the PC resends the magic
//!                     until it sees that, then sends the rest)
//!   size: u32         number of image bytes
//!   image: [u8; size] the flat kernel image (out/<name>.img)
//!   crc: u32          CRC-32 (IEEE, same as Python's zlib.crc32) of image
//! Pi -> PC: "OK\r\n" then the program starts, or "ERR <reason>\r\n" and the
//! chainloader waits again.
//!
//! Handing over: the received program must find the machine the way the
//! firmware leaves it (EL2, MMU and caches OFF). So we write the data cache
//! back to RAM, turn MMU and caches off, invalidate the instruction cache
//! and jump to LOAD_ADDR with x0 = device-tree address, like the firmware.
//!
//! Wiring/UART: RP1 UART0 at 115200 on GPIO14 (TX, physical pin 8) /
//! GPIO15 (RX, physical pin 10). LED on GPIO21 blinks slowly while waiting
//! and is ON while an image is being received.

#![no_std]
#![no_main]

use core::arch::asm;
use core::fmt::Write;
use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;
use pi5_hal::delay::{Delay, Timeout};
use pi5_hal::gpio::{Level, Output, OutputConfig};
use pi5_hal::uart::{Config, Uart};
use pi5_hal::{mmu, rt};

pi5_hal::entry!(main);

/// Where received programs go. NOT 0x80000: when the firmware boots us at
/// 0x2000000, something of its own stays alive at 0x80000-0x8f000 and
/// 0xa3000 (measured: those pages change by themselves), and overwriting it
/// kills the CPU. 0x200000 (2 MiB, also the Pi 5's usual kernel address) is
/// unused. build.sh links a copy of every example there: out/load/<name>.img.
const LOAD_ADDR: usize = 0x200000;
/// Must stay below our own image at 0x2000000.
const MAX_SIZE: usize = 0x2000000 - LOAD_ADDR;
/// Received bytes go here first and are copied to LOAD_ADDR only right
/// before the jump. Writing straight to 0x80000 while we were still running
/// corrupted the chainloader's own state (a "can't happen" index panic), so
/// something live seems to sit there; 256 MiB is far from everything.
const STAGE_ADDR: usize = 0x1000_0000;
const MAGIC: &[u8; 8] = b"PI5LOAD!";

/// Bitwise CRC-32 (IEEE 802.3, reflected, poly 0xEDB88320): identical to
/// Python's zlib.crc32. Slow-ish but far faster than 115200 baud.
fn crc32_update(mut crc: u32, byte: u8) -> u32 {
    crc ^= u32::from(byte);
    for _ in 0..8 {
        crc = if crc & 1 != 0 { (crc >> 1) ^ 0xEDB8_8320 } else { crc >> 1 };
    }
    crc
}

/// RX errors (bytes lost to noise/overrun) seen while receiving an image.
static RX_ERRORS: AtomicU32 = AtomicU32::new(0);
/// Image bytes received so far (reported on ERR).
static RX_COUNT: AtomicU32 = AtomicU32::new(0);

/// Blocking read with a timeout, so a broken transfer can't hang us forever.
fn read_byte_timeout(uart: &mut Uart, timeout: Duration) -> Option<u8> {
    let t = Timeout::new(timeout);
    loop {
        match uart.try_read_byte() {
            Some(Ok(b)) => return Some(b),
            Some(Err(_)) => {
                // framing/noise/overrun: the byte is lost, count it
                RX_ERRORS.fetch_add(1, Ordering::Relaxed);
            }
            None => {
                if t.expired() {
                    return None;
                }
            }
        }
    }
}

fn read_u32(uart: &mut Uart) -> Option<u32> {
    let mut v = [0u8; 4];
    for b in &mut v {
        *b = read_byte_timeout(uart, Duration::from_secs(2))?;
    }
    Some(u32::from_le_bytes(v))
}

/// Receive one image into LOAD_ADDR. Ok(size) or Err(reason).
fn receive(uart: &mut Uart, led: &mut Output) -> Result<usize, &'static str> {
    let size = read_u32(uart).ok_or("timeout reading size")? as usize;
    if size == 0 || size > MAX_SIZE {
        return Err("bad size");
    }
    led.set_high();
    RX_ERRORS.store(0, Ordering::Relaxed);
    RX_COUNT.store(0, Ordering::Relaxed);
    // Progress: size first (shows whether the header arrived intact), then
    // one '#' per KiB, so a stall shows exactly where it happened.
    let _ = writeln!(uart, "[chainloader] receiving {size} bytes");
    let mut crc = 0xFFFF_FFFFu32;
    for i in 0..size {
        if i % 1024 == 1023 {
            uart.write_byte(b'#');
        }
        let b = read_byte_timeout(uart, Duration::from_secs(2)).ok_or("timeout during image")?;
        RX_COUNT.store(i as u32 + 1, Ordering::Relaxed);
        crc = crc32_update(crc, b);
        // SAFETY: [STAGE_ADDR, STAGE_ADDR + MAX_SIZE) is free RAM, identity
        // mapped, far from our image and the firmware's.
        unsafe { ((STAGE_ADDR + i) as *mut u8).write_volatile(b) };
    }
    let crc = !crc;
    let expected = read_u32(uart).ok_or("timeout reading crc")?;
    led.set_low();
    if crc != expected {
        return Err("crc mismatch");
    }
    Ok(size)
}

/// Compare [LOAD_ADDR, +size) with the staging copy; print the result.
fn verify_copy(uart: &mut Uart, when: &str, size: usize) {
    for i in 0..size {
        // SAFETY: both ranges are RAM.
        let (a, b) = unsafe { (((LOAD_ADDR + i) as *const u8).read_volatile(), ((STAGE_ADDR + i) as *const u8).read_volatile()) };
        if a != b {
            let _ = writeln!(uart, "[chainloader] verify {when}: MISMATCH at {:#x}: {a:#04x} != {b:#04x}", LOAD_ADDR + i);
            return;
        }
    }
    let _ = writeln!(uart, "[chainloader] verify {when}: ok");
}

/// `dc civac` (clean + invalidate to the point of coherency) on every cache
/// line of [start, end). ~500k lines for 31.5 MiB: a few milliseconds.
fn dcache_clean_invalidate_range(start: usize, end: usize) {
    let line = mmu::dcache_line_size();
    let mut addr = start & !(line - 1);
    while addr < end {
        // SAFETY: cache maintenance only, on identity-mapped RAM.
        unsafe { asm!("dc civac, {}", in(reg) addr, options(nostack, preserves_flags)) };
        addr += line;
    }
    // SAFETY: barrier.
    unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
}

/// Clean caches, MMU + caches off, jump to `entry` with x0 = `dtb`.
/// Never returns.
unsafe fn jump_to(entry: usize, dtb: usize) -> ! {
    // SAFETY (whole block): we are at EL2 with an identity map, so turning
    // the MMU off does not move the code we are executing; nothing after
    // the SCTLR write touches the stack.
    unsafe {
        asm!(
            "mrs x9, sctlr_el2",
            "bic x9, x9, #(1 << 0)",  // M: MMU off
            "bic x9, x9, #(1 << 2)",  // C: data cache off
            "bic x9, x9, #(1 << 12)", // I: instruction cache off
            "dsb sy",
            "msr sctlr_el2, x9",
            "isb",
            "ic iallu",               // drop stale instructions
            "tlbi alle2",
            "dsb sy",
            "isb",
            "mov x0, x11",
            "mov x1, xzr",
            "mov x2, xzr",
            "mov x3, xzr",
            "br x10",
            // Fixed registers so the compiler can't put them in x9 (scratch
            // above) or x0-x3 (overwritten right before the jump).
            in("x10") entry,
            in("x11") dtb,
            options(noreturn),
        )
    }
}

fn main() -> ! {
    let p = pi5_hal::init();
    let delay = Delay::new();
    let mut led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());
    let mut uart = Uart::new(p.UART0, Config::default().with_baudrate(115_200))
        .unwrap()
        .with_tx(p.GPIO14)
        .with_rx(p.GPIO15);

    let _ = writeln!(uart, "\n[chainloader] ready at 0x2000000, loads to {LOAD_ADDR:#x} (staging at {STAGE_ADDR:#x}). Waiting for image...");

    let mut matched = 0usize;
    let mut idle = Timeout::new(Duration::from_millis(500));
    // Diagnostics: if bytes arrive but never form the magic, say so after a
    // quiet second instead of staying silent.
    let mut heard = 0u32;
    let mut errors = 0u32;
    let mut first = [0u8; 16];
    let mut quiet = Timeout::new(Duration::from_secs(1));
    loop {
        // Slow blink while waiting.
        if idle.expired() {
            led.toggle();
            idle = Timeout::new(Duration::from_millis(500));
        }
        if heard + errors > 0 && quiet.expired() {
            let n = (heard as usize).min(first.len());
            let _ = write!(uart, "[chainloader] heard {heard} bytes + {errors} rx errors but no PI5LOAD! magic; first bytes:");
            for b in &first[..n] {
                let _ = write!(uart, " {b:02x}");
            }
            let _ = writeln!(uart);
            heard = 0;
            errors = 0;
            matched = 0;
        }
        let b = match uart.try_read_byte() {
            None => continue,
            Some(Err(_)) => {
                errors += 1;
                quiet = Timeout::new(Duration::from_secs(1));
                continue;
            }
            Some(Ok(b)) => b,
        };
        if (heard as usize) < first.len() {
            first[heard as usize] = b;
        }
        heard += 1;
        quiet = Timeout::new(Duration::from_secs(1));

        // Look for the 8-byte magic anywhere in the stream.
        matched = if b == MAGIC[matched] { matched + 1 } else if b == MAGIC[0] { 1 } else { 0 };
        if matched < MAGIC.len() {
            continue;
        }
        matched = 0;
        heard = 0;
        errors = 0;
        // Handshake: the PC sends nothing more until it sees this, so the
        // magic can simply be resent if it got mangled.
        let _ = write!(uart, "GO\r\n");
        uart.flush();

        match receive(&mut uart, &mut led) {
            Ok(size) => {
                let _ = write!(uart, "\nOK\r\n");
                let _ = writeln!(uart, "[chainloader] got {size} bytes, crc ok, starting at {LOAD_ADDR:#x}");
                uart.flush();
                delay.delay_millis(5);
                led.set_low();
                // SAFETY: both ranges are RAM, `size` <= MAX_SIZE, no overlap.
                unsafe { core::ptr::copy_nonoverlapping(STAGE_ADDR as *const u8, LOAD_ADDR as *mut u8, size) };
                verify_copy(&mut uart, "after copy", size);
                let _ = writeln!(uart, "[chainloader] jumping");
                uart.flush();
                // Clean AND invalidate everything the new program may use
                // (its image, .bss, stack, page tables: all of
                // [LOAD_ADDR, 0x2000000)). Clean = the image reaches RAM, so
                // a core with caches off sees it. Invalidate = no stale lines
                // stay behind: the new program writes its page tables with
                // the cache OFF (straight to RAM) and then turns the cache
                // on; an old cached copy of that memory would then hide the
                // new tables and it would crash before doing anything.
                dcache_clean_invalidate_range(LOAD_ADDR, LOAD_ADDR + MAX_SIZE);
                // SAFETY: a complete, CRC-checked image is at LOAD_ADDR.
                unsafe { jump_to(LOAD_ADDR, rt::dtb_address()) }
            }
            Err(reason) => {
                let errs = RX_ERRORS.load(Ordering::Relaxed);
                let got = RX_COUNT.load(Ordering::Relaxed);
                let _ = write!(uart, "\nERR {reason} (got {got} image bytes, {errs} rx errors)\r\n");
                let _ = writeln!(uart, "[chainloader] waiting for image...");
            }
        }
    }
}

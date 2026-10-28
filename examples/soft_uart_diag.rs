//! Software UART on GPIO14 + UART0 diagnosis.
//!
//! The hardware UART0 doesn't seem to send anything, but we KNOW GPIO14 works
//! as a plain output (pin_toggle). So this program switches GPIO14 on and off
//! itself with exact 9600-baud timing ("bit-banging"): the ESP32 bridge
//! receives that as normal text, no new wires, no bridge changes.
//!
//! Wiring: Pi physical pin 8 (GPIO14) -> ESP32 GPIO25, GND -> GND.
//!
//! Every 5 s it prints a report: RP1 clock registers, the measured UART
//! clock, UART0's registers, and a live test where GPIO14 is briefly handed
//! to the real UART0 to see whether bytes leave its FIFO. If the hardware
//! UART works, the line "HW UART0 says hi" also arrives at the bridge.

#![no_std]
#![no_main]

use core::arch::asm;
use core::fmt::Write;
use pi5_hal::clocks;
use pi5_hal::gpio::{Level, Output, OutputConfig};
use pi5_hal::pac::mmio::{read32, write32};
use pi5_hal::pac::{pl011, rp1, rp1_clocks, rp1_gpio};
use pi5_hal::uart::{Config, Uart};

pi5_hal::entry!(main);

/// 9600 baud: 104 us per bit, so nanosecond spikes and microsecond
/// timing jitter can't matter. The ESP32 bridge's Pi side must match.
const BAUD: u64 = 9_600;

/// ARM generic timer counter (54 MHz on the Pi 5).
fn ticks() -> u64 {
    let t: u64;
    // SAFETY: reading the virtual/physical counter has no side effects.
    unsafe { asm!("isb", "mrs {}, cntpct_el0", out(reg) t) };
    t
}

fn tick_hz() -> u64 {
    let f: u64;
    // SAFETY: as above.
    unsafe { asm!("mrs {}, cntfrq_el0", out(reg) f) };
    f
}

/// Bit-banged UART transmitter: 8N1, LSB first, idle HIGH.
struct SoftTx {
    pin: Output,
    bit_ticks: u64,
}

impl SoftTx {
    fn new(pin: Output, baud: u64) -> Self {
        Self { pin, bit_ticks: tick_hz() / baud }
    }

    fn write_byte(&mut self, byte: u8) {
        // 1 start bit (0), 8 data bits (LSB first), 1 stop bit (1). Each edge
        // is scheduled against an ABSOLUTE deadline from the first edge, so
        // small delays (PCIe write latency to RP1) never add up.
        let frame: u16 = (1 << 9) | (u16::from(byte) << 1);
        let start = ticks();
        for i in 0..10 {
            let deadline = start + i * self.bit_ticks;
            while ticks() < deadline {}
            if frame & (1 << i) != 0 {
                self.pin.set_high();
            } else {
                self.pin.set_low();
            }
            // Read the pin's register back. RP1 sits behind PCIe, and PCIe
            // writes are "posted": the CPU continues immediately and the
            // write may reach RP1 later, possibly bunched together with the
            // next ones. A READ cannot complete before the earlier writes
            // have arrived, so this pins each edge to its intended moment.
            let _ = self.pin.is_set_high();
        }
        let end = start + 10 * self.bit_ticks;
        while ticks() < end {}
    }
}

impl Write for SoftTx {
    fn write_str(&mut self, s: &str) -> core::fmt::Result {
        for b in s.bytes() {
            if b == b'\n' {
                self.write_byte(b'\r');
            }
            self.write_byte(b);
        }
        Ok(())
    }
}

fn r32(addr: usize) -> u32 {
    // SAFETY: diagnostic reads of RP1 registers.
    unsafe { read32(addr) }
}

fn set_funcsel(gpio: u8, fsel: u32) {
    let ctrl = rp1_gpio::ctrl_addr(gpio);
    // SAFETY: we own GPIO14 (via the Output we hold); only FUNCSEL changes.
    unsafe { write32(ctrl, (read32(ctrl) & !rp1_gpio::CTRL_FUNCSEL_MASK) | fsel) };
}

fn main() -> ! {
    let p = pi5_hal::init();
    let tx_pin = Output::new(p.GPIO14, Level::High, OutputConfig::default());
    let mut led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());
    let mut out = SoftTx::new(tx_pin, BAUD);

    // Program UART0 exactly like uart_hello does, but WITHOUT taking the pins:
    // GPIO14 stays ours for the software UART except during the live test.
    let uart_result = Uart::new(p.UART0, Config::default().with_baudrate(BAUD as u32));
    let u = rp1::UART0_BASE;
    let c = rp1::CLOCKS_BASE;

    let mut round = 0u32;
    loop {
        led.toggle();
        round += 1;
        let _ = writeln!(out, "\n=== soft_uart_diag round {round} (software UART on GPIO14) ===");
        let bit_ticks = out.bit_ticks;
        let _ = writeln!(out, "timer: {} Hz, {bit_ticks} ticks per bit", tick_hz());

        let _ = writeln!(
            out,
            "CLK_UART: CTRL={:#010x} DIV_INT={:#010x} SEL={:#010x}   (CLK_SYS CTRL={:#010x})",
            r32(c + rp1_clocks::CLK_UART_CTRL),
            r32(c + rp1_clocks::CLK_UART_DIV_INT),
            r32(c + rp1_clocks::CLK_UART_SEL),
            r32(c + rp1_clocks::CLK_SYS_CTRL),
        );
        let _ = writeln!(out, "clk_sys measured: {} Hz", clocks::clk_sys_hz());
        match clocks::uart_clock_measured_hz() {
            Some(hz) => { let _ = writeln!(out, "clk_uart measured: {hz} Hz"); }
            None => { let _ = writeln!(out, "clk_uart measured: FAILED"); }
        }
        match &uart_result {
            Ok(uart) => { let _ = writeln!(out, "Uart::new: Ok, clock used for baud = {} Hz", uart.clock_hz()); }
            Err(e) => { let _ = writeln!(out, "Uart::new: ERROR {e:?}"); }
        }
        let _ = writeln!(
            out,
            "UART0 regs: CR={:#06x} LCR_H={:#06x} IBRD={} FBRD={} FR={:#06x} periph_id={:#010x}",
            r32(u + pl011::CR),
            r32(u + pl011::LCR_H),
            r32(u + pl011::IBRD),
            r32(u + pl011::FBRD),
            r32(u + pl011::FR),
            match &uart_result { Ok(uart) => uart.peripheral_id(), Err(_) => 0 },
        );
        let _ = writeln!(
            out,
            "GPIO14: CTRL={:#010x} PAD={:#010x}",
            r32(rp1_gpio::ctrl_addr(14)),
            r32(rp1_gpio::pad_addr(14)),
        );

        // Live test: hand GPIO14 to UART0 (FUNCSEL 4), queue a line, wait,
        // see whether the FIFO drained, give the pin back to SYS_RIO.
        let fr_before = r32(u + pl011::FR);
        set_funcsel(14, 4);
        for &b in b"\r\nHW UART0 says hi\r\n" {
            // SAFETY: UART0 data register; 19 bytes fit in the 32-byte FIFO.
            unsafe { write32(u + pl011::DR, u32::from(b)) };
        }
        let fr_queued = r32(u + pl011::FR);
        let wait_start = ticks();
        while ticks() - wait_start < tick_hz() / 20 {} // 50 ms (19 bytes at 9600 need ~20 ms)
        let fr_after = r32(u + pl011::FR);
        set_funcsel(14, rp1_gpio::FSEL_SYS_RIO);
        let drained = fr_after & pl011::FR_TXFE != 0;
        let _ = writeln!(
            out,
            "HW UART test: FR before={fr_before:#06x} queued={fr_queued:#06x} after 50ms={fr_after:#06x} -> {}",
            if drained { "FIFO EMPTIED: UART0 transmits" } else { "STILL FULL: UART0 is NOT transmitting" }
        );

        let wait_start = ticks();
        while ticks() - wait_start < 5 * tick_hz() {}
    }
}

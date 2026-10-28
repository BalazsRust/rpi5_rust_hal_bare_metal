//! Print a banner on the 3-pin debug UART (115200 8N1), then echo.
//!
//! Needs `enable_uart=1` in config.txt and a 3.3 V USB-serial adapter on the
//! JST-SH "UART" connector (Raspberry Pi Debug Probe, or any adapter:
//! connector pins are TX, GND, RX).
//!
//! The banner is also the hardware self-test: every line is a fact we could
//! only have guessed from the device tree.

#![no_std]
#![no_main]

use core::fmt::Write;
use pi5_hal::delay::Delay;
use pi5_hal::gpio::{Level, Output, OutputConfig};
use pi5_hal::uart::{Config, Uart};
use pi5_hal::{clocks, cpu, mmu, psci, rt};

pi5_hal::entry!(main);

fn main() -> ! {
    let p = pi5_hal::init();
    let delay = Delay::new();
    let mut led = Output::new(p.GPIO21, Level::High, OutputConfig::default());
    delay.delay_millis(2000); // checkpoint 1: LED ON for 2 s = program started

    let mut uart = Uart::new(p.UART0, Config::default().with_baudrate(115_200)).unwrap().with_tx(p.GPIO14).with_rx(p.GPIO15);
    led.set_low();
    delay.delay_millis(2000); // checkpoint 2: LED OFF for 2 s = UART setup survived

    writeln!(uart, "\n=== pi5-hal uart_hello ===").unwrap();
    led.set_high(); // checkpoint 3: LED ON again = banner was sent
    match clocks::uart_clock_measured_hz() {
        Some(hz) => writeln!(uart, "RP1 clk_uart = {hz} Hz (measured; programmed for 50000000)").unwrap(),
        None => writeln!(uart, "RP1 clk_uart: measurement failed, assuming 50000000 Hz").unwrap(),
    }
    writeln!(uart, "EL{}, core {}, MMU {}", cpu::current_el(), cpu::core_id(), if mmu::is_enabled() { "on" } else { "off" }).unwrap();
    writeln!(uart, "CNTFRQ_EL0 = {} Hz (expected 54000000)", Delay::frequency()).unwrap();
    writeln!(uart, "device tree at {:#x}", rt::dtb_address()).unwrap();
    writeln!(uart, "PL011 peripheral id = {:#010x} (expected 0x00341011)", uart.peripheral_id()).unwrap();
    match psci::version() {
        Some((major, minor)) => writeln!(uart, "PSCI v{major}.{minor}").unwrap(),
        None => writeln!(uart, "PSCI: not available").unwrap(),
    }
    // First touch of RP1: if the PCIe window is not where we think, this is
    // where the exception handler will print a data abort.
    writeln!(uart, "RP1 clk_sys = {} Hz ({})", clocks::clk_sys_hz(), if clocks::clk_sys_measured() { "measured" } else { "assumed" }).unwrap();
    writeln!(uart, "type something; it will be echoed and the LED toggles per byte").unwrap();

    loop {
        match uart.read_byte() {
            Ok(b) => {
                uart.write_byte(b);
                if b == b'\r' {
                    uart.write_byte(b'\n');
                }
                led.toggle();
            }
            // Ignore RX errors (noise on a floating/long wire, a reset of the
            // other side...). Printing a ~40-byte message per error used to
            // turn every bit of noise into a flood: while we were busy
            // printing, the 32-byte RX FIFO overran, which caused the next
            // error message, and so on forever.
            Err(_) => {}
        }
    }
}

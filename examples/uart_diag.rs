//! UART0 diagnosis with the LED as the only output (no serial needed).
//!
//! Wiring as for uart_hello: Pi physical pin 8 (GPIO14, TX) -> bridge RX,
//! GND -> GND. LED on physical pin 40 (GPIO21) / 39 (GND).
//!
//! What the LED does, in order:
//!   1. ON 1 s, OFF 1 s              program started
//!   2. one byte is put in the UART, then after 100 ms the FIFO is checked:
//!        3 SLOW blinks              the byte LEFT the UART: it transmits
//!        10 FAST blinks             the byte is STILL in the FIFO: the UART
//!                                   is not shifting (clock not running?)
//!   3. pause 2 s, then the measured clk_uart:
//!        ON solid for 3 s           measurement failed
//!        N long blinks, pause,      tens of MHz, then units of MHz
//!        M short blinks             (50 MHz = 5 long, 0 short)
//!   4. pause 2 s, then forever: LED toggles every 0.5 s and, if the TX FIFO
//!      is empty, "hello from pi5" is queued. The bridge should show bytes.

#![no_std]
#![no_main]

use pi5_hal::clocks;
use pi5_hal::delay::Delay;
use pi5_hal::gpio::{Level, Output, OutputConfig};
use pi5_hal::pac::mmio::{read32, write32};
use pi5_hal::pac::{pl011, rp1};
use pi5_hal::uart::{Config, Uart};

pi5_hal::entry!(main);

fn blink(led: &mut Output, delay: &Delay, times: u32, on_ms: u32, off_ms: u32) {
    for _ in 0..times {
        led.set_high();
        delay.delay_millis(on_ms);
        led.set_low();
        delay.delay_millis(off_ms);
    }
}

fn tx_fifo_empty() -> bool {
    // SAFETY: read-only access to the UART0 flag register.
    unsafe { read32(rp1::UART0_BASE + pl011::FR) & pl011::FR_TXFE != 0 }
}

fn main() -> ! {
    let p = pi5_hal::init();
    let delay = Delay::new();
    let mut led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());

    // 1. alive
    blink(&mut led, &delay, 1, 1000, 1000);

    // 2. does the UART shift bytes out?
    let _uart = Uart::new(p.UART0, Config::default()).unwrap().with_tx(p.GPIO14).with_rx(p.GPIO15);
    // Write straight into the data register so we never block here.
    // SAFETY: UART0 is ours; DR is its data register.
    unsafe { write32(rp1::UART0_BASE + pl011::DR, u32::from(b'U')) };
    delay.delay_millis(100); // one byte at 115200 takes ~0.09 ms
    if tx_fifo_empty() {
        blink(&mut led, &delay, 3, 500, 500);
    } else {
        blink(&mut led, &delay, 10, 100, 100);
    }
    delay.delay_millis(2000);

    // 3. measured UART clock
    match clocks::uart_clock_measured_hz() {
        None => {
            led.set_high();
            delay.delay_millis(3000);
            led.set_low();
        }
        Some(hz) => {
            let mhz = (hz + 500_000) / 1_000_000;
            blink(&mut led, &delay, mhz / 10, 600, 400);
            delay.delay_millis(1000);
            blink(&mut led, &delay, mhz % 10, 150, 350);
        }
    }
    delay.delay_millis(2000);

    // 4. heartbeat + text
    let msg = b"hello from pi5\r\n";
    loop {
        if tx_fifo_empty() {
            for &b in msg {
                // 16 bytes fit in the 32-entry FIFO; never blocks.
                // SAFETY: as above.
                unsafe { write32(rp1::UART0_BASE + pl011::DR, u32::from(b)) };
            }
        }
        led.toggle();
        delay.delay_millis(500);
    }
}

//! Wire test for the UART pins: drive GPIO14 (header pin 8, the UART TX pin)
//! as a PLAIN GPIO output, slowly: 1 s high, 1 s low, forever. The LED on
//! GPIO21 copies the level so you can see what the pin should be doing.
//!
//! The ESP32-C3 bridge prints the level it sees on its RX pin (GPIO20) in
//! every status line. If that level follows the LED, the wire from pin 8 to
//! the C3 is good and the Pi really drives the pin -> the remaining UART
//! problem is in the Pi's UART setup (pin mux / clock), not the wiring.
//!
//! No UART code at all here, on purpose: it takes the UART out of the test.

#![no_std]
#![no_main]

use pi5_hal::delay::Delay;
use pi5_hal::gpio::{Level, Output, OutputConfig};

pi5_hal::entry!(main);

fn main() -> ! {
    let p = pi5_hal::init();
    let delay = Delay::new();
    let mut tx = Output::new(p.GPIO14, Level::High, OutputConfig::default());
    let mut led = Output::new(p.GPIO21, Level::High, OutputConfig::default());

    loop {
        delay.delay_millis(1000);
        tx.toggle();
        led.toggle();
    }
}

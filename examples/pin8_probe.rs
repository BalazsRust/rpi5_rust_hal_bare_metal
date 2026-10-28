//! Does physical pin 8 (GPIO14) really go LOW, or does it just let go?
//!
//! An LED can't tell "driven LOW" from "floating": it is dark either way.
//! But the ESP32's RX pin has a pull-up resistor, so if GPIO14 only floats
//! instead of pulling low, the wire stays HIGH, and the ESP32 only ever sees
//! tiny dips at the edges ("glitches"). That would explain everything so far.
//!
//! Wiring: Pi physical pin 8 -> ESP32 GPIO25, Pi GND (physical pin 6) ->
//! ESP32 GND, LED on physical pins 40 (+) / 39 (GND) as usual.
//!
//! What it does, forever:
//!   GPIO14 HIGH for 2 s, then LOW for 2 s.
//!   50 ms after each change it reads RP1's OWN input buffer for GPIO14
//!   (RIO_IN): the real voltage on the pad, ESP32 pull-up included.
//!   The LED shows that measured level (ON = pad is high, OFF = pad is low).
//!
//! Result:
//!   LED on 2 s / off 2 s  -> pin 8 really drives LOW. Pi side is fine.
//!   LED stays ON          -> pin 8 does NOT pull low: it floats and the
//!                            ESP32 pull-up wins. Bug is in the Pi pin setup.

#![no_std]
#![no_main]

use pi5_hal::delay::Delay;
use pi5_hal::gpio::{Level, Output, OutputConfig};
use pi5_hal::pac::mmio::read32;
use pi5_hal::pac::rp1_gpio;

pi5_hal::entry!(main);

fn pad14_is_high() -> bool {
    // SAFETY: read-only access to the SYS_RIO input register of bank 0.
    unsafe { read32(rp1_gpio::rio_base(14) + rp1_gpio::RIO_IN) & rp1_gpio::rio_mask(14) != 0 }
}

fn main() -> ! {
    let p = pi5_hal::init();
    let delay = Delay::new();
    let mut pin8 = Output::new(p.GPIO14, Level::High, OutputConfig::default());
    let mut led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());

    loop {
        for level in [Level::High, Level::Low] {
            pin8.set_level(level);
            delay.delay_millis(50);
            led.set_level(if pad14_is_high() { Level::High } else { Level::Low });
            delay.delay_millis(1950);
        }
    }
}

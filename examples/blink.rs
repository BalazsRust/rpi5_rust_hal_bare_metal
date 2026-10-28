//! Blink an LED on GPIO21 (header pin 40), blocking style.
//!
//! Wiring: GPIO21 -> 330 ohm resistor -> LED anode; LED cathode -> GND (pin 39).
//!
//! Build:  cargo build --release --example blink
//! Image:  llvm-objcopy -O binary target/aarch64-unknown-none/release/examples/blink out/kernel8.img

#![no_std]
#![no_main]

use pi5_hal::delay::Delay;
use pi5_hal::gpio::{Level, Output, OutputConfig};

// Tells the runtime which function to call once the stack, .bss, MMU and
// exception vectors are set up.
pi5_hal::entry!(main);

fn main() -> ! {
    // Take the peripherals exactly once; the pin structs inside are proof of
    // ownership, not just numbers.
    let p = pi5_hal::init();

    // Pad config + SYS_RIO level/direction + FUNCSEL = 5: see pi5_hal::gpio.
    let mut led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());

    // Busy-wait delays from the ARM generic timer.
    let delay = Delay::new();

    loop {
        led.toggle(); // one store to the RP1 XOR alias of SYS_RIO.OUT
        delay.delay_millis(500);
    }
}

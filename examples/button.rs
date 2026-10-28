//! Mirror a button on GPIO17 (header pin 11) to the LED on GPIO21 (pin 40).
//!
//! Wiring: button between GPIO17 and GND (pin 9). The internal pull-up keeps
//! the pin high until the button pulls it low, so "pressed" reads Low.
//!
//! Shows both APIs: the inherent methods and the embedded-hal traits that
//! generic driver crates use.

#![no_std]
#![no_main]

use pi5_hal::delay::Delay;
use pi5_hal::embedded_hal::delay::DelayNs;
use pi5_hal::embedded_hal::digital::{InputPin, OutputPin};
use pi5_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};

pi5_hal::entry!(main);

fn main() -> ! {
    let p = pi5_hal::init();
    let mut button = Input::new(p.GPIO17, InputConfig::default().with_pull(Pull::Up));
    let mut led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());
    let mut delay = Delay::new();

    loop {
        // Inherent API:
        led.set_level(if button.is_low() { Level::High } else { Level::Low });
        delay.delay_millis(10);

        // Exactly the same thing through the embedded-hal traits:
        mirror(&mut button, &mut led, &mut delay);
    }
}

/// Works with any HAL's pins, not just ours.
fn mirror<I: InputPin, O: OutputPin, D: DelayNs>(button: &mut I, led: &mut O, delay: &mut D) {
    if button.is_low().unwrap_or(false) {
        let _ = led.set_high();
    } else {
        let _ = led.set_low();
    }
    delay.delay_ms(10);
}

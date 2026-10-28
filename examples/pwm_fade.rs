//! Breathe an LED on GPIO12 (header pin 32) with PWM0 channel 0.
//!
//! Wiring: GPIO12 -> 330 ohm -> LED -> GND.

#![no_std]
#![no_main]

use core::fmt::Write;
use pi5_executor::Spawner;
use pi5_hal::embedded_hal::pwm::SetDutyCycle;
use pi5_hal::pwm::{ChannelConfig, Pwm, PwmChannel};
use pi5_hal::time::Rate;
use pi5_hal::uart::{Config, Uart};
use pi5_time::{Duration, Timer};

#[pi5_executor::main]
async fn main(spawner: Spawner) {
    let p = pi5_hal::init();
    let mut uart = Uart::new(p.UART10, Config::default()).unwrap();

    let mut pwm = Pwm::new(p.PWM0);
    let ch = match pwm.channel(p.GPIO12, ChannelConfig::default().with_frequency(Rate::from_khz(1))) {
        Ok(ch) => ch,
        Err(e) => {
            let _ = writeln!(uart, "PWM setup failed: {e}");
            pi5_hal::cpu::halt();
        }
    };
    let _ = writeln!(uart, "pwm_fade: {} ({} ticks per period, clock {} Hz)", ch.frequency(), ch.range_ticks(), pwm.clock_hz());
    spawner.spawn(fade(ch).unwrap());
}

#[pi5_executor::task]
async fn fade(mut ch: PwmChannel) {
    loop {
        for percent in (0..=100).chain((0..100).rev()) {
            ch.set_duty_cycle_percent(percent).unwrap();
            Timer::after(Duration::from_millis(10)).await;
        }
    }
}

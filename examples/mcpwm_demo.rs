//! MCPWM demo: a complementary pair with dead time, plus a single output.
//!
//! Wiring (each through 330 ohm to an LED, LED to GND):
//!   physical pin 32 (GPIO12)  high side of the pair
//!   physical pin 33 (GPIO13)  low side of the pair
//!   physical pin 12 (GPIO18)  single output
//!
//! The pair's two LEDs cross-fade: one gets brighter while the other gets
//! dimmer, and they are never on at the same moment (2 us dead time). On a
//! scope at 20 kHz you can see the gaps between the two pulses. The single
//! output breathes on its own.

#![no_std]
#![no_main]

use pi5_executor::Spawner;
use pi5_hal::info;
use pi5_hal::mcpwm::{Complementary, CountMode, McPwm, McPwmOutput, OutputConfig, TimerConfig};
use pi5_hal::time::Rate;
use pi5_time::{Duration, Ticker};

#[pi5_executor::main]
async fn main(spawner: Spawner) {
    let p = pi5_hal::init();

    let timer = TimerConfig::new(Rate::from_khz(20)).with_mode(CountMode::UpDown);
    let mut mcpwm = match McPwm::new(p.PWM0, timer) {
        Ok(m) => m,
        Err(e) => panic!("MCPWM setup failed: {e}"),
    };
    let pair = match mcpwm.complementary(p.GPIO12, p.GPIO13, 2_000) {
        Ok(pair) => pair,
        Err(e) => panic!("pair setup failed: {e}"),
    };
    let single = match mcpwm.output(p.GPIO18, OutputConfig::default()) {
        Ok(out) => out,
        Err(e) => panic!("output setup failed: {e}"),
    };
    mcpwm.start();
    info!(
        "mcpwm_demo: {} Hz, {} ticks per period, dead time {} ticks",
        mcpwm.frequency().as_hz(),
        mcpwm.period_ticks(),
        pair.dead_time_ticks()
    );

    spawner.spawn(crossfade(pair).unwrap());
    spawner.spawn(breathe(single).unwrap());
}

#[pi5_executor::task]
async fn crossfade(mut pair: Complementary) {
    let mut ticker = Ticker::every(Duration::from_millis(20));
    loop {
        for percent in (0..=100).chain((0..100).rev()) {
            pair.set_duty_percent(percent);
            ticker.next().await;
        }
        info!("crossfade: one round");
    }
}

#[pi5_executor::task]
async fn breathe(mut out: McPwmOutput) {
    let mut ticker = Ticker::every(Duration::from_millis(7));
    loop {
        for percent in (0..=100).chain((0..100).rev()) {
            out.set_duty_percent(percent);
            ticker.next().await;
        }
    }
}

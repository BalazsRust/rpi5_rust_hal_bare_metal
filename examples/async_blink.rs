//! The embassy-style version of blink: two tasks sharing one core.
//!
//! Compare with an embassy program for an ESP32 or RP2040: only the crate
//! names differ.

#![no_std]
#![no_main]

use core::fmt::Write;
use pi5_executor::Spawner;
use pi5_hal::gpio::{Level, Output, OutputConfig};
use pi5_hal::uart::{Config, Uart};
use pi5_hal::Async;
use pi5_hal::info;
use pi5_time::{Duration, Instant, Timer};

#[pi5_executor::main]
async fn main(spawner: Spawner) {
    let p = pi5_hal::init();
    let led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());
    let uart = Uart::new(p.UART10, Config::default()).unwrap().into_async();

    spawner.spawn(blink(led).unwrap());
    spawner.spawn(heartbeat(uart).unwrap());
    // main returns; the executor keeps running the two tasks.
}

#[pi5_executor::task]
async fn blink(mut led: Output) {
    loop {
        led.toggle();
        Timer::after(Duration::from_millis(150)).await;
        info!("test");
    }
}

#[pi5_executor::task]
async fn heartbeat(mut uart: Uart<Async>) {
    let start = Instant::now();
    loop {
        let _ = writeln!(uart, "alive for {} ms", start.elapsed().as_millis());
        Timer::after_secs(1).await;
    }
}

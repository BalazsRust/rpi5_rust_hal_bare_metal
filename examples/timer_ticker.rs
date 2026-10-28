//! `Timer::after_millis` and `Ticker` side by side.
//!
//! - `with_timer` waits 500 ms after each line (`Timer::after_millis`).
//! - `with_ticker` runs on a fixed 333 ms beat (`Ticker`).
//!
//! Expected: "timer 500 ms", "timer 1000 ms", ... and "ticker 333 ms",
//! "ticker 666 ms", ... Logging no longer blocks, so the Timer loop isn't
//! pushed back by printing.
//!
//! The one difference that remains (same as embassy): if a task *computes*
//! for a while without `.await`, a Timer loop starts its next wait that
//! much later, while a Ticker catches up and keeps its beat.

#![no_std]
#![no_main]

use pi5_executor::Spawner;
use pi5_hal::info;
use pi5_time::{Duration, Instant, Ticker, Timer};

#[pi5_executor::main]
async fn main(spawner: Spawner) {
    let _p = pi5_hal::init();
    info!("timer_ticker started");
    let start = Instant::now();
    spawner.spawn(with_timer(start).unwrap());
    spawner.spawn(with_ticker(start).unwrap());
}

#[pi5_executor::task]
async fn with_timer(start: Instant) {
    loop {
        Timer::after_millis(500).await;
        info!("timer  {} ms", start.elapsed().as_millis());
    }
}

#[pi5_executor::task]
async fn with_ticker(start: Instant) {
    let mut ticker = Ticker::every(Duration::from_millis(333));
    loop {
        ticker.next().await;
        info!("ticker {} ms", start.elapsed().as_millis());
    }
}

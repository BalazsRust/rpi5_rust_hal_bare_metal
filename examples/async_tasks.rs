//! Several tasks cooperating: a button task sends events through a channel,
//! an LED task reacts, a ticker keeps time, and `select` races two futures.
//!
//! Wiring: button GPIO17 (pin 11) to GND; LED on GPIO21 (pin 40).

#![no_std]
#![no_main]

use core::fmt::Write;
use pi5_executor::Spawner;
use pi5_futures::select::{Either, select};
use pi5_hal::Async;
use pi5_hal::gpio::{Input, InputConfig, Level, Output, OutputConfig, Pull};
use pi5_hal::uart::{Config, Uart};
use pi5_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use pi5_sync::channel::Channel;
use pi5_sync::signal::Signal;
use pi5_time::{Duration, Instant, Ticker, Timer, with_timeout};

/// Button events, button task -> LED task. Static, so tasks can share it.
static BUTTON_EVENTS: Channel<CriticalSectionRawMutex, Instant, 4> = Channel::new();
/// "Stop blinking" request, UART task -> LED task.
static STOP: Signal<CriticalSectionRawMutex, ()> = Signal::new();

#[pi5_executor::main]
async fn main(spawner: Spawner) {
    let p = pi5_hal::init();
    let uart = Uart::new(p.UART10, Config::default()).unwrap().into_async();
    let button = Input::new(p.GPIO17, InputConfig::default().with_pull(Pull::Up));
    let led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());

    spawner.spawn(button_task(button).unwrap());
    spawner.spawn(led_task(led).unwrap());
    spawner.spawn(console_task(uart).unwrap());
}

/// Waits for presses (falling edges, since the pull-up makes idle = high)
/// and timestamps them into the channel.
#[pi5_executor::task]
async fn button_task(mut button: Input) {
    loop {
        button.wait_for_falling_edge().await;
        BUTTON_EVENTS.send(Instant::now()).await;
        Timer::after_millis(50).await; // crude debounce
    }
}

/// Blinks slowly; a button press makes it flash fast for a second; STOP ends it.
#[pi5_executor::task]
async fn led_task(mut led: Output) {
    let mut ticker = Ticker::every(Duration::from_millis(500));
    loop {
        // Race the next tick against the next button event.
        match select(ticker.next(), BUTTON_EVENTS.receive()).await {
            Either::First(()) => led.toggle(),
            Either::Second(_pressed_at) => {
                for _ in 0..10 {
                    led.toggle();
                    Timer::after_millis(100).await;
                }
                ticker.reset();
            }
        }
        if STOP.try_take().is_some() {
            led.set_low();
            return; // task ends; its pool slot is freed
        }
    }
}

/// Reports button presses with their latency, and stops the LED on 'q'.
#[pi5_executor::task]
async fn console_task(mut uart: Uart<Async>) {
    let _ = writeln!(uart, "async_tasks: press the button, or type q to stop the LED");
    loop {
        // Wait up to 5 s for a byte; otherwise print a reminder.
        match with_timeout(Duration::from_secs(5), uart.read_byte_async()).await {
            Ok(Ok(b'q')) => {
                STOP.signal(());
                let _ = writeln!(uart, "LED task asked to stop");
            }
            Ok(Ok(b)) => {
                let _ = writeln!(uart, "got {:?}", b as char);
            }
            Ok(Err(e)) => {
                let _ = writeln!(uart, "rx error: {e}");
            }
            Err(_timeout) => {
                let _ = writeln!(uart, "(still here, {} s up)", Instant::now().as_secs());
            }
        }
    }
}

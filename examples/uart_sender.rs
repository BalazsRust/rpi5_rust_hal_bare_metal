//! A line-for-line port of the ESP32 `uart_sender` program (esp-hal +
//! embassy) to the Pi 5. Same structure, same calls, different crate names.
//!
//! The ESP32 version used UART1 on GPIO4/GPIO16 at 9600 baud; here the
//! equivalent header UART is RP1's UART0 on GPIO14 (TX, pin 8) / GPIO15 (RX,
//! pin 10). Logging that went to `defmt` goes to the debug UART instead.

#![no_std]
#![no_main]

use core::fmt::Write;
use pi5_executor::Spawner;
use pi5_hal::Async;
use pi5_hal::gpio::{Level, Output, OutputConfig};
use pi5_hal::uart::{Config, Uart};
use pi5_time::{Duration, Timer};

#[pi5_executor::main]
async fn main(spawner: Spawner) {
    let peripherals = pi5_hal::init();
    let mut log = Uart::new(peripherals.UART10, Config::default()).unwrap();

    let _ = writeln!(log, "Embassy-style executor initialized!");
    let _ = writeln!(log, "About to init UART...");

    let uart = Uart::new(peripherals.UART0, Config::default().with_baudrate(9600))
        .unwrap()
        .with_tx(peripherals.GPIO14)
        .with_rx(peripherals.GPIO15)
        .into_async();

    let _ = writeln!(log, "UART ready!");

    let led = Output::new(peripherals.GPIO21, Level::High, OutputConfig::default());

    spawner.spawn(blink_task(led, uart, log).unwrap());
    // ("Hello world!" was logged here in the original; the log UART has moved
    // into the task so it can report what it receives.)
}

#[pi5_executor::task]
async fn blink_task(mut led: Output, mut uart: Uart<Async>, mut log: Uart) {
    uart.write_async(b"TEST\n").await.unwrap();
    uart.flush_async().await;

    let mut buf = [0u8; 5];
    match uart.read_exact_async(&mut buf).await {
        Ok(()) => {
            let _ = write!(log, "Got: ");
            for b in buf {
                let _ = write!(log, "{}", b as char);
            }
            let _ = writeln!(log);
        }
        Err(e) => {
            let _ = writeln!(log, "read error: {e}");
        }
    }

    loop {
        led.toggle();
        Timer::after(Duration::from_millis(500)).await;
    }
}

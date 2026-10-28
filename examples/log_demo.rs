//! defmt-style logging over the serial line: `info!`, `warn!`, ...
//!
//!   python3 tools/pi5load.py log_demo
//!
//! No Uart setup needed: the first log line sets UART0 up (115200 8N1,
//! GPIO14 = TX on physical pin 8).

#![no_std]
#![no_main]

use pi5_hal::delay::Delay;
use pi5_hal::gpio::{Level, Output, OutputConfig};
use pi5_hal::{debug, error, info, println, trace, warn};

pi5_hal::entry!(main);

fn main() -> ! {
    let p = pi5_hal::init();
    let delay = Delay::new();
    let mut led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());

    println!("=== log_demo ===");
    info!("hello! CNTFRQ = {} Hz", Delay::frequency());
    warn!("this is a warning");
    error!("this is an error (not a real one)");
    debug!("debug value: {:#010x}", 0xdead_beefu32);
    trace!("trace details");

    let mut n = 0u32;
    loop {
        led.toggle();
        info!("tick {n}, LED is {}", if led.is_set_high() { "on" } else { "off" });
        n += 1;
        delay.delay_millis(1000);
    }
}

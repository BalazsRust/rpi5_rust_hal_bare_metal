//! Scan I2C1 (SDA GPIO2 / pin 3, SCL GPIO3 / pin 5) for devices.
//!
//! Like `i2cdetect -y 1`: tries to read one byte from every address
//! 0x08..=0x77 and reports the ones that acknowledge. The Pi has 1.8 kOhm
//! pull-ups on these two pins already.

#![no_std]
#![no_main]

use core::fmt::Write;
use pi5_hal::i2c::{Config, Error, I2c};
use pi5_hal::time::Rate;
use pi5_hal::uart::{self, Uart};
use pi5_hal::{clocks, delay::Delay};

pi5_hal::entry!(main);

fn main() -> ! {
    let p = pi5_hal::init();
    let mut uart = Uart::new(p.UART10, uart::Config::default()).unwrap();
    let _ = writeln!(uart, "\ni2c_scan: clk_sys = {} Hz", clocks::clk_sys_hz());

    let mut i2c = match I2c::new(p.I2C1, Config::default().with_frequency(Rate::from_khz(100))) {
        Ok(i2c) => i2c.with_sda(p.GPIO2).with_scl(p.GPIO3),
        Err(e) => {
            let _ = writeln!(uart, "I2C1 init failed: {e}");
            pi5_hal::cpu::halt();
        }
    };

    let delay = Delay::new();
    loop {
        let _ = writeln!(uart, "     0  1  2  3  4  5  6  7  8  9  a  b  c  d  e  f");
        let mut found = 0;
        for row in (0x00..0x80).step_by(16) {
            let _ = write!(uart, "{row:02x}: ");
            for addr in row..row + 16 {
                if !(0x08..=0x77).contains(&addr) {
                    let _ = write!(uart, "   ");
                    continue;
                }
                let mut byte = [0u8];
                match i2c.read(addr, &mut byte) {
                    Ok(()) => {
                        found += 1;
                        let _ = write!(uart, "{addr:02x} ");
                    }
                    Err(Error::AddressNack) => {
                        let _ = write!(uart, "-- ");
                    }
                    Err(e) => {
                        let _ = write!(uart, "?? ");
                        let _ = writeln!(uart, "\n  (error at {addr:#04x}: {e})");
                    }
                }
            }
            let _ = writeln!(uart);
        }
        let _ = writeln!(uart, "{found} device(s). Rescanning in 5 s.\n");
        delay.delay_millis(5000);
    }
}

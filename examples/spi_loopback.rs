//! SPI0 loopback test: connect MOSI (GPIO10, pin 19) to MISO (GPIO9, pin 21)
//! with a jumper wire; every byte sent should come straight back.
//!
//! Also shows the hardware chip select on GPIO8 (pin 24) toggling, if you
//! have a scope or logic analyser handy.

#![no_std]
#![no_main]

use core::fmt::Write;
use pi5_hal::delay::Delay;
use pi5_hal::spi::{Config, Mode, Spi};
use pi5_hal::time::Rate;
use pi5_hal::uart::{self, Uart};

pi5_hal::entry!(main);

fn main() -> ! {
    let p = pi5_hal::init();
    let mut uart = Uart::new(p.UART10, uart::Config::default()).unwrap();

    let mut spi = match Spi::new(p.SPI0, Config::default().with_frequency(Rate::from_mhz(1)).with_mode(Mode::_0)) {
        Ok(spi) => spi.with_sck(p.GPIO11).with_mosi(p.GPIO10).with_miso(p.GPIO9).with_cs(p.GPIO8),
        Err(e) => {
            let _ = writeln!(uart, "SPI0 init failed: {e}");
            pi5_hal::cpu::halt();
        }
    };
    let _ = writeln!(uart, "\nspi_loopback: SCLK {} , FIFO depth {}", spi.frequency(), spi.fifo_depth());

    let delay = Delay::new();
    let mut round: u8 = 0;
    loop {
        let mut buf = [0u8; 8];
        for (i, b) in buf.iter_mut().enumerate() {
            *b = round.wrapping_add(i as u8);
        }
        let sent = buf;
        match spi.transfer_in_place(&mut buf) {
            Ok(()) if buf == sent => {
                let _ = writeln!(uart, "round {round}: OK  {sent:02x?}");
            }
            Ok(()) => {
                let _ = writeln!(uart, "round {round}: MISMATCH sent {sent:02x?} got {buf:02x?} (jumper MOSI-MISO missing?)");
            }
            Err(e) => {
                let _ = writeln!(uart, "round {round}: error {e}");
            }
        }
        round = round.wrapping_add(1);
        delay.delay_millis(1000);
    }
}

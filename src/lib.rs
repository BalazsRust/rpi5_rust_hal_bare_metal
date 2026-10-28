//! # pi5-hal: Layer 1, drivers and runtime for the Raspberry Pi 5
//!
//! Written in the spirit of `esp-hal`: take the peripherals once, turn typed
//! pins and peripheral tokens into drivers, use them blocking or `async`.
//! The async runtime that goes with it (`pi5-executor`, `pi5-time`,
//! `pi5-sync`, `pi5-futures`) is shaped like embassy, so an embassy program
//! for an ESP32 or RP2040 ports almost line for line:
//!
//! ```no_run
//! #![no_std]
//! #![no_main]
//!
//! use pi5_executor::Spawner;
//! use pi5_hal::gpio::{Level, Output, OutputConfig};
//! use pi5_hal::uart::{Config, Uart};
//! use pi5_time::{Duration, Timer};
//!
//! #[pi5_executor::main]
//! async fn main(spawner: Spawner) {
//!     let peripherals = pi5_hal::init();
//!     let uart = Uart::new(peripherals.UART10, Config::default()).unwrap().into_async();
//!     let led = Output::new(peripherals.GPIO21, Level::High, OutputConfig::default());
//!     spawner.spawn(blink_task(led, uart).unwrap());
//! }
//!
//! #[pi5_executor::task]
//! async fn blink_task(mut led: Output, mut uart: Uart<pi5_hal::Async>) {
//!     uart.write_async(b"TEST\n").await.unwrap();
//!     loop {
//!         led.toggle();
//!         Timer::after(Duration::from_millis(500)).await;
//!     }
//! }
//! ```
//!
//! ## The layers
//!
//! ```text
//!   your program          examples/*.rs
//!   ------------------------------------------------------------------
//!   Layer 2  async        pi5-executor  pi5-time  pi5-sync  pi5-futures
//!   Layer 1  drivers      pi5-hal (this crate): gpio uart i2c spi pwm,
//!                         rt (startup), mmu, multicore, Peripherals
//!   Layer 0  registers    pi5-pac: addresses + bit definitions, cited
//!   ------------------------------------------------------------------
//!   hardware              BCM2712 (4x Cortex-A76, PL011 debug UART, timer)
//!                            ==PCIe==  RP1 (GPIO, UART0, I2C, SPI, PWM)
//! ```
//!
//! Each driver's documentation names the `pi5_pac` constants it uses, so you
//! can always drop one layer to see which register is poked and why.
//!
//! ## The hardware in one paragraph
//!
//! The **BCM2712** holds the four Cortex-A76 cores, RAM, and a few "old
//! style" peripherals, among them the PL011 debug UART on the 3-pin JST
//! connector. The **RP1** is a separate I/O chip on a 4-lane PCIe link; it
//! owns the 40-pin header GPIOs and the UART/I2C/SPI/PWM blocks behind them.
//! From the CPU's point of view RP1's registers are just addresses inside a
//! PCIe window (`0x1F_0000_0000 + ...`), so a GPIO write is an ordinary
//! `str` instruction that happens to travel over PCIe.
//!
//! ## What the runtime sets up before `main`
//!
//! See [`rt`]: stack, zeroed `.bss`, exception vectors, FP/SIMD access, the
//! timer event stream, and the **MMU** with an identity map (RAM cacheable,
//! peripherals as Device memory). With the MMU off, AArch64 treats all memory
//! as Device memory: no caches (slow) and no working atomics (the exclusive
//! instructions behind `AtomicBool::compare_exchange` are not guaranteed on
//! Device memory). Both matter for a multi-core executor, which is why the
//! MMU is on by default. See [`mmu`].
//!
//! ## Address sources
//!
//! Every address comes from `raspberrypi/linux` branch `rpi-6.12.y` through
//! [`pi5_pac`]. **None have been verified on hardware by the author.** The
//! workspace README lists every assumption.

#![no_std]
#![deny(unsafe_op_in_unsafe_fn)]
#![warn(missing_docs)]

mod asyncutil;
pub mod clocks;
pub mod cpu;
pub mod delay;
pub mod gpio;
pub mod i2c;
pub mod mmu;
pub mod multicore;
pub mod peripherals;
pub mod psci;
pub mod pwm;
pub mod spi;
pub mod sync;
pub mod time;
pub mod uart;

mod mode;
pub use mode::{Async, Blocking, DriverMode};

#[cfg(feature = "rt")]
pub mod rt;

#[cfg(feature = "panic-handler")]
mod panic;

pub use peripherals::Peripherals;

/// Re-exports so user code can name the traits and the register layer
/// without adding dependencies.
pub use embedded_hal;
pub use pi5_pac as pac;

/// Take the peripherals, panicking if they were already taken.
///
/// Equivalent to `Peripherals::take().unwrap()`, mirroring `esp_hal::init`.
pub fn init() -> Peripherals {
    Peripherals::take().expect("pi5_hal::init() / Peripherals::take() called twice")
}

/// The usual imports: `use pi5_hal::prelude::*;`.
pub mod prelude {
    pub use crate::delay::Delay;
    pub use crate::gpio::{AnyPin, Input, InputConfig, Level, Output, OutputConfig, Pin, Pull};
    pub use crate::peripherals::Peripherals;
    pub use crate::time::Rate;
    pub use crate::{Async, Blocking};
    pub use core::fmt::Write as _;
    pub use embedded_hal::delay::DelayNs as _;
    pub use embedded_hal::digital::{InputPin as _, OutputPin as _, StatefulOutputPin as _};
    pub use embedded_hal::i2c::I2c as _;
    pub use embedded_hal::pwm::SetDutyCycle as _;
    pub use embedded_hal::spi::SpiBus as _;
}

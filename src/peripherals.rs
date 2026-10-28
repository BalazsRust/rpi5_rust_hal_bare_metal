//! The peripheral singleton.
//!
//! There is exactly one GPIO21 in the world, so there should be exactly one
//! *value* of type `GPIO21` in the program. Whoever holds it may configure
//! the pin; nobody else can even name it. [`Peripherals::take`] hands out the
//! set once and returns `None` afterwards. All tokens are zero-sized, so this
//! bookkeeping costs nothing at run time.

use crate::gpio::*;
use crate::i2c::{I2C0, I2C1, I2C2, I2C3};
use crate::multicore::{CORE1, CORE2, CORE3};
use crate::adc::ADC;
use crate::pwm::PWM0;
use crate::spi::{SPI0, SPI1};
use crate::uart::{UART0, UART10};
use core::sync::atomic::{AtomicBool, Ordering};

static TAKEN: AtomicBool = AtomicBool::new(false);

/// All peripherals this HAL knows about. Field names match the chip
/// documentation / device tree, hence the capitals.
#[allow(non_snake_case)]
#[derive(Debug)]
pub struct Peripherals {
    /// GPIO0, header 27 (ID_SD: HAT EEPROM, normally leave alone).
    pub GPIO0: GPIO0,
    /// GPIO1, header 28 (ID_SC: HAT EEPROM, normally leave alone).
    pub GPIO1: GPIO1,
    /// GPIO2, header 3 (I2C1 SDA; 1.8 kOhm pull-up on the board).
    pub GPIO2: GPIO2,
    /// GPIO3, header 5 (I2C1 SCL; 1.8 kOhm pull-up on the board).
    pub GPIO3: GPIO3,
    /// GPIO4, header 7.
    pub GPIO4: GPIO4,
    /// GPIO5, header 29.
    pub GPIO5: GPIO5,
    /// GPIO6, header 31.
    pub GPIO6: GPIO6,
    /// GPIO7, header 26 (SPI0 CE1).
    pub GPIO7: GPIO7,
    /// GPIO8, header 24 (SPI0 CE0).
    pub GPIO8: GPIO8,
    /// GPIO9, header 21 (SPI0 MISO).
    pub GPIO9: GPIO9,
    /// GPIO10, header 19 (SPI0 MOSI).
    pub GPIO10: GPIO10,
    /// GPIO11, header 23 (SPI0 SCLK).
    pub GPIO11: GPIO11,
    /// GPIO12, header 32 (PWM0 channel 0).
    pub GPIO12: GPIO12,
    /// GPIO13, header 33 (PWM0 channel 1).
    pub GPIO13: GPIO13,
    /// GPIO14, header 8 (UART0 TXD).
    pub GPIO14: GPIO14,
    /// GPIO15, header 10 (UART0 RXD).
    pub GPIO15: GPIO15,
    /// GPIO16, header 36.
    pub GPIO16: GPIO16,
    /// GPIO17, header 11.
    pub GPIO17: GPIO17,
    /// GPIO18, header 12 (PWM0 channel 2).
    pub GPIO18: GPIO18,
    /// GPIO19, header 35 (PWM0 channel 3).
    pub GPIO19: GPIO19,
    /// GPIO20, header 38.
    pub GPIO20: GPIO20,
    /// GPIO21, header 40.
    pub GPIO21: GPIO21,
    /// GPIO22, header 15.
    pub GPIO22: GPIO22,
    /// GPIO23, header 16.
    pub GPIO23: GPIO23,
    /// GPIO24, header 18.
    pub GPIO24: GPIO24,
    /// GPIO25, header 22.
    pub GPIO25: GPIO25,
    /// GPIO26, header 37.
    pub GPIO26: GPIO26,
    /// GPIO27, header 13.
    pub GPIO27: GPIO27,

    /// BCM2712 debug UART on the 3-pin JST connector (`/dev/ttyAMA10`).
    pub UART10: UART10,
    /// RP1 UART0 on GPIO14/15 (`/dev/ttyAMA0`).
    pub UART0: UART0,
    /// RP1 I2C0 (GPIO0/1 or 8/9).
    pub I2C0: I2C0,
    /// RP1 I2C1 (GPIO2/3, the usual bus).
    pub I2C1: I2C1,
    /// RP1 I2C2 (GPIO4/5 or 12/13).
    pub I2C2: I2C2,
    /// RP1 I2C3 (GPIO6/7, 14/15 or 22/23).
    pub I2C3: I2C3,
    /// RP1 SPI0 (GPIO7-11).
    pub SPI0: SPI0,
    /// RP1 SPI1 (GPIO16-21).
    pub SPI1: SPI1,
    /// RP1 PWM0 (GPIO12/13/18/19).
    pub PWM0: PWM0,
    /// RP1 ADC (temperature sensor; its inputs are not on the Pi 5 header).
    pub ADC: ADC,

    /// CPU core 1 (see [`crate::multicore`]).
    pub CORE1: CORE1,
    /// CPU core 2.
    pub CORE2: CORE2,
    /// CPU core 3.
    pub CORE3: CORE3,
}

impl Peripherals {
    /// Hand out the peripherals. `Some` the first time, `None` ever after.
    ///
    /// Uses an atomic swap, which is safe across cores because the runtime
    /// turned the MMU on before `main` (see [`crate::mmu`]).
    pub fn take() -> Option<Self> {
        if TAKEN.swap(true, Ordering::AcqRel) {
            return None;
        }
        // SAFETY: the flag guarantees this is the only instance.
        Some(unsafe { Self::steal() })
    }

    /// Create a second set of peripherals, bypassing the singleton check.
    ///
    /// # Safety
    /// You are responsible for not using the same peripheral from two places.
    pub unsafe fn steal() -> Self {
        Self {
            GPIO0: GPIO0::new(),
            GPIO1: GPIO1::new(),
            GPIO2: GPIO2::new(),
            GPIO3: GPIO3::new(),
            GPIO4: GPIO4::new(),
            GPIO5: GPIO5::new(),
            GPIO6: GPIO6::new(),
            GPIO7: GPIO7::new(),
            GPIO8: GPIO8::new(),
            GPIO9: GPIO9::new(),
            GPIO10: GPIO10::new(),
            GPIO11: GPIO11::new(),
            GPIO12: GPIO12::new(),
            GPIO13: GPIO13::new(),
            GPIO14: GPIO14::new(),
            GPIO15: GPIO15::new(),
            GPIO16: GPIO16::new(),
            GPIO17: GPIO17::new(),
            GPIO18: GPIO18::new(),
            GPIO19: GPIO19::new(),
            GPIO20: GPIO20::new(),
            GPIO21: GPIO21::new(),
            GPIO22: GPIO22::new(),
            GPIO23: GPIO23::new(),
            GPIO24: GPIO24::new(),
            GPIO25: GPIO25::new(),
            GPIO26: GPIO26::new(),
            GPIO27: GPIO27::new(),
            UART10: UART10::new(),
            UART0: UART0::new(),
            I2C0: I2C0::new(),
            I2C1: I2C1::new(),
            I2C2: I2C2::new(),
            I2C3: I2C3::new(),
            SPI0: SPI0::new(),
            SPI1: SPI1::new(),
            PWM0: PWM0::new(),
            ADC: ADC::new(),
            CORE1: CORE1::new(),
            CORE2: CORE2::new(),
            CORE3: CORE3::new(),
        }
    }
}

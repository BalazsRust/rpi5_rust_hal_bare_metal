//! RP1 ADC: 12-bit, 500 kS/s, 4 inputs + an on-chip temperature sensor.
//!
//! ```ignore
//! let mut adc = Adc::new(p.ADC);
//! let t = adc.read_temperature_millicelsius()?;     // e.g. 48_300 = 48.3 C
//! let raw = adc.read_raw(AdcChannel::Ain0)?;         // 0..=4095
//! let mv = adc.read_millivolts(AdcChannel::Ain0)?;   // 0..=3300
//! ```
//!
//! IMPORTANT for the Raspberry Pi 5: the four ADC inputs are dedicated RP1
//! pins, NOT GPIOs, and the board does not bring them out to the 40-pin
//! header. So you can't measure your own voltage with this ADC on a Pi 5;
//! the temperature sensor is the useful part. For real analog inputs use an
//! external ADC (ADS1115 on I2C, MCP3008 on SPI) or let the ESP32 measure.
//!
//! Sequence and register layout follow the Linux driver
//! (`drivers/hwmon/rp1-adc.c`): select the input, start one conversion,
//! wait for READY, check ERR, read RESULT. Reference: the 3.3 V rail
//! (`vref-supply = <&rp1_vdd_3v3>` in `rp1.dtsi`).

use crate::delay::Timeout;
use core::fmt;
use core::time::Duration;
use pi5_pac::mmio::{read32, write32};
use pi5_pac::rp1::ADC_BASE;
use pi5_pac::rp1_adc as r;

/// Reference voltage in millivolts: full scale (4095) = 3.3 V.
pub const VREF_MV: u32 = 3300;

/// Ownership token for the ADC; lives in [`crate::Peripherals`].
#[derive(Debug)]
pub struct ADC {
    _private: (),
}
impl ADC {
    pub(crate) const fn new() -> Self {
        Self { _private: () }
    }
}

/// The four external ADC inputs (not on the Pi 5's header, see the module
/// docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdcChannel {
    /// RP1 ADC input 0.
    Ain0 = 0,
    /// RP1 ADC input 1.
    Ain1 = 1,
    /// RP1 ADC input 2.
    Ain2 = 2,
    /// RP1 ADC input 3.
    Ain3 = 3,
}

/// Why a conversion failed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdcError {
    /// READY never came (ADC not clocked/powered?).
    Timeout,
    /// The ADC flagged the conversion as bad (`CS.ERR`).
    Conversion,
}

impl fmt::Display for AdcError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AdcError::Timeout => write!(f, "ADC conversion timed out"),
            AdcError::Conversion => write!(f, "ADC reported a conversion error"),
        }
    }
}

/// The RP1 ADC driver.
#[derive(Debug)]
pub struct Adc {
    _adc: ADC,
}

impl Adc {
    /// Clock the ADC from the 50 MHz crystal and power it up.
    pub fn new(adc: ADC) -> Self {
        crate::clocks::enable_adc_clock();
        // SAFETY: we own the ADC. Interrupts off (the driver polls), then
        // power the ADC on.
        unsafe {
            write32(ADC_BASE + r::INTE, 0);
            write32(ADC_BASE + r::SET + r::CS, r::CS_EN);
        }
        // Wait until it reports ready for the first conversion.
        let t = Timeout::new(Duration::from_millis(10));
        while !t.expired() && unsafe { read32(ADC_BASE + r::CS) } & r::CS_READY == 0 {}
        Self { _adc: adc }
    }

    /// One conversion of `input` (0-3, or 4 = temperature sensor).
    fn convert(&mut self, input: u32) -> Result<u16, AdcError> {
        // SAFETY: we own the ADC; SET/CLR aliases change only the given bits.
        unsafe {
            write32(ADC_BASE + r::CLR + r::CS, r::CS_AINSEL_MASK << r::CS_AINSEL_SHIFT);
            write32(ADC_BASE + r::SET + r::CS, input << r::CS_AINSEL_SHIFT);
            write32(ADC_BASE + r::SET + r::CS, r::CS_START_ONCE);
        }
        // One conversion takes 96 ADC clocks (~2 us at 50 MHz).
        let t = Timeout::new(Duration::from_millis(1));
        loop {
            // SAFETY: reading the status register.
            let cs = unsafe { read32(ADC_BASE + r::CS) };
            if cs & r::CS_READY != 0 {
                if cs & r::CS_ERR != 0 {
                    return Err(AdcError::Conversion);
                }
                break;
            }
            if t.expired() {
                return Err(AdcError::Timeout);
            }
        }
        // SAFETY: reading the result register.
        Ok((unsafe { read32(ADC_BASE + r::RESULT) } & r::RESULT_MASK) as u16)
    }

    /// Raw 12-bit reading of `channel`: 0 (0 V) ..= 4095 (3.3 V).
    pub fn read_raw(&mut self, channel: AdcChannel) -> Result<u16, AdcError> {
        self.convert(channel as u32)
    }

    /// Voltage on `channel` in millivolts (0..=3300).
    pub fn read_millivolts(&mut self, channel: AdcChannel) -> Result<u32, AdcError> {
        Ok(raw_to_millivolts(self.read_raw(channel)?))
    }

    /// RP1 chip temperature in thousandths of a degree Celsius
    /// (48_300 = 48.3 C). Formula from the Linux driver:
    /// T = 27 - (V - 0.706 V) / 0.001721 V/C.
    pub fn read_temperature_millicelsius(&mut self) -> Result<i32, AdcError> {
        // SAFETY: we own the ADC; powers the temperature sensor up.
        unsafe { write32(ADC_BASE + r::SET + r::CS, r::CS_TS_EN) };
        let mv = raw_to_millivolts(self.convert(r::TEMP_INPUT)?) as i64;
        Ok((27_000 - (mv - 706) * 1_000_000 / 1721) as i32)
    }
}

/// Raw 12-bit value -> millivolts, rounded (same as Linux:
/// `vref_mv * val / 0xfff`).
pub fn raw_to_millivolts(raw: u16) -> u32 {
    (VREF_MV * u32::from(raw) + 0xfff / 2) / 0xfff
}

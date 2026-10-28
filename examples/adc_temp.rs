//! Read the RP1 ADC: chip temperature + the 4 raw inputs, once a second.
//!
//!   ./build.sh adc_temp && python3 tools/pi5load.py adc_temp
//!
//! On a Pi 5 the 4 ADC inputs are not on the 40-pin header, so their
//! numbers are whatever the board does with those RP1 pins; the
//! temperature is the meaningful reading.

#![no_std]
#![no_main]

use pi5_hal::adc::{Adc, AdcChannel};
use pi5_hal::delay::Delay;
use pi5_hal::{error, info};

pi5_hal::entry!(main);

fn main() -> ! {
    let p = pi5_hal::init();
    let delay = Delay::new();
    let mut adc = Adc::new(p.ADC);
    info!("RP1 clk_adc = {} Hz", pi5_hal::clocks::enable_adc_clock());

    loop {
        match adc.read_temperature_millicelsius() {
            Ok(mc) => info!("RP1 temperature: {}.{} C", mc / 1000, (mc % 1000).abs() / 100),
            Err(e) => error!("temperature: {e}"),
        }
        for ch in [AdcChannel::Ain0, AdcChannel::Ain1, AdcChannel::Ain2, AdcChannel::Ain3] {
            match adc.read_raw(ch) {
                Ok(raw) => info!("  {ch:?}: raw {raw:4} = {} mV", pi5_hal::adc::raw_to_millivolts(raw)),
                Err(e) => error!("  {ch:?}: {e}"),
            }
        }
        delay.delay_millis(1000);
    }
}

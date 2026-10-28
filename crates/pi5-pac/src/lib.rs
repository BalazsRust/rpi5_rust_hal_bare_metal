//! # pi5-pac: Layer 0, the register map
//!
//! "PAC" = Peripheral Access Crate. This crate contains **only facts about the
//! hardware**: where registers live and what their bits mean, plus the two
//! volatile load/store functions needed to touch them. No drivers, no logic,
//! no state. When you want to know what a HAL function *really* does, follow
//! its constants down here; every constant says which source file and line
//! it was taken from.
//!
//! All sources are the Raspberry Pi Linux tree, `raspberrypi/linux`, branch
//! `rpi-6.12.y`, unless noted otherwise:
//!
//! | module          | source                                                          |
//! |-----------------|-----------------------------------------------------------------|
//! | [`bcm2712`]     | `arch/arm64/boot/dts/broadcom/bcm2712.dtsi`, `bcm2712-rpi-5-b.dts` |
//! | [`rp1`]         | `arch/arm64/boot/dts/broadcom/rp1.dtsi`                          |
//! | [`rp1_gpio`]    | `drivers/pinctrl/pinctrl-rp1.c`                                  |
//! | [`rp1_pins`]    | `drivers/pinctrl/pinctrl-rp1.c` (the `PIN(...)` function table)  |
//! | [`rp1_pwm`]     | `drivers/pwm/pwm-rp1.c`                                          |
//! | [`rp1_clocks`]  | `drivers/clk/clk-rp1.c`                                          |
//! | [`pl011`]       | ARM DDI 0183 (PrimeCell UART PL011 TRM)                           |
//! | [`dw_i2c`]      | `drivers/i2c/busses/i2c-designware-core.h`                       |
//! | [`dw_spi`]      | `drivers/spi/spi-dw.h`                                           |
//!
//! **None of the addresses have been verified on hardware** by the author of
//! this crate. They are derived from the sources above; the README of the
//! workspace lists the assumptions.

#![no_std]
#![warn(missing_docs)]

pub mod bcm2712;
pub mod rp1_adc;
pub mod dw_i2c;
pub mod dw_spi;
pub mod mmio;
pub mod pl011;
pub mod rp1;
pub mod rp1_clocks;
pub mod rp1_gpio;
pub mod rp1_pins;
pub mod rp1_pwm;

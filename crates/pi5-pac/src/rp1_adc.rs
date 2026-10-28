//! RP1 ADC registers (`drivers/hwmon/rp1-adc.c` in the Raspberry Pi kernel).
//! 12-bit SAR ADC, 500 kS/s, 4 external inputs + an on-chip temperature
//! sensor (input 4). Atomic aliases: +0x2000 SET, +0x3000 CLR.

/// Control / status.
pub const CS: usize = 0x00;
/// Result of the last conversion (12 bits).
pub const RESULT: usize = 0x04;
/// FIFO control / status.
pub const FCS: usize = 0x08;
/// FIFO data.
pub const FIFO: usize = 0x0c;
/// Clock divider for free-running mode.
pub const DIV: usize = 0x10;
/// Interrupt enable.
pub const INTE: usize = 0x18;

/// Write-to-set alias offset.
pub const SET: usize = 0x2000;
/// Write-to-clear alias offset.
pub const CLR: usize = 0x3000;

/// CS: power the ADC up.
pub const CS_EN: u32 = 1 << 0;
/// CS: power the temperature sensor up.
pub const CS_TS_EN: u32 = 1 << 1;
/// CS: start one conversion.
pub const CS_START_ONCE: u32 = 1 << 2;
/// CS: conversion finished (also: ADC ready for the next one).
pub const CS_READY: u32 = 1 << 8;
/// CS: the last conversion had an error.
pub const CS_ERR: u32 = 1 << 9;
/// CS: input select, bits 14:12.
pub const CS_AINSEL_SHIFT: u32 = 12;
/// CS: input select mask (after shifting).
pub const CS_AINSEL_MASK: u32 = 0x7;

/// RESULT: the 12 valid bits.
pub const RESULT_MASK: u32 = 0xfff;

/// Input number of the temperature sensor.
pub const TEMP_INPUT: u32 = 4;

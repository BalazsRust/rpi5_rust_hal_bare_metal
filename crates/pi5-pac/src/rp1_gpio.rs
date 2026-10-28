//! RP1 GPIO registers (`drivers/pinctrl/pinctrl-rp1.c`).
//!
//! Each GPIO pin is touched through three separate blocks:
//!
//! | block    | per-pin register                          | role                                          |
//! |----------|-------------------------------------------|-----------------------------------------------|
//! | IO_BANK  | `STATUS @ 8n+0`, `CTRL @ 8n+4`            | pin mux: which peripheral owns the pin (FUNCSEL) |
//! | SYS_RIO  | `OUT @ 0`, `OE @ 4`, `IN @ 8`, bit n = pin n | software GPIO ("register I/O")               |
//! | PADS     | `GPIO @ 4n+4`                             | the physical pad: pulls, drive strength, input buffer |
//!
//! RP1 has 54 GPIOs in three banks (`pinctrl-rp1.c:291-295`): bank 0 = GPIO
//! 0-27 (the 40-pin header), bank 1 = GPIO 28-33, bank 2 = GPIO 34-53. Inside
//! each block the banks are `0x4000` apart.

use crate::rp1::{IO_BANK0_BASE, PADS_BANK0_BASE, SYS_RIO0_BASE};

/// Total number of GPIOs (`pinctrl-rp1.c:40`).
pub const NUM_GPIOS: u8 = 54;
/// Distance between banks inside each block (`pinctrl-rp1.c:291-295`).
pub const BANK_STRIDE: usize = 0x4000;

/// One row of the `rp1_iobanks[]` table (`pinctrl-rp1.c:291-295`).
#[derive(Debug, Clone, Copy)]
pub struct Bank {
    /// First GPIO number of the bank.
    pub min_gpio: u8,
    /// How many GPIOs the bank has.
    pub num_gpios: u8,
    /// Offset of the bank inside IO_BANK / SYS_RIO.
    pub offset: usize,
    /// Offset of pin 0's pad register inside PADS (the +4 skips the bank's
    /// VOLTAGE_SELECT register at offset 0).
    pub pads_offset: usize,
}

/// `pinctrl-rp1.c:291-295`.
pub const BANKS: [Bank; 3] = [
    Bank { min_gpio: 0, num_gpios: 28, offset: 0x0000, pads_offset: 0x0004 },
    Bank { min_gpio: 28, num_gpios: 6, offset: 0x4000, pads_offset: 0x4004 },
    Bank { min_gpio: 34, num_gpios: 20, offset: 0x8000, pads_offset: 0x8004 },
];

// --- IO_BANK: STATUS / CTRL (pinctrl-rp1.c:48-84) ----------------------------

/// `RP1_GPIO_STATUS`, per pin, stride 8.
pub const GPIO_STATUS: usize = 0x0000;
/// `RP1_GPIO_CTRL`, per pin, stride 8.
pub const GPIO_CTRL: usize = 0x0004;

/// STATUS: raw level events (`pinctrl-rp1.c:55-58`).
pub const STATUS_EVENT_FALLING: u32 = 1 << 20;
/// See [`STATUS_EVENT_FALLING`].
pub const STATUS_EVENT_RISING: u32 = 1 << 21;
/// See [`STATUS_EVENT_FALLING`].
pub const STATUS_EVENT_LOW: u32 = 1 << 22;
/// See [`STATUS_EVENT_FALLING`].
pub const STATUS_EVENT_HIGH: u32 = 1 << 23;

/// CTRL.FUNCSEL, bits 4:0 (`pinctrl-rp1.c:66-67`): which function drives the pad.
pub const CTRL_FUNCSEL_MASK: u32 = 0x0000_001f;
/// CTRL.OUTOVER, bits 13:12 (`:68-69`): 0 = output from peripheral, 1 = inverted,
/// 2 = force low, 3 = force high.
pub const CTRL_OUTOVER_MASK: u32 = 0x0000_3000;
/// CTRL.OEOVER, bits 15:14 (`:70-71`): 0 = output-enable from peripheral,
/// 1 = inverted, 2 = force disabled, 3 = force enabled.
pub const CTRL_OEOVER_MASK: u32 = 0x0000_c000;
/// CTRL.INOVER, bits 17:16 (`:72-73`): 0 = input from pad, 1 = inverted, 2/3 = force.
pub const CTRL_INOVER_MASK: u32 = 0x0003_0000;
/// CTRL: interrupt-enable bits 20-27 and IRQRESET bit 28 (`:74-82`). Unused here.
pub const CTRL_IRQEN_MASK: u32 = 0x1FF0_0000;

/// FUNCSEL value for "software GPIO via SYS_RIO" (`RP1_FSEL_GPIO`, `:101`).
pub const FSEL_SYS_RIO: u32 = 0x05;
/// FUNCSEL value that disconnects the pad (`RP1_FSEL_NONE_HW`, `:103`).
pub const FSEL_NONE: u32 = 0x1f;
/// Number of real alternate functions, FSEL 0..=8 (`RP1_FSEL_COUNT`, `:98`).
pub const FSEL_COUNT: u32 = 9;

// --- SYS_RIO (pinctrl-rp1.c:123-125) ----------------------------------------

/// Output level, one bit per pin of the bank.
pub const RIO_OUT: usize = 0x00;
/// Output enable, one bit per pin (1 = drive the pad).
pub const RIO_OE: usize = 0x04;
/// Input level as seen on the pad, one bit per pin.
pub const RIO_IN: usize = 0x08;

// --- PADS (pinctrl-rp1.c:127-143) --------------------------------------------

/// Faster output edges (more EMI).
pub const PAD_SLEWFAST: u32 = 1 << 0;
/// Input hysteresis (Schmitt trigger): clean edges from slow/noisy signals.
pub const PAD_SCHMITT: u32 = 1 << 1;
/// PULL field, bits 3:2: 0 = none, 1 = down, 2 = up (`RP1_PUD_*`, `:94-96`).
pub const PAD_PULL_MASK: u32 = 0x0000_000c;
/// See [`PAD_PULL_MASK`].
pub const PAD_PULL_DOWN: u32 = 1 << 2;
/// See [`PAD_PULL_MASK`].
pub const PAD_PULL_UP: u32 = 2 << 2;
/// DRIVE field, bits 5:4: 0 = 2 mA, 1 = 4 mA, 2 = 8 mA, 3 = 12 mA (`:140-143`).
pub const PAD_DRIVE_MASK: u32 = 0x0000_0030;
/// See [`PAD_DRIVE_MASK`].
pub const PAD_DRIVE_SHIFT: u32 = 4;
/// Input buffer enable. Without it the pin always reads 0.
pub const PAD_IN_ENABLE: u32 = 1 << 6;
/// Output driver forced off, regardless of OE.
pub const PAD_OUT_DISABLE: u32 = 1 << 7;

// --- Address helpers ---------------------------------------------------------

/// Bank index and offset-within-bank of a GPIO (`pinctrl-rp1.c:1605-1614`).
pub const fn bank_of(gpio: u8) -> (usize, usize) {
    match gpio {
        0..=27 => (0, gpio as usize),
        28..=33 => (1, gpio as usize - 28),
        _ => (2, gpio as usize - 34),
    }
}

/// CPU address of the pin's IO_BANK `CTRL` register
/// (`pinctrl-rp1.c:1616-1617`: `gpio_base + bank->gpio_offset + j * 8`, then `+ 4`).
pub const fn ctrl_addr(gpio: u8) -> usize {
    let (bank, j) = bank_of(gpio);
    IO_BANK0_BASE + BANKS[bank].offset + j * 8 + GPIO_CTRL
}

/// CPU address of the pin's IO_BANK `STATUS` register.
pub const fn status_addr(gpio: u8) -> usize {
    let (bank, j) = bank_of(gpio);
    IO_BANK0_BASE + BANKS[bank].offset + j * 8 + GPIO_STATUS
}

/// CPU address of the SYS_RIO block of the pin's bank
/// (`pinctrl-rp1.c:1620`: `rio_base + bank->rio_offset`). Add [`RIO_OUT`] etc.
pub const fn rio_base(gpio: u8) -> usize {
    let (bank, _) = bank_of(gpio);
    SYS_RIO0_BASE + BANKS[bank].offset
}

/// CPU address of the pin's PADS register
/// (`pinctrl-rp1.c:1621-1622`: `pads_base + bank->pads_offset + j * 4`).
pub const fn pad_addr(gpio: u8) -> usize {
    let (bank, j) = bank_of(gpio);
    PADS_BANK0_BASE + BANKS[bank].pads_offset + j * 4
}

/// The pin's bit inside its bank's SYS_RIO words.
pub const fn rio_mask(gpio: u8) -> u32 {
    let (_, j) = bank_of(gpio);
    1 << j
}

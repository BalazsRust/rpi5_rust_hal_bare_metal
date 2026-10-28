//! Which alternate function each header GPIO gets for each FUNCSEL value.
//!
//! Copied from the `PIN(n, f0..f8)` table in `pinctrl-rp1.c:481-509`: entry
//! `k` of a row is the function selected by writing `k` into CTRL.FUNCSEL.
//! FUNCSEL 5 is always SYS_RIO (software GPIO).
//!
//! Also the 40-pin header position of each GPIO, for documentation.

/// A peripheral function a pad can be connected to. Only the ones that appear
/// on header pins are listed; `None` marks an unused slot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Function {
    /// Unused slot.
    Unused,
    /// SPI0..SPI5 (DesignWare SSI).
    Spi(u8),
    /// Parallel display (DPI).
    Dpi,
    /// PL011 UART0..UART4.
    Uart(u8),
    /// DesignWare I2C0..I2C3.
    I2c(u8),
    /// Infrared.
    Ir,
    /// Software GPIO through SYS_RIO.
    SysRio,
    /// GPIO through the RP1's internal processor.
    ProcRio,
    /// Programmable I/O.
    Pio,
    /// General-purpose clock output 0..2.
    Gpclk(u8),
    /// UART0 modem lines on GPIO4-7 (RI, DTR, DCD, DSR).
    UartModem,
    /// PWM0 channel (the channel number is in [`pwm0_channel`]).
    Pwm0,
    /// Analogue audio.
    Aaud,
    /// I2S0 / I2S1.
    I2s(u8),
    /// SD card interface.
    Sd0,
    /// Display tearing-effect inputs.
    DsiTe,
    /// Microphone interface.
    Mic,
}

use Function::*;

/// `pinctrl-rp1.c:482-509`, rows for GPIO0..=GPIO27.
pub const FUNCTIONS: [[Function; 9]; 28] = [
    /* 0 */ [Spi(0), Dpi, Uart(1), I2c(0), Unused, SysRio, ProcRio, Pio, Spi(2)],
    /* 1 */ [Spi(0), Dpi, Uart(1), I2c(0), Unused, SysRio, ProcRio, Pio, Spi(2)],
    /* 2 */ [Spi(0), Dpi, Uart(1), I2c(1), Ir, SysRio, ProcRio, Pio, Spi(2)],
    /* 3 */ [Spi(0), Dpi, Uart(1), I2c(1), Ir, SysRio, ProcRio, Pio, Spi(2)],
    /* 4 */ [Gpclk(0), Dpi, Uart(2), I2c(2), UartModem, SysRio, ProcRio, Pio, Spi(3)],
    /* 5 */ [Gpclk(1), Dpi, Uart(2), I2c(2), UartModem, SysRio, ProcRio, Pio, Spi(3)],
    /* 6 */ [Gpclk(2), Dpi, Uart(2), I2c(3), UartModem, SysRio, ProcRio, Pio, Spi(3)],
    /* 7 */ [Spi(0), Dpi, Uart(2), I2c(3), UartModem, SysRio, ProcRio, Pio, Spi(3)],
    /* 8 */ [Spi(0), Dpi, Uart(3), I2c(0), Unused, SysRio, ProcRio, Pio, Spi(4)],
    /* 9 */ [Spi(0), Dpi, Uart(3), I2c(0), Unused, SysRio, ProcRio, Pio, Spi(4)],
    /* 10 */ [Spi(0), Dpi, Uart(3), I2c(1), Unused, SysRio, ProcRio, Pio, Spi(4)],
    /* 11 */ [Spi(0), Dpi, Uart(3), I2c(1), Unused, SysRio, ProcRio, Pio, Spi(4)],
    /* 12 */ [Pwm0, Dpi, Uart(4), I2c(2), Aaud, SysRio, ProcRio, Pio, Spi(5)],
    /* 13 */ [Pwm0, Dpi, Uart(4), I2c(2), Aaud, SysRio, ProcRio, Pio, Spi(5)],
    /* 14 */ [Pwm0, Dpi, Uart(4), I2c(3), Uart(0), SysRio, ProcRio, Pio, Spi(5)],
    /* 15 */ [Pwm0, Dpi, Uart(4), I2c(3), Uart(0), SysRio, ProcRio, Pio, Spi(5)],
    /* 16 */ [Spi(1), Dpi, DsiTe, Unused, Uart(0), SysRio, ProcRio, Pio, Unused],
    /* 17 */ [Spi(1), Dpi, DsiTe, Unused, Uart(0), SysRio, ProcRio, Pio, Unused],
    /* 18 */ [Spi(1), Dpi, I2s(0), Pwm0, I2s(1), SysRio, ProcRio, Pio, Gpclk(1)],
    /* 19 */ [Spi(1), Dpi, I2s(0), Pwm0, I2s(1), SysRio, ProcRio, Pio, Unused],
    /* 20 */ [Spi(1), Dpi, I2s(0), Gpclk(0), I2s(1), SysRio, ProcRio, Pio, Unused],
    /* 21 */ [Spi(1), Dpi, I2s(0), Gpclk(1), I2s(1), SysRio, ProcRio, Pio, Unused],
    /* 22 */ [Sd0, Dpi, I2s(0), I2c(3), I2s(1), SysRio, ProcRio, Pio, Unused],
    /* 23 */ [Sd0, Dpi, I2s(0), I2c(3), I2s(1), SysRio, ProcRio, Pio, Unused],
    /* 24 */ [Sd0, Dpi, I2s(0), Unused, I2s(1), SysRio, ProcRio, Pio, Spi(2)],
    /* 25 */ [Sd0, Dpi, I2s(0), Mic, I2s(1), SysRio, ProcRio, Pio, Spi(3)],
    /* 26 */ [Sd0, Dpi, I2s(0), Mic, I2s(1), SysRio, ProcRio, Pio, Spi(5)],
    /* 27 */ [Sd0, Dpi, I2s(0), Mic, I2s(1), SysRio, ProcRio, Pio, Spi(1)],
];

/// The FUNCSEL value that connects `gpio` to `function`, if that pad has it.
pub const fn fsel_for(gpio: u8, function: Function) -> Option<u8> {
    if gpio >= 28 {
        return None;
    }
    let row = &FUNCTIONS[gpio as usize];
    let mut k = 0;
    while k < 9 {
        if matches_fn(row[k], function) {
            return Some(k as u8);
        }
        k += 1;
    }
    None
}

const fn matches_fn(a: Function, b: Function) -> bool {
    match (a, b) {
        (Unused, Unused) => false, // "unused" never matches anything
        (Spi(x), Spi(y)) | (Uart(x), Uart(y)) | (I2c(x), I2c(y)) | (Gpclk(x), Gpclk(y)) | (I2s(x), I2s(y)) => x == y,
        (Dpi, Dpi) | (Ir, Ir) | (SysRio, SysRio) | (ProcRio, ProcRio) | (Pio, Pio) | (UartModem, UartModem)
        | (Pwm0, Pwm0) | (Aaud, Aaud) | (Sd0, Sd0) | (DsiTe, DsiTe) | (Mic, Mic) => true,
        _ => false,
    }
}

/// PWM0 channel driven by a GPIO when its FUNCSEL selects [`Function::Pwm0`]
/// (Raspberry Pi `pwm-2chan` overlay documentation: GPIO12 ch0, GPIO13 ch1,
/// GPIO14 ch2, GPIO15 ch3, GPIO18 ch2, GPIO19 ch3).
pub const fn pwm0_channel(gpio: u8) -> Option<u8> {
    match gpio {
        12 => Some(0),
        13 => Some(1),
        14 | 18 => Some(2),
        15 | 19 => Some(3),
        _ => None,
    }
}

/// Role of a GPIO on SPI0/SPI1 when its FUNCSEL selects that SPI.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpiRole {
    /// Serial clock.
    Sclk,
    /// Master out, slave in.
    Mosi,
    /// Master in, slave out.
    Miso,
    /// Chip select `n`.
    Cs(u8),
}

/// SPI0 (GPIO7-11, FSEL 0) and SPI1 (GPIO16-21, FSEL 0) header roles, from the
/// classic Raspberry Pi pinout (the RP1 keeps the BCM283x assignment).
pub const fn spi_role(spi: u8, gpio: u8) -> Option<SpiRole> {
    match (spi, gpio) {
        (0, 7) => Some(SpiRole::Cs(1)),
        (0, 8) => Some(SpiRole::Cs(0)),
        (0, 9) => Some(SpiRole::Miso),
        (0, 10) => Some(SpiRole::Mosi),
        (0, 11) => Some(SpiRole::Sclk),
        (1, 16) => Some(SpiRole::Cs(2)),
        (1, 17) => Some(SpiRole::Cs(1)),
        (1, 18) => Some(SpiRole::Cs(0)),
        (1, 19) => Some(SpiRole::Miso),
        (1, 20) => Some(SpiRole::Mosi),
        (1, 21) => Some(SpiRole::Sclk),
        _ => None,
    }
}

/// Role of a GPIO on an I2C bus: the even pin of each pair is SDA, the odd is SCL.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum I2cRole {
    /// Serial data.
    Sda,
    /// Serial clock.
    Scl,
}

/// Which I2C line a header pin carries when muxed to I2C (`pinctrl-rp1.c`
/// pairs pins as SDA/SCL; the Pi 5 pinout confirms GPIO2 = SDA, GPIO3 = SCL).
pub const fn i2c_role(gpio: u8) -> I2cRole {
    if gpio.is_multiple_of(2) { I2cRole::Sda } else { I2cRole::Scl }
}

/// Role of a GPIO on UART0 when muxed to it (FSEL 4): GPIO14 TXD, GPIO15 RXD,
/// GPIO16 CTS, GPIO17 RTS.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UartRole {
    /// Transmit data.
    Tx,
    /// Receive data.
    Rx,
    /// Clear to send (input).
    Cts,
    /// Request to send (output).
    Rts,
}

/// See [`UartRole`].
pub const fn uart0_role(gpio: u8) -> Option<UartRole> {
    match gpio {
        14 => Some(UartRole::Tx),
        15 => Some(UartRole::Rx),
        16 => Some(UartRole::Cts),
        17 => Some(UartRole::Rts),
        _ => None,
    }
}

/// Physical 40-pin header position of GPIO0..=GPIO27.
pub const HEADER_PIN: [u8; 28] = [
    27, 28, 3, 5, 7, 29, 31, 26, 24, 21, 19, 23, 32, 33, 8, 10, 36, 11, 12, 35, 38, 40, 15, 16, 18, 22, 37, 13,
];

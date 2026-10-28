//! ARM PrimeCell PL011 UART registers (ARM DDI 0183G, chapter 3).
//!
//! Both UARTs this HAL knows about are PL011s: `uart10` inside the BCM2712
//! and `uart0` inside RP1 (`rp1.dtsi:62-64`). Same registers, different base
//! address and clock.

/// Data register: write = transmit, read = receive. Bits 8-11 of a read carry
/// the error flags of that byte.
pub const DR: usize = 0x000;
/// Receive status / error clear.
pub const RSR_ECR: usize = 0x004;
/// Flag register.
pub const FR: usize = 0x018;
/// IrDA low-power counter (unused).
pub const ILPR: usize = 0x020;
/// Integer baud rate divisor (16 bits).
pub const IBRD: usize = 0x024;
/// Fractional baud rate divisor (6 bits, 1/64ths).
pub const FBRD: usize = 0x028;
/// Line control: word length, FIFOs, parity, stop bits.
pub const LCR_H: usize = 0x02C;
/// Control: enables.
pub const CR: usize = 0x030;
/// Interrupt FIFO level select.
pub const IFLS: usize = 0x034;
/// Interrupt mask set/clear.
pub const IMSC: usize = 0x038;
/// Raw interrupt status.
pub const RIS: usize = 0x03C;
/// Masked interrupt status.
pub const MIS: usize = 0x040;
/// Interrupt clear.
pub const ICR: usize = 0x044;
/// DMA control.
pub const DMACR: usize = 0x048;
/// Peripheral identification registers 0-3 (one byte each, 4 bytes apart).
pub const PERIPH_ID0: usize = 0xFE0;

/// DR: framing error (no valid stop bit).
pub const DR_FE: u32 = 1 << 8;
/// DR: parity error.
pub const DR_PE: u32 = 1 << 9;
/// DR: break (line held low for a whole frame).
pub const DR_BE: u32 = 1 << 10;
/// DR: overrun (FIFO was full, data lost).
pub const DR_OE: u32 = 1 << 11;

/// FR: clear-to-send input.
pub const FR_CTS: u32 = 1 << 0;
/// FR: UART busy transmitting.
pub const FR_BUSY: u32 = 1 << 3;
/// FR: receive FIFO empty.
pub const FR_RXFE: u32 = 1 << 4;
/// FR: transmit FIFO full.
pub const FR_TXFF: u32 = 1 << 5;
/// FR: receive FIFO full.
pub const FR_RXFF: u32 = 1 << 6;
/// FR: transmit FIFO empty.
pub const FR_TXFE: u32 = 1 << 7;

/// LCR_H: send break.
pub const LCR_H_BRK: u32 = 1 << 0;
/// LCR_H: parity enable.
pub const LCR_H_PEN: u32 = 1 << 1;
/// LCR_H: even parity select (1 = even, 0 = odd).
pub const LCR_H_EPS: u32 = 1 << 2;
/// LCR_H: two stop bits.
pub const LCR_H_STP2: u32 = 1 << 3;
/// LCR_H: enable the 16-entry FIFOs.
pub const LCR_H_FEN: u32 = 1 << 4;
/// LCR_H: word length field, bits 6:5 (0 = 5 bits .. 3 = 8 bits).
pub const LCR_H_WLEN_MASK: u32 = 0b11 << 5;
/// See [`LCR_H_WLEN_MASK`].
pub const LCR_H_WLEN_SHIFT: u32 = 5;
/// LCR_H: stick parity.
pub const LCR_H_SPS: u32 = 1 << 7;

/// CR: UART enable.
pub const CR_UARTEN: u32 = 1 << 0;
/// CR: loopback enable.
pub const CR_LBE: u32 = 1 << 7;
/// CR: transmit enable.
pub const CR_TXE: u32 = 1 << 8;
/// CR: receive enable.
pub const CR_RXE: u32 = 1 << 9;
/// CR: request-to-send output.
pub const CR_RTS: u32 = 1 << 11;
/// CR: hardware RTS flow control.
pub const CR_RTSEN: u32 = 1 << 14;
/// CR: hardware CTS flow control.
pub const CR_CTSEN: u32 = 1 << 15;

/// All interrupt bits, for ICR.
pub const ICR_ALL: u32 = 0x7ff;

/// Depth of the transmit and receive FIFOs.
pub const FIFO_DEPTH: usize = 16;

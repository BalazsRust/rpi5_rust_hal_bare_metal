//! Synopsys DesignWare APB SSI (SPI) registers (`drivers/spi/spi-dw.h`).
//!
//! RP1's SPI controllers are the "APB SSI" flavour (`rp1.dtsi: compatible =
//! "snps,dw-apb-ssi"`), which fixes the CTRLR0 bit layout: the `DW_PSSI_*`
//! definitions, not the `DW_HSSI_*` ones.
//!
//! Operation: write frames into `DR` (they go into the TX FIFO), the
//! controller clocks them out and every received frame lands in the RX FIFO,
//! also read through `DR`. The chip select lines in `SER` are asserted by
//! hardware while there is something to send; they drop as soon as the TX
//! FIFO runs empty, which is why a transfer must keep the FIFO fed.

/// `DW_SPI_CTRLR0` (`:37`).
pub const CTRLR0: usize = 0x00;
/// `DW_SPI_CTRLR1`: number of frames - 1 for receive-only modes (`:38`).
pub const CTRLR1: usize = 0x04;
/// `DW_SPI_SSIENR`: 1 = controller enabled (`:39`). Disabling flushes the FIFOs.
pub const SSIENR: usize = 0x08;
/// `DW_SPI_SER`: slave (chip select) enable bits (`:41`).
pub const SER: usize = 0x10;
/// `DW_SPI_BAUDR`: SCLK divider, even, >= 2 (`:42`).
pub const BAUDR: usize = 0x14;
/// `DW_SPI_TXFTLR`: TX FIFO threshold (`:43`).
pub const TXFTLR: usize = 0x18;
/// `DW_SPI_RXFTLR` (`:44`).
pub const RXFTLR: usize = 0x1c;
/// `DW_SPI_TXFLR`: entries in the TX FIFO (`:45`).
pub const TXFLR: usize = 0x20;
/// `DW_SPI_RXFLR`: entries in the RX FIFO (`:46`).
pub const RXFLR: usize = 0x24;
/// `DW_SPI_SR`: status (`:47`).
pub const SR: usize = 0x28;
/// `DW_SPI_IMR`: interrupt mask (`:48`).
pub const IMR: usize = 0x2c;
/// `DW_SPI_ISR` (`:49`).
pub const ISR: usize = 0x30;
/// `DW_SPI_RISR`: raw interrupt status (`:50`).
pub const RISR: usize = 0x34;
/// `DW_SPI_ICR`: reading clears all interrupts (`:55`).
pub const ICR: usize = 0x48;
/// `DW_SPI_IDR` (`:59`).
pub const IDR: usize = 0x58;
/// `DW_SPI_VERSION`: ASCII version, e.g. "*102" (`:60`).
pub const VERSION: usize = 0x5c;
/// `DW_SPI_DR`: data register (`:61`).
pub const DR: usize = 0x60;
/// `DW_SPI_RX_SAMPLE_DLY` (`:62`).
pub const RX_SAMPLE_DLY: usize = 0xf0;

// --- CTRLR0, APB SSI layout (spi-dw.h:66-87) ------------------------------------

/// Data frame size - 1, bits 3:0 (`DW_PSSI_CTRLR0_DFS_MASK`).
pub const CTRLR0_DFS_MASK: u32 = 0xf;
/// Data frame size - 1, bits 20:16, on cores built with 32-bit frames
/// (`DW_PSSI_CTRLR0_DFS32_MASK`). Which one applies is detected at run time
/// (`spi-dw-core.c`, `dw_spi_hw_init`): write all-ones to CTRLR0 and see
/// whether bits 3:0 stick.
pub const CTRLR0_DFS32_MASK: u32 = 0x1f << 16;
/// Shift of the DFS32 field.
pub const CTRLR0_DFS32_SHIFT: u32 = 16;
/// Frame format, bits 5:4: 0 = Motorola SPI (`:69-70`).
pub const CTRLR0_FRF_MASK: u32 = 0x3 << 4;
/// Clock phase, bit 6 (`:76`).
pub const CTRLR0_SCPH: u32 = 1 << 6;
/// Clock polarity, bit 7 (`:77`).
pub const CTRLR0_SCPOL: u32 = 1 << 7;
/// Transfer mode, bits 9:8: 0 = transmit & receive (`:79-80`).
pub const CTRLR0_TMOD_MASK: u32 = 0x3 << 8;
/// Shift register loop (loopback test mode), bit 11 (`:86`).
pub const CTRLR0_SRL: u32 = 1 << 11;

// --- SR (spi-dw.h:102-109) ---------------------------------------------------

/// Busy.
pub const SR_BUSY: u32 = 1 << 0;
/// TX FIFO not full.
pub const SR_TF_NOT_FULL: u32 = 1 << 1;
/// TX FIFO empty.
pub const SR_TF_EMPTY: u32 = 1 << 2;
/// RX FIFO not empty.
pub const SR_RF_NOT_EMPTY: u32 = 1 << 3;
/// RX FIFO full.
pub const SR_RF_FULL: u32 = 1 << 4;
/// Transmission error.
pub const SR_TX_ERR: u32 = 1 << 5;
/// Data collision (another master drove the bus).
pub const SR_DCOL: u32 = 1 << 6;

// --- RISR / IMR (spi-dw.h:112-118) -------------------------------------------

/// TX FIFO empty.
pub const INT_TXEI: u32 = 1 << 0;
/// TX FIFO overflow.
pub const INT_TXOI: u32 = 1 << 1;
/// RX FIFO underflow.
pub const INT_RXUI: u32 = 1 << 2;
/// RX FIFO overflow: we did not drain fast enough, data was lost.
pub const INT_RXOI: u32 = 1 << 3;
/// RX FIFO full.
pub const INT_RXFI: u32 = 1 << 4;
/// Multi-master contention.
pub const INT_MSTI: u32 = 1 << 5;

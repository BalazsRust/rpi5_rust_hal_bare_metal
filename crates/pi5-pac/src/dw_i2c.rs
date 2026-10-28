//! Synopsys DesignWare APB I2C registers (`drivers/i2c/busses/i2c-designware-core.h`).
//!
//! RP1's seven I2C controllers are this IP (`rp1.dtsi: compatible =
//! "snps,designware-i2c"`). The controller is driven through a command FIFO:
//! each entry in `DATA_CMD` is one byte to send *or* one "read a byte"
//! request, plus flags for RESTART/STOP. Received bytes pop out of the same
//! register.

/// `DW_IC_CON`: configuration (`:47`).
pub const CON: usize = 0x00;
/// `DW_IC_TAR`: target (slave) address (`:48`).
pub const TAR: usize = 0x04;
/// `DW_IC_SAR`: own address when acting as slave (`:49`).
pub const SAR: usize = 0x08;
/// `DW_IC_DATA_CMD`: command FIFO / receive data (`:50`).
pub const DATA_CMD: usize = 0x10;
/// Standard-speed SCL high count.
pub const SS_SCL_HCNT: usize = 0x14;
/// Standard-speed SCL low count.
pub const SS_SCL_LCNT: usize = 0x18;
/// Fast-speed SCL high count.
pub const FS_SCL_HCNT: usize = 0x1c;
/// Fast-speed SCL low count.
pub const FS_SCL_LCNT: usize = 0x20;
/// `DW_IC_INTR_STAT` (`:57`).
pub const INTR_STAT: usize = 0x2c;
/// `DW_IC_INTR_MASK` (`:58`).
pub const INTR_MASK: usize = 0x30;
/// `DW_IC_RAW_INTR_STAT`: status bits regardless of mask (`:59`).
pub const RAW_INTR_STAT: usize = 0x34;
/// `DW_IC_RX_TL`: receive FIFO threshold (`:60`).
pub const RX_TL: usize = 0x38;
/// `DW_IC_TX_TL`: transmit FIFO threshold (`:61`).
pub const TX_TL: usize = 0x3c;
/// `DW_IC_CLR_INTR`: reading clears all clearable interrupts (`:62`).
pub const CLR_INTR: usize = 0x40;
/// `DW_IC_CLR_TX_ABRT`: reading clears TX_ABRT (`:67`).
pub const CLR_TX_ABRT: usize = 0x54;
/// `DW_IC_CLR_STOP_DET` (`:70`).
pub const CLR_STOP_DET: usize = 0x60;
/// `DW_IC_ENABLE` (`:73`).
pub const ENABLE: usize = 0x6c;
/// `DW_IC_STATUS` (`:74`).
pub const STATUS: usize = 0x70;
/// `DW_IC_TXFLR`: bytes in the transmit FIFO (`:75`).
pub const TXFLR: usize = 0x74;
/// `DW_IC_RXFLR`: bytes in the receive FIFO (`:76`).
pub const RXFLR: usize = 0x78;
/// `DW_IC_SDA_HOLD` (`:77`).
pub const SDA_HOLD: usize = 0x7c;
/// `DW_IC_TX_ABRT_SOURCE`: why the last transfer was aborted (`:78`).
pub const TX_ABRT_SOURCE: usize = 0x80;
/// `DW_IC_ENABLE_STATUS` (`:79`).
pub const ENABLE_STATUS: usize = 0x9c;
/// `DW_IC_FS_SPKLEN`: spike suppression length.
pub const FS_SPKLEN: usize = 0xa0;
/// `DW_IC_COMP_PARAM_1`: hardware parameters incl. FIFO depths (`:84`).
pub const COMP_PARAM_1: usize = 0xf4;
/// `DW_IC_COMP_VERSION` (`:85`).
pub const COMP_VERSION: usize = 0xf8;
/// `DW_IC_COMP_TYPE` (`:88`); reads [`COMP_TYPE_VALUE`] on a real DW I2C.
pub const COMP_TYPE: usize = 0xfc;
/// `"DW" + 0x0140` (`:89`).
pub const COMP_TYPE_VALUE: u32 = 0x4457_0140;

/// CON.MASTER_MODE (`:27`).
pub const CON_MASTER: u32 = 1 << 0;
/// CON.SPEED = standard (100 kHz) (`:28`).
pub const CON_SPEED_STD: u32 = 1 << 1;
/// CON.SPEED = fast (400 kHz / 1 MHz) (`:29`).
pub const CON_SPEED_FAST: u32 = 2 << 1;
/// CON.SPEED mask (`:31`).
pub const CON_SPEED_MASK: u32 = 0b11 << 1;
/// CON: 10-bit addressing as master (`:33`).
pub const CON_10BITADDR_MASTER: u32 = 1 << 4;
/// CON: allow RESTART conditions (`:34`).
pub const CON_RESTART_EN: u32 = 1 << 5;
/// CON: disable slave mode (`:35`).
pub const CON_SLAVE_DISABLE: u32 = 1 << 6;

/// DATA_CMD: data byte, bits 7:0 (`:41`).
pub const DATA_CMD_DAT_MASK: u32 = 0xff;
/// DATA_CMD.CMD, bit 8: 1 = read a byte, 0 = write the byte in bits 7:0
/// (`i2c-designware-master.c:88-89`, `cmd | 0x100`).
pub const DATA_CMD_READ: u32 = 1 << 8;
/// DATA_CMD.STOP, bit 9: issue STOP after this byte (`master.c:75`).
pub const DATA_CMD_STOP: u32 = 1 << 9;
/// DATA_CMD.RESTART, bit 10: issue RESTART before this byte (`master.c:78`).
pub const DATA_CMD_RESTART: u32 = 1 << 10;

/// RAW_INTR_STAT / INTR_* bits (`:91-104`).
pub const INTR_RX_UNDER: u32 = 1 << 0;
/// Receive FIFO overflowed.
pub const INTR_RX_OVER: u32 = 1 << 1;
/// Receive FIFO reached threshold.
pub const INTR_RX_FULL: u32 = 1 << 2;
/// Transmit FIFO overflowed (we wrote while full).
pub const INTR_TX_OVER: u32 = 1 << 3;
/// Transmit FIFO at/below threshold.
pub const INTR_TX_EMPTY: u32 = 1 << 4;
/// Transfer aborted; see TX_ABRT_SOURCE.
pub const INTR_TX_ABRT: u32 = 1 << 6;
/// Bus activity.
pub const INTR_ACTIVITY: u32 = 1 << 8;
/// STOP condition seen.
pub const INTR_STOP_DET: u32 = 1 << 9;
/// START condition seen.
pub const INTR_START_DET: u32 = 1 << 10;
/// Master is holding the bus with an empty TX FIFO.
pub const INTR_MST_ON_HOLD: u32 = 1 << 13;

/// ENABLE.ENABLE (`:115`).
pub const ENABLE_ENABLE: u32 = 1 << 0;
/// ENABLE.ABORT: abort the current transfer (`:116`).
pub const ENABLE_ABORT: u32 = 1 << 1;

/// STATUS: bus activity (`:119`).
pub const STATUS_ACTIVITY: u32 = 1 << 0;
/// STATUS: transmit FIFO not full (room for a command).
pub const STATUS_TFNF: u32 = 1 << 1;
/// STATUS: transmit FIFO empty (`:120`).
pub const STATUS_TFE: u32 = 1 << 2;
/// STATUS: receive FIFO not empty (`:121`).
pub const STATUS_RFNE: u32 = 1 << 3;
/// STATUS: receive FIFO full.
pub const STATUS_RFF: u32 = 1 << 4;
/// STATUS: master state machine active (`:122`).
pub const STATUS_MASTER_ACTIVITY: u32 = 1 << 5;

/// TX_ABRT_SOURCE: 7-bit address not acknowledged (`:159,175`).
pub const ABRT_7B_ADDR_NOACK: u32 = 1 << 0;
/// 10-bit address first byte NACKed.
pub const ABRT_10ADDR1_NOACK: u32 = 1 << 1;
/// 10-bit address second byte NACKed.
pub const ABRT_10ADDR2_NOACK: u32 = 1 << 2;
/// A data byte was not acknowledged (`:162,178`).
pub const ABRT_TXDATA_NOACK: u32 = 1 << 3;
/// General call not acknowledged.
pub const ABRT_GCALL_NOACK: u32 = 1 << 4;
/// Master mode disabled while trying to transfer (`:168,184`).
pub const ABRT_MASTER_DIS: u32 = 1 << 11;
/// Arbitration lost against another master (`ARB_LOST`, bit 12 in the databook).
pub const ABRT_ARB_LOST: u32 = 1 << 12;
/// SDA stuck low (`:173,189`).
pub const ABRT_SDA_STUCK_AT_LOW: u32 = 1 << 17;

/// COMP_PARAM_1: TX FIFO depth - 1 in bits 23:16, RX FIFO depth - 1 in bits
/// 15:8 (DesignWare databook; `i2c_dw_configure_fifo_master` reads them).
pub const fn tx_fifo_depth(comp_param_1: u32) -> u32 {
    ((comp_param_1 >> 16) & 0xff) + 1
}
/// See [`tx_fifo_depth`].
pub const fn rx_fifo_depth(comp_param_1: u32) -> u32 {
    ((comp_param_1 >> 8) & 0xff) + 1
}

/// I2C timing targets used by Linux (`i2c-designware-master.c:98-176`), in ns.
pub mod timing {
    /// Standard mode tHIGH / tHD;STA.
    pub const SS_HIGH_NS: u32 = 4000;
    /// Standard mode tLOW.
    pub const SS_LOW_NS: u32 = 4700;
    /// Fast mode tHIGH.
    pub const FS_HIGH_NS: u32 = 600;
    /// Fast mode tLOW.
    pub const FS_LOW_NS: u32 = 1300;
    /// Default SDA/SCL fall time when the device tree gives none (300 ns).
    pub const DEFAULT_FALL_NS: u32 = 300;

    /// `i2c_dw_scl_hcnt` with `cond = 0` (`i2c-designware-common.c`):
    /// `round(ic_clk_khz * (t_high + t_fall) / 1e6) - 3`.
    pub const fn scl_hcnt(ic_clk_khz: u32, t_high_ns: u32, t_fall_ns: u32) -> u32 {
        let x = ic_clk_khz as u64 * (t_high_ns + t_fall_ns) as u64;
        ((x + 500_000) / 1_000_000) as u32 - 3
    }

    /// `i2c_dw_scl_lcnt`: `round(ic_clk_khz * (t_low + t_fall) / 1e6) - 1`.
    pub const fn scl_lcnt(ic_clk_khz: u32, t_low_ns: u32, t_fall_ns: u32) -> u32 {
        let x = ic_clk_khz as u64 * (t_low_ns + t_fall_ns) as u64;
        ((x + 500_000) / 1_000_000) as u32 - 1
    }
}

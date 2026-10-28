//! BCM2712 (the main SoC): the few peripherals we use directly.
//!
//! The BCM2712 "soc" bus is a 2 GiB window at CPU address `0x10_0000_0000`:
//! `bcm2712.dtsi:191` says `ranges = <0x00000000 0x10 0x00000000 0x80000000>`,
//! i.e. soc-local address `X` is CPU physical address `0x10_0000_0000 + X`.

/// CPU address of soc-local address 0 (`bcm2712.dtsi:191`).
pub const SOC_BASE: usize = 0x10_0000_0000;
/// Size of the soc window (`bcm2712.dtsi:191`). The HAL maps this whole range
/// as Device memory.
pub const SOC_SIZE: usize = 0x8000_0000;

/// The debug UART (`uart10`, a PL011) on the 3-pin JST connector.
/// `bcm2712.dtsi:241-243`: `serial@7d001000 { reg = <0x7d001000 0x200>; }`.
/// Linux: `/dev/ttyAMA10`, console at 115200 8N1 (`bcm2712-rpi.dtsi:117`).
pub const UART10_BASE: usize = SOC_BASE + 0x7D00_1000;
/// `clk_uart`, the fixed 9.216 MHz clock feeding uart10 (`bcm2712.dtsi:28-31`).
/// 9,216,000 / (16 x 115200) = 5.0 exactly.
pub const UART10_CLOCK_HZ: u32 = 9_216_000;
/// Expected PrimeCell peripheral ID of uart10 (`bcm2712.dtsi:247`).
pub const UART10_PERIPH_ID: u32 = 0x0034_1011;

/// The PCIe root complex that RP1 hangs off (`pcie2`, `bcm2712.dtsi:526-528`:
/// `reg = <0x10 0x00120000 0x00 0x9310>`). Not programmed by the HAL; listed
/// because the Linux GPIO driver pokes its Slot Control register (+0xc0) as a
/// write fence when PCIe power management is active (`pinctrl-rp1.c:586-606`).
pub const PCIE2_BASE: usize = SOC_BASE + 0x0012_0000;

/// CPU address where PCIe bus address 0 of `pcie2` appears (`bcm2712.dtsi:552`:
/// `<0x02000000 0x00 0x00000000 0x1f 0x00000000 0x00 0xfffffffc>`). RP1 maps its
/// peripherals at PCIe address 0 (`bcm2712-rpi-5-b.dts:141-143`), so this is
/// also [`crate::rp1::BASE`].
pub const PCIE2_OUTBOUND_WINDOW: usize = 0x1F_0000_0000;

/// Where the firmware loads `kernel8.img` and jumps to (default
/// `kernel_address` for 64-bit images).
pub const KERNEL_LOAD_ADDRESS: usize = 0x8_0000;

/// ARM Generic Timer frequency the firmware is expected to program into
/// `CNTFRQ_EL0`: the 54 MHz crystal (`bcm2712.dtsi:14-19`, `clk_osc`). The HAL
/// always reads the register instead of trusting this.
pub const EXPECTED_CNTFRQ_HZ: u64 = 54_000_000;

/// Number of CPU cores (`bcm2712.dtsi:54-150`: cpu@0..cpu@3, Cortex-A76).
pub const NUM_CORES: usize = 4;

/// MPIDR_EL1 values of the cores (`bcm2712.dtsi:57,81,105,129`: `reg = <0x000>`,
/// `<0x100>`, `<0x200>`, `<0x300>`). The core number is in **Aff1** (bits 15:8),
/// Aff0 is always 0.
pub const CORE_MPIDR: [u64; NUM_CORES] = [0x000, 0x100, 0x200, 0x300];

/// How to call the firmware's PSCI implementation (`bcm2712.dtsi:165-168`:
/// `psci { method = "smc"; compatible = "arm,psci-1.0", "arm,psci-0.2"; }`).
/// Secondary cores are started with `CPU_ON` (`enable-method = "psci"`).
pub mod psci {
    /// `PSCI_VERSION` (PSCI spec 5.1.1).
    pub const VERSION: u32 = 0x8400_0000;
    /// `CPU_OFF` (5.1.3).
    pub const CPU_OFF: u32 = 0x8400_0002;
    /// `CPU_ON`, SMC64 calling convention (5.1.4): x1 = target MPIDR,
    /// x2 = entry point physical address, x3 = context id (lands in x0).
    pub const CPU_ON_64: u32 = 0xC400_0003;
    /// `AFFINITY_INFO` (SMC64): 0 = core on, 1 = off, 2 = on pending.
    pub const AFFINITY_INFO_64: u32 = 0xC400_0004;
    /// `SYSTEM_OFF` (5.1.8).
    pub const SYSTEM_OFF: u32 = 0x8400_0008;
    /// `SYSTEM_RESET` (5.1.9).
    pub const SYSTEM_RESET: u32 = 0x8400_0009;

    /// Return codes (PSCI spec table 5).
    pub const SUCCESS: i64 = 0;
    /// Function not supported.
    pub const NOT_SUPPORTED: i64 = -1;
    /// Bad arguments.
    pub const INVALID_PARAMETERS: i64 = -2;
    /// Call denied.
    pub const DENIED: i64 = -3;
    /// Core already running.
    pub const ALREADY_ON: i64 = -4;
    /// Core is being started.
    pub const ON_PENDING: i64 = -5;
    /// Firmware internal failure.
    pub const INTERNAL_FAILURE: i64 = -6;
    /// Core not present.
    pub const NOT_PRESENT: i64 = -7;
    /// Core disabled.
    pub const DISABLED: i64 = -8;
    /// Entry point address invalid.
    pub const INVALID_ADDRESS: i64 = -9;
}

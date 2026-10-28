//! RP1 (the I/O south-bridge): where its peripheral blocks live.
//!
//! RP1 is a separate chip on a 4-lane PCIe link. Inside RP1 its peripherals
//! sit at `0x4000_0000`-`0x4041_0000`. Three device-tree `ranges` chain that
//! to a CPU address:
//!
//! 1. `rp1.dtsi` register addresses are RP1-local (`0xc0 0x400d0000` etc.).
//! 2. `bcm2712-rpi-5-b.dts:141-143`: RP1 `0xc0_4000_0000 + X` = PCIe bus
//!    address `0 + X`, window size `0x410000`.
//! 3. `bcm2712.dtsi:552`: PCIe bus address `0 + X` = CPU address
//!    `0x1F_0000_0000 + X`.
//!
//! So RP1 register `0x400X_XXXX` is CPU address `0x1F_000X_XXXX`. Step 3 is a
//! window that Linux's PCIe driver programs itself; bare-metal code relies on
//! the firmware having configured the same window, and on `pciex4_reset=0` in
//! `config.txt` so the firmware does not reset RP1 before jumping to us.
//! **Unverified on hardware.**

/// CPU address of RP1-local address `0x4000_0000`.
pub const BASE: usize = 0x1F_0000_0000;
/// RP1-local address that [`BASE`] corresponds to.
pub const LOCAL_BASE: u32 = 0x4000_0000;
/// Size of the RP1 peripheral window (`bcm2712-rpi-5-b.dts:143`).
pub const WINDOW_SIZE: usize = 0x0041_0000;

/// Convert an RP1-local address from the device tree into a CPU address.
pub const fn cpu_addr(rp1_local: u32) -> usize {
    BASE + (rp1_local - LOCAL_BASE) as usize
}

/// Atomic register aliases (`pinctrl-rp1.c:43-46`). Every RP1 register `R`
/// also exists at `R + 0x1000` (write = XOR bits), `R + 0x2000` (write = SET
/// bits) and `R + 0x3000` (write = CLEAR bits). One store through an alias
/// changes only the bits you name, without a read-modify-write.
pub const XOR_OFFSET: usize = 0x1000;
/// SET alias offset, see [`XOR_OFFSET`].
pub const SET_OFFSET: usize = 0x2000;
/// CLEAR alias offset, see [`XOR_OFFSET`].
pub const CLR_OFFSET: usize = 0x3000;

// --- Instances (rp1.dtsi) ---------------------------------------------------

/// Clock generator block (`rp1.dtsi:24-27`: `clocks@18000`, size `0x10038`).
pub const CLOCKS_BASE: usize = cpu_addr(0x4001_8000);

/// PL011 UART0 (`rp1.dtsi:62-64`: `serial@30000`). Header pins GPIO14/15 at
/// FSEL 4 (see [`crate::rp1_pins`]). Clocked by `clk_uart`.
pub const UART0_BASE: usize = cpu_addr(0x4003_0000);
/// PL011 UART1..5 (`rp1.dtsi:79-158`), 0x4000 apart.
pub const UART1_BASE: usize = cpu_addr(0x4003_4000);

/// DesignWare SSI SPI0 (`rp1.dtsi:179-181`: `spi@50000`, `snps,dw-apb-ssi`,
/// `num-cs = <2>`). Header: GPIO7 CE1, GPIO8 CE0, GPIO9 MISO, GPIO10 MOSI,
/// GPIO11 SCLK at FSEL 0. Clocked by `clk_sys`.
pub const SPI0_BASE: usize = cpu_addr(0x4005_0000);
/// SPI1 (`rp1.dtsi:194`: `spi@54000`). Header: GPIO16-21 at FSEL 0.
pub const SPI1_BASE: usize = cpu_addr(0x4005_4000);
/// SPI2 (`rp1.dtsi:209`).
pub const SPI2_BASE: usize = cpu_addr(0x4005_8000);
/// SPI3 (`rp1.dtsi:224`).
pub const SPI3_BASE: usize = cpu_addr(0x4005_C000);

/// DesignWare I2C0 (`rp1.dtsi:313-315`: `i2c@70000`, `snps,designware-i2c`).
/// Header: GPIO0/1 or GPIO8/9 at FSEL 3. Clocked by `clk_sys`.
pub const I2C0_BASE: usize = cpu_addr(0x4007_0000);
/// I2C1 (`rp1.dtsi:323-325`: `i2c@74000`). Header: GPIO2/3 (the classic
/// "i2c-1" pins, with 1.8 kOhm pull-ups on the board) or GPIO10/11 at FSEL 3.
pub const I2C1_BASE: usize = cpu_addr(0x4007_4000);
/// I2C2 (`rp1.dtsi:333`). Header: GPIO4/5 or GPIO12/13 at FSEL 3.
pub const I2C2_BASE: usize = cpu_addr(0x4007_8000);
/// I2C3 (`rp1.dtsi:343`). Header: GPIO6/7, GPIO14/15 or GPIO22/23 at FSEL 3.
pub const I2C3_BASE: usize = cpu_addr(0x4007_C000);
/// SCL rise/fall times the device tree declares for RP1's I2C pads
/// (`rp1.dtsi:328-329`), used for the timing formulas.
pub const I2C_SCL_RISE_NS: u32 = 65;
/// See [`I2C_SCL_RISE_NS`].
pub const I2C_SCL_FALL_NS: u32 = 100;

/// PWM0 (`rp1.dtsi:397-399`: `pwm@98000`, 4 channels). Header: GPIO12/13 at
/// FSEL 0 (channels 0/1), GPIO18/19 at FSEL 3 (channels 2/3). Clocked by
/// `clk_pwm0`, which Linux sets to 50 MHz (`rp1.dtsi:402-403`).
pub const PWM0_BASE: usize = cpu_addr(0x4009_8000);
/// PWM1 (`rp1.dtsi:407`), drives the fan connector (GPIO45); not on the header.
pub const PWM1_BASE: usize = cpu_addr(0x4009_C000);

/// GPIO pin-mux block IO_BANK0 (`rp1.dtsi:483`: `0x400d0000`, size `0xc000`).
pub const IO_BANK0_BASE: usize = cpu_addr(0x400D_0000);
/// Software GPIO block SYS_RIO0 (`rp1.dtsi:484`: `0x400e0000`).
pub const SYS_RIO0_BASE: usize = cpu_addr(0x400E_0000);
/// Pad control block PADS_BANK0 (`rp1.dtsi:485`: `0x400f0000`).
pub const PADS_BANK0_BASE: usize = cpu_addr(0x400F_0000);

// --- Clock rates --------------------------------------------------------------

/// RP1's crystal (`rp1.dtsi:1248-1253`: `clk_xosc`, 50 MHz). Hardware fact,
/// does not depend on any driver having run.
pub const XOSC_HZ: u32 = 50_000_000;
/// `clk_sys`, the clock feeding I2C and SPI. Linux *asks* for 200 MHz
/// (`rp1.dtsi:38,55`, `assigned-clock-rates`); what the firmware leaves it at
/// before Linux runs is unknown, so the HAL measures it with the frequency
/// counter in [`crate::rp1_clocks`] and only uses this as a fallback.
pub const CLK_SYS_HZ_NOMINAL: u32 = 200_000_000;
/// `pll_sys_pri_ph`, 100 MHz under Linux (`rp1.dtsi:39,56`).
pub const PLL_SYS_PRI_PH_HZ_NOMINAL: u32 = 100_000_000;

//! RP1 clock generator registers (`drivers/clk/clk-rp1.c`).
//!
//! Every peripheral clock has a CTRL register (enable + source select) and an
//! integer divider register, some also a fractional divider:
//!
//! ```text
//! rate = parent_rate / (DIV_INT + DIV_FRAC / 2^16)        (clk-rp1.c:1006-1030)
//! ```
//!
//! The source is chosen with CTRL.SRC (bits 1:0): value 1 (`AUX_SEL`,
//! `clk-rp1.c:181`) means "use the auxiliary source selected by CTRL.AUXSRC
//! (bits 9:5)" (`clk-rp1.c:1136-1160`). The aux parent lists below are copied
//! from the `REGISTER_CLK(...)` descriptors.
//!
//! The block also contains a **frequency counter** that measures any clock
//! against the crystal (`clockman_measure_clock`, `clk-rp1.c`): the HAL uses it
//! to find out what `clk_sys` really runs at.

// --- Per-clock registers (clk-rp1.c:49-77) -------------------------------------

/// `CLK_SYS_CTRL`.
pub const CLK_SYS_CTRL: usize = 0x0014;
/// `CLK_SYS_DIV_INT`.
pub const CLK_SYS_DIV_INT: usize = 0x0018;
/// `CLK_SYS_SEL` (read-only: which source is currently active).
pub const CLK_SYS_SEL: usize = 0x0020;

/// `CLK_UART_CTRL` (`clk-rp1.c:61`).
pub const CLK_UART_CTRL: usize = 0x0054;
/// `CLK_UART_DIV_INT` (8-bit divider, `clk-rp1.c:199`).
pub const CLK_UART_DIV_INT: usize = 0x0058;
/// `CLK_UART_SEL`.
pub const CLK_UART_SEL: usize = 0x0060;

/// `CLK_PWM0_CTRL` (`clk-rp1.c:69`).
pub const CLK_PWM0_CTRL: usize = 0x0074;
/// `CLK_PWM0_DIV_INT` (16-bit divider, `clk-rp1.c:200`).
pub const CLK_PWM0_DIV_INT: usize = 0x0078;
/// `CLK_PWM0_DIV_FRAC`.
pub const CLK_PWM0_DIV_FRAC: usize = 0x007c;
/// `CLK_PWM0_SEL`.
pub const CLK_PWM0_SEL: usize = 0x0080;

/// `CLK_PWM1_CTRL` (`clk-rp1.c:74`).
pub const CLK_PWM1_CTRL: usize = 0x0084;
/// `CLK_PWM1_DIV_INT`.
pub const CLK_PWM1_DIV_INT: usize = 0x0088;
/// `CLK_PWM1_DIV_FRAC`.
pub const CLK_PWM1_DIV_FRAC: usize = 0x008c;
/// `CLK_PWM1_SEL`.
pub const CLK_PWM1_SEL: usize = 0x0090;

// --- CTRL bits (clk-rp1.c:181, 240-244) ----------------------------------------

/// `CLK_CTRL_ENABLE` = `BIT(11)`.
pub const CTRL_ENABLE: u32 = 1 << 11;
/// `CLK_CTRL_AUXSRC_MASK` = `0x3e0`, shift 5.
pub const CTRL_AUXSRC_MASK: u32 = 0x0000_03e0;
/// See [`CTRL_AUXSRC_MASK`].
pub const CTRL_AUXSRC_SHIFT: u32 = 5;
/// CTRL.SRC field (`clk_src_mask = 0x3`, shift 0, `clk-rp1.c:243,1857`).
pub const CTRL_SRC_MASK: u32 = 0x0000_0003;
/// SRC value meaning "take the AUXSRC parent" (`AUX_SEL`, `clk-rp1.c:181`).
pub const CTRL_SRC_AUX: u32 = 1;
/// Fractional divider precision (`CLK_DIV_FRAC_BITS`, `clk-rp1.c:244`).
pub const DIV_FRAC_BITS: u32 = 16;

/// Aux parents of `clk_pwm0` (`clk-rp1.c:1942-1952`): index 2 is the 50 MHz
/// crystal, which is what the HAL selects.
pub mod pwm0_aux {
    /// (slot 0 is unused: "pll_audio_pri_ph" commented out)
    pub const PLL_VIDEO_SEC: u32 = 1;
    /// The crystal, [`crate::rp1::XOSC_HZ`].
    pub const XOSC: u32 = 2;
    /// External clock sources `clksrc_gp0..5` are 3..=8.
    pub const CLKSRC_GP0: u32 = 3;
}

/// Aux parents of `clk_uart` (`clk-rp1.c:1900-1910`).
pub mod uart_aux {
    /// `pll_sys_pri_ph` (100 MHz under Linux).
    pub const PLL_SYS_PRI_PH: u32 = 0;
    /// `pll_video`.
    pub const PLL_VIDEO: u32 = 1;
    /// The crystal, [`crate::rp1::XOSC_HZ`].
    pub const XOSC: u32 = 2;
}

// --- Frequency counter (clk-rp1.c:169-179, 203-205, clockman_measure_clock) ----

/// Reference clock frequency in kHz (the crystal).
pub const FC0_REF_KHZ: usize = 0x021c;
/// Lower bound for the measurement window.
pub const FC0_MIN_KHZ: usize = 0x0220;
/// Upper bound.
pub const FC0_MAX_KHZ: usize = 0x0224;
/// Delay before counting.
pub const FC0_DELAY: usize = 0x0228;
/// Measurement interval (power of two reference cycles).
pub const FC0_INTERVAL: usize = 0x022c;
/// Which clock to measure; writing starts the measurement.
pub const FC0_SRC: usize = 0x0230;
/// Status: bit 4 DONE, bit 8 RUNNING.
pub const FC0_STATUS: usize = 0x0234;
/// Result in kHz, with 5 fractional bits.
pub const FC0_RESULT: usize = 0x0238;
/// `FC0_STATUS_DONE`.
pub const FC0_STATUS_DONE: u32 = 1 << 4;
/// `FC0_STATUS_RUNNING`.
pub const FC0_STATUS_RUNNING: u32 = 1 << 8;
/// `FC0_RESULT_FRAC_SHIFT`.
pub const FC0_RESULT_FRAC_SHIFT: u32 = 5;
/// Stride between the frequency counter register blocks (`FC_SIZE`).
pub const FC_SIZE: usize = 0x20;
/// `FC_NUM(idx, off) = idx * 32 + off` (`clk-rp1.c:179`): counter block
/// `idx`, source number `off` within it.
pub const fn fc_num(idx: u32, off: u32) -> u32 {
    idx * 32 + off
}
/// `clk_sys`'s frequency-counter source (`clk-rp1.c:1856`).
pub const FC_SRC_CLK_SYS: u32 = fc_num(0, 4);
/// `clk_pwm0`'s source (`clk-rp1.c:1953`).
pub const FC_SRC_CLK_PWM0: u32 = fc_num(0, 5);
/// `clk_uart`'s source (`clk-rp1.c:1911`).
pub const FC_SRC_CLK_UART: u32 = fc_num(6, 7);

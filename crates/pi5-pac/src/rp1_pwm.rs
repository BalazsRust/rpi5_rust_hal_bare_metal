//! RP1 PWM block registers (`drivers/pwm/pwm-rp1.c:22-31`).
//!
//! Each block has four channels. A channel counts clock ticks from 0 to
//! `RANGE - 1` and drives the output high while the count is below `DUTY`
//! (`pwm-rp1.c:75-83`: `DUTY = duty_ns / clk_period`, `RANGE = period_ns /
//! clk_period`). Changes take effect when `SET_UPDATE` is written to
//! `GLOBAL_CTRL` (`pwm-rp1.c:46-49`).

/// `PWM_GLOBAL_CTRL`: bits 0-3 enable channels 0-3, bit 31 = apply changes.
pub const GLOBAL_CTRL: usize = 0x000;
/// `PWM_CHANNEL_ENABLE(x)` = `BIT(x)` in GLOBAL_CTRL.
pub const fn global_ctrl_enable(channel: u8) -> u32 {
    1 << channel
}
/// `SET_UPDATE` = `BIT(31)`: latch new RANGE/DUTY/CTRL values.
pub const GLOBAL_CTRL_SET_UPDATE: u32 = 1 << 31;

/// `PWM_CHANNEL_CTRL(x)` = `0x014 + x * 16`.
pub const fn channel_ctrl(channel: u8) -> usize {
    0x014 + channel as usize * 16
}
/// `PWM_RANGE(x)` = `0x018 + x * 16`: period in clock ticks.
pub const fn range(channel: u8) -> usize {
    0x018 + channel as usize * 16
}
/// Phase register, `0x01c + x * 16` (between RANGE and DUTY; not used by Linux).
pub const fn phase(channel: u8) -> usize {
    0x01c + channel as usize * 16
}
/// `PWM_DUTY(x)` = `0x020 + x * 16`: high time in clock ticks.
pub const fn duty(channel: u8) -> usize {
    0x020 + channel as usize * 16
}

/// CHANNEL_CTRL.MODE, bits 1:0 (`PWM_MODE_MASK`). 0 = off.
pub const CTRL_MODE_MASK: u32 = 0x3;
/// MODE value 1: "trailing edge mark/space modulation", the normal PWM mode
/// (`pwm-rp1.c:26-27`, `PWM_CHANNEL_DEFAULT = BIT(8) + BIT(0)`).
pub const CTRL_MODE_TRAILING_EDGE_MS: u32 = 0x1;
/// `PWM_POLARITY` = `BIT(3)`: invert the output.
pub const CTRL_POLARITY_INVERTED: u32 = 1 << 3;
/// `BIT(8)`: "FIFO_POP_MASK", part of the default channel setup.
pub const CTRL_FIFO_POP_MASK: u32 = 1 << 8;
/// What Linux writes when it claims a channel (`pwm-rp1.c:27`).
pub const CTRL_DEFAULT: u32 = CTRL_FIFO_POP_MASK | CTRL_MODE_TRAILING_EDGE_MS;

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

/// `COMMON_RANGE`: period for channels with `CTRL_BIND` set.
pub const COMMON_RANGE: usize = 0x008;
/// `COMMON_DUTY`: compare value for channels with `CTRL_BIND` set.
pub const COMMON_DUTY: usize = 0x00c;
/// `DUTY_FIFO`: duty values fed round-robin to channels with `CTRL_USEFIFO`.
pub const DUTY_FIFO: usize = 0x010;

/// CHANNEL_CTRL.MODE, bits 2:0 (RP1 datasheet table 32).
pub const CTRL_MODE_MASK: u32 = 0x7;
/// MODE 0: output held at 0.
pub const CTRL_MODE_ZERO: u32 = 0x0;
/// MODE 1: "trailing edge mark/space", the normal PWM mode: count 0..RANGE,
/// output 1 while count < DUTY (`pwm-rp1.c:26-27`).
pub const CTRL_MODE_TRAILING_EDGE_MS: u32 = 0x1;
/// MODE 2: phase-correct (centre-aligned): count RANGE..0..RANGE, output 1
/// while count < DUTY. Period is 2 x RANGE.
pub const CTRL_MODE_PHASE_CORRECT: u32 = 0x2;
/// MODE 3: pulse-density: DUTY/RANGE of the clock ticks are 1, spread out.
pub const CTRL_MODE_PDM: u32 = 0x3;
/// MODE 4: DUTY shifted out MSB first, bit `31 - count % 32`.
pub const CTRL_MODE_SERIALISER_MSB: u32 = 0x4;
/// MODE 5: pulse-position: one tick high when count == DUTY.
pub const CTRL_MODE_PPM: u32 = 0x5;
/// MODE 6: leading edge: count RANGE..0, output 1 while count < DUTY (the
/// pulse sits at the end of the period).
pub const CTRL_MODE_LEADING_EDGE_MS: u32 = 0x6;
/// MODE 7: DUTY shifted out LSB first, bit `count % 32`.
pub const CTRL_MODE_SERIALISER_LSB: u32 = 0x7;
/// `INVERT` = `BIT(3)`: invert the output.
pub const CTRL_POLARITY_INVERTED: u32 = 1 << 3;
/// `BIND` = `BIT(4)`: use COMMON_RANGE and COMMON_DUTY (or the FIFO).
pub const CTRL_BIND: u32 = 1 << 4;
/// `USEFIFO` = `BIT(5)`: take duty values from DUTY_FIFO (needs BIND).
pub const CTRL_USEFIFO: u32 = 1 << 5;
/// `SDM` = `BIT(6)`: sigma-delta noise shaping of the duty value.
pub const CTRL_SDM: u32 = 1 << 6;
/// `DITHER` = `BIT(7)`: dither inside the sigma-delta loop.
pub const CTRL_DITHER: u32 = 1 << 7;
/// `BIT(8)`: "FIFO_POP_MASK", part of the default channel setup.
pub const CTRL_FIFO_POP_MASK: u32 = 1 << 8;
/// What Linux writes when it claims a channel (`pwm-rp1.c:27`).
pub const CTRL_DEFAULT: u32 = CTRL_FIFO_POP_MASK | CTRL_MODE_TRAILING_EDGE_MS;

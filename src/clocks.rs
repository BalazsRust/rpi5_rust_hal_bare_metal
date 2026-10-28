//! RP1 clock generator: measure `clk_sys`, switch on the PWM and UART clocks.
//!
//! Registers: [`pi5_pac::rp1_clocks`] (`clk-rp1.c`).
//!
//! Under Linux the `clk-rp1` driver programs every RP1 clock. Bare metal
//! inherits whatever RP1's own boot firmware left behind, which is not
//! documented. Two ways to deal with that:
//!
//! * **Measure.** The block has a frequency counter that counts a chosen
//!   clock against the 50 MHz crystal. [`clk_sys_hz`] uses it once and caches
//!   the result; the I2C and SPI drivers derive their timing from it.
//! * **Program.** For the PWM and UART0 clocks, whose state is unknown, we
//!   simply select the crystal as source (divider 1 -> exactly 50 MHz) and
//!   enable them. The crystal is the one clock whose frequency is a hardware
//!   fact (`rp1.dtsi:1248-1253`).

use crate::delay::Timeout;
use crate::sync::SpinLock;
use core::sync::atomic::{AtomicU32, Ordering};
use core::time::Duration;
use pi5_pac::mmio::{read32, write32};
use pi5_pac::rp1::{CLK_SYS_HZ_NOMINAL, CLOCKS_BASE, XOSC_HZ};
use pi5_pac::rp1_clocks as r;

/// Serialises read-modify-writes of the clock registers across cores.
static LOCK: SpinLock<()> = SpinLock::new(());
/// Cached result of the first `clk_sys` measurement (0 = not yet measured).
static CLK_SYS_HZ: AtomicU32 = AtomicU32::new(0);

/// Measure a clock with the frequency counter. `fc_src` is one of the
/// `FC_SRC_*` constants (`fc_num(block, source)`). Returns `None` if the
/// counter does not answer within 100 ms or the result is implausible.
///
/// Sequence from `clockman_measure_clock` (`clk-rp1.c`): wait until the
/// counter is idle, program the reference frequency (kHz), the accepted
/// range, interval and delay, write the source (this starts it), wait for
/// DONE, read the result (kHz with 5 fractional bits), write source 0 to
/// stop it.
pub fn measure(fc_src: u32) -> Option<u32> {
    let block = CLOCKS_BASE + (fc_src / 32) as usize * r::FC_SIZE;
    let src = fc_src % 32;
    if src == 0 {
        return None;
    }
    let _guard = LOCK.lock();
    // SAFETY: clock block registers, serialised by the lock.
    unsafe {
        let t = Timeout::new(Duration::from_millis(100));
        while read32(block + r::FC0_STATUS) & r::FC0_STATUS_RUNNING != 0 {
            if t.expired() {
                return None;
            }
        }
        write32(block + r::FC0_REF_KHZ, XOSC_HZ / 1000);
        write32(block + r::FC0_MIN_KHZ, 0);
        write32(block + r::FC0_MAX_KHZ, 0x1ff_ffff);
        write32(block + r::FC0_INTERVAL, 8);
        write32(block + r::FC0_DELAY, 7);
        write32(block + r::FC0_SRC, src);
        let t = Timeout::new(Duration::from_millis(100));
        while read32(block + r::FC0_STATUS) & r::FC0_STATUS_DONE == 0 {
            if t.expired() {
                write32(block + r::FC0_SRC, 0);
                return None;
            }
        }
        let result = read32(block + r::FC0_RESULT);
        write32(block + r::FC0_SRC, 0);
        let khz = result >> r::FC0_RESULT_FRAC_SHIFT;
        let hz = khz.saturating_mul(1000) + ((result & 0x1f) * 1000) / 32;
        if (500_000..=1_000_000_000).contains(&hz) { Some(hz) } else { None }
    }
}

/// `clk_sys` in Hz: measured once, cached; falls back to the nominal 200 MHz
/// (`pi5_pac::rp1::CLK_SYS_HZ_NOMINAL`) if the measurement fails.
pub fn clk_sys_hz() -> u32 {
    let cached = CLK_SYS_HZ.load(Ordering::Relaxed);
    if cached != 0 {
        return cached;
    }
    let hz = measure(r::FC_SRC_CLK_SYS).unwrap_or(CLK_SYS_HZ_NOMINAL);
    CLK_SYS_HZ.store(hz, Ordering::Relaxed);
    hz
}

/// Was `clk_sys` actually measured (as opposed to assumed)?
pub fn clk_sys_measured() -> bool {
    CLK_SYS_HZ.load(Ordering::Relaxed) != 0 && measure(r::FC_SRC_CLK_SYS).is_some()
}

/// Program one aux-sourced clock: source = `aux`, integer divider `div_int`,
/// fraction 0, enabled. Returns the resulting frequency.
fn program_aux_clock(ctrl: usize, div_int: usize, div_frac: Option<usize>, aux: u32, parent_hz: u32, div: u32) -> u32 {
    let _guard = LOCK.lock();
    // SAFETY: clock block registers, serialised by the lock.
    unsafe {
        let c = CLOCKS_BASE;
        // Dividers first, then switch source + enable in one write.
        write32(c + div_int, div);
        if let Some(frac) = div_frac {
            write32(c + frac, 0);
        }
        let mut v = read32(c + ctrl);
        v &= !(r::CTRL_AUXSRC_MASK | r::CTRL_SRC_MASK);
        v |= (aux << r::CTRL_AUXSRC_SHIFT) & r::CTRL_AUXSRC_MASK;
        v |= r::CTRL_SRC_AUX;
        v |= r::CTRL_ENABLE;
        write32(c + ctrl, v);
    }
    parent_hz / div
}

/// Run `clk_pwm0` from the crystal: 50 MHz. Idempotent. Returns the frequency.
pub fn enable_pwm0_clock() -> u32 {
    program_aux_clock(r::CLK_PWM0_CTRL, r::CLK_PWM0_DIV_INT, Some(r::CLK_PWM0_DIV_FRAC), r::pwm0_aux::XOSC, XOSC_HZ, 1)
}

/// Run `clk_uart` (RP1's UART0-5) from the crystal: 50 MHz. Idempotent.
/// 50 MHz / (16 x 115200) = 27.13, which the PL011's fractional divider
/// handles with 0.006 % error.
///
/// Returns the frequency the UART really runs at: we MEASURE it with the
/// frequency counter after programming, because on hardware the "should be
/// 50 MHz" assumption produced a wrong baud rate (the ESP32-C3 bridge saw
/// only framing/glitch errors). The baud divisor is computed from this value,
/// so even if our clock programming has no effect, the baud rate comes out
/// right as long as the measurement works.
pub fn enable_uart_clock() -> u32 {
    let programmed = program_aux_clock(r::CLK_UART_CTRL, r::CLK_UART_DIV_INT, None, r::uart_aux::XOSC, XOSC_HZ, 1);
    let measured = measure(r::FC_SRC_CLK_UART);
    UART_CLK_MEASURED.store(measured.unwrap_or(0), Ordering::Relaxed);
    measured.unwrap_or(programmed)
}

/// Last measured `clk_uart` in Hz (0 = measurement failed / not done yet).
static UART_CLK_MEASURED: AtomicU32 = AtomicU32::new(0);

/// The `clk_uart` frequency measured by [`enable_uart_clock`], if any.
pub fn uart_clock_measured_hz() -> Option<u32> {
    match UART_CLK_MEASURED.load(Ordering::Relaxed) {
        0 => None,
        hz => Some(hz),
    }
}

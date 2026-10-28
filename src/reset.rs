//! Restart the Pi from software, and restart it on request over the serial
//! line (feature `reset-watch`, on by default).
//!
//! `cargo run` (tools/pi5load.py) asks for a restart by sending the magic
//! text `\0PI5-RESTART-NOW\0`. The ESP32 bridge turns that into a ~300 ms LOW pulse on the Pi's
//! RX wire (GPIO15, physical pin 10). Normal serial data is never LOW for
//! longer than one byte (87 us at 115200), so a long LOW is unmistakable.
//!
//! The reset watch runs on CPU core 3, started by [`crate::init`]. It looks
//! at the pin level only (it never reads the UART), so it doesn't take bytes
//! away from the program, and it works whatever the program on core 0 is
//! doing, even if it is stuck in an endless loop.
//!
//! It triggers when the line was LOW for 150 ms..1.5 s and then went HIGH
//! again. Requiring the release means an unplugged/unpowered ESP32 (which
//! may hold the line LOW forever) can't cause a reboot loop.
//!
//! Don't want it (you need core 3, or GPIO15 for something else)? Use
//! [`crate::init_without_reset_watch`] instead of `init()`.
//!
//! Programs loaded by the UART chainloader don't start their own: the
//! chainloader's watch keeps running on core 3 (it lives at 0x2000000,
//! out of the loaded program's way), so `init()` finds core 3 already on.

use crate::delay::Delay;
use crate::gpio::{AnyPin, Pull, claim_pin};
use crate::multicore::{CORE3, Never, Stack, spawn_core3};
use core::sync::atomic::{AtomicBool, AtomicI64, AtomicU32, Ordering};
use pi5_pac::mmio::{read32, write32};
use pi5_pac::bcm2712::{CORE_MPIDR, psci as ids};
use pi5_pac::rp1_pins::Function;

/// GPIO the reset request arrives on: UART0 RX.
const RESET_GPIO: u8 = 15;
/// LOW shorter than this is ignored (ms).
const MIN_LOW_MS: u64 = 150;
/// LOW longer than this is "ESP32 unplugged", not a request (ms).
const MAX_LOW_MS: u64 = 1500;

static mut WATCH_STACK: Stack<16384> = Stack::new();
static STOP: AtomicBool = AtomicBool::new(false);
static STOPPED: AtomicBool = AtomicBool::new(false);
static RUNNING: AtomicBool = AtomicBool::new(false);
/// Core 3 was already on: the chainloader's reset watch is still running
/// (it is left on on purpose), so this program needs none of its own.
static INHERITED: AtomicBool = AtomicBool::new(false);
/// Loop count of the watch; if it grows, core 3 really is running.
static TICKS: AtomicU32 = AtomicU32::new(0);
/// PSCI error from starting core 3 (0 = no error).
static START_ERR: AtomicI64 = AtomicI64::new(0);

/// Start the reset watch on core 3. Called by [`crate::init`].
pub(crate) fn start_watch() {
    // GPIO15 as UART0 RX with pull-up: makes its input readable and idle
    // HIGH. Programs that use UART0 configure it the same way.
    // SAFETY: GPIO15 is reserved for this by `init()`'s documentation.
    let _ = claim_pin(&unsafe { AnyPin::steal(RESET_GPIO) }, Function::Uart(0), Pull::Up);
    STOP.store(false, Ordering::Relaxed);
    STOPPED.store(false, Ordering::Relaxed);
    // Started by the chainloader, core 3 is already ON and running the
    // chainloader's watch: then there is nothing to do (ALREADY_ON). Retry
    // briefly while PSCI says "on pending".
    let t = crate::delay::Timeout::new(core::time::Duration::from_millis(300));
    loop {
        // SAFETY: only `init()` gets here, once; the stack is used by core 3 only.
        let stack = unsafe { &mut *core::ptr::addr_of_mut!(WATCH_STACK) };
        match spawn_core3(CORE3::new(), stack, watch) {
            Ok(()) => {
                RUNNING.store(true, Ordering::Release);
                return;
            }
            Err(crate::multicore::Error::Psci(code)) if code == ids::ALREADY_ON => {
                INHERITED.store(true, Ordering::Release);
                return;
            }
            Err(crate::multicore::Error::Psci(code)) if code == ids::ON_PENDING && !t.expired() => {
                Delay::new().delay_millis(5);
            }
            Err(crate::multicore::Error::Psci(code)) => {
                START_ERR.store(code, Ordering::Relaxed);
                return;
            }
            Err(crate::multicore::Error::StackTooSmall) => {
                START_ERR.store(i64::MIN, Ordering::Relaxed);
                return;
            }
        }
    }
}

fn watch() -> Never {
    let delay = Delay::new();
    let ms = Delay::frequency() / 1000;
    let mut low_since: Option<u64> = None;
    loop {
        if STOP.load(Ordering::Acquire) {
            STOPPED.store(true, Ordering::Release);
            crate::psci::cpu_off();
            crate::cpu::halt();
        }
        TICKS.fetch_add(1, Ordering::Relaxed);
        let now = Delay::now();
        if crate::gpio::input_level(RESET_GPIO) {
            if let Some(start) = low_since.take() {
                let held = (now - start) / ms;
                if (MIN_LOW_MS..=MAX_LOW_MS).contains(&held) {
                    system_reset();
                }
            }
        } else if low_since.is_none() {
            low_since = Some(now);
        }
        // 1 ms between looks: no constant traffic on the PCIe link to RP1.
        delay.delay_millis(1);
    }
}

/// Is the reset watch working? Waits ~20 ms to see core 3's loop count
/// move. `Err` says why not, for printing.
pub fn watch_status() -> Result<(), &'static str> {
    if INHERITED.load(Ordering::Acquire) {
        return Ok(());
    }
    if !RUNNING.load(Ordering::Acquire) {
        return Err(match START_ERR.load(Ordering::Relaxed) {
            0 => "not started (init_without_reset_watch, or feature off)",
            i64::MIN => "core 3 stack too small",
            code => crate::psci::error_name(code),
        });
    }
    let before = TICKS.load(Ordering::Relaxed);
    let t = crate::delay::Timeout::new(core::time::Duration::from_millis(200));
    while TICKS.load(Ordering::Relaxed) == before && !t.expired() {}
    if TICKS.load(Ordering::Relaxed) == before {
        return Err("core 3 was started but is not running");
    }
    Ok(())
}

/// Stop the reset watch and power core 3 off (so a new program can start
/// it again). The chainloader calls this right before jumping.
pub fn stop_watch() {
    if !RUNNING.swap(false, Ordering::AcqRel) {
        return;
    }
    STOP.store(true, Ordering::Release);
    let t = crate::delay::Timeout::new(core::time::Duration::from_millis(100));
    while !STOPPED.load(Ordering::Acquire) && !t.expired() {}
    // Wait until the firmware really reports core 3 OFF, so the next
    // program's init() can start it again.
    let t = crate::delay::Timeout::new(core::time::Duration::from_millis(200));
    while crate::psci::affinity_info(CORE_MPIDR[3]) != 1 && !t.expired() {
        Delay::new().delay_millis(1);
    }
}

/// Restart the whole Pi now: the BCM2712 watchdog (the classic Raspberry
/// Pi reboot, same steps as Linux's bcm2835_wdt), and if that hasn't
/// happened after 100 ms, PSCI SYSTEM_RESET.
pub fn system_reset() -> ! {
    watchdog_start();
    Delay::new().delay_millis(100);
    // SAFETY: intended effect.
    unsafe { crate::psci::smc(PSCI_SYSTEM_RESET, 0, 0, 0) };
    crate::cpu::halt()
}

const PSCI_SYSTEM_RESET: u32 = 0x8400_0009;

/// BCM2712 power manager (`bcm2712.dtsi`: `watchdog@7d200000`).
const PM_BASE: usize = pi5_pac::bcm2712::SOC_BASE + 0x7d20_0000;
const PM_RSTC: usize = 0x1c;
const PM_RSTS: usize = 0x20;
const PM_WDOG: usize = 0x24;
const PM_PASSWORD: u32 = 0x5a00_0000;
const PM_RSTC_WRCFG_CLR: u32 = 0xffff_ffcf;
const PM_RSTC_WRCFG_FULL_RESET: u32 = 0x0000_0020;
const PM_RSTS_PARTITION_CLR: u32 = 0xffff_faaa;

/// Arm the watchdog to fire after 10 ticks (~150 us): full reset.
fn watchdog_start() {
    // SAFETY: power-manager registers; the password makes writes effective.
    unsafe {
        let rsts = read32(PM_BASE + PM_RSTS) & PM_RSTS_PARTITION_CLR;
        write32(PM_BASE + PM_RSTS, rsts | PM_PASSWORD); // boot partition 0
        write32(PM_BASE + PM_WDOG, 10 | PM_PASSWORD);
        let rstc = read32(PM_BASE + PM_RSTC) & PM_RSTC_WRCFG_CLR;
        write32(PM_BASE + PM_RSTC, rstc | PM_PASSWORD | PM_RSTC_WRCFG_FULL_RESET);
    }
}

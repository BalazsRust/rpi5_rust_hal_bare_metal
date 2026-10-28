//! PSCI: asking the firmware to start cores, reset, or power off.
//!
//! The Pi 5 firmware keeps a small "secure monitor" running at EL3. We talk
//! to it with the `smc` instruction ("secure monitor call"): put a function ID
//! in `x0`, arguments in `x1`-`x3`, execute `smc #0`, read the result from
//! `x0`. The device tree says this is how cores are brought up
//! (`bcm2712.dtsi:165-168`, `method = "smc"`, `enable-method = "psci"`); it is
//! the same mechanism Linux uses. Function IDs: [`pi5_pac::bcm2712::psci`].

use core::arch::asm;
use pi5_pac::bcm2712::psci as ids;

/// Perform one SMC call following the SMC Calling Convention: `x0` carries
/// the function ID and the return value, `x1`-`x3` the arguments. The
/// monitor may clobber `x4`-`x17`.
///
/// # Safety
/// The call does whatever the firmware implements for `function`; starting a
/// core at a bad address, for example, is not something Rust can check.
pub unsafe fn smc(function: u32, arg1: u64, arg2: u64, arg3: u64) -> i64 {
    let ret: i64;
    // SAFETY: the caller takes responsibility for the function's effects.
    unsafe {
        asm!(
            "smc #0",
            inout("x0") u64::from(function) => ret,
            in("x1") arg1,
            in("x2") arg2,
            in("x3") arg3,
            out("x4") _, out("x5") _, out("x6") _, out("x7") _,
            out("x8") _, out("x9") _, out("x10") _, out("x11") _,
            out("x12") _, out("x13") _, out("x14") _, out("x15") _,
            out("x16") _, out("x17") _,
            options(nostack)
        );
    }
    ret
}

/// PSCI version as `(major, minor)`, or `None` if the call is not supported.
pub fn version() -> Option<(u16, u16)> {
    // SAFETY: PSCI_VERSION has no side effects.
    let v = unsafe { smc(ids::VERSION, 0, 0, 0) };
    if v < 0 {
        return None;
    }
    Some((((v >> 16) & 0xffff) as u16, (v & 0xffff) as u16))
}

/// Start the core with the given MPIDR at `entry` (physical address, MMU off,
/// EL2) with `context` in its `x0`. See [`crate::multicore`] for the safe API.
///
/// # Safety
/// `entry` must point at code prepared to run on a freshly started core.
pub unsafe fn cpu_on(target_mpidr: u64, entry: usize, context: usize) -> Result<(), i64> {
    // SAFETY: forwarded.
    let r = unsafe { smc(ids::CPU_ON_64, target_mpidr, entry as u64, context as u64) };
    if r == ids::SUCCESS { Ok(()) } else { Err(r) }
}

/// Is a core on? PSCI `AFFINITY_INFO`: 0 = on, 1 = off, 2 = on pending,
/// negative = error.
pub fn affinity_info(target_mpidr: u64) -> i64 {
    // SAFETY: a query, no side effects.
    unsafe { smc(ids::AFFINITY_INFO_64, target_mpidr, 0, 0) }
}

/// Describe a PSCI error code.
pub fn error_name(code: i64) -> &'static str {
    match code {
        ids::SUCCESS => "success",
        ids::NOT_SUPPORTED => "not supported",
        ids::INVALID_PARAMETERS => "invalid parameters",
        ids::DENIED => "denied",
        ids::ALREADY_ON => "already on",
        ids::ON_PENDING => "on pending",
        ids::INTERNAL_FAILURE => "internal failure",
        ids::NOT_PRESENT => "not present",
        ids::DISABLED => "disabled",
        ids::INVALID_ADDRESS => "invalid address",
        _ => "unknown",
    }
}

/// Power off this core. Returns an error code only if the firmware refused.
pub fn cpu_off() -> i64 {
    // SAFETY: CPU_OFF only affects the calling core.
    unsafe { smc(ids::CPU_OFF, 0, 0, 0) }
}

/// Reboot the whole system. If the firmware does not implement it, falls
/// back to the watchdog (see [`crate::reset::system_reset`]).
pub fn system_reset() -> ! {
    crate::reset::system_reset()
}

/// Power the system off. If the firmware does not implement it, halts.
pub fn system_off() -> ! {
    // SAFETY: intended effect.
    unsafe { smc(ids::SYSTEM_OFF, 0, 0, 0) };
    crate::cpu::halt()
}

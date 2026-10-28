//! Small helpers around AArch64 system registers and instructions.
//!
//! "System registers" (`CurrentEL`, `MPIDR_EL1`, the timer registers, ...)
//! are not memory mapped; they are read with `mrs` and written with `msr`.
//! Rust exposes those through inline assembly.

use core::arch::asm;

/// The exception level we are running at: 1, 2 or 3.
///
/// The Pi firmware enters `kernel8.img` in **EL2** (the hypervisor level) and
/// this crate stays there. Linux drops to EL1 early; we have no reason to.
#[inline]
pub fn current_el() -> u8 {
    let el: u64;
    // SAFETY: reading CurrentEL has no side effects.
    unsafe { asm!("mrs {}, CurrentEL", out(reg) el, options(nomem, nostack, preserves_flags)) };
    ((el >> 2) & 0b11) as u8
}

/// Which CPU core we are on, 0..=3.
///
/// `MPIDR_EL1` ("multiprocessor affinity") describes where a core sits in the
/// topology. On the BCM2712 the core number is affinity level 1, bits 15:8
/// (`bcm2712.dtsi`: `cpu1 { reg = <0x100> }`); level 0 is always 0 because
/// the A76 has one hardware thread per core.
#[inline]
pub fn core_id() -> usize {
    let mpidr: u64;
    // SAFETY: MPIDR_EL1 is read-only.
    unsafe { asm!("mrs {}, mpidr_el1", out(reg) mpidr, options(nomem, nostack, preserves_flags)) };
    ((mpidr >> 8) & 0xff) as usize
}

/// `nop`: do nothing for one instruction slot.
#[inline(always)]
pub fn nop() {
    // SAFETY: no effects.
    unsafe { asm!("nop", options(nomem, nostack, preserves_flags)) };
}

/// `wfe` ("wait for event"): sleep until an event arrives.
///
/// Events come from `sev` on any core, from the timer event stream the
/// runtime enables (every ~38 us), or from an interrupt (which we keep
/// masked). The executors' idle loops use it so idle cores stop burning
/// power. A `wfe` after an event was already received returns at once: the
/// event register is sticky, so wake-ups cannot be lost between "check queue"
/// and "sleep".
#[inline(always)]
pub fn wfe() {
    // SAFETY: only affects power state.
    unsafe { asm!("wfe", options(nomem, nostack, preserves_flags)) };
}

/// `sev`: send an event to every core, waking any that sit in [`wfe`].
#[inline(always)]
pub fn sev() {
    // SAFETY: no memory effects.
    unsafe { asm!("sev", options(nomem, nostack, preserves_flags)) };
}

/// `isb`: instruction synchronisation barrier. Makes system-register writes
/// (`msr`) before it visible to the instructions after it.
#[inline(always)]
pub fn isb() {
    // SAFETY: barrier.
    unsafe { asm!("isb", options(nostack, preserves_flags)) };
}

/// `dsb sy`: data synchronisation barrier. Waits until every memory access
/// and cache/TLB maintenance operation before it has completed.
#[inline(always)]
pub fn dsb() {
    // SAFETY: barrier.
    unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
}

/// `dmb ish`: data memory barrier, inner-shareable domain. Orders memory
/// accesses as seen by the other cores. Rust's atomics insert these for you
/// (that is what `Ordering::Acquire/Release` compile to); you only need it
/// by hand around non-atomic shared data.
#[inline(always)]
pub fn dmb() {
    // SAFETY: barrier.
    unsafe { asm!("dmb ish", options(nostack, preserves_flags)) };
}

/// Stop this core forever, in low power.
pub fn halt() -> ! {
    loop {
        wfe();
    }
}

//! `critical-section` implementation (feature `critical-section-impl`, on by
//! default), so crates like heapless, embassy-sync or once_cell work.
//!
//! A critical section must keep out BOTH interrupts on this core AND the
//! other three cores. So `acquire`:
//!   1. masks IRQ/FIQ on this core (saving the old DAIF mask), then
//!   2. takes one global spin lock shared by all cores.
//!
//! Nesting (`critical_section::with` inside another one on the same core)
//! is allowed: the lock remembers which core owns it, and a nested acquire
//! only notes "already held" in the restore state instead of spinning on
//! itself forever.

use core::sync::atomic::{AtomicUsize, Ordering};

/// Owning core + 1, or 0 when free.
static OWNER: AtomicUsize = AtomicUsize::new(0);

/// Bit 63 of the restore state: "this was a nested acquire, don't unlock".
/// DAIF itself only uses bits 9:6, so the bits can't collide.
const NESTED: u64 = 1 << 63;

struct Pi5CriticalSection;
critical_section::set_impl!(Pi5CriticalSection);

// SAFETY: interrupts are masked on this core and the global lock excludes
// the other cores for as long as the section is held; `release` restores
// exactly the state `acquire` saved.
unsafe impl critical_section::Impl for Pi5CriticalSection {
    unsafe fn acquire() -> u64 {
        let daif: u64;
        // SAFETY: reading DAIF and masking IRQ/FIQ at EL2 has no other effect.
        unsafe {
            core::arch::asm!("mrs {}, daif", out(reg) daif, options(nomem, nostack, preserves_flags));
            core::arch::asm!("msr daifset, #3", options(nomem, nostack, preserves_flags));
        }
        let me = crate::cpu::core_id() + 1;
        if OWNER.load(Ordering::Relaxed) == me {
            return daif | NESTED;
        }
        while OWNER.compare_exchange_weak(0, me, Ordering::Acquire, Ordering::Relaxed).is_err() {
            core::hint::spin_loop();
        }
        daif
    }

    unsafe fn release(state: u64) {
        if state & NESTED == 0 {
            OWNER.store(0, Ordering::Release);
        }
        let daif = state & !NESTED;
        // SAFETY: restores the interrupt mask saved by `acquire`.
        unsafe { core::arch::asm!("msr daif, {}", in(reg) daif, options(nomem, nostack, preserves_flags)) };
    }
}

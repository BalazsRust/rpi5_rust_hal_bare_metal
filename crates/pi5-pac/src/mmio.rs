//! Memory-mapped I/O primitives: the only way registers are ever touched.
//!
//! A peripheral register is a 32-bit word at a fixed physical address. Reading
//! or writing it is an ordinary load/store with two twists that make a plain
//! `*ptr = value` unusable:
//!
//! 1. **Side effects.** Writing the same value twice can mean "toggle twice";
//!    reading a UART data register *consumes* a byte. The compiler must not
//!    merge, reorder or drop these accesses as it may for normal memory.
//!    `core::ptr::read_volatile` / `write_volatile` promise exactly that.
//! 2. **Width.** Registers want whole 32-bit accesses. Using `u32` pointers
//!    guarantees `ldr w`/`str w` instructions.
//!
//! Ordering between CPU and device: the HAL maps every peripheral window as
//! *Device-nGnRnE* memory in its page table (and before the MMU is on, all
//! memory is treated that way). Device memory accesses are never reordered,
//! merged or speculated, so two register accesses in program order reach the
//! bus in program order and no extra barriers are needed between them.

/// Read a 32-bit register.
///
/// # Safety
/// `addr` must be a valid, 4-byte-aligned register address and reading it
/// must be acceptable (some registers have read side effects).
#[inline(always)]
pub unsafe fn read32(addr: usize) -> u32 {
    // SAFETY: the caller guarantees `addr` is a valid, aligned register.
    unsafe { core::ptr::read_volatile(addr as *const u32) }
}

/// Write a 32-bit register.
///
/// # Safety
/// `addr` must be a valid, 4-byte-aligned register address and `value` must
/// be meaningful for that register.
#[inline(always)]
pub unsafe fn write32(addr: usize, value: u32) {
    // SAFETY: the caller guarantees `addr` is a valid, aligned register.
    unsafe { core::ptr::write_volatile(addr as *mut u32, value) }
}

/// Read-modify-write: `reg = (reg & !clear) | set`.
///
/// # Safety
/// Same requirements as [`read32`] and [`write32`].
#[inline(always)]
pub unsafe fn modify32(addr: usize, clear: u32, set: u32) {
    // SAFETY: forwarded to the caller.
    unsafe {
        let v = read32(addr);
        write32(addr, (v & !clear) | set);
    }
}

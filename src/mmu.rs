//! The MMU: one page table, identity mapped, caches on.
//!
//! # Why bother
//!
//! With the MMU off, AArch64 treats *every* data access as Device-nGnRnE
//! memory: uncached, strictly ordered, no speculation. Two consequences:
//!
//! * It is slow. Every stack access goes to DRAM. Roughly 10-50x slower code.
//! * `ldxr`/`stxr`, the "exclusive" instructions behind every atomic
//!   read-modify-write (`compare_exchange`, `fetch_add`, spinlocks), are not
//!   guaranteed to work on Device memory (they need the cache's exclusive
//!   monitor). A multi-core executor cannot be built without them.
//!
//! # What we map
//!
//! We do not need virtual memory, only *attributes*. So the table maps each
//! address to itself ("identity map") and only says what kind of memory it is:
//!
//! | CPU address range                 | what                        | attribute          |
//! |-----------------------------------|-----------------------------|--------------------|
//! | `0x0_0000_0000` - `0x0_3FFF_FFFF` | first 1 GiB of RAM          | Normal, cacheable, inner-shareable |
//! | `0x10_0000_0000` - `0x10_7FFF_FFFF` | BCM2712 peripherals (`soc`) | Device-nGnRnE, XN  |
//! | `0x1F_0000_0000` - `0x1F_3FFF_FFFF` | PCIe window holding RP1     | Device-nGnRnE, XN  |
//!
//! Everything else is unmapped: touching it raises a translation fault that
//! the exception handler prints, which is a feature (a wild pointer is caught
//! instead of silently corrupting memory). Only 1 GiB of RAM is mapped because
//! that is all this crate uses; a Pi 5 has at least 2 GiB, so the mapping can
//! never point at non-existent memory, which the CPU is allowed to prefetch from.
//!
//! # How the table is shaped
//!
//! With the 4 KiB granule, a level-1 entry covers 1 GiB. We need addresses up
//! to `0x1F_4000_0000` = 2^37, so the virtual address space is 37 bits
//! (`TCR_EL2.T0SZ = 64 - 37 = 27`); at that size the walk starts at level 1
//! with a 128-entry table. Every entry is a *block descriptor* (bit 0 = 1,
//! bit 1 = 0) carrying the attributes directly, so there is exactly one table
//! and no second level. [`__pi5_rt_l1_table`] is that table (sized 512 for alignment).
//!
//! # The three registers (all `_EL2`, we run at EL2)
//!
//! * `MAIR_EL2`: a palette of up to 8 memory types; descriptors refer to a slot.
//!   Slot 0 = `0xFF` (Normal, write-back, read/write-allocate), slot 1 = `0x00`
//!   (Device-nGnRnE), slot 2 = `0x04` (Device-nGnRE).
//! * `TCR_EL2`: table geometry, see [`TCR_VALUE`].
//! * `TTBR0_EL2`: physical address of the table.
//!
//! Then `SCTLR_EL2.M` (MMU), `.C` (data cache) and `.I` (instruction cache)
//! are set; the asm in [`crate::rt`] does that, after `tlbi alle2`, so no
//! stale translations survive.
//!
//! # Caches and the table itself
//!
//! The table is filled while the MMU (and so the data cache) is off, meaning
//! the writes go straight to DRAM. The MMU's table walker reads through the
//! cache, so any *stale* cache line covering the table could hide the new
//! contents; [`dcache_invalidate_range`] discards such lines first. The same
//! is done for the whole image (`.bss`, stacks) that was written cache-off.
//!
//! Secondary cores reuse the same table: they enable their MMU with the same
//! three register values before touching any memory, and from then on all
//! cores are cache-coherent with each other (inner-shareable mapping).

use core::arch::asm;
use core::cell::UnsafeCell;

/// A 4 KiB-aligned level-1 table. 512 entries x 8 bytes = 4 KiB; only the
/// first 128 are used (37-bit address space / 1 GiB per entry).
#[repr(C, align(4096))]
pub struct PageTable(UnsafeCell<[u64; 512]>);

// SAFETY: written once by core 0 before any other core exists, then read-only
// (and only ever read by the hardware table walker).
unsafe impl Sync for PageTable {}

/// The one page table. Named without mangling so the startup assembly can load
/// its address into `TTBR0_EL2` with `adrp`.
#[unsafe(no_mangle)]
pub static __pi5_rt_l1_table: PageTable = PageTable(UnsafeCell::new([0; 512]));

/// `MAIR_EL2`: slot 0 Normal WB RW-allocate (0xFF), slot 1 Device-nGnRnE
/// (0x00), slot 2 Device-nGnRE (0x04). One byte per slot, slot 0 lowest.
pub const MAIR_VALUE: u64 = 0x0000_0000_0004_00FF;

/// `TCR_EL2` (EL2 without VHE), field by field:
///
/// | bits  | field | value | meaning                                   |
/// |-------|-------|-------|-------------------------------------------|
/// | 5:0   | T0SZ  | 27    | 37-bit virtual addresses                  |
/// | 9:8   | IRGN0 | 1     | table walks: inner write-back cacheable   |
/// | 11:10 | ORGN0 | 1     | table walks: outer write-back cacheable   |
/// | 13:12 | SH0   | 3     | table walks: inner shareable              |
/// | 15:14 | TG0   | 0     | 4 KiB granule                             |
/// | 18:16 | PS    | 2     | 40-bit physical addresses                 |
/// | 23    | RES1  | 1     |                                           |
/// | 31    | RES1  | 1     |                                           |
pub const TCR_VALUE: u64 = 0x8082_351B;

/// `SCTLR_EL2` bits the runtime sets: M (bit 0), C (bit 2), I (bit 12).
pub const SCTLR_MMU_CACHES: u64 = (1 << 0) | (1 << 2) | (1 << 12);

// Block descriptor bits (ARM ARM, "VMSAv8-64 block and page descriptor").
const DESC_BLOCK: u64 = 0b01; // bits 1:0: valid block (0b11 would be a table)
const DESC_ATTRIDX_SHIFT: u64 = 2; // bits 4:2: MAIR slot
const DESC_AP_RW: u64 = 1 << 6; // AP[2:1] = 0b01: read/write (AP[1] must be 1 at EL2)
const DESC_SH_OUTER: u64 = 0b10 << 8;
const DESC_SH_INNER: u64 = 0b11 << 8; // multi-core cache coherence needs inner-shareable
const DESC_AF: u64 = 1 << 10; // access flag: must be set or the first access faults
const DESC_XN: u64 = 1 << 54; // execute-never: never fetch instructions from devices

/// MAIR slot of Normal memory.
pub const ATTR_NORMAL: u64 = 0;
/// MAIR slot of Device-nGnRnE memory.
pub const ATTR_DEVICE: u64 = 1;

/// Descriptor for a 1 GiB block of Normal cacheable RAM at `pa`.
pub const fn normal_block(pa: u64) -> u64 {
    pa | DESC_BLOCK | (ATTR_NORMAL << DESC_ATTRIDX_SHIFT) | DESC_AP_RW | DESC_SH_INNER | DESC_AF
}

/// Descriptor for a 1 GiB block of Device memory at `pa`.
pub const fn device_block(pa: u64) -> u64 {
    pa | DESC_BLOCK | (ATTR_DEVICE << DESC_ATTRIDX_SHIFT) | DESC_AP_RW | DESC_SH_OUTER | DESC_AF | DESC_XN
}

/// How much RAM to map, in 1 GiB blocks.
pub const RAM_GIB: usize = 1;

/// Fill [`__pi5_rt_l1_table`]. Called by `_start` on core 0 with the MMU off.
///
/// # Safety
/// Must run exactly once, before any other core is started, with the MMU off.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn __pi5_rt_init_mmu_tables() {
    let table = __pi5_rt_l1_table.0.get() as *mut u64;
    // SAFETY: single core, MMU off, exclusive access to the table.
    unsafe {
        for i in 0..512 {
            table.add(i).write_volatile(0);
        }
        for gib in 0..RAM_GIB {
            table.add(gib).write_volatile(normal_block((gib as u64) << 30));
        }
        // BCM2712 soc window: 0x10_0000_0000, 2 GiB (pi5_pac::bcm2712::SOC_SIZE).
        let soc_first = pi5_pac::bcm2712::SOC_BASE >> 30;
        let soc_blocks = pi5_pac::bcm2712::SOC_SIZE >> 30;
        for i in soc_first..soc_first + soc_blocks {
            table.add(i).write_volatile(device_block((i as u64) << 30));
        }
        // RP1 PCIe window: 0x1F_0000_0000, 1 GiB is plenty (RP1 uses 4 MiB).
        let rp1 = pi5_pac::rp1::BASE >> 30;
        table.add(rp1).write_volatile(device_block((rp1 as u64) << 30));
    }

    // Everything written so far (table, .bss, stack) went to DRAM because the
    // cache is off. Discard any stale cache lines covering those addresses so
    // that, once caches are on, DRAM is what gets read.
    dsb();
    unsafe extern "C" {
        static __image_start: u8;
        static __image_end: u8;
    }
    // SAFETY: linker-provided symbols; only their addresses are used.
    let (start, end) = (core::ptr::addr_of!(__image_start) as usize, core::ptr::addr_of!(__image_end) as usize);
    dcache_invalidate_range(start, end);
    dcache_invalidate_range(table as usize, table as usize + 4096);
}

/// Is `SCTLR_EL2.M` set?
pub fn is_enabled() -> bool {
    let sctlr: u64;
    // SAFETY: reading SCTLR_EL2 at EL2.
    unsafe { asm!("mrs {}, sctlr_el2", out(reg) sctlr, options(nomem, nostack, preserves_flags)) };
    sctlr & 1 != 0
}

/// Data cache line size in bytes, from `CTR_EL0.DminLine` (log2 of words).
pub fn dcache_line_size() -> usize {
    let ctr: u64;
    // SAFETY: CTR_EL0 is read-only.
    unsafe { asm!("mrs {}, ctr_el0", out(reg) ctr, options(nomem, nostack, preserves_flags)) };
    4 << ((ctr >> 16) & 0xf)
}

/// Invalidate (discard without writing back) every data cache line that
/// overlaps `[start, end)`. `dc ivac` = "data cache invalidate by virtual
/// address to point of coherency".
pub fn dcache_invalidate_range(start: usize, end: usize) {
    let line = dcache_line_size();
    let mut addr = start & !(line - 1);
    while addr < end {
        // SAFETY: cache maintenance on an address we own.
        unsafe { asm!("dc ivac, {}", in(reg) addr, options(nostack, preserves_flags)) };
        addr += line;
    }
    dsb();
}

/// Write back (clean) every data cache line overlapping `[start, end)` to
/// memory, keeping it in the cache. Needed before handing data to something
/// that reads memory without going through our caches (a core whose MMU is
/// still off, or a DMA engine).
pub fn dcache_clean_range(start: usize, end: usize) {
    let line = dcache_line_size();
    let mut addr = start & !(line - 1);
    while addr < end {
        // SAFETY: cache maintenance on an address we own.
        unsafe { asm!("dc cvac, {}", in(reg) addr, options(nostack, preserves_flags)) };
        addr += line;
    }
    dsb();
}

#[inline(always)]
fn dsb() {
    // SAFETY: barrier.
    unsafe { asm!("dsb sy", options(nostack, preserves_flags)) };
}

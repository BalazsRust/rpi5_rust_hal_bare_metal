//! Runtime: what happens between the firmware's jump and your `main`.
//!
//! The firmware gives us a CPU with nothing set up: no stack, `.bss` not
//! zeroed, no exception vectors, MMU off, FP/SIMD instructions trapping. Rust
//! code assumes all of that is handled, so some assembly must run first. This
//! module contains it, plus the [`crate::entry!`] macro that connects it to your
//! `fn main() -> !`. (`#[pi5_executor::main]` expands to the same thing.)
//!
//! # State at entry (what we rely on)
//!
//! From the Linux arm64 boot protocol, which the Pi firmware follows:
//!
//! * core 0 only (the Pi 5 uses PSCI; cores 1-3 wait inside the firmware
//!   until [`crate::multicore`] asks for them), in **EL2**, AArch64
//! * MMU off, data cache off, interrupts (IRQ/FIQ/SError) **masked**
//! * `x0` = physical address of the device tree blob, `x1`-`x3` = 0
//! * PC = `0x80000` = first byte of `kernel8.img`
//!
//! # What `_start` does, in order
//!
//! 1. Saves `x0` (the DTB pointer).
//! 2. Parks any core that is not core 0 (defensive; should not happen).
//! 3. Refuses to continue unless we are in EL2 (the MMU code is EL2-specific).
//! 4. `__pi5_rt_core_init` (also run by every secondary core later):
//!    - un-trap FP/SIMD: the `aarch64-unknown-none` target compiles with NEON
//!      on, so even `memcpy` may use `q` registers; at reset those trap
//!      (`CPTR_EL2.TFP`, `CPACR_EL1.FPEN`)
//!    - install the exception vector table (`VBAR_EL2`)
//!    - `CNTVOFF_EL2 = 0` so every core's virtual counter equals the shared
//!      physical one (its reset value is unknown per core)
//!    - enable the timer **event stream** (`CNTHCTL_EL2.EVNTEN`, `EVNTI = 10`):
//!      a wake-up event every 2^11 counter ticks (~38 us at 54 MHz) so that
//!      `wfe` idle loops can poll timers without any interrupt controller setup
//! 5. Selects `SP_EL2` and points it at `__stack_top` from the linker script.
//! 6. Zeroes `.bss`.
//! 7. Builds the page table ([`crate::mmu`]) and turns on the MMU and caches
//!    (`__pi5_rt_enable_mmu`).
//! 8. Calls `__pi5_rt_main`, which records the DTB address and calls `main`.
//!
//! Secondary cores enter at `__pi5_rt_secondary_start` (see
//! [`crate::multicore`]): steps 4 and 7, then the stack and closure they were
//! given.

use core::arch::global_asm;
use core::sync::atomic::{AtomicUsize, Ordering};

/// Per-core emergency stacks for the exception handler, 4 KiB each. The
/// exception may be *caused* by a broken stack pointer, so the handler never
/// trusts the current one.
#[unsafe(no_mangle)]
static mut __pi5_rt_exc_stacks: ExcStacks = ExcStacks([0; 4 * 4096]);

#[repr(C, align(16))]
struct ExcStacks([u8; 4 * 4096]);

global_asm!(
    // ------------------------------------------------------------------
    // Entry point: byte 0 of kernel8.img (linker script: KEEP(*(.text._start))).
    // ------------------------------------------------------------------
    ".section .text._start, \"ax\"",
    ".global _start",
    ".type _start, %function",
    "_start:",
    "    mov  x19, x0",                 // x0 = device tree address; x19 is callee-saved

    // ---- 2. only core 0 continues -------------------------------------
    "    mrs  x1, mpidr_el1",
    "    and  x1, x1, #0xffffff",       // Aff2:Aff1:Aff0 all zero = core 0
    "    cbz  x1, 1f",
    "0:  wfe",
    "    b    0b",
    "1:",
    // ---- 3. EL2 required ----------------------------------------------
    "    mrs  x1, CurrentEL",
    "    lsr  x1, x1, #2",
    "    cmp  x1, #2",
    "    b.ne 0b",                      // not EL2: park (no way to report yet)

    // ---- 4. per-core init ---------------------------------------------
    "    bl   __pi5_rt_core_init",      // leaf: safe to call before SP exists

    // ---- 5. stack -----------------------------------------------------
    "    msr  spsel, #1",               // use SP_EL2, not SP_EL0
    "    adrp x2, __stack_top",
    "    add  x2, x2, :lo12:__stack_top",
    "    mov  sp, x2",

    // ---- 6. zero .bss -------------------------------------------------
    "    adrp x2, __bss_start",
    "    add  x2, x2, :lo12:__bss_start",
    "    adrp x3, __bss_end",
    "    add  x3, x3, :lo12:__bss_end",
    "4:  cmp  x2, x3",
    "    b.hs 5f",
    "    str  xzr, [x2], #8",           // store zero, post-increment by 8
    "    b    4b",
    "5:",

    // ---- 7. MMU + caches ----------------------------------------------
    "    bl   __pi5_rt_init_mmu_tables", // Rust, runs with MMU off
    "    bl   __pi5_rt_enable_mmu",

    // ---- 8. into Rust -------------------------------------------------
    "    mov  x0, x19",
    "    bl   __pi5_rt_main",
    "6:  wfe",                          // never reached
    "    b    6b",
    ".size _start, . - _start",

    // ------------------------------------------------------------------
    // Per-core initialisation. Leaf function: uses x0/x1 only, no stack.
    // ------------------------------------------------------------------
    ".global __pi5_rt_core_init",
    ".type __pi5_rt_core_init, %function",
    "__pi5_rt_core_init:",
    // CPTR_EL2 = 0x33ff: all RES1 bits set, TFP (bit 10) clear -> no FP trap.
    // Same value Linux's head.S writes.
    "    mov  x0, #0x33ff",
    "    msr  cptr_el2, x0",
    // CPACR_EL1.FPEN = 0b11 (bits 21:20): no FP trap at EL1/EL0 either.
    "    mov  x0, #0x300000",
    "    msr  cpacr_el1, x0",
    // Exception vectors.
    "    adrp x0, __pi5_rt_vectors",
    "    add  x0, x0, :lo12:__pi5_rt_vectors",
    "    msr  vbar_el2, x0",
    // Virtual counter offset 0: CNTVCT_EL0 == CNTPCT_EL0 on every core.
    "    msr  cntvoff_el2, xzr",
    // CNTHCTL_EL2: EL1PCTEN|EL1PCEN (bits 1:0), EVNTEN (bit 2), EVNTI = 10 (bits 7:4).
    "    mrs  x0, cnthctl_el2",
    "    orr  x0, x0, #0x3",
    "    bic  x0, x0, #0xf0",
    "    mov  x1, #0xa4",
    "    orr  x0, x0, x1",
    "    msr  cnthctl_el2, x0",
    // Same bits in CNTKCTL_EL1 (the EL1-side control), so the event stream
    // runs whichever of the two the hardware honours at EL2.
    "    mrs  x0, cntkctl_el1",
    "    bic  x0, x0, #0xf0",
    "    orr  x0, x0, x1",
    "    msr  cntkctl_el1, x0",
    "    isb",
    "    ret",

    // ------------------------------------------------------------------
    // Turn on the MMU, data cache and instruction cache. Leaf, no stack.
    // The table must already be filled (core 0 does that once).
    // ------------------------------------------------------------------
    ".global __pi5_rt_enable_mmu",
    ".type __pi5_rt_enable_mmu, %function",
    "__pi5_rt_enable_mmu:",
    "    mov  x0, #0x00ff",             // MAIR_EL2 = 0x000400FF, see mmu::MAIR_VALUE
    "    movk x0, #0x0004, lsl #16",
    "    msr  mair_el2, x0",
    "    mov  x0, #0x351b",             // TCR_EL2 = 0x8082351B, see mmu::TCR_VALUE
    "    movk x0, #0x8082, lsl #16",
    "    msr  tcr_el2, x0",
    "    adrp x0, __pi5_rt_l1_table",
    "    add  x0, x0, :lo12:__pi5_rt_l1_table",
    "    msr  ttbr0_el2, x0",
    "    isb",
    "    tlbi alle2",                   // throw away any cached translations
    "    dsb  sy",
    "    isb",
    "    ic   iallu",                   // and any stale instruction cache lines
    "    dsb  sy",
    "    isb",
    "    mrs  x0, sctlr_el2",
    "    orr  x0, x0, #(1 << 0)",       // M: MMU on
    "    orr  x0, x0, #(1 << 2)",       // C: data cache on
    "    orr  x0, x0, #(1 << 12)",      // I: instruction cache on
    "    msr  sctlr_el2, x0",
    "    isb",
    "    ret",

    // ------------------------------------------------------------------
    // Secondary core entry (PSCI CPU_ON lands here, MMU off, x0 = context).
    // The context points at a `multicore::CoreBootInfo` in Normal memory,
    // which we may only read AFTER our MMU is on (cache coherence).
    // ------------------------------------------------------------------
    ".global __pi5_rt_secondary_start",
    ".type __pi5_rt_secondary_start, %function",
    "__pi5_rt_secondary_start:",
    "    mov  x19, x0",
    "    bl   __pi5_rt_core_init",
    "    bl   __pi5_rt_enable_mmu",
    "    msr  spsel, #1",
    "    ldr  x0, [x19, #0]",           // CoreBootInfo.stack_top
    "    mov  sp, x0",
    "    ldr  x1, [x19, #8]",           // CoreBootInfo.trampoline
    "    ldr  x0, [x19, #16]",          // CoreBootInfo.closure
    "    blr  x1",                      // never returns
    "7:  wfe",
    "    b    7b",

    // ------------------------------------------------------------------
    // Exception vector table: 16 vectors x 128 bytes, 2 KiB aligned.
    // We never return from an exception, so each entry just records which
    // vector fired and jumps to common code that collects:
    //   ESR_EL2  "exception syndrome": what kind of exception and why
    //   ELR_EL2  "exception link": PC of the faulting instruction
    //   FAR_EL2  "fault address": the data address that faulted
    // ------------------------------------------------------------------
    ".section .vectors, \"ax\"",
    ".balign 0x800",
    ".global __pi5_rt_vectors",
    "__pi5_rt_vectors:",
    ".macro PI5_VECTOR kind",
    "    .balign 0x80",
    "    mov  x0, #\\kind",
    "    b    __pi5_rt_vector_common",
    ".endm",
    "    PI5_VECTOR 0",   // current EL, SP_EL0: synchronous
    "    PI5_VECTOR 1",   //                     IRQ
    "    PI5_VECTOR 2",   //                     FIQ
    "    PI5_VECTOR 3",   //                     SError
    "    PI5_VECTOR 4",   // current EL, SP_ELx (us): synchronous
    "    PI5_VECTOR 5",   //                          IRQ
    "    PI5_VECTOR 6",   //                          FIQ
    "    PI5_VECTOR 7",   //                          SError
    "    PI5_VECTOR 8",   // lower EL, AArch64: synchronous
    "    PI5_VECTOR 9",   //                    IRQ
    "    PI5_VECTOR 10",  //                    FIQ
    "    PI5_VECTOR 11",  //                    SError
    "    PI5_VECTOR 12",  // lower EL, AArch32: synchronous
    "    PI5_VECTOR 13",  //                    IRQ
    "    PI5_VECTOR 14",  //                    FIQ
    "    PI5_VECTOR 15",  //                    SError
    ".purgem PI5_VECTOR",

    "__pi5_rt_vector_common:",
    "    mrs  x1, esr_el2",
    "    mrs  x2, elr_el2",
    "    mrs  x3, far_el2",
    // Fresh per-core stack: top = __pi5_rt_exc_stacks + 16 KiB - core * 4 KiB.
    "    mrs  x4, mpidr_el1",
    "    ubfx x4, x4, #8, #8",          // Aff1 = core number
    "    lsl  x4, x4, #12",
    "    adrp x5, __pi5_rt_exc_stacks",
    "    add  x5, x5, :lo12:__pi5_rt_exc_stacks",
    "    add  x5, x5, #0x4000",
    "    sub  x5, x5, x4",
    "    mov  sp, x5",
    "    bl   __pi5_rt_exception",
    "8:  wfe",
    "    b    8b",
);

static DTB_ADDRESS: AtomicUsize = AtomicUsize::new(0);

/// Physical address of the device tree blob the firmware passed in `x0`.
/// (Not parsed by this crate; exposed because it is interesting.)
pub fn dtb_address() -> usize {
    DTB_ADDRESS.load(Ordering::Relaxed)
}

unsafe extern "C" {
    /// Generated by [`entry!`] or `#[pi5_executor::main]` in the user's crate.
    fn __pi5_rt_user_main() -> !;
}

/// Called by `_start` on core 0 with the MMU on, a stack, and zeroed `.bss`.
#[unsafe(no_mangle)]
unsafe extern "C" fn __pi5_rt_main(dtb: usize) -> ! {
    DTB_ADDRESS.store(dtb, Ordering::Relaxed);
    // SAFETY: `entry!` guarantees this symbol is a `fn() -> !`.
    unsafe { __pi5_rt_user_main() }
}

/// Declare the function that runs after the runtime has set things up.
///
/// ```ignore
/// pi5_hal::entry!(main);
/// fn main() -> ! { loop {} }
/// ```
///
/// Expands to an `extern "C"` shim named `__pi5_rt_user_main` that the
/// startup code calls, after checking at compile time that `main` really has
/// the type `fn() -> !`. Forgetting this macro (or `#[pi5_executor::main]`)
/// shows up as a linker error: `undefined symbol: __pi5_rt_user_main`.
#[macro_export]
macro_rules! entry {
    ($f:path) => {
        #[unsafe(no_mangle)]
        pub extern "C" fn __pi5_rt_user_main() -> ! {
            let f: fn() -> ! = $f;
            f()
        }
    };
}

// ---------------------------------------------------------------------------
// Exception reporting
// ---------------------------------------------------------------------------

fn vector_name(kind: u64) -> &'static str {
    match kind {
        0 => "Synchronous (current EL, SP_EL0)",
        1 => "IRQ (current EL, SP_EL0)",
        2 => "FIQ (current EL, SP_EL0)",
        3 => "SError (current EL, SP_EL0)",
        4 => "Synchronous (current EL)",
        5 => "IRQ (current EL)",
        6 => "FIQ (current EL)",
        7 => "SError (current EL)",
        8 => "Synchronous (lower EL, AArch64)",
        9 => "IRQ (lower EL, AArch64)",
        10 => "FIQ (lower EL, AArch64)",
        11 => "SError (lower EL, AArch64)",
        12 => "Synchronous (lower EL, AArch32)",
        13 => "IRQ (lower EL, AArch32)",
        14 => "FIQ (lower EL, AArch32)",
        15 => "SError (lower EL, AArch32)",
        _ => "?",
    }
}

/// Decode `ESR_EL2.EC` (bits 31:26), the exception class.
fn exception_class(esr: u64) -> &'static str {
    match (esr >> 26) & 0x3f {
        0x00 => "unknown reason",
        0x07 => "FP/SIMD instruction trapped (CPTR_EL2.TFP / CPACR_EL1.FPEN)",
        0x0e => "illegal execution state",
        0x15 => "SVC",
        0x16 => "HVC",
        0x17 => "SMC",
        0x18 => "MSR/MRS/system instruction trapped",
        0x20 | 0x21 => "instruction abort",
        0x22 => "PC alignment fault",
        0x24 | 0x25 => "data abort",
        0x26 => "SP alignment fault",
        0x2c => "floating-point exception",
        0x2f => "SError",
        0x30 | 0x31 => "breakpoint",
        0x32 | 0x33 => "software step",
        0x34 | 0x35 => "watchpoint",
        0x3c => "BRK instruction",
        _ => "?",
    }
}

/// For data/instruction aborts, decode the fault status code (bits 5:0).
fn fault_status(esr: u64) -> Option<&'static str> {
    let ec = (esr >> 26) & 0x3f;
    if !matches!(ec, 0x20 | 0x21 | 0x24 | 0x25) {
        return None;
    }
    Some(match esr & 0x3f {
        0x00..=0x03 => "address size fault",
        0x04..=0x07 => "translation fault: address not mapped (see pi5_hal::mmu for what is)",
        0x08..=0x0b => "access flag fault",
        0x0c..=0x0f => "permission fault",
        0x10 => "synchronous external abort: the bus returned an error. Peripheral not \
                 reachable? (RP1 needs pciex4_reset=0 in config.txt)",
        0x11 => "synchronous tag check fault",
        0x18 => "synchronous parity/ECC error",
        0x21 => "alignment fault: unaligned access to Device memory",
        _ => "other",
    })
}

/// Target of every exception vector. Prints a diagnosis and halts this core.
#[unsafe(no_mangle)]
extern "C" fn __pi5_rt_exception(kind: u64, esr: u64, elr: u64, far: u64) -> ! {
    use core::fmt::Write;

    // SAFETY: normal execution on this core is over.
    let mut uart = unsafe { crate::uart::Uart::<crate::Blocking>::emergency() };
    let _ = writeln!(uart, "\n!!! EXCEPTION on core {}: {}", crate::cpu::core_id(), vector_name(kind));
    let _ = writeln!(uart, "    ESR = {esr:#018x}  ({})", exception_class(esr));
    if let Some(fsc) = fault_status(esr) {
        let _ = writeln!(uart, "          {fsc}");
    }
    let _ = writeln!(uart, "    ELR = {elr:#018x}  (faulting instruction)");
    let _ = writeln!(uart, "    FAR = {far:#018x}  (faulting address)");
    let _ = writeln!(uart, "    EL{}, MMU {}", crate::cpu::current_el(), if crate::mmu::is_enabled() { "on" } else { "off" });
    uart.flush();
    crate::cpu::halt()
}

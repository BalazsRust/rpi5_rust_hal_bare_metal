//! Running code on cores 1-3, embassy-rp `multicore` style.
//!
//! ```no_run
//! use pi5_hal::multicore::{spawn_core1, Stack};
//! static mut CORE1_STACK: Stack<65536> = Stack::new();
//!
//! # fn demo(p: pi5_hal::Peripherals) {
//! spawn_core1(p.CORE1, unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) }, || {
//!     // runs on core 1, forever (typically: an executor's `run`)
//!     loop { pi5_hal::cpu::wfe() }
//! }).unwrap();
//! # }
//! ```
//!
//! # How a core is started
//!
//! 1. The closure is moved to the top of the stack memory you provide, and a
//!    small [`CoreBootInfo`] record (stack top, a trampoline that knows the
//!    closure's type, the closure's address) is placed just below it.
//! 2. Those lines are cleaned to DRAM (belt and braces: the new core will
//!    enable its MMU and become cache-coherent *before* reading them, but a
//!    clean costs nothing).
//! 3. `PSCI CPU_ON` ([`crate::psci`]) asks the firmware to start the core at
//!    `__pi5_rt_secondary_start` with the record's address in `x0`.
//! 4. The new core (see [`crate::rt`]) runs the per-core init (FP, vectors,
//!    counter offset, event stream), enables the MMU with the shared page
//!    table, loads its stack pointer from the record and jumps to the
//!    trampoline, which reads the closure out of memory and calls it.
//!
//! The closure never returns; a core that wants to stop can call
//! [`crate::psci::cpu_off`] or [`crate::cpu::halt`].

use crate::cpu;
use crate::mmu;
use crate::psci;
use core::mem::{MaybeUninit, align_of, size_of};
use pi5_pac::bcm2712::CORE_MPIDR;

/// A type with no values; closures given to [`spawn_core`] must never return.
/// (`!` is not yet allowed in that position on stable Rust.)
pub enum Never {}

/// Stack memory for a secondary core. Must be `static` and 16-byte aligned,
/// which this type guarantees.
#[repr(C, align(16))]
pub struct Stack<const SIZE: usize> {
    /// The raw bytes.
    pub mem: [MaybeUninit<u8>; SIZE],
}

impl<const SIZE: usize> Stack<SIZE> {
    /// An uninitialised stack.
    pub const fn new() -> Self {
        Self { mem: [MaybeUninit::uninit(); SIZE] }
    }
}

impl<const SIZE: usize> Default for Stack<SIZE> {
    fn default() -> Self {
        Self::new()
    }
}

/// What `__pi5_rt_secondary_start` reads. Field order is fixed: the assembly
/// loads them at offsets 0, 8 and 16.
#[repr(C)]
pub struct CoreBootInfo {
    stack_top: usize,
    trampoline: unsafe extern "C" fn(*mut ()) -> !,
    closure: *mut (),
}

/// Why a core could not be started.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// The stack is too small to hold the closure plus a working margin.
    StackTooSmall,
    /// The firmware refused; see [`psci::error_name`].
    Psci(i64),
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::StackTooSmall => write!(f, "stack too small"),
            Error::Psci(code) => write!(f, "PSCI CPU_ON failed: {} ({code})", psci::error_name(*code)),
        }
    }
}

mod sealed {
    pub trait Sealed {}
}

/// Ownership token for a secondary core; lives in [`crate::Peripherals`].
pub trait Core: sealed::Sealed {
    /// Core number, 1..=3.
    const ID: usize;
}

macro_rules! core_token {
    ($name:ident, $id:literal) => {
        #[doc = concat!("Ownership token for core ", stringify!($id), ".")]
        #[derive(Debug)]
        pub struct $name {
            _private: (),
        }
        impl $name {
            pub(crate) const fn new() -> Self {
                Self { _private: () }
            }
        }
        impl sealed::Sealed for $name {}
        impl Core for $name {
            const ID: usize = $id;
        }
    };
}
core_token!(CORE1, 1);
core_token!(CORE2, 2);
core_token!(CORE3, 3);

/// Start core 1 running `entry` on `stack`.
pub fn spawn_core1<F, const SIZE: usize>(core: CORE1, stack: &'static mut Stack<SIZE>, entry: F) -> Result<(), Error>
where
    F: FnOnce() -> Never + Send + 'static,
{
    spawn_core(core, stack, entry)
}

/// Start core 2 running `entry` on `stack`.
pub fn spawn_core2<F, const SIZE: usize>(core: CORE2, stack: &'static mut Stack<SIZE>, entry: F) -> Result<(), Error>
where
    F: FnOnce() -> Never + Send + 'static,
{
    spawn_core(core, stack, entry)
}

/// Start core 3 running `entry` on `stack`.
pub fn spawn_core3<F, const SIZE: usize>(core: CORE3, stack: &'static mut Stack<SIZE>, entry: F) -> Result<(), Error>
where
    F: FnOnce() -> Never + Send + 'static,
{
    spawn_core(core, stack, entry)
}

/// Start any secondary core running `entry` on `stack`.
pub fn spawn_core<C, F, const SIZE: usize>(_core: C, stack: &'static mut Stack<SIZE>, entry: F) -> Result<(), Error>
where
    C: Core,
    F: FnOnce() -> Never + Send + 'static,
{
    let base = stack.mem.as_mut_ptr() as usize;
    let end = base + SIZE;

    // Closure at the very top, aligned for its type (or 16).
    let align = align_of::<F>().max(16);
    let closure_addr = (end - size_of::<F>()) & !(align - 1);
    // Boot record below it.
    let info_addr = (closure_addr - size_of::<CoreBootInfo>()) & !15;
    // The new core's stack starts below the record and grows down.
    let stack_top = info_addr & !15;
    if stack_top < base + 4096 {
        return Err(Error::StackTooSmall);
    }

    // SAFETY: the addresses are inside `stack`, which we own exclusively, and
    // are aligned for their types.
    unsafe {
        (closure_addr as *mut F).write(entry);
        (info_addr as *mut CoreBootInfo).write(CoreBootInfo {
            stack_top,
            trampoline: core_trampoline::<F>,
            closure: closure_addr as *mut (),
        });
    }

    // Make sure the record and closure are in DRAM, not only in our cache.
    mmu::dcache_clean_range(stack_top, end);
    cpu::dsb();

    unsafe extern "C" {
        fn __pi5_rt_secondary_start();
    }
    // SAFETY: the entry point is the runtime's secondary-core trampoline and
    // `info_addr` points at a valid record it knows how to read.
    unsafe { psci::cpu_on(CORE_MPIDR[C::ID], __pi5_rt_secondary_start as usize, info_addr) }.map_err(Error::Psci)
}

/// Runs on the new core, with its MMU on and `sp` set: take the closure out
/// of the stack memory and call it.
#[allow(unreachable_code)]
unsafe extern "C" fn core_trampoline<F: FnOnce() -> Never>(closure: *mut ()) -> ! {
    // SAFETY: `spawn_core` wrote an `F` there and nobody else reads it.
    let f: F = unsafe { (closure as *mut F).read() };
    match f() {}
}

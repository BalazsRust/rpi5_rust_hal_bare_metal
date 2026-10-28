//! Measure the CPU clock two ways, against the 54 MHz system timer:
//!
//! 1. PMU cycle counter: CPU cycles counted during 100 ms.
//! 2. A chain of dependent `add`s: each one needs the previous result, so
//!    the Cortex-A76 does exactly one per clock cycle. N adds = N cycles.
//!
//! The two numbers should agree.

#![no_std]
#![no_main]

use pi5_hal::delay::Delay;
use pi5_hal::info;

fn cycles() -> u64 {
    let c: u64;
    // SAFETY: reading the cycle counter has no side effects.
    unsafe { core::arch::asm!("isb", "mrs {}, pmccntr_el0", out(reg) c, options(nostack)) };
    c
}

/// `n` dependent adds, 10 per loop round (the loop's own compare/branch run
/// in parallel with them on this out-of-order core).
fn add_chain(n: u64) {
    let mut x: u64 = 0;
    let mut i = n / 10;
    // SAFETY: plain arithmetic on registers.
    unsafe {
        core::arch::asm!(
            "2:",
            "add {x}, {x}, #1", "add {x}, {x}, #1", "add {x}, {x}, #1", "add {x}, {x}, #1", "add {x}, {x}, #1",
            "add {x}, {x}, #1", "add {x}, {x}, #1", "add {x}, {x}, #1", "add {x}, {x}, #1", "add {x}, {x}, #1",
            "subs {i}, {i}, #1",
            "b.ne 2b",
            x = inout(reg) x,
            i = inout(reg) i,
            options(nomem, nostack),
        );
    }
    let _ = (x, i);
}

pi5_hal::entry!(main);

fn main() -> ! {
    let _p = pi5_hal::init();
    // SAFETY: enable the PMU cycle counter (PMCR.E = 1, LC = 1 for 64 bits,
    // no divider), let it count at EL2 where we run (PMCCFILTR.NSH, bit 27;
    // by default it does not), and turn on counter 31 (the cycle counter).
    unsafe {
        core::arch::asm!("msr pmcr_el0, {}", in(reg) (1u64 << 0) | (1u64 << 6));
        core::arch::asm!("msr pmccfiltr_el0, {}", in(reg) 1u64 << 27);
        core::arch::asm!("msr pmcntenset_el0, {}", in(reg) 1u64 << 31);
        core::arch::asm!("isb");
    }
    let freq = Delay::frequency();
    loop {
        let (t0, c0) = (Delay::now(), cycles());
        while Delay::now() - t0 < freq / 10 {}
        let (t1, c1) = (Delay::now(), cycles());
        let pmu_mhz = (c1 - c0) * freq / (t1 - t0) / 1_000_000;

        const ADDS: u64 = 100_000_000;
        let t0 = Delay::now();
        add_chain(ADDS);
        let t1 = Delay::now();
        let add_mhz = ADDS * freq / (t1 - t0) / 1_000_000;

        info!("CPU clock: {} MHz (cycle counter), {} MHz (add chain)", pmu_mhz, add_mhz);
        Delay::new().delay_millis(1000);
    }
}

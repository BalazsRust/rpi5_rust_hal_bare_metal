//! All four cores: one executor per core, talking through a channel.
//!
//! Core 0 runs `main` and a printer task. Cores 1-3 are started with PSCI
//! and each runs its own executor with a worker task that sends a message
//! every second (core 1 also blinks the LED). The printer prints whatever
//! arrives, tagged with the sender's core.
//!
//! This is the embassy-rp `multicore` pattern generalised to 4 cores.

#![no_std]
#![no_main]

use core::fmt::Write;
use pi5_executor::{Executor, Spawner};
use pi5_hal::Async;
use pi5_hal::gpio::{Level, Output, OutputConfig};
use pi5_hal::multicore::{Stack, spawn_core1, spawn_core2, spawn_core3};
use pi5_hal::uart::{Config, Uart};
use pi5_hal::{cpu, mmu};
use pi5_sync::blocking_mutex::raw::CriticalSectionRawMutex;
use pi5_sync::channel::Channel;
use pi5_time::{Duration, Instant, Timer};

/// Each secondary core needs its own stack. 64 KiB is generous.
static mut CORE1_STACK: Stack<65536> = Stack::new();
static mut CORE2_STACK: Stack<65536> = Stack::new();
static mut CORE3_STACK: Stack<65536> = Stack::new();

#[derive(Clone, Copy)]
struct Msg {
    core: usize,
    count: u32,
    sent_at: Instant,
}

/// Cross-core channel: needs the CriticalSection (spinlock) strategy.
static MESSAGES: Channel<CriticalSectionRawMutex, Msg, 16> = Channel::new();

#[pi5_executor::main]
async fn main(spawner: Spawner) {
    let p = pi5_hal::init();
    let mut uart = Uart::new(p.UART10, Config::default()).unwrap().into_async();
    let _ = writeln!(uart, "multicore: core {} running main, MMU {}", cpu::core_id(), if mmu::is_enabled() { "on" } else { "off" });

    let led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());

    // Each closure runs on its core forever: it builds an executor on that
    // core's stack and runs it with one worker task.
    spawn_core1(p.CORE1, unsafe { &mut *core::ptr::addr_of_mut!(CORE1_STACK) }, move || {
        let mut executor = Executor::new();
        let executor = unsafe { pi5_executor::_export::make_static(&mut executor) };
        executor.run(|spawner| {
            spawner.spawn(worker(1).unwrap());
            spawner.spawn(blink(led).unwrap());
        })
    })
    .unwrap_or_else(|e| panic!("core 1: {e}"));

    spawn_core2(p.CORE2, unsafe { &mut *core::ptr::addr_of_mut!(CORE2_STACK) }, || {
        let mut executor = Executor::new();
        let executor = unsafe { pi5_executor::_export::make_static(&mut executor) };
        executor.run(|spawner| spawner.spawn(worker(2).unwrap()))
    })
    .unwrap_or_else(|e| panic!("core 2: {e}"));

    spawn_core3(p.CORE3, unsafe { &mut *core::ptr::addr_of_mut!(CORE3_STACK) }, || {
        let mut executor = Executor::new();
        let executor = unsafe { pi5_executor::_export::make_static(&mut executor) };
        executor.run(|spawner| spawner.spawn(worker(3).unwrap()))
    })
    .unwrap_or_else(|e| panic!("core 3: {e}"));

    spawner.spawn(printer(uart).unwrap());
}

/// Runs on cores 1-3 (pool_size = 3: one instance per core).
#[pi5_executor::task(pool_size = 3)]
async fn worker(id: usize) {
    let mut count = 0;
    loop {
        // Sanity check: are we really on the core we think?
        let core = cpu::core_id();
        MESSAGES.send(Msg { core: if core == id { core } else { 100 + core }, count, sent_at: Instant::now() }).await;
        count += 1;
        Timer::after(Duration::from_millis(1000)).await;
    }
}

#[pi5_executor::task]
async fn blink(mut led: Output) {
    loop {
        led.toggle();
        Timer::after_millis(250).await;
    }
}

/// Runs on core 0.
#[pi5_executor::task]
async fn printer(mut uart: Uart<Async>) {
    loop {
        let m = MESSAGES.receive().await;
        let latency = m.sent_at.elapsed();
        let _ = writeln!(uart, "core {} -> #{} (delivered to core {} after {} us)", m.core, m.count, cpu::core_id(), latency.as_micros());
    }
}

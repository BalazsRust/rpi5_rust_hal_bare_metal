# pi5-hal

A bare-metal Rust HAL for the Raspberry Pi 5 (BCM2712 + RP1), written to be
read as much as to be used. It is shaped like `esp-hal` for the drivers and
like `embassy` for the async runtime, so a program written for an ESP32 or
RP2040 with those crates ports almost line for line.

```rust
#![no_std]
#![no_main]

use pi5_executor::Spawner;
use pi5_hal::gpio::{Level, Output, OutputConfig};
use pi5_hal::uart::{Config, Uart};
use pi5_time::{Duration, Timer};

#[pi5_executor::main]
async fn main(spawner: Spawner) {
    let p = pi5_hal::init();
    let uart = Uart::new(p.UART10, Config::default()).unwrap().into_async();
    let led = Output::new(p.GPIO21, Level::High, OutputConfig::default());
    spawner.spawn(blink_task(led, uart).unwrap());
}

#[pi5_executor::task]
async fn blink_task(mut led: Output, mut uart: Uart<pi5_hal::Async>) {
    uart.write_async(b"TEST\n").await.unwrap();
    loop {
        led.toggle();
        Timer::after(Duration::from_millis(500)).await;
    }
}
```

**Status: compiles, not yet run on hardware.** Every address comes from the
Raspberry Pi Linux tree (branch `rpi-6.12.y`) and is cited at the place it is
used, but nothing here has been verified on a real Pi 5 by the author. The
list of assumptions is at the end of this file. The first thing to run is
`uart_hello`, which prints a line for each assumption it can check.

## The layers

```
  your program         examples/*.rs
  ---------------------------------------------------------------------------
  Layer 2  async       pi5-executor   Spawner, #[main], #[task], one executor per core
                       pi5-time       Duration, Instant, Timer, Ticker, with_timeout
                       pi5-sync       Channel, Signal, Mutex (across tasks and cores)
                       pi5-futures    select, join, yield_now, block_on
  Layer 1  drivers     pi5-hal        gpio, uart, i2c, spi, pwm (Blocking + Async),
                                      rt (startup), mmu, multicore, psci, Peripherals
  Layer 0  registers   pi5-pac        addresses and bit definitions, each with its source
  ---------------------------------------------------------------------------
  hardware             BCM2712: 4x Cortex-A76, PL011 debug UART, generic timer
                          ==PCIe==  RP1: GPIO, UART0, I2C0-3, SPI0-1, PWM0
```

Each layer only talks to the one below it. When a Layer 1 function does
something you want to understand, its documentation names the `pi5_pac`
constant it uses, and that constant names the kernel source line it came
from. Suggested reading order, bottom up:

1. `crates/pi5-pac/src/mmio.rs` - why `read_volatile`/`write_volatile` (20 lines).
2. `crates/pi5-pac/src/rp1_gpio.rs` - the three GPIO register blocks.
3. `src/gpio.rs` - `Output::new` is four register writes; read why each one.
4. `src/rt.rs` - the assembly between the firmware's jump and `main`.
5. `src/mmu.rs` - a page table in one 128-entry array.
6. `crates/pi5-executor/src/raw/mod.rs` - what a task physically is.
7. `crates/pi5-futures/src/select.rs` - async in 20 lines.

## Building

Toolchain: Rust 1.85+ (stable or nightly), target `aarch64-unknown-none`,
and an `objcopy`.

```sh
rustup target add aarch64-unknown-none
sudo apt install llvm            # for llvm-objcopy; or: cargo install cargo-binutils

cd pi5_hal
cargo build --release --example blink
llvm-objcopy -O binary target/aarch64-unknown-none/release/examples/blink out/kernel8.img

# or, all examples at once (out/<name>.img, out/kernel8.img = the first one):
./build.sh
./build.sh multicore
```

`.cargo/config.toml` sets the target and passes the linker script, so no
`cargo rustc -- -C link-arg=...` is needed. (If you prefer the explicit form:
`cargo rustc --release --example blink -- -C link-arg=-Tlink.ld`.)

Copy `out/kernel8.img` to the boot partition and put this in `config.txt`:

```ini
kernel=kernel8.img      # our image (the firmware looks for kernel_2712.img first)
arm_64bit=1             # Pi 5 is 64-bit only; harmless, makes the intent explicit
pciex4_reset=0          # do NOT reset RP1 before starting the kernel; without this
                        # the GPIO/UART0/I2C/SPI/PWM registers are unreachable
enable_uart=1           # console UART. The 3-pin debug port (UART10) is driven by
                        # this HAL directly; the line is here for the header UART
uart_2ndstage=1         # optional: bootloader log on the debug port, proves the
                        # serial adapter is wired right before our code runs
```

Serial: a 3.3 V USB-serial adapter on the JST-SH "UART" connector between
the micro-HDMI ports (or the Raspberry Pi Debug Probe), 115200 8N1.

## Examples

| example        | what it shows                                                      | wiring                              |
|----------------|--------------------------------------------------------------------|-------------------------------------|
| `blink`        | blocking `Output`, `Delay`                                         | LED on GPIO21 (pin 40)              |
| `button`       | `Input` with pull-up, embedded-hal traits                          | button GPIO17 (pin 11) to GND, LED  |
| `uart_hello`   | debug UART, prints EL/MMU/clock/ID checks, echoes                  | serial adapter                      |
| `async_blink`  | `#[main]`, two tasks, `Timer`                                      | LED, serial                         |
| `uart_sender`  | the ESP32 `uart_sender` program ported 1:1 (RP1 UART0 on GPIO14/15)| serial on pins 8/10 at 9600, LED    |
| `async_tasks`  | `Channel`, `Signal`, `Ticker`, `select`, `with_timeout`, GPIO edges| button, LED, serial                 |
| `multicore`    | all 4 cores, one executor each, messages through a `Channel`       | LED, serial                         |
| `i2c_scan`     | `I2c` on GPIO2/3, like `i2cdetect`                                 | any I2C device on pins 3/5          |
| `spi_loopback` | `Spi` on SPI0, hardware CS                                         | jumper GPIO10 (pin 19) to GPIO9 (21)|
| `pwm_fade`     | `Pwm` channel 0 breathing an LED                                   | LED on GPIO12 (pin 32)              |

## API cheat sheet

```rust
let p = pi5_hal::init();                      // or Peripherals::take()

// GPIO
let mut led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());
led.set_high(); led.toggle(); led.is_set_high();
let mut btn = Input::new(p.GPIO17, InputConfig::default().with_pull(Pull::Up));
btn.is_low(); btn.wait_for_falling_edge().await;

// UART (debug port: fixed pins; RP1 UART0: pick pins)
let mut dbg = Uart::new(p.UART10, Config::default())?;
let mut hdr = Uart::new(p.UART0, Config::default().with_baudrate(9600))?.with_tx(p.GPIO14).with_rx(p.GPIO15);
writeln!(dbg, "x = {x}")?;  dbg.write(b"..")?;  dbg.read(&mut buf)?;  dbg.read_byte()?;
let mut a = dbg.into_async();  a.write_async(b"..").await?;  a.read_async(&mut buf).await?;

// I2C
let mut i2c = I2c::new(p.I2C1, Config::default().with_frequency(Rate::from_khz(400)))?.with_sda(p.GPIO2).with_scl(p.GPIO3);
i2c.write(0x68, &[0x6B, 0])?;  i2c.write_read(0x68, &[0x75], &mut id)?;  i2c.into_async().write_read_async(..).await?;

// SPI
let mut spi = Spi::new(p.SPI0, Config::default().with_frequency(Rate::from_mhz(1)))?.with_sck(p.GPIO11).with_mosi(p.GPIO10).with_miso(p.GPIO9).with_cs(p.GPIO8);
spi.transfer_in_place(&mut buf)?;  spi.write(&tx)?;  spi.transfer(&mut rx, &tx)?;

// PWM
let mut pwm = Pwm::new(p.PWM0);
let mut ch = pwm.channel(p.GPIO12, ChannelConfig::default().with_frequency(Rate::from_khz(1)))?;
ch.set_duty_cycle_percent(30)?;   // embedded_hal::pwm::SetDutyCycle

// async runtime (embassy names)
#[pi5_executor::main] async fn main(spawner: Spawner) { spawner.spawn(task(args).unwrap()); }
#[pi5_executor::task(pool_size = 2)] async fn task(..) { Timer::after_millis(10).await; }
Ticker::every(Duration::from_hz(50)).next().await;   with_timeout(Duration::from_secs(1), fut).await;
select(a, b).await;  join(a, b).await;  yield_now().await;
static CH: Channel<CriticalSectionRawMutex, Msg, 8> = Channel::new();  CH.send(m).await;  CH.receive().await;
static SIG: Signal<CriticalSectionRawMutex, ()> = Signal::new();       SIG.signal(());   SIG.wait().await;

// more cores
static mut STACK1: Stack<65536> = Stack::new();
spawn_core1(p.CORE1, unsafe { &mut *addr_of_mut!(STACK1) }, || {
    let mut ex = Executor::new();
    let ex = unsafe { pi5_executor::_export::make_static(&mut ex) };
    ex.run(|spawner| spawner.spawn(task_on_core1().unwrap()))
})?;
```

Name mapping from the crates you may know:

| embassy / esp-hal                         | here                                   |
|-------------------------------------------|----------------------------------------|
| `embassy_executor::{Spawner, main, task}` | `pi5_executor::{Spawner, main, task}`  |
| `embassy_time::{Duration, Instant, Timer, Ticker}` | `pi5_time::{...}`             |
| `embassy_sync::channel::Channel`          | `pi5_sync::channel::Channel`           |
| `embassy_futures::select::select`         | `pi5_futures::select::select`          |
| `esp_hal::init(...)`                      | `pi5_hal::init()`                      |
| `esp_hal::gpio::{Output, Input, Level, Pull, OutputConfig, InputConfig}` | `pi5_hal::gpio::{...}` |
| `esp_hal::uart::Uart::new(..).with_tx(..).with_rx(..).into_async()` | same, `pi5_hal::uart` |
| `esp_hal::{Blocking, Async}`              | `pi5_hal::{Blocking, Async}`           |
| `embassy_rp::multicore::{spawn_core1, Stack}` | `pi5_hal::multicore::{spawn_core1, spawn_core2, spawn_core3, Stack}` |

Differences worth knowing: there are no interrupts in this HAL, so async
drivers *poll* (they yield to other tasks between register checks instead
of sleeping until an IRQ); `Timer` wake-ups have ~40 us granularity (the
executor sleeps in `wfe` and the CPU's timer event stream wakes it); task
storage for `#[task]` comes from a 32 KiB arena (`pi5_executor::raw::TASK_ARENA_SIZE`).

## Using it from your own crate

```toml
[dependencies]
pi5-hal      = { path = "../pi5_hal" }
pi5-executor = { path = "../pi5_hal/crates/pi5-executor" }
pi5-time     = { path = "../pi5_hal/crates/pi5-time" }
pi5-sync     = { path = "../pi5_hal/crates/pi5-sync" }
pi5-futures  = { path = "../pi5_hal/crates/pi5-futures" }
```

Copy `.cargo/config.toml` into your crate (target + `-Tlink.ld`; the script
is found through pi5-hal's build script) and add `panic = "abort"` to your
profiles. Your `main.rs` needs `#![no_std]`, `#![no_main]` and either
`#[pi5_executor::main]` or `pi5_hal::entry!(main)`.

## What happens before `main`

`src/rt.rs`, 70 instructions of assembly, in order: save the device-tree
pointer; park any core that is not core 0; require EL2; un-trap FP/SIMD;
install the exception vectors; zero the virtual-counter offset; enable the
timer event stream; set the stack pointer; zero `.bss`; build the page table
(`src/mmu.rs`); enable MMU + caches; call `main`.

Why the MMU: with it off, AArch64 treats all memory as Device memory, which
is uncached (slow) and on which the exclusive instructions behind atomics
and spinlocks are not guaranteed to work. The page table is an identity map
with three kinds of entry (RAM cacheable, BCM2712 peripherals Device, RP1
window Device); everything else is unmapped, so a wild pointer produces a
printed translation fault instead of silent corruption.

If anything faults, the exception handler prints the vector, `ESR` (decoded),
`ELR` (the faulting instruction address, look it up with
`llvm-objdump -d target/.../examples/<name> | grep -B5 <addr>`) and `FAR`.
A "synchronous external abort" at an address in `0x1F_...` means RP1 did not
answer: check `pciex4_reset=0`.

## Unverified assumptions (what the first hardware run must confirm)

Each item says where it comes from. `uart_hello` checks the ones marked (U).

1. **RP1 at CPU `0x1F_0000_0000`.** Chained from `pcie2` `ranges`
   (`bcm2712.dtsi:552`) and `rp1` `ranges` (`bcm2712-rpi-5-b.dts:141-143`).
   Linux programs that PCIe window itself; we assume the firmware leaves the
   same one, and that `pciex4_reset=0` keeps RP1 alive. (U: `clk_sys` line)
2. **Debug UART at `0x10_7D00_1000`, clock 9.216 MHz.** `bcm2712.dtsi:191,
   241-249, 28-31`. (U: the peripheral ID must read `0x00341011`, and the
   banner must be readable at 115200.)
3. **Entry state**: EL2, MMU off, core 0 only, `x0` = DTB, interrupts masked
   (Linux arm64 boot protocol). (U: EL and core are printed.)
4. **`CNTFRQ_EL0` = 54 MHz** set by firmware (`bcm2712.dtsi:14-19`). We read
   it; only the comment assumes the value. (U: printed.)
5. **MPIDR core numbering in Aff1** (`bcm2712.dtsi: cpu1 reg = <0x100>`).
6. **PSCI `CPU_ON` via SMC** starts cores 1-3 at our entry point in EL2
   (`bcm2712.dtsi:165-168`, PSCI spec). The exact behaviour of the Pi 5
   firmware's monitor (which EL, state of `CPTR_EL2`, caches) is assumed to
   match the spec and what Linux relies on. (U: PSCI version is printed.)
7. **Page table / cache maintenance sequence** (`src/mmu.rs`): standard
   ARMv8 procedure; a mistake here hangs before anything prints. If
   `uart_hello` prints nothing at all, try the first experiment below.
8. **RP1 register maps** for GPIO, I2C (DesignWare), SPI (DesignWare APB
   SSI), PWM, clocks: from the Linux drivers named in `crates/pi5-pac`.
   Pin-function (FUNCSEL) table: `pinctrl-rp1.c:481-509`.
9. **RP1 clocks at boot** are undocumented. `clk_sys` is *measured* with
   RP1's frequency counter (falls back to 200 MHz); the PWM and UART0 clocks
   are switched to the 50 MHz crystal by us, so their rate is known if the
   clock block works as `clk-rp1.c` describes.
10. **Timer event stream** (`CNTHCTL_EL2`/`CNTKCTL_EL1.EVNTEN`) wakes `wfe`;
    without it, the executor's idle loop would only wake on `sev` from
    another core and timers would stall. ARMv8 requires the feature.
11. **DesignWare SPI frame-size field and FIFO depth** are probed at init
    the way `spi-dw-core.c` does; **I2C FIFO depths** come from
    `COMP_PARAM_1`. Both will fail loudly (`ConfigError`) if the block does
    not answer.
12. **Not implemented at all**: interrupts, DMA, USB, Ethernet, display,
    SD card, CAN (the Pi 5 has no CAN controller), RP1's PIO, GPIO banks 1-2.

Experiments if the first run prints nothing: (a) build `uart_hello` with
the MMU-enable call commented out in `src/rt.rs` (`bl __pi5_rt_enable_mmu`)
to separate "MMU setup wrong" from "UART wrong" - single-core blocking
examples work without the MMU, the async ones do not; (b) use
`Uart::new_firmware_configured(p.UART10)` instead of `Uart::new` to rule
out the clock assumption; (c) toggle GPIO21 before touching anything else.

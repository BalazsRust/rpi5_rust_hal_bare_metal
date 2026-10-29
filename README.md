# pi5-hal

A bare-metal Rust HAL for the **Raspberry Pi 5**: no Linux, your program is
the only thing running on the Pi. It feels like programming an ESP32 with
`esp-hal` + `embassy`: the same names, the same `#[main]`/`#[task]` async
style, `info!()` logging, and `cargo run --release` builds your program and
sends it to the Pi over a serial cable, through an ESP32 that acts as a
USB-serial bridge.

```rust
#![no_std]
#![no_main]

use pi5_executor::Spawner;
use pi5_hal::gpio::{Level, Output, OutputConfig};
use pi5_hal::info;
use pi5_time::{Duration, Ticker, Timer};

#[pi5_executor::main]
async fn main(spawner: Spawner) {
    let p = pi5_hal::init();
    let led = Output::new(p.GPIO21, Level::Low, OutputConfig::default());
    info!("hello from the Pi 5");
    spawner.spawn(blink(led).unwrap());
    spawner.spawn(heartbeat().unwrap());
}

#[pi5_executor::task]
async fn blink(mut led: Output) {
    let mut ticker = Ticker::every(Duration::from_millis(250));
    loop {
        led.toggle();
        ticker.next().await;
    }
}

#[pi5_executor::task]
async fn heartbeat() {
    loop {
        info!("still alive");
        Timer::after_millis(1000).await;
    }
}
```

> **Made with AI.** Most of this HAL (drivers, runtime, tools, and this
> documentation) was written by an AI assistant, guided, tested on real
> hardware and corrected by a human. Register addresses come from the
> Raspberry Pi Linux sources and the RP1 datasheet and are cited in the
> code, but read the code critically and test before you rely on it.

## Contents

- [What works](#what-works)
- [Quick start (for everyone)](#quick-start-for-everyone)
- [Making a new project: pi5-generate](#making-a-new-project-pi5-generate)
- [Sending programs over UART with the ESP32](#sending-programs-over-uart-with-the-esp32)
- [Examples](#examples)
- [CPU speed](#cpu-speed)
- [The layers](#the-layers) and [API cheat sheet](#api-cheat-sheet) for programmers
- [Building by hand](#building-by-hand), [What happens before `main`](#what-happens-before-main), [Not verified yet](#not-verified-yet)

## What works

Tested on a real Raspberry Pi 5:

- GPIO output (LED), the RP1 UART0 on the header, `info!()` logging
- the UART chainloader (send programs from the PC, no SD card or stick
  swapping) and automatic restart of the Pi on every `cargo run`
- the async runtime: tasks, `Timer`, `Ticker`
- measuring the CPU clock (`cpu_freq`: 1500 MHz, see [CPU speed](#cpu-speed))

Written and compiling, but not yet tried on hardware: I2C, SPI, PWM and
MCPWM, the ADC (temperature), GPIO inputs/edges, multicore, `pi5-sync`
channels. If one of them misbehaves, the driver's source cites the
register documentation it follows.

Not implemented: interrupts (drivers poll instead), DMA, USB, Ethernet,
display, SD card, RP1's PIO, GPIO banks 1-2. The Pi 5 has no CAN
controller, and its 4 ADC inputs are not wired to the 40-pin header.

## Quick start (for everyone)

You need:

- a Raspberry Pi 5 and its power supply
- a USB stick (or SD card) for the Pi to boot from
- an ESP32 dev board with a USB port (the classic ESP32, e.g. ESP32-DevKitC)
- 3 jumper wires, and an LED + 330 ohm resistor to see something blink
- a Linux PC (Ubuntu/Debian; other systems work with small changes)

### 1. Install the tools on the PC (once)

```sh
# Rust
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
rustup target add aarch64-unknown-none

# objcopy (turns the compiled program into a plain image), Python serial library
sudo apt install llvm python3-serial

# allow your user to open /dev/ttyUSB0 (log out and in afterwards)
sudo usermod -aG dialout $USER

# the project generator, as a command
mkdir -p ~/.local/bin
ln -s "$PWD/tools/pi5-generate" ~/.local/bin/pi5-generate   # run inside pi5_hal/
```

### 2. Put the bridge program on the ESP32 (once)

The ESP32 passes bytes between the PC (USB) and the Pi (UART), and can
restart the Pi when the PC asks. Its program is the separate `esp32rpi5`
project (esp-hal, Rust). With the ESP32 Rust toolchain installed
(`espup`, `espflash`, see the esp-rs book):

```sh
cd esp32rpi5
cargo run --release        # flashes the ESP32 with espflash
```

### 3. Wire the ESP32 to the Pi

| Raspberry Pi 5                     | ESP32  |
|------------------------------------|--------|
| physical pin 8 (GPIO14, Pi TX)     | GPIO25 (ESP32 RX) |
| physical pin 10 (GPIO15, Pi RX)    | GPIO26 (ESP32 TX) |
| physical pin 6 (GND)               | GND    |

Both boards use 3.3 V signals, so they connect directly. Do **not** connect
5 V to the ESP32's pins. The LED for the examples: physical pin 40
(GPIO21) -> 330 ohm -> LED -> GND (physical pin 39).

"Physical pin N" is the position on the 40-pin header (pin 1 has a square
solder pad; odd pins are in the row nearer the board's middle); "GPIO N" is
the chip's name for that pin. They are different numbers, so always check
which one a diagram means.

### 4. Prepare the boot stick (once)

Format the stick as FAT32. Copy the Raspberry Pi firmware files onto it:
the easiest way is to write Raspberry Pi OS (Lite) with Raspberry Pi
Imager and keep its boot partition. Then build the chainloader and copy it
over:

```sh
cd pi5_hal
./build.sh chainloader             # makes out/chainloader.img
cp out/chainloader.img /media/$USER/<your stick>/
```

and replace the stick's `config.txt` with:

```ini
kernel=chainloader.img
kernel_address=0x2000000
os_check=0
pciex4_reset=0
```

- `kernel=` / `kernel_address=`: start our chainloader instead of Linux,
  high in memory, out of the way of your programs (they run at 0x200000).
- `os_check=0`: the firmware does not insist on a Linux kernel.
- `pciex4_reset=0`: keep the RP1 chip (GPIO, UART, PWM ...) running when
  the firmware hands over; without it no pin works.

Put the stick in the Pi. From now on you never touch it again: every new
program goes over the serial cable.

### 5. Make your first project and run it

```sh
pi5-generate                       # asks for a name and a template
cd my_robot
cargo run --release
```

Plug in the ESP32 by USB and power the Pi. The first time, if nothing
happens, power-cycle the Pi; after that `cargo run` restarts it by itself.
You see:

```
[pi5load] restarting the Pi...
[chainloader] got 26304 bytes, crc ok, starting at 0x200000
[pi5load] Pi accepted the image. Terminal: what you type goes to the Pi (Ctrl-C to quit)
0.004211 INFO  my_robot started
```

Edit `src/main.rs`, run `cargo run --release` again, done.

## Making a new project: pi5-generate

`pi5-generate` works like `esp-generate`: it creates a ready-to-run
project folder next to wherever you are.

```sh
pi5-generate                       # asks for the name and the template
pi5-generate my_robot              # asks only for the template
pi5-generate my_robot -t async     # no questions
```

Templates:

| template | what you get |
|----------|--------------|
| `blink`  | simplest program: blinks the LED on physical pin 40 (GPIO21), prints with `info!` |
| `uart`   | LED + serial: prints hello, echoes what you type in the terminal, LED toggles per key |
| `async`  | embassy-style tasks: one blinks with a `Ticker`, one prints every second with `Timer` |

What it creates:

```
my_robot/
  Cargo.toml            depends on pi5-hal (path to this repository)
  .cargo/config.toml    target, linker settings, and the runner: tools/pi5run.py
  src/main.rs           your program
  README.md, .gitignore
```

`cargo run --release` builds the program, then the runner (`pi5run.py`)
turns it into an image and hands it to `pi5load.py`, which restarts the
Pi, sends the image, and stays open as a terminal. Ctrl-C quits.

Different serial port? `PI5_PORT=/dev/ttyACM0 cargo run --release`.

Don't create projects *inside* the `pi5_hal` folder (the generator refuses);
put them next to it.

## Sending programs over UART with the ESP32

How a program gets to the Pi:

```
 PC  --USB-->  ESP32 (bridge)  --UART 115200-->  Pi 5: chainloader  -->  your program
```

1. `pi5load.py` sends a restart request; the ESP32 holds the Pi's RX wire
   LOW for 0.3 s; the reset watch on the Pi's core 3 sees that and
   restarts the Pi.
2. The Pi boots the chainloader from the stick, which says "waiting for
   image".
3. `pi5load.py` sends the image with a CRC; the chainloader checks it,
   copies it to 0x200000 and jumps there.
4. `pi5load.py` stays open as a terminal: your `info!` lines come out,
   your keystrokes go in.

Send any of the built-in examples:

```sh
cd pi5_hal
./build.sh                         # builds all examples into out/load/
python3 tools/pi5load.py blink     # restart the Pi, send, open terminal
python3 tools/pi5load.py timer_ticker
python3 tools/pi5load.py cpu_freq --port /dev/ttyUSB1
```

Options of `pi5load.py`:

| option | meaning |
|--------|---------|
| `--port /dev/ttyX` | serial port (default `/dev/ttyUSB0`, or `$PI5_PORT`) |
| `--no-reset`       | don't restart the Pi first (it is already waiting) |
| `--wait N`         | seconds to wait for the chainloader (default 60) |
| `--no-wait`        | send at once, don't wait for the ready line |
| `--exact`          | send this exact file (used by `cargo run`) |

Everything is also written to `out/pi5load.log` (with timestamps), handy
when the terminal scrolled away.

Troubleshooting:

- **"cannot open /dev/ttyUSB0"**: ESP32 not plugged in, wrong port, or your
  user is not in the `dialout` group.
- **Waits forever for the chainloader**: power-cycle the Pi once by hand
  (or try `--no-wait` if it already printed "waiting for image").
  Check the wiring (TX to RX, RX to TX, GND), and that the stick has
  `chainloader.img` and the `config.txt` above.
- **No restart**: the reset watch runs on core 3. A program started with
  `init_without_reset_watch()` (or one that crashed the whole Pi) needs a
  manual power-cycle.
- **Garbled text**: both sides must be at 115200 baud.

## Examples

All in `examples/`; `./build.sh` builds them, `python3 tools/pi5load.py <name>` runs one.

| example        | what it shows                                                      | wiring                              |
|----------------|--------------------------------------------------------------------|-------------------------------------|
| `blink`        | blocking `Output`, `Delay`                                         | LED on physical pin 40 (GPIO21)     |
| `button`       | `Input` with pull-up, embedded-hal traits                          | button physical pin 11 (GPIO17) to GND, LED |
| `log_demo`     | `info!`, `warn!`, `error!`, levels                                 | ESP32 bridge                        |
| `timer_ticker` | `Timer::after_millis` and `Ticker` side by side                    | ESP32 bridge                        |
| `cpu_freq`     | measures the CPU clock (cycle counter + add chain)                 | ESP32 bridge                        |
| `adc_temp`     | RP1 chip temperature with the ADC                                  | ESP32 bridge                        |
| `async_blink`  | `#[main]`, two tasks, `Timer`                                      | LED                                 |
| `async_tasks`  | `Channel`, `Signal`, `Ticker`, `select`, `with_timeout`, GPIO edges| button, LED                         |
| `multicore`    | all 4 cores, one executor each, messages through a `Channel`       | LED                                 |
| `uart_sender`  | an ESP32 `uart_sender` program ported 1:1 (UART0 on GPIO14/15)     | ESP32 bridge, LED                   |
| `i2c_scan`     | `I2c` on GPIO2/3, like `i2cdetect`                                 | I2C device on physical pins 3/5     |
| `spi_loopback` | `Spi` on SPI0, hardware CS                                         | jumper physical pin 19 (GPIO10) to physical pin 21 (GPIO9) |
| `pwm_fade`     | `Pwm` channel 0 breathing an LED                                   | LED on physical pin 32 (GPIO12)     |
| `mcpwm_demo`   | `McPwm` complementary pair with dead time + one single output      | LEDs on physical pins 32, 33, 12 (GPIO12, 13, 18) |
| `uart_hello`   | debug UART (3-pin connector), boot checks, echo                    | Raspberry Pi Debug Probe            |
| `chainloader`  | the loader on the stick (`./build.sh chainloader`)                 | -                                   |

## CPU speed

Measured with `cpu_freq`: the Pi 5 runs at **1500 MHz** here (an ESP32
runs at up to 240 MHz). The Cortex-A76 is rated for 2400 MHz, but the
firmware starts it at 1.5 GHz and only Linux would raise it. To run at full
speed, add this to `config.txt` on the stick (not tested yet; the CPU gets
warmer, so use a heatsink or fan):

```ini
force_turbo=1
```

Then run `cpu_freq` again to check.

Speed in practice: per clock tick an A76 core does about 3-4 instructions
where an ESP32 does about 1, and there are 4 cores. But every GPIO, UART
or PWM register sits on the RP1 chip behind a PCIe link, so each access
takes around 1 us: toggling a pin in a tight loop is *slower* than on an
ESP32, while computing is much faster.

## The layers

```
  your program         examples/*.rs
  ---------------------------------------------------------------------------
  Layer 2  async       pi5-executor   Spawner, #[main], #[task], one executor per core
                       pi5-time       Duration, Instant, Timer, Ticker, with_timeout
                       pi5-sync       Channel, Signal, Mutex (across tasks and cores)
                       pi5-futures    select, join, yield_now, block_on
  Layer 1  drivers     pi5-hal        gpio, uart, i2c, spi, pwm, mcpwm (Blocking + Async),
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

## API cheat sheet

```rust
let p = pi5_hal::init();                      // or Peripherals::take()

// Logging (over the ESP32 bridge, shows up in the `cargo run` terminal)
info!("x = {}", x);  warn!("..");  error!("..");  debug!("..");  println!("plain");
pi5_hal::log::set_max_level(pi5_hal::log::Level::Info);   // hide debug!/trace!

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

// Motor-control PWM (ESP32 MCPWM style): shared timer, centre-aligned,
// complementary pair with 500 ns dead time
let mut mcpwm = McPwm::new(p.PWM0, TimerConfig::new(Rate::from_khz(20)))?;
let mut bridge = mcpwm.complementary(p.GPIO12, p.GPIO13, 500)?;
mcpwm.start();
bridge.set_duty_percent(40);

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

`Timer` vs `Ticker` (same as embassy): `Timer::after_millis(500)` waits
500 ms *from now*, so if the loop body takes time, that adds up;
`Ticker::every(..)` keeps a fixed beat. Logging does not count as loop
time under the executor: `info!` copies the line into a 4 KB buffer and
returns, and the executor sends it while idle, so both give 0, 500, 1000 ...
ms. (Blocking programs without the executor print the old, waiting way.)

## Building by hand

`pi5-generate` projects need none of this; it is for working on the HAL.

```sh
cd pi5_hal
./build.sh                  # all examples: out/<name>.img (boot directly), out/load/<name>.img (chainloader)
./build.sh blink            # one example
./build.sh chainloader      # the loader for the stick
cargo build --release --example blink   # just compile
```

`.cargo/config.toml` sets the target and passes the linker script. Programs
for the chainloader are linked at 0x200000 (`build.sh` passes
`--defsym=__load_address=0x200000`; note: it must come *before* `-Tlink.ld`).

Booting a program directly, without the chainloader: copy
`out/<name>.img` to the stick as `kernel8.img` and use

```ini
kernel=kernel8.img
os_check=0
pciex4_reset=0
```

## Using it from your own crate (without pi5-generate)

```toml
[dependencies]
pi5-hal      = { path = "../pi5_hal" }
pi5-executor = { path = "../pi5_hal/crates/pi5-executor" }
pi5-time     = { path = "../pi5_hal/crates/pi5-time" }
pi5-sync     = { path = "../pi5_hal/crates/pi5-sync" }
pi5-futures  = { path = "../pi5_hal/crates/pi5-futures" }
```

Copy `.cargo/config.toml` from a generated project (it has the target, the
linker arguments and the runner) and add `panic = "abort"` to your
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

## Not verified yet

Confirmed on hardware: RP1 at CPU address `0x1F_0000_0000` with
`pciex4_reset=0`; RP1 UART0 and GPIO; entry at EL2 with `x0` = DTB; the
54 MHz generic timer and the `wfe` timer event stream; MMU/cache setup;
PSCI `CPU_ON` (the reset watch runs on core 3); the PM watchdog reset; CPU
clock 1500 MHz.

Still assumptions (from the Linux sources named in `crates/pi5-pac`):

1. **RP1 register maps** for I2C (DesignWare), SPI (DesignWare APB SSI),
   PWM (also the RP1 datasheet, section 3.4), ADC, clocks. The MCPWM dead
   time relies on the PWM counter behaving as the datasheet describes in
   phase-correct mode: check it with a scope or logic analyser.
2. **RP1 clocks at boot** are undocumented. `clk_sys` is measured with
   RP1's frequency counter; the PWM, ADC and UART0 clocks are switched to
   the 50 MHz crystal by the HAL.
3. **Debug UART (UART10) at `0x10_7D00_1000`, 9.216 MHz** (`bcm2712.dtsi`).
4. **Running all 4 cores at once** with user code (core 3 also runs the
   reset watch: use `init_without_reset_watch()` if you need it).

If a program crashes, the exception handler prints the vector, `ESR`
(decoded), `ELR` (look the address up with




`llvm-objdump -d target/.../<name> | grep -B5 <addr>`) and `FAR` on UART0,
so it shows up in the `cargo run` terminal. A "synchronous external abort"
at an address in `0x1F_...` means RP1 did not answer: check
`pciex4_reset=0`.
### showcase
https://github.com/user-attachments/assets/3be7b5d3-3c1f-48b4-b04d-bad2b6b18d2e

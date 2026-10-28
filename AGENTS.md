# AGENTS.md

Guide for AI coding agents working on this repository. Read `README.md`
for the user-facing picture; this file is what you need to change code
without breaking things that were hard to get working.

## What this is

A bare-metal Rust HAL for the Raspberry Pi 5 (BCM2712 SoC + RP1 I/O chip),
no OS. Drivers are shaped like `esp-hal`, the async runtime like `embassy`
(own implementation: do **not** swap in the real embassy-executor /
embassy-time crates; the owner decided against that). Programs are sent to
the Pi over UART through an ESP32 bridge and a chainloader.

## Layout

```
src/                     pi5-hal (Layer 1): drivers + runtime
  rt.rs mmu.rs           startup assembly, page table, exception vectors
  gpio.rs uart.rs i2c.rs spi.rs pwm.rs mcpwm.rs adc.rs clocks.rs
  log.rs                 info!/warn!/... + buffered output, drained by the executor
  reset.rs               reset watch on core 3 + system_reset (watchdog, then PSCI)
  multicore.rs psci.rs   CPU_ON, per-core stacks
  critical_section_impl.rs
crates/pi5-pac           Layer 0: register addresses/bits, each citing its source
crates/pi5-executor      Layer 2: executor, #[main]/#[task] (macros in pi5-executor-macros)
crates/pi5-time          Duration, Instant, Timer, Ticker, with_timeout
crates/pi5-sync          Channel, Signal, Mutex
crates/pi5-futures       select, join, yield_now, block_on
examples/                one program each; chainloader.rs is the boot-stick loader
tools/pi5load.py         uploader + terminal (PC side of the chainloader protocol)
tools/pi5run.py          cargo runner for generated projects (objcopy + pi5load)
tools/pi5-generate       project generator (templates live inside the script)
build.sh                 builds examples into out/<name>.img and out/load/<name>.img
```

The ESP32 bridge firmware is a separate project (`esp32rpi5`, esp-hal), not
in this repository.

## Build and check

```sh
./build.sh                      # all examples; must finish with no warnings
./build.sh <example>            # one example
./build.sh chainloader          # linked at 0x2000000, for the boot stick
cargo clippy --release -p pi5-hal
```

Target `aarch64-unknown-none`, set by `.cargo/config.toml`. Nothing runs on
the host: there are no unit tests. "Done" means it builds warning-free and,
for behaviour changes, the owner ran it on the Pi. Never claim hardware
behaviour you have not seen in a log.

New example: add the file, a `[[example]]` entry in `Cargo.toml`
(`required-features = ["rt", "panic-handler"]`), and its name in the
`EXAMPLES=(...)` list in `build.sh`. If you change a template in
`tools/pi5-generate`, generate a project outside this repo and
`cargo build --release` it.

## Running on hardware

`python3 tools/pi5load.py <example>` restarts the Pi, uploads
`out/load/<example>.img` and stays open as a terminal. Everything is
appended to `out/pi5load.log` (runs separated by `=====` headers, RX
chunks timestamped). When the owner says "read the log", read the end of
that file instead of asking for a paste.

## Hard-won facts (do not undo)

- **RP1 address window**: CPU address = `0x1F_0000_0000 + (rp1_addr - 0x4000_0000)`.
  RP1 blocks have atomic aliases: +0x2000 SET, +0x3000 CLR.
  `pciex4_reset=0` in config.txt is required, or RP1 does not answer.
- **Memory map with the chainloader**: the stick boots `chainloader.img`
  at `0x2000000` (`kernel_address=0x2000000`). The firmware keeps memory at
  `0x80000-0x8f000` and around `0xa3000` LIVE in that mode: never write
  there. Loaded programs are linked and run at `0x200000`; the staging
  buffer is `0x10000000`.
- **Linker**: `--defsym=__load_address=...` must come BEFORE `-Tlink.ld`
  on the linker command line, or lld ignores it and links at 0x80000.
- **Upload protocol**: PC sends `PI5LOAD!`, Pi answers `GO`, PC sends u32
  size + image + CRC32, Pi answers `OK` or `ERR ...`. The HAL turns `\n`
  into `\r\n`, so the PC matches `GO\r` / `OK\r`. Sending is paced
  (256-byte chunks) because the bridge is store-and-forward at 115200.
- **Restart**: pi5load sends `\x00PI5-RESTART-NOW\x00`; the ESP32 holds the
  Pi's RX line (GPIO15) LOW for 300 ms; the reset watch on core 3 resets
  the Pi when it sees LOW for 150 ms..1.5 s followed by HIGH (PM watchdog,
  then PSCI SYSTEM_RESET). Do not use a serial BREAK: it jams the ESP32
  board's CP2102 clone. That chip also replays old bytes for ~20 ms after
  the port opens, so pi5load drains for 0.2 s first.
- **The chainloader leaves its reset watch running on core 3** when it
  jumps (stopping and restarting core 3 via PSCI left it dead). A loaded
  program's `init()` gets `ALREADY_ON` and treats the watch as inherited.
  Programs that need core 3 or GPIO15 call `init_without_reset_watch()`.
- **GPIO14/15 (physical pins 8/10)** are UART0 to the ESP32 and the reset
  watch input. Never reuse them in examples or drivers by default.
- **Logging**: `info!` & co. go to RP1 UART0 at 115200. Under the executor
  output is buffered (4 KB ring in `src/log.rs`) and drained by the
  executor's idle hook `__pi5_executor_idle` (defined in `log.rs`, called
  in `crates/pi5-executor/src/thread.rs` before `wfe`); without the
  executor logging blocks. The panic handler flushes the ring first.
- **Timer vs Ticker**: same semantics as embassy. `Timer::after` measures
  from now (drifts by loop work time); `Ticker` keeps a fixed schedule.
- **EL2**: programs run at EL2. The PMU cycle counter needs
  `PMCCFILTR_EL0.NSH` (bit 27) or it reads 0. CPU clock is 1500 MHz
  (firmware default; `force_turbo=1` would give 2400, untested).
- **No interrupts** (GIC not set up): async drivers poll; the executor
  sleeps in `wfe`, woken by the timer event stream (~40 us) or `sev`.
- **RP1 PWM** (datasheet RP-008370-DS, section 3.4): CHANx_CTRL.MODE is
  bits 2:0; mode 2 is phase-correct (period = 2 x RANGE); BIND shares
  COMMON_RANGE *and* COMMON_DUTY. Only PWM0 reaches the header (GPIO12, 13,
  18, 19). MCPWM dead time is emulated (inverted low side with DUTY + dead
  time), so it needs up-down counting.
- **ADC**: the Pi 5 does not bring RP1's ADC inputs to the header; only the
  temperature sensor is useful.

## Conventions

- Name pins as "physical pin N (GPIO M)" in docs, comments and messages:
  header position and GPIO number are different and easy to mix up.
- Every register address or bit in `pi5-pac` cites where it came from
  (Linux file:line or datasheet table). Keep doing that.
- `#![no_std]`, no heap. `unsafe` blocks get a `// SAFETY:` comment.
- Missing docs are warnings: document public items.
- Match the surrounding style: short doc comments that explain *why*,
  plain English, no unnecessary abstractions.
- Keep the README's "What works" list honest: something moves to "tested"
  only after it ran on a real Pi.

#!/usr/bin/env bash
# Build one or all examples and turn them into flat kernel images.
#
#   ./build.sh            # all examples -> out/<name>.img, out/kernel8.img = blink
#   ./build.sh multicore  # one example  -> out/multicore.img and out/kernel8.img
#
# Every example is also built a second time, linked at 0x200000, into
# out/load/<name>.img: that copy is for the UART chainloader
# (tools/pi5load.py). See examples/chainloader.rs for why not 0x80000.
#
# Copy out/kernel8.img to the boot partition next to a config.txt containing
# the lines listed in README.md.
set -euo pipefail
cd "$(dirname "$0")"

OBJCOPY=$(command -v rust-objcopy || command -v llvm-objcopy || command -v aarch64-linux-gnu-objcopy || true)
OBJDUMP=$(command -v rust-objdump || command -v llvm-objdump || command -v aarch64-linux-gnu-objdump || true)
if [ -z "$OBJCOPY" ]; then
    echo "need rust-objcopy (cargo install cargo-binutils), llvm-objcopy or aarch64-linux-gnu-objcopy" >&2
    exit 1
fi

if [ $# -ge 1 ]; then
    EXAMPLES=("$@")
    DEFAULT="$1"
else
    EXAMPLES=(blink button uart_hello log_demo pin_toggle soft_uart_diag uart_diag async_blink uart_sender async_tasks multicore i2c_scan spi_loopback pwm_fade)
    DEFAULT=blink
fi

mkdir -p out
for ex in "${EXAMPLES[@]}"; do
    if [ "$ex" = chainloader ]; then
        # The chainloader is linked at 0x2000000 (config.txt needs
        # kernel_address=0x2000000) so 0x80000 is free for the programs it
        # receives. `cargo rustc` passes the extra flag to this binary only.
        cargo rustc --release --example chainloader -- -C link-arg=--defsym=__load_address=0x2000000
    else
        cargo build --release --example "$ex"
    fi
    elf="target/aarch64-unknown-none/release/examples/$ex"
    "$OBJCOPY" -O binary "$elf" "out/$ex.img"
    printf '%-14s %8d bytes\n' "$ex" "$(stat -c %s "out/$ex.img")"
    if [ "$ex" != chainloader ]; then
        mkdir -p out/load
        cargo rustc --release --example "$ex" -- -C link-arg=--defsym=__load_address=0x200000
        "$OBJCOPY" -O binary "$elf" "out/load/$ex.img"
    fi
done
# Leave the ELF of the default example linked at 0x80000 for the check below.
if [ "$DEFAULT" != chainloader ]; then
    cargo build --release --example "$DEFAULT"
fi
cp "out/$DEFAULT.img" out/kernel8.img
echo "out/kernel8.img = $DEFAULT"

if [ -n "$OBJDUMP" ] && [ "$DEFAULT" != chainloader ]; then
    echo
    echo "first instructions of _start (must be at 0x80000):"
    "$OBJDUMP" -d --start-address=0x80000 --stop-address=0x80020 "target/aarch64-unknown-none/release/examples/$DEFAULT" | sed -n '/<_start>/,$p' | head -12
fi

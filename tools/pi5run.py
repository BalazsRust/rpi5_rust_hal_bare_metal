#!/usr/bin/env python3
"""Cargo runner for Raspberry Pi 5 projects made with pi5-generate.

`cargo run --release` builds the ELF and then calls
    python3 pi5run.py target/aarch64-unknown-none/release/<name>
This script turns the ELF into a flat image (<elf>.img, like build.sh does)
and sends it to the UART chainloader with pi5load.py, which then stays open
as a terminal to the running program.

The project is linked at 0x200000 (see its .cargo/config.toml), the address
where the chainloader runs programs. Serial port: /dev/ttyUSB0, or set
PI5_PORT=/dev/ttyACM0 (for example) in the environment.
"""
import os
import shutil
import subprocess
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def find_objcopy():
    for tool in ("rust-objcopy", "llvm-objcopy", "aarch64-linux-gnu-objcopy"):
        path = shutil.which(tool)
        if path:
            return path
    sys.exit("[pi5run] need llvm-objcopy (apt install llvm) or rust-objcopy (cargo install cargo-binutils)")


def main():
    if len(sys.argv) < 2:
        sys.exit("usage: pi5run.py <elf> [pi5load options...]")
    elf = sys.argv[1]
    img = elf + ".img"
    subprocess.run([find_objcopy(), "-O", "binary", elf, img], check=True)
    print(f"[pi5run] {img}: {os.path.getsize(img)} bytes", flush=True)
    print("[pi5run] power the Pi OFF and ON now (the chainloader waits after boot)", flush=True)
    load = os.path.join(HERE, "pi5load.py")
    os.execvp(sys.executable, [sys.executable, load, img, "--exact", *sys.argv[2:]])


if __name__ == "__main__":
    main()

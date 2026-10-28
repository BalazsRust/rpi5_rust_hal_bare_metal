#!/usr/bin/env python3
"""Send a kernel image to the pi5_hal chainloader over serial, then show
what the new program prints.

    python3 tools/pi5load.py blink
    python3 tools/pi5load.py uart_hello --port /dev/ttyUSB0
    python3 tools/pi5load.py out/load/blink.img

Any form of the name works (blink, out/blink.img, ...): it always sends
out/load/<name>.img, the copy build.sh links at 0x200000, where the
chainloader runs programs. The normal out/<name>.img builds are linked at
0x80000 (for booting straight from the stick) and would crash.

Steps: (re)start the Pi with the chainloader on the boot stick, then run
this. It waits for the chainloader's "Waiting for image" line (so you can
also start this first and power the Pi on afterwards), sends
"PI5LOAD!" + size + image + CRC-32, waits for "OK", and then prints
then works as a two-way terminal (keys -> Pi, Pi -> screen) until Ctrl-C.
"""
import argparse
import os
import select
import struct
import sys
import termios
import time
import tty
import zlib

import serial  # pyserial

DEFAULT_PORT = os.environ.get("PI5_PORT", "/dev/ttyUSB0")

MAGIC = b"PI5LOAD!"
CHUNK = 64          # bytes per write
BAUD = 115200


def wait_for(port, needle, timeout):
    """Read (and echo) until `needle` shows up. Returns True if found."""
    buf = b""
    end = time.time() + timeout
    while time.time() < end:
        data = port.read(256)
        if data:
            sys.stdout.write(data.decode("utf-8", "replace"))
            sys.stdout.flush()
            buf = (buf + data)[-1024:]
            if needle in buf:
                return True
    return False


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("image", help="example name (e.g. blink) or an out/load/<name>.img path")
    ap.add_argument("--port", default=DEFAULT_PORT, help="serial port (default /dev/ttyUSB0, or $PI5_PORT)")
    ap.add_argument("--wait", type=float, default=60, help="seconds to wait for the chainloader (default 60)")
    ap.add_argument("--no-wait", action="store_true", help="send immediately, don't wait for the ready line")
    ap.add_argument("--exact", action="store_true", help="send the given file as it is (it must be linked at 0x200000); used by pi5run.py")
    args = ap.parse_args()

    # Whatever is given (blink, out/blink, out/blink.img, out/load/blink.img),
    # send out/load/<name>.img: the build linked at 0x200000, where the
    # chainloader runs programs. The plain out/<name>.img (0x80000) would crash.
    if not args.exact:
        name = os.path.basename(args.image).removesuffix(".img")
        root = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
        args.image = os.path.relpath(os.path.join(root, "out", "load", name + ".img"))
        if not os.path.exists(args.image):
            print(f"[pi5load] {args.image} not found; build it first: ./build.sh {name}")
            return 1
    image = open(args.image, "rb").read()
    crc = zlib.crc32(image) & 0xFFFFFFFF
    print(f"[pi5load] {args.image}: {len(image)} bytes, crc32 {crc:#010x}")

    try:
        port = serial.Serial(args.port, BAUD, timeout=0.1)
    except serial.SerialException as e:
        print(f"[pi5load] can't open {args.port}: {e}")
        print("          Is the ESP32 plugged in? Is a monitor (cargo run / espflash / tio) still using the port? Close it.")
        return 1

    if not args.no_wait:
        print(f"[pi5load] waiting up to {args.wait:.0f} s for the chainloader (power-cycle the Pi now)...")
        if not wait_for(port, b"aiting for image", args.wait):
            print("\n[pi5load] no chainloader seen; try --no-wait if it is already waiting")
            return 1

    # The ESP32 bridge forwards PC->Pi at the same 115200 baud it receives,
    # with a 128-byte hardware FIFO in between. Sending in small chunks with
    # a short pause keeps that FIFO from overflowing.
    byte_time = 10 / BAUD
    # Handshake: send the magic until the chainloader answers "GO".
    for attempt in range(1, 6):
        port.reset_input_buffer()
        port.write(MAGIC)
        port.flush()
        print(f"[pi5load] magic sent (try {attempt}), waiting for GO...")
        if wait_for(port, b"GO\r", 1.5):
            break
    else:
        print("\n[pi5load] chainloader never answered GO: PC->Pi bytes are not arriving (check wire Pi physical pin 10 (GPIO15) <- ESP32 GPIO26, and that the ESP32 bridge runs)")
        return 1
    payload = struct.pack("<I", len(image)) + image + struct.pack("<I", crc)
    start = time.time()
    for i in range(0, len(payload), CHUNK):
        chunk = payload[i:i + CHUNK]
        port.write(chunk)
        port.flush()
        time.sleep(len(chunk) * byte_time * 1.25)
        if i % (CHUNK * 64) == 0:
            pct = 100 * i // len(payload)
            sys.stdout.write(f"\r[pi5load] sending... {pct:3d}%")
            sys.stdout.flush()
    print(f"\r[pi5load] sent {len(payload)} bytes in {time.time() - start:.1f} s")

    if not wait_for(port, b"OK\r", 8):
        # Keep listening: a late OK/ERR or a crash report still tells us
        # what happened.
        print("\n[pi5load] no OK after 8 s; listening 20 s more for anything...")
        t0 = time.time()
        while time.time() - t0 < 20:
            data = port.read(256)
            if data:
                sys.stdout.write(f"\n[+{time.time() - t0:4.1f}s] " + data.decode("utf-8", "replace"))
                sys.stdout.flush()
        print("\n[pi5load] no OK from the chainloader")
        return 1
    print("[pi5load] Pi accepted the image. Terminal: what you type goes to the Pi (Ctrl-C to quit)")
    terminal(port)
    return 0


def terminal(port):
    """Two-way terminal like tio: Pi output -> screen, keys -> Pi.

    The keyboard is put in raw mode, so every key is sent at once (no need
    to press Enter) and Enter goes out as '\\r', like a serial terminal.
    Ctrl-C quits.
    """
    if not sys.stdin.isatty():
        # Not a real terminal (piped): just show the Pi's output.
        while True:
            data = port.read(256)
            if data:
                sys.stdout.write(data.decode("utf-8", "replace"))
                sys.stdout.flush()
    fd = sys.stdin.fileno()
    saved = termios.tcgetattr(fd)
    try:
        tty.setraw(fd)
        while True:
            ready, _, _ = select.select([fd, port], [], [], 0.1)
            if port in ready:
                data = port.read(port.in_waiting or 1)
                if data:
                    os.write(sys.stdout.fileno(), data)
            if fd in ready:
                keys = os.read(fd, 64)
                if b"\x03" in keys:  # Ctrl-C
                    break
                port.write(keys)
    finally:
        termios.tcsetattr(fd, termios.TCSADRAIN, saved)
        print("\n[pi5load] stopped")

if __name__ == "__main__":
    try:
        sys.exit(main())
    except KeyboardInterrupt:
        print("\n[pi5load] stopped")
        sys.exit(130)

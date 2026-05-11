#!/usr/bin/env python3
"""Cross-platform UART capture for the silicon-anchor protocol.

Reads lines from a serial port until either a sentinel line
(default '=== END ===') appears, the byte budget is exhausted, or
the wall-clock timeout fires. Writes the raw stream to stdout (or
to a file with --out).

Designed to be invoked by capture.sh — keep this script's
dependencies minimal: stdlib + pyserial.

Usage:
  capture.py --port /dev/cu.usbmodem11403 --baud 115200 \\
             --sentinel '=== END ===' --timeout 1800 \\
             --out output.csv
"""
from __future__ import annotations

import argparse
import sys
import time

try:
    import serial  # type: ignore
except ImportError:
    sys.stderr.write(
        "ERROR: pyserial not installed. Run: pip3 install pyserial\n")
    sys.exit(2)


def main() -> int:
    p = argparse.ArgumentParser()
    p.add_argument("--port", required=True,
                   help="serial device path (e.g. /dev/cu.usbmodem11403)")
    p.add_argument("--baud", type=int, default=115200,
                   help="baud rate (default 115200)")
    p.add_argument("--sentinel", default="=== END ===",
                   help="line marking end-of-capture")
    p.add_argument("--timeout", type=int, default=1800,
                   help="wall-clock timeout in seconds (default 1800)")
    p.add_argument("--out", default="-",
                   help="output path or '-' for stdout (default '-')")
    p.add_argument("--max-bytes", type=int, default=64 * 1024 * 1024,
                   help="byte-budget ceiling (default 64 MiB)")
    args = p.parse_args()

    out = sys.stdout if args.out == "-" else open(args.out, "w")
    deadline = time.monotonic() + args.timeout
    bytes_written = 0
    sentinel_seen = False

    try:
        # serial timeout = 1s so we wake periodically to check the
        # wall-clock budget even if the firmware is silent.
        ser = serial.Serial(args.port, args.baud, timeout=1)
    except serial.SerialException as e:
        sys.stderr.write(f"ERROR opening {args.port}: {e}\n")
        return 3

    try:
        while time.monotonic() < deadline and bytes_written < args.max_bytes:
            line_bytes = ser.readline()
            if not line_bytes:
                continue  # serial timeout, loop back to check deadline
            try:
                line = line_bytes.decode("utf-8", errors="replace")
            except Exception:
                line = line_bytes.decode("latin-1", errors="replace")
            out.write(line)
            out.flush()
            bytes_written += len(line_bytes)
            if line.rstrip("\r\n") == args.sentinel:
                sentinel_seen = True
                break
    finally:
        ser.close()
        if out is not sys.stdout:
            out.close()

    if not sentinel_seen:
        sys.stderr.write(
            f"WARN: sentinel '{args.sentinel}' not seen "
            f"(timeout={args.timeout}s, bytes={bytes_written})\n")
        return 1
    sys.stderr.write(
        f"OK: sentinel seen at {bytes_written} bytes\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

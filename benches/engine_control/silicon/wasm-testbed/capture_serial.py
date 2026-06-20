#!/usr/bin/env python3
"""Reliable G474RE serial capture. cat/stty misses boot output (0 bytes ~50% of
runs); pyserial captures every time. Start this FIRST, then `west flash`.
Port: lex-sort-first /dev/cu.usbmodem* is the ST-LINK VCP (do not hardcode)."""
import glob, serial, sys
port = sorted(glob.glob("/dev/cu.usbmodem*"))[0]
s = serial.Serial(port, 115200, timeout=float(sys.argv[1]) if len(sys.argv) > 1 else 14)
d = s.read(8000)
sys.stdout.write(d.decode(errors="replace") if d else f"0 bytes from {port}\n")

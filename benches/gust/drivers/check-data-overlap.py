#!/usr/bin/env python3
"""Fail if a fused core module's data segments overlap.

Found the hard way (gale#266): `meld fuse --memory shared` merges memories but
does NOT rebase addresses, and every wit-bindgen component places `.rodata` at
the wasm-ld default base 1048576. Fusing three of them stacks all three on the
same address. meld exits 0 and prints "Fusion complete!"; nothing downstream
notices, because the dissolve gate checks undefined SYMBOLS and object SIZES and
nothing looks at where data lands.

Data segments are applied in order at instantiation, so a later segment silently
overwrites an earlier one. On the isolation core that put a 320-byte mutable
RegionTable on top of another component's `.data`.

An overlap is never intentional, so this is a hard error regardless of flags.

Usage:  check-data-overlap.py                 sweep every composed artifact
        check-data-overlap.py <module.wasm>   check one module
Exit:   0 = disjoint (or ledgered);  5 = overlapping;  1 = could not inspect

The sweep (gale#439) used to be one hardcoded fixture while the gate's name
claimed all fused data was disjoint; there were five, and one overlaps. The list
below MUST match COMPOSED in tools/xtask/src/gates/graph_env.rs -- the CI
agreement step compares the two implementations' verdicts.
"""
import pathlib
import re
import subprocess
import sys

BENCH = pathlib.Path(__file__).resolve().parent.parent  # benches/gust
COMPOSED = [
    "drivers/measurements/wcet-fixture/gustos.loom.wasm",
    "drivers/measurements/wcet-fixture/gustos.loom.named.wasm",
    "drivers/os-node/repro-757/loom.wasm",
    "wasm-kernel/fused.wasm",
    "wasm-kernel/gust_kernel.wasm",
]
# Deliberate overlaps, each with its reason. Fails in BOTH directions.
KNOWN_OVERLAP = {}

# (data (;N;) (memory M)? (i32.const OFF) "payload")  -- memory index is elided for 0
SEG = re.compile(
    r'\(data \(;(\d+);\)(?: \(memory (\d+)\))? \(i32\.const (\d+)\)\s*"((?:[^"\\]|\\.)*)"'
)


def payload_len(s: str) -> int:
    """Byte length of a WAT data payload: \\XX is one byte, \\\\ is one byte."""
    return len(re.sub(r'\\[0-9a-fA-F]{2}', '.', s).replace('\\\\', '.'))


def main(path: str) -> int:
    try:
        wat = subprocess.run(
            ['wasm-tools', 'print', path],
            capture_output=True, text=True, check=True,
        ).stdout
    except (subprocess.CalledProcessError, FileNotFoundError) as e:
        print(f"  could not disassemble {path}: {e}", file=sys.stderr)
        return 1

    segs = []
    for m in SEG.finditer(wat):
        idx, mem, off = int(m.group(1)), int(m.group(2) or 0), int(m.group(3))
        n = payload_len(m.group(4))
        segs.append((idx, mem, off, off + n, n))

    if not segs:
        print("  no data segments — nothing to check")
        return 0

    for i, mem, lo, hi, n in segs:
        print(f"  data {i}: mem {mem}  [{lo} .. {hi})  len={n}")

    overlaps = [
        (a, b)
        for a in segs for b in segs
        if a[0] < b[0] and a[1] == b[1] and a[2] < b[3] and b[2] < a[3]
    ]
    if not overlaps:
        print(f"  ok: {len(segs)} data segments, all disjoint")
        return 0

    print(f"\n  FAIL: {len(overlaps)} overlapping data-segment pair(s) — later segments")
    print("        overwrite earlier ones at instantiation, so component state collides.")
    for a, b in overlaps:
        print(f"    data {a[0]} [{a[2]}..{a[3]})  vs  data {b[0]} [{b[2]}..{b[3]})")
    print("\n  Cause: --memory shared merges memories without rebasing addresses, and")
    print("  every component's .rodata starts at the wasm-ld default base (1048576).")
    print("  Fix: build with `-C link-arg=--emit-relocs` retained on the FINAL link")
    print("  (`wasm-ld -r` is NOT sufficient — its stored values are addends, not final")
    print("  addresses), then fuse with --address-rebase. See gale#266, meld#370.")
    return 5


def sweep() -> int:
    bad = []
    for x in KNOWN_OVERLAP:
        if x not in COMPOSED:
            bad.append(f"{x}: on KNOWN_OVERLAP but not a swept composed artifact")
    for rel in COMPOSED:
        print(f"== {rel}")
        rc = main(str(BENCH / rel))
        if rc == 1:
            return 1
        if rc == 0 and rel in KNOWN_OVERLAP:
            bad.append(f"{rel}: on KNOWN_OVERLAP but now disjoint -- remove it from the ledger")
        elif rc == 5 and rel in KNOWN_OVERLAP:
            print(f"  KNOWN-OVERLAP: {KNOWN_OVERLAP[rel]}")
        elif rc == 5:
            bad.append(f"{rel}: overlapping data segments, not on the ledger")
    if bad:
        print(f"\nFAIL: {len(bad)} problem(s):")
        for b in bad:
            print(f"  {b}")
        return 5
    print(f"\nswept all {len(COMPOSED)} composed artifact(s); {len(KNOWN_OVERLAP)} on the ledger")
    return 0


if __name__ == '__main__':
    if len(sys.argv) == 1:
        sys.exit(sweep())
    if len(sys.argv) != 2:
        print(__doc__)
        sys.exit(1)
    sys.exit(main(sys.argv[1]))

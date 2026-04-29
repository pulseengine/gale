#!/usr/bin/env python3
"""Tag a raw QEMU/Renode UART capture with a run-id + variant prefix
so analyze.py can pool across runs while still tracking per-run drops.

Mirrors engine_control/tag_events.py shape, plus the new
`telemetry_emits,N` line emitted by the macro bench's footer.

Usage:
  tag_events.py <raw_path> <run_id> <variant>

Writes to stdout."""

from __future__ import annotations

import sys


def main(argv: list[str]) -> int:
    if len(argv) != 4:
        print("usage: tag_events.py <raw> <run_id> <variant>",
              file=sys.stderr)
        return 2
    raw_path, run_id, variant = argv[1], argv[2], argv[3]
    try:
        with open(raw_path, "rt", errors="replace") as f:
            for line in f:
                line = line.rstrip("\r\n")
                if not line:
                    continue
                if line.startswith("E,"):
                    print(f"R{run_id},{variant},{line}")
                elif (line.startswith("drops,")
                      or line.startswith("samples,")
                      or line.startswith("telemetry_emits,")):
                    print(f"M,R{run_id},{variant},{line}")
                elif line == "=== END ===":
                    print(f"M,R{run_id},{variant},END")
                elif (line.startswith("cycles_per_sec,")
                      or line.startswith("target_samples,")
                      or line.startswith("build,")):
                    print(f"M,R{run_id},{variant},{line}")
                elif line.startswith("#"):
                    print(f"# R{run_id} {variant}: {line[1:].strip()}")
    except FileNotFoundError:
        print(f"# tag_events: missing {raw_path}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))

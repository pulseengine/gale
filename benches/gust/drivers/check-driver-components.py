#!/usr/bin/env python3
"""VER-DRV-COMPONENT-001 — the gate REQ-DRV-COMPONENT-001 needs to stay true.

Two checks, per driver and whole-graph:

  1. NO RAW `env` IMPORT. Every thin-seam driver's capability dependencies must
     arrive as WIT-typed `gust:hal/*` imports, not as undefined symbols nobody
     declared. A raw `env` import is a dependency the component model cannot see.
  2. IT ACTUALLY COMPONENTIZES. `wasm-tools component new` + `validate` must
     succeed, and the component's WIT must be introspectable.

The rule is "no raw `env`", NOT "imports gust:hal". Three drivers legitimately
import something else or nothing at all: `hm-thin` imports NOTHING (pure scalar
predicates, zero seams), `mpu-thin` imports `gust:mpu/regs`, `switch-thin`
imports `gust:switch/ctx`. An earlier draft of this gate required `gust:hal`
specifically and failed all three — a gate that is wrong about what it is
checking is worse than no gate.

Why this is not a shell script: the verdict is the product here, and shell's
defaults lose verdicts quietly (a `grep -q` under `set -o pipefail` can fail on
SIGPIPE; `cmd | head` masks the exit status of `cmd`). This mirrors the existing
check-data-overlap.py / check-wcet-evt.py gates rather than inventing a third
convention.

Exit codes — distinct so a CI log says WHICH property failed:
  0  all drivers clean
  2  usage / tooling error (wasm-tools missing, no drivers found)
  3  a driver has a raw `env` import
  4  a driver fails to componentize or validate
  5  a component's WIT cannot be read (not introspectable)
  6  the NEGATIVE CONTROL passed when it must fail  <- the gate is not biting

Run `--self-test` to exercise (6): it synthesizes a module with a raw `env`
import and asserts this checker rejects it. A gate that has never been shown to
fail is not a gate.
"""
from __future__ import annotations
import argparse, pathlib, re, subprocess, sys, tempfile

IMPORT_RE = re.compile(r'\(import\s+"([^"]+)"\s+"([^"]+)"')


def run(cmd: list[str], **kw) -> subprocess.CompletedProcess:
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def need_wasm_tools() -> None:
    if run(["wasm-tools", "--version"]).returncode != 0:
        sys.exit("FATAL: wasm-tools not on PATH (exit 2)")


def imports_of(wasm: pathlib.Path) -> list[tuple[str, str]]:
    p = run(["wasm-tools", "print", str(wasm)])
    if p.returncode != 0:
        sys.exit(f"FATAL: wasm-tools print failed for {wasm}: {p.stderr.strip()[:200]} (exit 2)")
    return IMPORT_RE.findall(p.stdout)


def check_module(wasm: pathlib.Path, name: str, quiet: bool = False) -> int:
    """Return an exit code for one driver module. 0 = clean."""
    imps = imports_of(wasm)
    env = sorted({f for m, f in imps if m == "env"})
    hal = sorted({m for m, _ in imps if m.startswith("gust:hal/")})
    if env:
        if not quiet:
            print(f"  FAIL {name}: {len(env)} raw env import(s): {', '.join(env)}")
        return 3

    with tempfile.TemporaryDirectory() as td:
        comp = pathlib.Path(td) / "c.wasm"
        p = run(["wasm-tools", "component", "new", str(wasm), "-o", str(comp)])
        if p.returncode != 0:
            if not quiet:
                print(f"  FAIL {name}: component new failed: {p.stderr.strip()[:160]}")
            return 4
        if run(["wasm-tools", "validate", str(comp)]).returncode != 0:
            if not quiet:
                print(f"  FAIL {name}: component does not validate")
            return 4
        w = run(["wasm-tools", "component", "wit", str(comp)])
        if w.returncode != 0:
            if not quiet:
                print(f"  FAIL {name}: component WIT not introspectable: {w.stderr.strip()[:120]}")
            return 5
        size = comp.stat().st_size

    if not quiet:
        typed = sorted({m for m, _ in imps})
        print(f"  ok   {name}: 0 env, imports {', '.join(typed) or '(none)'}, component {size} B")
    return 0


def self_test() -> int:
    """NEGATIVE CONTROL: a module with a raw env import MUST be rejected with 3."""
    wat = '(module (import "env" "mmio_write32" (func (param i32 i32))))'
    with tempfile.TemporaryDirectory() as td:
        src = pathlib.Path(td) / "bad.wat"
        bad = pathlib.Path(td) / "bad.wasm"
        src.write_text(wat)
        if run(["wasm-tools", "parse", str(src), "-o", str(bad)]).returncode != 0:
            sys.exit("FATAL: could not build the negative control (exit 2)")
        rc = check_module(bad, "<negative-control>", quiet=True)
    if rc == 3:
        print("  ok   negative control REJECTED with exit 3 — the gate bites")
        return 0
    print(f"  FAIL negative control returned {rc}, expected 3 — THE GATE DOES NOT BITE")
    return 6


def main() -> int:
    ap = argparse.ArgumentParser(description="VER-DRV-COMPONENT-001 gate")
    ap.add_argument("--drivers-dir", default=str(pathlib.Path(__file__).parent))
    ap.add_argument("--self-test", action="store_true",
                    help="run only the negative control")
    ap.add_argument("--no-build", action="store_true",
                    help="use existing target/ artefacts instead of rebuilding. UNSAFE for a "
                         "gate: a stale wasm makes this report on code that is not checked out.")
    args = ap.parse_args()

    need_wasm_tools()
    if args.self_test:
        return self_test()

    root = pathlib.Path(args.drivers_dir)
    drivers = sorted(d for d in root.glob("*-thin") if d.is_dir())
    if not drivers:
        sys.exit(f"FATAL: no *-thin drivers under {root} (exit 2)")

    print(f"VER-DRV-COMPONENT-001 — {len(drivers)} thin-seam driver(s)")
    worst, checked, missing = 0, 0, []
    for d in drivers:
        if not args.no_build:
            b = run(["cargo", "build", "--release", "--target", "wasm32-unknown-unknown", "-q"],
                    cwd=str(d))
            if b.returncode != 0:
                print(f"  FAIL {d.name}: cargo build failed: {b.stderr.strip()[:160]}")
                worst = worst or 2
                continue
        built = sorted((d / "target/wasm32-unknown-unknown/release").glob("*.wasm"))
        if not built:
            missing.append(d.name)
            continue
        rc = check_module(built[0], d.name)
        checked += 1
        worst = worst or rc

    if missing:
        # Not a pass: an unbuilt driver is an UNCHECKED driver, and silently
        # skipping it is how a gate reports green over an untested surface.
        print(f"  FAIL not built, therefore unchecked: {', '.join(missing)}")
        print("       build with: cargo build --release --target wasm32-unknown-unknown")
        worst = worst or 2

    print(f"\n  checked {checked} of {len(drivers)}")
    if worst == 0:
        rc = self_test()
        if rc != 0:
            return rc
        print("\nVER-DRV-COMPONENT-001: PASS")
    else:
        print(f"\nVER-DRV-COMPONENT-001: FAIL (exit {worst})")
    return worst


if __name__ == "__main__":
    sys.exit(main())

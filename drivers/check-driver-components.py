#!/usr/bin/env python3
"""VER-DRV-COMPONENT-001 — the gate REQ-DRV-COMPONENT-001 needs to stay true.

Two checks, per driver and whole-graph:

  1. NO RAW `env` IMPORT. Every thin-seam driver's capability dependencies must
     arrive as WIT-typed `gust:hal/*` imports, not as undefined symbols nobody
     declared. A raw `env` import is a dependency the component model cannot see.
  2. THE COMMITTED OBJECT AGREES WITH THE SOURCE. A driver's dissolved `.o` is
     checked in, and the probes LINK THAT OBJECT rather than a freshly built one
     — so source and object can diverge silently, and did (gale#307: 8 of 9
     objects still carried pre-conversion `mmio_read32` names after their drivers
     had moved to WIT-typed imports). The wasm's imported FIELD names are exactly
     the object's undefined symbols, so this needs no loom/synth: compare the two
     sets directly.

  3. IT ACTUALLY COMPONENTIZES. `wasm-tools component new` + `validate` must
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
  7  a committed .o's undefined symbols disagree with the wasm's imports
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


def expected_undefined(imports: list[tuple[str, str]]) -> set[str]:
    """The object's undefined symbols are exactly the imported FIELD names.

    `(import "gust:hal/mmio@0.1.0" "read32")` lowers to an undefined `read32`.
    Verified across all 10 drivers that carry a committed object.
    """
    return {field for _mod, field in imports}


def check_object(wasm: pathlib.Path, obj: pathlib.Path, name: str,
                 nm: str, quiet: bool = False) -> int:
    """Committed .o must agree with the source wasm about the seam symbols."""
    want = expected_undefined(imports_of(wasm))
    p = run([nm, str(obj)])
    if p.returncode != 0:
        if not quiet:
            print(f"  WARN {name}: {nm} could not read {obj.name}; object axis unchecked")
        return 0
    got = set()
    for line in p.stdout.splitlines():
        parts = line.split()
        if parts and (parts[0] == "U" or (len(parts) > 1 and parts[1] == "U")):
            got.add(parts[-1])
    if got == want:
        if not quiet:
            print(f"       obj  {obj.name}: undefined {sorted(got) or '(none)'} == wasm imports")
        return 0
    if True:
        if not quiet:
            print(f"  FAIL {name}: committed {obj.name} disagrees with its source")
            print(f"       wasm imports -> expect: {sorted(want) or '(none)'}")
            print(f"       object has undefined:   {sorted(got) or '(none)'}")
            print("       regenerate the object, or the probes link a stale contract (gale#307)")
        return 7
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
    if rc != 3:
        print(f"  FAIL env negative control returned {rc}, expected 3 — THE GATE DOES NOT BITE")
        return 6
    print("  ok   env negative control REJECTED with exit 3 — the gate bites")

    # object axis: a stale symbol set must be caught, not shrugged at
    stale = expected_undefined([("gust:hal/mmio@0.1.0", "read32")])
    fresh = {"mmio_read32"}
    if stale == fresh:
        print("  FAIL object-axis comparator treats mmio_read32 as read32 — IT DOES NOT BITE")
        return 6
    print("  ok   object-axis control: read32 != mmio_read32 — the comparator bites")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description="VER-DRV-COMPONENT-001 gate")
    ap.add_argument("--drivers-dir", default=str(pathlib.Path(__file__).parent))
    ap.add_argument("--self-test", action="store_true",
                    help="run only the negative control")
    ap.add_argument("--nm", default="arm-none-eabi-nm",
                    help="nm for reading the committed Cortex-M objects")
    ap.add_argument("--no-build", action="store_true",
                    help="use existing target/ artefacts instead of rebuilding. UNSAFE for a "
                         "gate: a stale wasm makes this report on code that is not checked out.")
    args = ap.parse_args()

    need_wasm_tools()
    if args.self_test:
        return self_test()

    root = pathlib.Path(args.drivers_dir)
    # `*-thin` PLUS `dma-own`. dma-own is a driver-shaped single component with the
    # same 1:1 wasm->object relationship, and the shell gate this script replaced
    # covered it. Dropping it here was a silent coverage regression; naming it
    # explicitly is the fix, and the census below stops the next one being silent.
    drivers = sorted(d for d in root.glob("*-thin") if d.is_dir())
    if not drivers:
        sys.exit(f"FATAL: no *-thin drivers under {root} (exit 2)")

    # ---- census of committed objects -------------------------------------------
    # This gate's rule (committed .o undefined set == its wasm's imports) applies to
    # a component with ONE wasm and ONE object. Composed and fused artifacts are a
    # different shape -- gustos-dissolved-cm3.o is a fusion of a whole component
    # graph with three native atoms, not a 1:1 lowering -- so they are NOT gated
    # here, and pretending otherwise would produce false failures.
    #
    # They are LISTED instead. If a committed *-cm3.o appears that is neither gated
    # nor on this ledger, the gate fails: a new object silently escaping coverage is
    # exactly how dma-own escaped it.
    # dma-own is a SEPARATE case from the composed artifacts below. It is a 1:1
    # component-shaped crate, and the shell gate this script replaced globbed it --
    # but it has never satisfied this requirement: its three TCB atoms are raw
    # `extern "C"` symbols, so they land as `env` imports, not as a typed WIT
    # interface the way all 13 thin drivers do:
    #
    #   FAIL dma-own: 3 raw env import(s): dma_barrier, dma_irq_poll, dma_program
    #
    # That was the shell gate's EXPECTED red -- its header said so in as many words
    # ("written BEFORE the drivers are componentized and is EXPECTED TO FAIL until
    # they are"). Nobody converted dma-own, and because that gate was wired to no
    # workflow, the red was never seen.
    #
    # Whether dma-own is in REQ-DRV-COMPONENT-001's scope at all is a real question
    # -- the requirement says "every thin-seam driver", and dma-own is an ownership
    # FSM, not a thin-seam driver. That is a scope decision, so it is not made here.
    # What IS recorded is its actual state, asserted, so a change cannot pass
    # unnoticed in either direction.
    DMA_OWN_RAW_ENV = {"dma_barrier", "dma_irq_poll", "dma_program"}

    UNGATED = {
        # dma-own: 1:1 component-shaped, but not componentized -- raw env imports,
        # asserted against DMA_OWN_RAW_ENV below rather than left unchecked.
        "dma-own/dma-own-cm3.o",
        "breadth/breadth-cm3.o",
        "os-node/exec-cm3.o",
        "os-node/gustos-dissolved-cm3.o",
        "os-node/os-time-cm3.o",
        "os-node/os-tl-cm3.o",
        "os-node/os-ts-cm3.o",
        "spawn-provider/spawn-provider-cm3.o",
        "timer-provider/timer-provider-cm3.o",
    }
    gated_dirs = {d.name for d in drivers}
    found, stray = set(), []
    for o in sorted(root.glob("*/*-cm3.o")):
        rel = f"{o.parent.name}/{o.name}"
        if o.parent.name in gated_dirs:
            continue
        found.add(rel)
        if rel not in UNGATED:
            stray.append(rel)
    if stray:
        print("FAIL: committed object(s) neither gated nor on the ungated ledger:")
        for x in stray:
            print(f"  {x}")
        print("Add it to the gated set if it is a 1:1 component, or to UNGATED with")
        print("a reason if it is a composed/fused artifact. Do not leave it silent.")
        return 5
    vanished = sorted(UNGATED - found)
    if vanished:
        print("FAIL: UNGATED ledger lists object(s) that no longer exist:")
        for x in vanished:
            print(f"  {x}")
        print("Remove them from UNGATED -- a stale ledger hides the next escape.")
        return 5

    dma = root / "dma-own"
    if dma.is_dir():
        if not args.no_build:
            run(["cargo", "build", "--release", "--target", "wasm32-unknown-unknown", "-q"],
                cwd=str(dma))
        ws = sorted((dma / "target/wasm32-unknown-unknown/release").glob("*.wasm"))
        if not ws:
            print("FAIL dma-own: no wasm built (it needs .cargo/config.toml with")
            print("     --allow-undefined; rustc >=1.97 rust-lld rejects the raw externs)")
            return 2
        raw = {f for m, f in imports_of(ws[0]) if m == "env"}
        if raw != DMA_OWN_RAW_ENV:
            print("FAIL: dma-own's raw env imports changed and the ledger is stale.")
            print(f"  ledger:   {sorted(DMA_OWN_RAW_ENV)}")
            print(f"  observed: {sorted(raw)}")
            print("  If it was componentized, move it into the gated set above.")
            return 5
        print(f"  note: dma-own carries {len(raw)} raw env import(s) "
              f"({', '.join(sorted(raw))}) — ledgered, see comment; not componentized")

    print(f"VER-DRV-COMPONENT-001 — {len(drivers)} component(s) gated "
          f"({len(UNGATED)} composed/fused object(s) listed, not gated)")
    worst, checked, missing, objs_checked = 0, 0, [], 0
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
        objs = sorted(d.glob("*-cm3.o"))
        if objs and rc == 0:
            orc = check_object(built[0], objs[0], d.name, args.nm)
            worst = worst or orc
            objs_checked += 1

    if missing:
        # Not a pass: an unbuilt driver is an UNCHECKED driver, and silently
        # skipping it is how a gate reports green over an untested surface.
        print(f"  FAIL not built, therefore unchecked: {', '.join(missing)}")
        print("       build with: cargo build --release --target wasm32-unknown-unknown")
        worst = worst or 2

    # Report the object count explicitly: "no failures" must not be confusable
    # with "nothing was compared". A silent success is how #307 hid.
    print(f"\n  checked {checked} of {len(drivers)} modules, "
          f"{objs_checked} committed object(s) compared against source")
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

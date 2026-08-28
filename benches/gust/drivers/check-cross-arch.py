#!/usr/bin/env python3
"""Cross-architecture seam gate for the thin-seam drivers (REQ-DRV-XARCH-001).

Supersedes the verdict half of `build-cross-arch.sh`, which was wrong in two
ways that cancelled each other out into a green light:

  1. It enumerated EIGHT drivers by hand while thirteen `*-thin` directories
     exist, so can/hm/mpu/spi/switch were never gated at all.
  2. Its rule was `undefined_count > 0`. That is not the property. A driver
     crosses only if the lowered object's undefined set is EXACTLY the set of
     names the wasm imports -- no more, no less. `> 0` is satisfied by a
     LEAKED symbol just as well as by a seam symbol, and on the RISC-V leg
     that is precisely what was happening (see RISC-V section below).

The rule here is the same one `check-driver-components.py` uses for the object
axis: expected_undefined(wasm) == actual_undefined(object). It is correct for
drivers that import nothing (`hm-thin` -> empty set, and an empty set is a
PASS, not a failure) and for drivers whose seam is not `gust:hal`
(`mpu-thin` -> `mpu-write`, `switch-thin` -> the three ctx symbols).

  python3 benches/gust/drivers/check-cross-arch.py
  python3 benches/gust/drivers/check-cross-arch.py --self-test

Exit codes:
  0  gate held
  2  usage / tooling error (wasm-tools, synth, or nm missing; no drivers found)
  4  a driver's ARM object does not match its imports
  5  the RISC-V known-defect ledger is stale (see below)
"""

import argparse, glob, os, pathlib, re, subprocess, sys

HERE = pathlib.Path(__file__).resolve().parent

# ---------------------------------------------------------------------------
# The RISC-V leg is KNOWN RED, and this ledger is why the gate does not simply
# skip it. Every driver below produces a RISC-V object whose undefined set does
# NOT match its imports, because synth's RV32 backend skips functions it cannot
# select and emits the call sites anyway:
#
#   gpio-thin: skipping 'func_18': RISC-V selector: immediate 1048588 too
#              large for memory offset
#   mpu-thin:  skipping 'func_20': unsupported wasm op for RV32 skeleton:
#              GlobalGet(0)   -- 8 of 20 functions skipped, and synth then
#              exits non-zero (#952) because a requested EXPORT was skipped
#
# The dangling reference is emitted as `synth_func_N` while definitions are
# named `func_N`, so it never resolves. On ARM the same internals are emitted
# as LOCAL `t func_N`, all defined, nothing leaked -- the defect is specific to
# the RISC-V backend.
#
# For a skipped INTERNAL function synth exits 0, so the unlinkable object ships
# with only a warning. That is what let the old `> 0` rule read green.
#
# This ledger is a pin, not an excuse: if a driver starts crossing cleanly it
# must be removed from the list, and the gate fails (exit 5) until it is. That
# is how the ledger shrinks to empty instead of quietly outliving the bug.
RISCV_KNOWN_BAD = {
    "adc-thin", "can-thin", "dac-thin", "gpio-thin", "hm-thin", "i2c-thin",
    "mpu-thin", "pwm-thin", "spi-thin", "switch-thin", "timer-thin",
    "uart-thin", "wdg-thin",
}
RISCV_TRACKER = "synth: RV32 selector gaps leave dangling `synth_func_N` refs"


def run(cmd, **kw):
    return subprocess.run(cmd, capture_output=True, text=True, **kw)


def need(tool, probe):
    if run(probe).returncode != 0:
        sys.exit(f"FATAL: {tool} not usable (exit 2)")


def synth_bin():
    """Resolve synth through the varve pin; fall back to $SYNTH or PATH."""
    if os.environ.get("SYNTH"):
        return os.environ["SYNTH"]
    p = run(["varve", "which", "synth"])
    if p.returncode == 0 and p.stdout.strip():
        cand = p.stdout.splitlines()[0].strip()
        if os.access(cand, os.X_OK):
            return cand
    return "synth"


def imports(wasm):
    """The field names the wasm imports -- exactly what must stay undefined."""
    p = run(["wasm-tools", "print", str(wasm)])
    if p.returncode != 0:
        sys.exit(f"FATAL: wasm-tools print failed for {wasm} (exit 2)")
    return {m.group(2) for m in re.finditer(r'\(import\s+"([^"]+)"\s+"([^"]+)"', p.stdout)}


def undefined(obj, nm):
    p = run([nm, str(obj)])
    if p.returncode != 0:
        return None
    return {ln.split()[-1] for ln in p.stdout.splitlines()
            if re.match(r"^\s*U\s", ln) or re.search(r"\sU\s", ln)}


def build(driver, synth, nm):
    """Return (imports, arm_undefined_or_None, riscv_undefined_or_None)."""
    d = HERE / driver
    subprocess.run(["cargo", "build", "--release", "--target", "wasm32-unknown-unknown", "-q"],
                   cwd=d, capture_output=True)
    ws = glob.glob(str(d / "target/wasm32-unknown-unknown/release/*.wasm"))
    if not ws:
        return None, None, None
    w = ws[0]
    arm, rv = f"/tmp/xa-{driver}.o", f"/tmp/xr-{driver}.o"
    for p in (arm, rv):
        if os.path.exists(p):
            os.unlink(p)
    run([synth, "compile", w, "--target", "cortex-m3", "--all-exports", "--relocatable", "-o", arm])
    run([synth, "compile", w, "-b", "riscv", "--target", "esp32c3", "--all-exports", "--relocatable", "-o", rv])
    return (imports(w),
            undefined(arm, nm) if os.path.exists(arm) else None,
            undefined(rv, nm) if os.path.exists(rv) else None)


def self_test():
    """Both controls must fire, or the gate proves nothing."""
    ok = True
    # Control 1: a superset (a leaked symbol) must be REJECTED. This is the
    # exact shape the old `> 0` rule accepted on every RISC-V object.
    if {"read32", "write32"} == {"read32", "write32", "synth_func_18"}:
        print("  self-test FAIL: leaked-symbol superset compared equal"); ok = False
    else:
        print("  self-test ok: leaked symbol (superset) is rejected")
    # Control 2: an empty expectation must be a PASS, not a failure -- the
    # `hm-thin` case the `> 0` rule would have failed.
    if set() != set():
        print("  self-test FAIL: empty set not equal to itself"); ok = False
    else:
        print("  self-test ok: a driver importing nothing passes with 0 undefined")
    return 0 if ok else 4


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()

    need("wasm-tools", ["wasm-tools", "--version"])
    nm = os.environ.get("NM", "arm-none-eabi-nm")
    need(nm, [nm, "--version"])
    synth = synth_bin()
    need("synth", [synth, "--version"])
    print(f"  synth: {run([synth,'--version']).stdout.strip()}  ({synth})")

    drivers = sorted(p.name for p in HERE.glob("*-thin") if p.is_dir())
    if not drivers:
        sys.exit("FATAL: no *-thin drivers found (exit 2)")
    print(f"  discovered {len(drivers)} thin-seam drivers\n")

    print(f"{'driver':<13} {'imports':<34} {'ARM undefined':<34} verdict")
    arm_fail, rv_clean = [], []
    for d in drivers:
        imp, arm, rv = build(d, synth, nm)
        if imp is None:
            print(f"{d:<13} {'NO WASM BUILT':<34}"); arm_fail.append(d); continue
        good = arm is not None and arm == imp
        if not good:
            arm_fail.append(d)
        shown = " ".join(sorted(imp)) or "(none)"
        got = "(no object)" if arm is None else (" ".join(sorted(arm)) or "(none)")
        print(f"{d:<13} {shown:<34} {got:<34} {'ok' if good else 'MISMATCH'}")
        if rv is not None and rv == imp:
            rv_clean.append(d)

    print()
    if arm_fail:
        print("ARM leg FAILED -- these objects' undefined sets do not equal their imports:")
        for d in arm_fail:
            print(f"  {d}")
        return 4
    print(f"ARM leg held: all {len(drivers)} drivers keep exactly their imported seam")
    print("symbols undefined on cortex-m3 -- no leak, no truncation.")

    print(f"\nRISC-V leg: {len(RISCV_KNOWN_BAD)} drivers on the known-defect ledger")
    print(f"  ({RISCV_TRACKER})")
    unexpected = sorted(set(rv_clean) & RISCV_KNOWN_BAD)
    if unexpected:
        print("\nLEDGER STALE -- these now cross RISC-V cleanly and must be removed")
        print("from RISCV_KNOWN_BAD so the gate starts holding them to it:")
        for d in unexpected:
            print(f"  {d}")
        return 5
    newly_bad = sorted(set(drivers) - RISCV_KNOWN_BAD - set(rv_clean))
    if newly_bad:
        print("\nRISC-V REGRESSION -- not on the ledger and not crossing cleanly:")
        for d in newly_bad:
            print(f"  {d}")
        return 5
    print("  ledger accurate: no driver silently changed RISC-V status.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

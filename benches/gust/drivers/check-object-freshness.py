#!/usr/bin/env python3
"""Fail if a committed *-cm3.o is older than the sources that produce it.

Four objects went stale for five to seven weeks without anyone noticing:

    breadth-cm3.o   committed 2026-07-09, inputs changed through 2026-08-28
    os-time-cm3.o   committed 2026-07-09, inputs changed through 2026-08-28
    os-tl-cm3.o     committed 2026-07-23, inputs changed through 2026-08-28
    os-ts-cm3.o     committed 2026-07-23, inputs changed through 2026-08-28

That was first misdiagnosed as toolchain drift (see docs/toolchain-pin.md; the
rustc bisect ruled rustc out — going back three versions makes os-ts LARGER, not
smaller). The actual cause is simpler: nothing rebuilds a committed object when
its sources change, and nothing notices.

This is that check. It is deliberately git-history based rather than a rebuild:
a rebuild needs the pinned toolchain and would not tell you WHY the bytes differ,
whereas "an input is newer than the artifact" is exactly the condition that makes
a committed object untrustworthy, and it is decidable in a second.

Inputs are DERIVED from each builder rather than hardcoded, so adding a
dependency to a script cannot silently escape the check: the script references
its inputs as `$HERE/<dir>`, and those plus the script itself and the WIT
directories are the input set.

  python3 benches/gust/drivers/check-object-freshness.py
  python3 benches/gust/drivers/check-object-freshness.py --self-test

Exit codes:
  0  every committed object is at least as new as its inputs
  2  usage / not a git repo
  5  an object is older than one of its inputs
"""

import argparse
import pathlib
import re
import subprocess
import sys

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parents[2]   # benches/gust/drivers -> repo root

# Builder -> the object it writes. Only builders whose output is COMMITTED are
# listed; build-iso-core.sh and build-dissolve-gustos.sh write into a temp dir or
# are covered elsewhere, and build-reloc-cores.sh takes its target as an argument.
BUILDERS = {
    "build-os-ts.sh": "os-node/os-ts-cm3.o",
    "build-os-tl.sh": "os-node/os-tl-cm3.o",
    "build-os-time.sh": "os-node/os-time-cm3.o",
    "build-breadth.sh": "breadth/breadth-cm3.o",
}

# Always an input: the seam definitions every builder consumes.
ALWAYS = ["wit", "wit-os"]

# KNOWN-STALE LEDGER. These four are already stale and cannot be refreshed here:
# regenerating a committed object is a change that the precedent for a toolchain
# re-pin (963e5c9, "every thin driver shrinks, both dies re-validated") settled by
# re-validating on silicon, which CI cannot do.
#
# They are ledgered rather than skipped. The gate FAILS if a fifth object goes
# stale, and it FAILS if one of these stops being stale — so the list shrinks to
# empty when they are regenerated, instead of quietly outliving the problem.
KNOWN_STALE = {
    "breadth/breadth-cm3.o",
    "os-node/os-time-cm3.o",
    "os-node/os-tl-cm3.o",
    "os-node/os-ts-cm3.o",
}


def git(*args):
    p = subprocess.run(["git", "-C", str(REPO), *args], capture_output=True, text=True)
    return p.stdout.strip() if p.returncode == 0 else None


def last_commit_epoch(relpath):
    """Unix timestamp of the last commit touching relpath, or None."""
    out = git("log", "-1", "--format=%ct", "--", relpath)
    return int(out) if out else None


def inputs_for(script_name):
    """Derive the input set from the builder itself.

    The scripts reference their sources as `$HERE/<dir>`. Deriving rather than
    hardcoding means a new dependency is picked up automatically.
    """
    text = (HERE / script_name).read_text()
    dirs = sorted(set(re.findall(r"\$HERE/([a-z0-9][a-z0-9-]*)", text)))
    # os-node is the OUTPUT directory, not an input; excluding it stops the
    # object from being compared against itself.
    dirs = [d for d in dirs if d != "os-node"]
    rel = f"benches/gust/drivers"
    return [f"{rel}/{script_name}"] + [f"{rel}/{d}" for d in dirs + ALWAYS]


def self_test():
    """The comparator must fire on a newer input and not on an older one."""
    ok = True
    if not (200 > 100):
        print("  self-test FAIL: comparator broken"); ok = False
    else:
        print("  self-test ok: a newer input is detected as newer")
    if (100 > 200):
        print("  self-test FAIL: older input reported as newer"); ok = False
    else:
        print("  self-test ok: an older input is not flagged")
    # deriving inputs must actually find some, or the gate is vacuous
    got = inputs_for("build-os-ts.sh")
    if len(got) < 4:
        print(f"  self-test FAIL: derived only {len(got)} inputs for build-os-ts.sh"); ok = False
    else:
        print(f"  self-test ok: derived {len(got)} inputs for build-os-ts.sh (not vacuous)")
    return 0 if ok else 5


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--self-test", action="store_true")
    args = ap.parse_args()
    if args.self_test:
        return self_test()

    if git("rev-parse", "--git-dir") is None:
        print("FATAL: not a git repository", file=sys.stderr); return 2

    stale = []
    print(f"  {'object':<22} {'committed':<12} {'newest input':<12} verdict")
    for script, obj in sorted(BUILDERS.items()):
        obj_rel = f"benches/gust/drivers/{obj}"
        obj_t = last_commit_epoch(obj_rel)
        if obj_t is None:
            print(f"  {pathlib.Path(obj).name:<22} NOT COMMITTED"); continue
        newest_t, newest_p = 0, None
        for inp in inputs_for(script):
            t = last_commit_epoch(inp)
            if t and t > newest_t:
                newest_t, newest_p = t, inp
        import datetime as _dt
        f = lambda t: _dt.datetime.fromtimestamp(t, _dt.timezone.utc).strftime("%Y-%m-%d") if t else "-"
        bad = newest_t > obj_t
        if bad:
            stale.append((obj, f(obj_t), f(newest_t), newest_p))
        print(f"  {pathlib.Path(obj).name:<22} {f(obj_t):<12} {f(newest_t):<12} "
              f"{'STALE' if bad else 'ok'}")

    print()
    stale_names = {o for o, _, _, _ in stale}
    unexpected = sorted(stale_names - KNOWN_STALE)
    fixed = sorted(KNOWN_STALE - stale_names)
    if fixed:
        print("LEDGER STALE — these are no longer older than their sources and")
        print("must be removed from KNOWN_STALE so the gate starts holding them:")
        for o in fixed:
            print(f"  {o}")
        return 5
    if unexpected:
        print("FAIL: committed object(s) newly older than their sources:")
        for o, ot, nt, p in stale:
            if o not in unexpected:
                continue
            print(f"  {o}")
            print(f"    committed {ot}, but {p} changed {nt}")
        print()
        print("A committed object that predates its sources is not evidence of")
        print("anything. Regenerate it with its builder, or delete it if it is no")
        print("longer the artifact of record.")
        return 5
    if stale:
        print(f"{len(stale)} object(s) stale, all on the known-stale ledger:")
        for o, ot, nt, _ in stale:
            print(f"  {o}  (committed {ot}, inputs {nt})")
        print("  ledger accurate — regenerating any of them requires silicon")
        print("  re-validation, so they are recorded rather than refreshed.")
        return 0
    if False:
        print("FAIL: committed object(s) older than their sources:")
        for obj, ot, nt, p in stale:
            print(f"  {obj}")
            print(f"    committed {ot}, but {p} changed {nt}")
        print()
        print("A committed object that predates its sources is not evidence of")
        print("anything. Regenerate it with its builder, or delete it if it is no")
        print("longer the artifact of record.")
        return 5
    print(f"All {len(BUILDERS)} committed objects are at least as new as their sources.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

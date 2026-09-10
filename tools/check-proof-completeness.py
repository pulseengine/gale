#!/usr/bin/env python3
"""Fail if a proof file's completeness silently changes.

`coqc` SUCCEEDS on a file whose every theorem ends in `Admitted.` -- an admit is
not an error, it is a deferral. So `bazel test //proofs:..._test` is green whether
a theorem is proven or merely stated, and the "Rocq Proofs (14 files)" job cannot
tell the two apart. Measured on main:

    10 files fully discharged
     3 files ENTIRELY admitted -- 74 theorems, zero Qed
     1 file EMPTY (heap_proofs.v, 0 bytes) but still a named CI target

The Lean half of the same workflow already learned a version of this (gale#286:
"'0 sorry' was a grep result rather than a verification result") and the fix there
was to actually INVOKE the targets. That closed "nothing ran it" but not "it ran
and could not distinguish". Lean is currently clean -- 0 `sorry` in 8 files -- so
this gate covers Rocq and watches Lean so it stays that way.

This does NOT demand the admits be discharged. It makes them VISIBLE and pins
them: the ledger below is the admitted state of the repo, and the gate fails if
a file gains admits, if a NEW file is admitted, or if a ledgered file is fixed
without shrinking the ledger. The last case matters -- a ledger nobody prunes
becomes a permanent excuse.

Exit codes:
  0  the census matches the ledger
  1  completeness changed (new/grown/shrunk admits, or a new empty proof file)
  2  usage / no proofs directory
  6  the NEGATIVE CONTROL passed when it must fail  <- the gate is not biting
"""
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parent.parent
ROCQ = ROOT / "proofs"
LEAN = ROOT / "proofs" / "lean"

ADMIT = re.compile(r"^\s*(Admitted\.|admit\.)", re.M)
QED = re.compile(r"\bQed\.")
THEOREM = re.compile(r"^\s*(Theorem|Lemma|Corollary)\b", re.M)
SORRY = re.compile(r"\bsorry\b")

# LEDGER of incomplete proofs. Each entry is the EXACT admit count. These three
# files state their theorems and prove none of them; two carry an honest reason
# in-file ("Z.lor_le may not exist in Coq 9.0", "needs Coq 9.0 tactic
# debugging"). Recorded rather than hidden, so the FV posture is legible from
# the gate rather than only from reading 800 lines of Rocq.
ADMITTED_LEDGER = {
    "poll_proofs.v": 22,
    "sched_proofs.v": 23,
    "thread_lifecycle_proofs.v": 29,
}

# Empty proof files that are nonetheless named CI targets. An empty file passes
# `coqc` trivially, so a target pointing at one is a green check over nothing.
EMPTY_LEDGER = {"heap_proofs.v"}


def census(text: str) -> tuple[int, int, int]:
    return len(ADMIT.findall(text)), len(QED.findall(text)), len(THEOREM.findall(text))


def check(files, lean_files) -> tuple[int, list[str]]:
    problems: list[str] = []
    print("  file                                admits   Qed  theorems  verdict")
    for f in files:
        text = f.read_text(errors="ignore")
        a, q, t = census(text)
        name = f.name
        if not text.strip():
            verdict = "EMPTY (ledgered)" if name in EMPTY_LEDGER else "EMPTY — NOT LEDGERED"
            if name not in EMPTY_LEDGER:
                problems.append(f"{name}: empty proof file, not on the ledger")
            print(f"  {name:<34} {a:>6} {q:>5} {t:>9}  {verdict}")
            continue
        expected = ADMITTED_LEDGER.get(name, 0)
        if a == expected == 0:
            verdict = "proven"
        elif a == expected:
            verdict = f"admitted (ledgered {expected})"
        elif a > expected:
            verdict = f"ADMITS GREW {expected} -> {a}"
            problems.append(f"{name}: admits grew {expected} -> {a}")
        else:
            verdict = f"admits SHRANK {expected} -> {a} — prune the ledger"
            problems.append(f"{name}: admits shrank {expected} -> {a}; update ADMITTED_LEDGER")
        print(f"  {name:<34} {a:>6} {q:>5} {t:>9}  {verdict}")

    for name in ADMITTED_LEDGER:
        if not any(f.name == name for f in files):
            problems.append(f"{name}: on the ledger but no longer exists")
    for name in EMPTY_LEDGER:
        if not any(f.name == name for f in files):
            problems.append(f"{name}: on the empty-ledger but no longer exists")

    for f in lean_files:
        n = len(SORRY.findall(f.read_text(errors="ignore")))
        if n:
            problems.append(f"lean/{f.name}: {n} sorry — Lean has been clean; do not start now")
    return (1 if problems else 0), problems


def self_test() -> int:
    ok = True
    if not ADMIT.findall("Admitted.\n"):
        print("  MISSED: bare Admitted. not caught"); ok = False
    else:
        print("  ok   bare `Admitted.` is caught")
    if not ADMIT.findall("    admit.\n"):
        print("  MISSED: indented admit. not caught"); ok = False
    else:
        print("  ok   indented `admit.` is caught")
    # The false positive that fooled me first time: prose about a Rust method
    # named `admit` must NOT count. executor_proofs.v mentions `Tasks::admit`
    # in a comment, which a naive substring grep reads as an admitted proof.
    if ADMIT.findall("        * [Tasks::admit] clears the ready bit\n"):
        print("  BROKEN: prose mentioning `admit` was counted"); ok = False
    else:
        print("  ok   prose `Tasks::admit` is NOT counted")
    if len(QED.findall("Proof. trivial. Qed.\n")) != 1:
        print("  MISSED: inline Qed not counted"); ok = False
    else:
        print("  ok   inline `Qed.` is counted")
    print("  self-test PASS" if ok else "  self-test FAIL")
    return 0 if ok else 6


def main() -> int:
    if "--self-test" in sys.argv:
        return self_test()
    if not ROCQ.is_dir():
        print(f"FATAL: no proofs directory at {ROCQ}", file=sys.stderr)
        return 2
    files = sorted(ROCQ.glob("*.v"))
    if not files:
        print("FATAL: no .v files -- the sweep would be vacuous", file=sys.stderr)
        return 2
    lean_files = sorted(LEAN.glob("*.lean")) if LEAN.is_dir() else []

    rc, problems = check(files, lean_files)
    total_admits = sum(ADMITTED_LEDGER.values())
    print()
    if problems:
        print("FAIL: proof completeness changed:")
        for p in problems:
            print(f"  {p}")
        print()
        print("  If a proof was discharged, SHRINK the ledger in this file.")
        print("  If a new admit is intended, add it with a reason in-file.")
        return 1
    print(f"  ok: {len(files)} Rocq file(s), {len(lean_files)} Lean file(s)")
    print(f"      {total_admits} theorem(s) admitted across {len(ADMITTED_LEDGER)} ledgered file(s);")
    print(f"      {len(EMPTY_LEDGER)} empty file(s) ledgered. Every other proof closes with Qed.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

#!/usr/bin/env python3
"""Fail if a workflow retry loop cannot report failure.

Found the hard way. `llvm-lto-test (msgq)` failed on a PR that touched only rivet
YAML. The log showed the Zephyr SDK's GNU toolchain install failing three times:

    Installing 'arm-zephyr-eabi' GNU toolchain ...
    ERROR: GNU toolchain download failed
    FATAL ERROR: command "/root/zephyr-sdk-1.0.1/setup.sh -t arm-zephyr-eabi -h" failed

...and the step that ran it PASSED. The step after it, `Setup SDK paths`, failed
instead — looking for a toolchain that had never been installed. The install
reported success while installing nothing, and the error surfaced one step later
wearing a different name.

The cause is the retry idiom:

    for attempt in 1 2 3; do CMD && break || sleep 5; done

When every attempt fails, the last command executed is `sleep`, which succeeds,
so the loop's exit status is 0. `bash -e` does not help: a failing command in an
`&&`/`||` list is exempt, and the list's status is the final `sleep`. Verified
directly -- `for attempt in 1 2 3; do false && break || sleep 0.1; done` exits 0.

There were 28 of these, in 10 workflows, guarding every `west update` and
`west sdk install` in the repo.

The replacement exits non-zero when the attempts run out, and backs off:

    n=0; until CMD; do n=$((n+1));
      if [ $n -ge 3 ]; then echo "::error::CMD failed after 3 attempts" >&2; exit 1; fi;
      sleep $((15*n)); done

Exit codes:
  0  no unfailable retry loop found
  1  at least one found
  2  usage / no workflows directory
  6  the NEGATIVE CONTROL passed when it must fail  <- the gate is not biting
"""
import pathlib
import re
import sys

WF = pathlib.Path(__file__).resolve().parent.parent / ".github" / "workflows"

# `CMD && break || sleep N`, on one line or split across a `for ... do/done`.
# The defect is the `&& break || <anything>` shape: the trailing branch decides
# the loop's status, and it is chosen to succeed.
SWALLOW = re.compile(r"&&\s*break\s*\|\|")


def scan(text: str) -> list[tuple[int, str]]:
    return [
        (i, line.strip())
        for i, line in enumerate(text.splitlines(), 1)
        if SWALLOW.search(line)
    ]


def self_test() -> int:
    """The gate must be OBSERVED to catch the shape, and to leave the fix alone."""
    ok = True
    bad = 'for attempt in 1 2 3; do west update && break || sleep 15; done'
    good = ('n=0; until west update; do n=$((n+1)); '
            'if [ $n -ge 3 ]; then echo "::error::west update failed" >&2; exit 1; fi; '
            'sleep $((15*n)); done')
    if not scan(bad):
        print("  MISSED: the unfailable form was not caught"); ok = False
    else:
        print("  ok   unfailable form is caught")
    if scan(good):
        print("  BROKEN: the corrected form was flagged"); ok = False
    else:
        print("  ok   corrected form is not flagged")
    # A retry that ends in something OTHER than sleep is still the same defect.
    if not scan("do CMD && break || true; done"):
        print("  MISSED: `|| true` variant not caught"); ok = False
    else:
        print("  ok   `|| true` variant is caught")
    print("  self-test PASS" if ok else "  self-test FAIL")
    return 0 if ok else 6


def main() -> int:
    if "--self-test" in sys.argv:
        return self_test()
    if not WF.is_dir():
        print(f"FATAL: no workflows directory at {WF}", file=sys.stderr)
        return 2
    files = sorted(WF.glob("*.yml"))
    if not files:
        print("FATAL: no workflow files -- the sweep would be vacuous", file=sys.stderr)
        return 2

    hits = []
    for f in files:
        for ln, text in scan(f.read_text()):
            hits.append((f.name, ln, text))

    if hits:
        print(f"FAIL: {len(hits)} retry loop(s) that cannot report failure:")
        for name, ln, text in hits:
            print(f"  {name}:{ln}")
            print(f"    {text[:110]}")
        print()
        print("  `CMD && break || sleep N` exits 0 when every attempt fails: the last")
        print("  command run is the sleep. Use instead:")
        print("    n=0; until CMD; do n=$((n+1));")
        print('      if [ $n -ge 3 ]; then echo "::error::CMD failed after 3 attempts" >&2; exit 1; fi;')
        print("      sleep $((N*n)); done")
        return 1

    print(f"ok: {len(files)} workflow(s) swept, no unfailable retry loops")
    return 0


if __name__ == "__main__":
    sys.exit(main())

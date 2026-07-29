#!/usr/bin/env python3
"""Traceability-completeness gate: a requirement whose V is closed must say so.

v0.6.0 / D3. The release-completeness gate asks "does every approved/implemented
artifact have its V closed?". That question is only answerable if requirement statuses
track their verification artifacts. They drifted: 12 requirements carried a `verified`
`VER-*` while still reading `implemented`/`proposed` themselves — the whole verified
driver suite, gpio, timer, and the syscall seam. The gate misreported them as not-done.
It erred safe, but it lied, and nothing stopped it recurring.

This check fails when a requirement's matching verification artifact is `verified` but
the requirement itself is not. Pairing is by id: `REQ-FOO-001` <-> `VER-FOO-001`.

    python3 tools/check-v-closure.py            # gate (exit 1 on drift)
    python3 tools/check-v-closure.py --list     # show every REQ/VER pair + status
"""
import glob
import re
import sys

REQ_RE = re.compile(r"- id: ([A-Z][A-Z0-9-]+)\n(?:.*\n)*?\s+status: (\w+)")


def load_statuses(root="artifacts"):
    """id -> status, across every artifact file."""
    status = {}
    for path in sorted(glob.glob(f"{root}/*.yaml")):
        text = open(path).read()
        for match in REQ_RE.finditer(text):
            status[match.group(1)] = match.group(2)
    return status


def pairs(status):
    """(req_id, ver_id, req_status, ver_status) for every REQ-* with a matching VER-*."""
    out = []
    for req_id, req_status in sorted(status.items()):
        if not req_id.startswith("REQ-"):
            continue
        ver_id = "VER-" + req_id[len("REQ-"):]
        if ver_id in status:
            out.append((req_id, ver_id, req_status, status[ver_id]))
    return out


def main():
    status = load_statuses()
    listed = pairs(status)

    if "--list" in sys.argv:
        for req_id, ver_id, req_status, ver_status in listed:
            print(f"{req_status:12} {req_id:26} <- {ver_status:12} {ver_id}")
        return 0

    # The drift: V closed, requirement not.
    drift = [p for p in listed if p[3] == "verified" and p[2] != "verified"]
    if drift:
        print("Traceability drift — requirement status lags its closed V:\n")
        for req_id, ver_id, req_status, _ in drift:
            print(f"  {req_id:26} is '{req_status}' but {ver_id} is 'verified'")
        print(
            f"\n{len(drift)} requirement(s) whose V is closed still read as not-done, so the\n"
            "release-completeness gate misreports them. Promote each to 'verified' (or, if the\n"
            "V should not have been closed, fix the VER artifact instead)."
        )
        return 1

    print(f"V-closure traceability: OK — {len(listed)} REQ/VER pairs, no requirement lags its closed V.")
    return 0


if __name__ == "__main__":
    sys.exit(main())

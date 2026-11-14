#!/usr/bin/env python3
"""check-selfhosted-fork-guard.py: no fork PR is routed to a self-hosted runner.

Adapted from pulseengine/ordeal scripts/check_selfhosted_fork_guard.py (ordeal#322),
with a --self-test added (a gate nobody has watched fail is a gate nobody knows works).

Every job in .github/workflows/ whose runs-on names `self-hosted` must use one of two
fail-closed forms. The self-hosted labels are chosen only for a non-PR event or a PR
whose head repository is this one; a fork, or a deleted fork whose head.repo is null,
falls to the hosted runner:

  literal  ${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository) && fromJSON('[... "self-hosted" ...]') || 'ubuntu-latest' }}
  matrix   ${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository || !contains(matrix.runner, 'self-hosted')) && fromJSON(matrix.runner) || 'ubuntu-latest' }}

Rejected:
  - a plain self-hosted label (list or string);
  - the fail-open form `head.repo.fork && 'ubuntu-latest' || [...]`, which picks
    self-hosted whenever .fork is not true, including a null head.repo;
  - any self-hosted job in a workflow with a trigger outside TRUSTED: the expression
    trusts every event that is not `pull_request`, so a trigger a fork can cause under
    another name (pull_request_target, workflow_run, issue_comment, ...) would reach
    the runner;
  - no self-hosted job found at all (a check that compared nothing is not a pass).

This routing is not the security boundary (a fork PR can edit the workflow file);
the org's runner-group policy and fork-PR approval are. It keeps well-formed routing
from failing open.

Exit 0 = every self-hosted job is guarded; 1 = a violation or nothing found.
"""
import pathlib
import re
import sys
import tempfile

import yaml

GUARD = (r"\(github\.event_name != 'pull_request' \|\| "
         r"github\.event\.pull_request\.head\.repo\.full_name == github\.repository")
LITERAL = re.compile(r"^\$\{\{ " + GUARD + r"\) && fromJSON\('\[[^\]]*\"self-hosted\"[^\]]*\]'\) \|\| 'ubuntu-latest' \}\}$")
MATRIX = re.compile(r"^\$\{\{ " + GUARD + r" \|\| !contains\(matrix\.runner, 'self-hosted'\)\) && fromJSON\(matrix\.runner\) \|\| 'ubuntu-latest' \}\}$")
TRUSTED = {"push", "pull_request", "schedule", "workflow_dispatch", "merge_group"}


def check(wfdir):
    """Return (seen, bad) over every workflow in wfdir."""
    bad, seen = {}, []
    for wf in sorted([*wfdir.glob("*.yml"), *wfdir.glob("*.yaml")]):
        doc = yaml.safe_load(wf.read_text()) or {}
        on = doc.get(True, doc.get("on")) or {}
        events = set(on) if isinstance(on, (dict, list)) else {on}
        for job, spec in (doc.get("jobs") or {}).items():
            ro = (spec or {}).get("runs-on")
            text = " ".join(map(str, ro)) if isinstance(ro, list) else str(ro or "")
            uses_matrix = "matrix.runner" in text
            if "self-hosted" not in text and not uses_matrix:
                continue
            if uses_matrix:
                runners = [str(e.get("runner", "")) for e in
                           (((spec.get("strategy") or {}).get("matrix") or {}).get("include") or [])]
                if not any("self-hosted" in r for r in runners):
                    continue
            where = f"{wf.name}:{job}"
            seen.append(where)
            untrusted = sorted(str(e) for e in events - TRUSTED)
            if untrusted:
                bad[where] = f"workflow trigger(s) outside the allowlist: {', '.join(untrusted)}"
            elif isinstance(ro, list) or not (LITERAL.match(text.strip()) or MATRIX.match(text.strip())):
                bad[where] = "runs-on is not a fail-closed same-repo form"
    return seen, bad


def report(seen, bad):
    for w in seen:
        print(f"FAIL  {w}: {bad[w]}" if w in bad else f"ok    {w}")
    if not seen:
        print("FAIL  no self-hosted job found: this check compared nothing")
        return 1
    if bad:
        print(f"FAIL  {len(bad)} of {len(seen)} self-hosted job(s) can be reached by a fork")
        return 1
    print(f"PASS  {len(seen)} self-hosted job(s), all fail-closed")
    return 0


GOOD = ("${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name"
        " == github.repository) && fromJSON('[\"self-hosted\",\"light\"]') || 'ubuntu-latest' }}")
CASES = [  # (name, workflow text, expect_pass)
    ("fail-closed literal", f"on: [pull_request, push]\njobs:\n  a:\n    runs-on: {GOOD}\n", True),
    ("fail-closed matrix",
     "on: pull_request\njobs:\n  a:\n    runs-on: ${{ (github.event_name != 'pull_request' || "
     "github.event.pull_request.head.repo.full_name == github.repository || !contains(matrix.runner, "
     "'self-hosted')) && fromJSON(matrix.runner) || 'ubuntu-latest' }}\n    strategy:\n      matrix:\n"
     "        include:\n          - runner: '[\"self-hosted\", \"rust-cpu\"]'\n", True),
    ("plain label list", "on: pull_request\njobs:\n  a:\n    runs-on: [self-hosted, light]\n", False),
    ("plain label string", "on: pull_request\njobs:\n  a:\n    runs-on: self-hosted\n", False),
    ("fail-open fork form",
     "on: pull_request\njobs:\n  a:\n    runs-on: ${{ github.event.pull_request.head.repo.fork && "
     "'ubuntu-latest' || fromJSON('[\"self-hosted\",\"light\"]') }}\n", False),
    ("good form under pull_request_target",
     f"on: pull_request_target\njobs:\n  a:\n    runs-on: {GOOD}\n", False),
    ("good form under workflow_run",
     f"on:\n  workflow_run:\n    workflows: [x]\njobs:\n  a:\n    runs-on: {GOOD}\n", False),
    ("no self-hosted job at all", "on: pull_request\njobs:\n  a:\n    runs-on: ubuntu-latest\n", False),
]


def self_test():
    broken = 0
    for name, text, expect_pass in CASES:
        with tempfile.TemporaryDirectory() as d:
            p = pathlib.Path(d)
            (p / "w.yml").write_text(text)
            seen, bad = check(p)
            passed = bool(seen) and not bad
            ok = passed == expect_pass
            broken += not ok
            print(f"  self-test {'ok  ' if ok else 'FAIL'} {name}: "
                  f"{'accepted' if passed else 'rejected'} (expected {'accept' if expect_pass else 'reject'})")
    return 1 if broken else 0


if __name__ == "__main__":
    if "--self-test" in sys.argv[1:]:
        sys.exit(self_test())
    root = pathlib.Path(__file__).resolve().parent.parent
    sys.exit(report(*check(root / ".github" / "workflows")))

#!/usr/bin/env python3
"""check-selfhosted-fork-guard.py: no fork PR is routed to a self-hosted runner.

Adapted from pulseengine/ordeal scripts/check_selfhosted_fork_guard.py (ordeal#322),
with a --self-test added (a gate nobody has watched fail is a gate nobody knows works).

GitHub routes by label, so a job reaches the self-hosted pool through any label it
names (`light`, `[linux, light]`, `{group: X}`, a matrix value), with or without the
word `self-hosted`. So every job's runs-on is ALLOWLISTED: a hosted image
(ubuntu-*/macos-*/windows-*), `${{ matrix.X }}` whose every value is a hosted image,
or one of two fail-closed forms. The self-hosted labels are chosen only for a non-PR event or a PR
whose head repository is this one; a fork, or a deleted fork whose head.repo is null,
falls to the hosted runner:

  literal  ${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository) && fromJSON('[... "self-hosted" ...]') || 'ubuntu-latest' }}
  matrix   ${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name == github.repository || !contains(matrix.runner, 'self-hosted')) && fromJSON(matrix.runner) || 'ubuntu-latest' }}

Rejected:
  - any other runs-on: a pool label, a label list, a runner group, a matrix value
    that is not a hosted image, a runner from needs, inputs or vars;
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


HOSTED = re.compile(r"^(ubuntu|macos|windows)-[A-Za-z0-9._-]+$")
MATRIX_REF = re.compile(r"^\$\{\{\s*matrix\.([A-Za-z0-9_-]+)\s*\}\}$")


def matrix_values(spec, key):
    """Every literal value `key` takes in the job's matrix (list and include)."""
    m = ((spec.get("strategy") or {}).get("matrix")) or {}
    if not isinstance(m, dict):
        return None  # a matrix built from an expression: values unknown
    vals = []
    v = m.get(key)
    if isinstance(v, list):
        vals += v
    elif v is not None:
        return None
    for inc in m.get("include") or []:
        if isinstance(inc, dict) and key in inc:
            vals.append(inc[key])
    return vals


def check(wfdir):
    """Return (seen, bad). GitHub routes by label, so a job reaches the self-hosted
    pool through any label it names, `self-hosted` or not (`light`, `[linux, light]`,
    `{group: X}`, a matrix value). So runs-on is ALLOWLISTED: a hosted image, one of
    the two fail-closed forms, or `${{ matrix.X }}` whose every value is a hosted
    image. Anything else, including a runner taken from needs, inputs or vars, fails.
    `seen` counts the jobs on a fail-closed form (the self-hosted ones)."""
    bad, seen = {}, []
    for wf in sorted([*wfdir.glob("*.yml"), *wfdir.glob("*.yaml")]):
        doc = yaml.safe_load(wf.read_text()) or {}
        on = doc.get(True, doc.get("on")) or {}
        events = set(on) if isinstance(on, (dict, list)) else {on}
        for job, spec in (doc.get("jobs") or {}).items():
            spec = spec or {}
            where = f"{wf.name}:{job}"
            ro = spec.get("runs-on")
            if ro is None:
                if spec.get("uses"):
                    continue  # a reusable-workflow call; its jobs are checked in its own file
                bad[where] = "no runs-on"
                continue
            if not isinstance(ro, str):
                bad[where] = f"runs-on is a {type(ro).__name__}, not a hosted image or a fail-closed form"
                continue
            t = ro.strip()
            if HOSTED.match(t):
                continue
            if LITERAL.match(t) or MATRIX.match(t):
                seen.append(where)
                untrusted = sorted(str(e) for e in events - TRUSTED)
                if untrusted:
                    bad[where] = f"self-hosted under trigger(s) outside the allowlist: {', '.join(untrusted)}"
                continue
            mref = MATRIX_REF.match(t)
            if mref:
                vals = matrix_values(spec, mref.group(1))
                if vals and all(isinstance(v, str) and HOSTED.match(v) for v in vals):
                    continue
                bad[where] = f"runs-on is matrix.{mref.group(1)} with a value that is not a hosted image: {vals}"
                continue
            bad[where] = f"runs-on is not a hosted image or a fail-closed form: {t}"
    return seen, bad


def report(seen, bad):
    for w in seen:
        print(f"FAIL  {w}: {bad[w]}" if w in bad else f"ok    {w}")
    for w in sorted(set(bad) - set(seen)):
        print(f"FAIL  {w}: {bad[w]}")
    if not seen:
        print("FAIL  no self-hosted job found: this check compared nothing")
        return 1
    if bad:
        print(f"FAIL  {len(bad)} job(s) could reach the self-hosted pool unguarded")
        return 1
    print(f"PASS  {len(seen)} self-hosted job(s), all fail-closed")
    return 0


GOOD = ("${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name"
        " == github.repository) && fromJSON('[\"self-hosted\",\"light\"]') || 'ubuntu-latest' }}")
G = f"  good:\n    runs-on: {GOOD}\n"  # a guarded self-hosted job, so a case is never "compared nothing"
MATRIX_GOOD = ("${{ (github.event_name != 'pull_request' || github.event.pull_request.head.repo.full_name"
               " == github.repository || !contains(matrix.runner, 'self-hosted')) && fromJSON(matrix.runner)"
               " || 'ubuntu-latest' }}")
CASES = [  # (name, workflow text, expect_pass)
    # must pass
    ("fail-closed literal", f"on: [pull_request, push]\njobs:\n{G}", True),
    ("fail-closed matrix", f"on: pull_request\njobs:\n  a:\n    runs-on: {MATRIX_GOOD}\n    strategy:\n"
     "      matrix:\n        include:\n          - runner: '[\"self-hosted\", \"rust-cpu\"]'\n", True),
    ("hosted image next to a guarded job", f"on: pull_request\njobs:\n  a:\n    runs-on: ubuntu-24.04\n{G}", True),
    ("matrix.os of hosted images", f"on: pull_request\njobs:\n  a:\n    runs-on: ${{{{ matrix.os }}}}\n"
     f"    strategy:\n      matrix:\n        os: [ubuntu-22.04, macos-15]\n{G}", True),
    ("reusable-workflow call (no runs-on)", f"on: pull_request\njobs:\n  a:\n    uses: ./.github/workflows/x.yml\n{G}", True),
    # must fail
    ("plain self-hosted list", f"on: pull_request\njobs:\n  a:\n    runs-on: [self-hosted, light]\n{G}", False),
    ("plain self-hosted string", f"on: pull_request\njobs:\n  a:\n    runs-on: self-hosted\n{G}", False),
    ("bare pool label, no 'self-hosted'", f"on: pull_request\njobs:\n  a:\n    runs-on: light\n{G}", False),
    ("label list without 'self-hosted'", f"on: pull_request\njobs:\n  a:\n    runs-on: [linux, light]\n{G}", False),
    ("runner group", f"on: pull_request\njobs:\n  a:\n    runs-on:\n      group: pool\n{G}", False),
    ("matrix value is a pool label", f"on: pull_request\njobs:\n  a:\n    runs-on: ${{{{ matrix.os }}}}\n"
     f"    strategy:\n      matrix:\n        os: [ubuntu-22.04, light]\n{G}", False),
    ("runner from a needs output", f"on: pull_request\njobs:\n  a:\n    runs-on: ${{{{ needs.x.outputs.r }}}}\n{G}", False),
    ("runner from vars", f"on: pull_request\njobs:\n  a:\n    runs-on: ${{{{ vars.RUNNER }}}}\n{G}", False),
    ("fail-open fork form", "on: pull_request\njobs:\n  a:\n    runs-on: ${{ github.event.pull_request.head.repo.fork && "
     "'ubuntu-latest' || fromJSON('[\"self-hosted\",\"light\"]') }}\n" + G, False),
    ("good form under pull_request_target", f"on: pull_request_target\njobs:\n{G}", False),
    ("good form under workflow_run", f"on:\n  workflow_run:\n    workflows: [x]\njobs:\n{G}", False),
    ("good form under workflow_call", f"on: workflow_call\njobs:\n{G}", False),
    ("good form under issue_comment", f"on: issue_comment\njobs:\n{G}", False),
    ("job with no runs-on and no uses", f"on: pull_request\njobs:\n  a:\n    steps: []\n{G}", False),
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

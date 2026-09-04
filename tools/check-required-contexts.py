#!/usr/bin/env python3
"""Assert every required status check will actually be produced on every PR.

A required context that never reports does not turn a PR red -- it leaves it
BLOCKED on a check that will never arrive, which looks like "still running"
forever.  gale#294 put 19 contexts on main; gale#340 was one of them silently
acquiring that shape, because a `paths:` key was left behind with nothing under
it and `paths: null` is not obviously either a filter or not-a-filter.

This checks the direction that CI can check without admin rights: for every name
in .github/required-contexts.txt, some job in some workflow produces it, and that
workflow's `pull_request` trigger is unconditional -- no path filter (INCLUDING an
empty or null one), no branch restriction that excludes main, no `types:` that
drops a default PR activity.

The other direction -- that this file still matches the real branch protection --
needs admin rights the default GITHUB_TOKEN does not have.  Pass --protection FILE
with the API's JSON to check it; without it, that direction is reported as
UNGUARDED rather than silently assumed.
"""
import argparse, json, os, pathlib, re, sys, tempfile

try:
    import yaml
except ImportError:
    sys.exit("check-required-contexts: PyYAML is required (python3 -m pip install pyyaml)")

# A pull_request event fires on these activity types unless `types:` overrides it.
DEFAULT_PR_TYPES = {"opened", "synchronize", "reopened"}


def trigger_state(wf):
    """Why (or whether) this workflow reports on every PR against main."""
    on = wf.get(True) if True in wf else wf.get("on")
    if isinstance(on, str):
        return ("OK", None) if on == "pull_request" else ("NO_PR", f"only `on: {on}`")
    if isinstance(on, list):
        return ("OK", None) if "pull_request" in on else ("NO_PR", "no pull_request trigger")
    if not isinstance(on, dict):
        return ("NO_PR", "unparseable `on:`")
    if "pull_request" not in on:
        return ("NO_PR", "no pull_request trigger")
    pr = on["pull_request"]
    if pr is None:          # bare `pull_request:` -- fires on everything
        return ("OK", None)
    if not isinstance(pr, dict):
        return ("NO_PR", f"unparseable pull_request: {pr!r}")

    # The gale#340 case: presence of the KEY is what matters, not its value.  A
    # `paths:` with nothing under it parses as None, and whether GitHub treats
    # that as a filter is not something this repo should be betting a required
    # context on.
    for k in ("paths", "paths-ignore"):
        if k in pr:
            v = pr[k]
            detail = f"`{k}:` present"
            if v is None:
                detail += " with no entries (parses as null -- ambiguous, and that is the point)"
            else:
                detail += f" ({len(v)} pattern(s))"
            return ("PATH_FILTERED", detail)

    if "branches" in pr and pr["branches"] is not None:
        if "main" not in pr["branches"]:
            return ("BRANCH_MISMATCH", f"branches={pr['branches']} excludes main")
    if "branches-ignore" in pr and pr["branches-ignore"]:
        return ("BRANCH_MISMATCH", f"branches-ignore={pr['branches-ignore']}")

    if "types" in pr and pr["types"] is not None:
        missing = DEFAULT_PR_TYPES - set(pr["types"])
        if missing:
            return ("TYPES_RESTRICTED", f"types={pr['types']} drops {sorted(missing)}")

    return ("OK", None)


def index_jobs(wfdir):
    """job display name -> [(workflow filename, trigger state, detail)]"""
    idx = {}
    for path in sorted(pathlib.Path(wfdir).glob("*.y*ml")):
        try:
            wf = yaml.safe_load(path.read_text())
        except Exception as e:
            print(f"  ! {path.name}: unparseable ({e})", file=sys.stderr)
            continue
        if not isinstance(wf, dict):
            continue
        state, detail = trigger_state(wf)
        for jid, job in (wf.get("jobs") or {}).items():
            name = (job or {}).get("name") or jid
            idx.setdefault(name, []).append((path.name, state, detail))
    return idx


def check(listfile, wfdir, protection=None):
    contexts = [l.strip() for l in pathlib.Path(listfile).read_text().splitlines() if l.strip()]
    idx = index_jobs(wfdir)
    bad = []

    print(f"  {len(contexts)} required context(s) against {wfdir}\n")
    for ctx in contexts:
        # A matrix job's check is "Name (v1, v2)"; the job itself is named "Name".
        hits = idx.get(ctx) or idx.get(re.sub(r"\s*\(.*\)\s*$", "", ctx)) or []
        if not hits:
            bad.append((ctx, "UNMAPPED", "no job in any workflow produces this name"))
            print(f"    UNMAPPED       {ctx}")
            continue
        for fn, state, detail in hits:
            if state == "OK":
                print(f"    ok             {ctx}   [{fn}]")
            else:
                bad.append((ctx, state, f"{fn}: {detail}"))
                print(f"    {state:<14} {ctx}   [{fn}] -- {detail}")

    print()
    if protection:
        actual = set(json.loads(pathlib.Path(protection).read_text())
                     ["required_status_checks"]["contexts"])
        listed = set(contexts)
        for extra in sorted(actual - listed):
            bad.append((extra, "NOT_LISTED", "required on main but absent from this file"))
            print(f"    NOT_LISTED     {extra}")
        for stale in sorted(listed - actual):
            bad.append((stale, "NOT_REQUIRED", "in this file but not required on main"))
            print(f"    NOT_REQUIRED   {stale}")
        if actual == listed:
            print(f"    this file matches branch protection exactly ({len(actual)} contexts)")
    else:
        print("    UNGUARDED: no --protection given, so 'this file still matches the real")
        print("    branch protection' was NOT checked. Reading protection needs admin")
        print("    rights the default GITHUB_TOKEN does not have.")

    print()
    if bad:
        print(f"  FAIL: {len(bad)} required context(s) may not be produced on a PR")
        return 1
    print(f"  PASS: every required context maps to a job that reports on every PR")
    return 0


# --- negative control -------------------------------------------------------
# The gate has to be shown to DISCRIMINATE, not merely to run.  Each planted
# workflow below is the real shape of a way a required context stops arriving.
PLANTS = {
    "dangling-paths": ("the gale#340 shape: a `paths:` key with nothing under it",
                       "on:\n  pull_request:\n    branches: [main]\n    paths:\n"),
    "real-paths":     ("an ordinary path filter",
                       "on:\n  pull_request:\n    branches: [main]\n    paths: ['src/**']\n"),
    "no-pr":          ("push-only, so no PR ever gets a check",
                       "on:\n  push:\n    branches: [main]\n"),
    "types":          ("`types:` that drops synchronize, so pushes to the PR do not re-report",
                       "on:\n  pull_request:\n    branches: [main]\n    types: [opened]\n"),
    "branch":         ("a branches list that excludes main",
                       "on:\n  pull_request:\n    branches: [develop]\n"),
}


def self_test():
    ok = True
    with tempfile.TemporaryDirectory() as td:
        td = pathlib.Path(td)
        wfd = td / "workflows"; wfd.mkdir()
        lst = td / "list.txt"
        lst.write_text("The Gate\n")

        clean = "name: clean\non:\n  pull_request:\n    branches: [main]\n"
        job = "jobs:\n  g:\n    name: \"The Gate\"\n    runs-on: ubuntu-24.04\n    steps: [{run: 'true'}]\n"

        print("  negative control -- each planted workflow MUST make the gate fail\n")
        for key, (why, trig) in PLANTS.items():
            (wfd / "w.yml").write_text(f"name: {key}\n{trig}{job}")
            rc = check(lst, wfd)
            verdict = "caught" if rc != 0 else "MISSED"
            print(f"  --> {verdict}: {key} ({why})\n")
            if rc == 0:
                ok = False

        # ...and an unmappable name, the other way a context never arrives.
        (wfd / "w.yml").write_text(f"{clean}{job}")
        lst.write_text("The Gate\nA Context Nothing Produces\n")
        rc = check(lst, wfd)
        print(f"  --> {'caught' if rc else 'MISSED'}: a context no job produces\n")
        if rc == 0:
            ok = False

        # Positive control: the clean case must PASS, or the gate is just a
        # rubber stamp that always says no.
        lst.write_text("The Gate\n")
        rc = check(lst, wfd)
        print(f"  --> {'ok' if rc == 0 else 'BROKEN'}: the clean case passes\n")
        if rc != 0:
            ok = False

    print("  self-test PASS" if ok else "  self-test FAIL")
    return 0 if ok else 1


def main():
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--list", default=".github/required-contexts.txt")
    ap.add_argument("--workflows", default=".github/workflows")
    ap.add_argument("--protection", help="JSON from the branch-protection API, if readable")
    ap.add_argument("--self-test", action="store_true", help="run the negative control and exit")
    a = ap.parse_args()
    return self_test() if a.self_test else check(a.list, a.workflows, a.protection)


if __name__ == "__main__":
    sys.exit(main())

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
drops a default PR activity -- and that it also triggers on `merge_group`.  main
merges through a merge queue, and the queue waits for every required context on
its own merge_group run: a workflow without that trigger reports nothing there,
so the queued PR sits until the queue's timeout evicts it.  A job SKIPPED by its
`if:` in the queue still reports (skipped counts as success); only a workflow
that never starts does not.

Reporting is not enough: a job skipped by its `if:` reports "skipped", which
branch protection counts as passing. The heavy jobs use that on purpose, to skip
when tools/ci/classify-changes.py finds none of their inputs changed. So this
also checks that a required job can be skipped by NOTHING ELSE:
  (b) its `if:` is absent, or is `!cancelled() && needs.<C>.outputs.<o> != 'false'`
      (`||` of such terms allowed) where <C> runs the classifier, is in its
      `needs:`, and wires <o> to the classifier step; it needs no non-required,
      non-classifier job; any matrix is provably non-empty;
  (c) the classifier it reads fails OPEN: run on push, merge_group, a PR with no
      base, a failed diff and an empty diff, every output it is read for is true.

The other direction -- that this file still matches the real branch protection --
needs admin rights the default GITHUB_TOKEN does not have.  Pass --protection FILE
with the API's JSON to check it; without it, that direction is reported as
UNGUARDED rather than silently assumed.
"""
import argparse, json, os, pathlib, re, shutil, subprocess, sys, tempfile

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

    # The merge queue runs required checks on `merge_group`, not `pull_request`.
    # merge_group's only filter is `types:`, and its only activity type is
    # checks_requested, so a bare key or that one type is the whole trigger.
    if "merge_group" not in on:
        return ("NO_MERGE_GROUP", "no merge_group trigger -- never reports in the merge queue")
    mg = on["merge_group"]
    if isinstance(mg, dict) and mg.get("types") is not None \
            and "checks_requested" not in mg["types"]:
        return ("NO_MERGE_GROUP", f"merge_group types={mg['types']} drops checks_requested")

    return ("OK", None)


def index_jobs(wfdir):
    """job display name -> [(workflow filename, trigger state, detail, workflow, job id)]"""
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
            idx.setdefault(name, []).append((path.name, state, detail, wf, jid))
    return idx

# --- (b) a required job may be skipped ONLY by the change classifier ---------
#
# A job skipped by its `if:` reports "skipped", which branch protection counts as
# passing (GitHub docs, "Using conditions to control job execution": "A job that
# is skipped will report its status as Success. It will not prevent a pull
# request from merging, even if it is a required check."). That is what lets the
# heavy jobs skip on a PR that does not touch their inputs -- and it is also
# what makes ANY other `if:` on a required job a silent bypass. So the only
# admissible `if:` on a required job is
#
#     !cancelled() && needs.<C>.outputs.<o> != 'false' [ || ... ]
#
# where <C> is a job in the same workflow that runs tools/ci/classify-changes.py
# and is in the job's `needs:`. `!= 'false'` (not `== 'true'`) and `!cancelled()`
# (not the implicit success()) mean a classifier that failed, wrote nothing, or
# was skipped RUNS the job instead of skipping it.

CLASSIFIER = "tools/ci/classify-changes.py"
ATOM = re.compile(r"^needs\.([A-Za-z0-9_-]+)\.outputs\.([A-Za-z0-9_-]+)\s*!=\s*'false'$")


def classifier_jobs(wf):
    """{job id: (script path, {output names wired to the classifier step})}"""
    out = {}
    for jid, job in (wf.get("jobs") or {}).items():
        job = job or {}
        for st in job.get("steps") or []:
            run = (st or {}).get("run") or ""
            m = re.search(r"\bpython3\s+(\S*classify-changes\.py)\b", run)
            if not m:
                continue
            sid = (st or {}).get("id")
            outs = set()
            for k, v in (job.get("outputs") or {}).items():
                if sid and re.fullmatch(r"\$\{\{\s*steps\." + re.escape(sid) + r"\.outputs\." + re.escape(k) + r"\s*\}\}", str(v).strip()):
                    outs.add(k)
            out[jid] = (m.group(1), outs)
    return out


def split_top(expr, op):
    parts, depth, cur, i = [], 0, "", 0
    while i < len(expr):
        if expr[i] == "(":
            depth += 1
        elif expr[i] == ")":
            depth -= 1
        if depth == 0 and expr.startswith(op, i):
            parts.append(cur.strip()); cur = ""; i += len(op); continue
        cur += expr[i]; i += 1
    parts.append(cur.strip())
    return parts


def unparen(e):
    e = e.strip()
    while e.startswith("(") and e.endswith(")") and split_top(e[1:-1], "&&") is not None:
        inner, depth, ok = e[1:-1], 0, True
        for ch in inner:
            depth += ch == "("; depth -= ch == ")"
            if depth < 0:
                ok = False; break
        if not ok:
            break
        e = inner.strip()
    return e


def job_skip_state(wf, jid, required_jids):
    """(state, detail): can this required job be skipped by anything but the classifier?"""
    job = (wf.get("jobs") or {}).get(jid) or {}
    cls = classifier_jobs(wf)
    needs = job.get("needs") or []
    needs = [needs] if isinstance(needs, str) else list(needs)
    cond = job.get("if")
    used = []   # (classifier job, output) the if reads

    if cond is not None:
        if isinstance(cond, bool) or not isinstance(cond, str):
            return ("SKIPPABLE", f"`if: {cond!r}` is not a classifier condition"), used
        e = cond.strip()
        if e.startswith("${{") and e.endswith("}}"):
            e = e[3:-2].strip()
        guard, atoms = False, 0
        for conj in split_top(e, "&&"):
            conj = unparen(conj)
            if conj in ("!cancelled()", "always()"):
                guard = True
                continue
            for atom in split_top(conj, "||"):
                atom = unparen(atom)
                m = ATOM.match(atom)
                if not m:
                    return ("SKIPPABLE", f"`if:` term `{atom}` can skip it for a reason other than the "
                                         f"classifier's `needs.<job>.outputs.<x> != 'false'`"), used
                c, o = m.group(1), m.group(2)
                if c not in cls:
                    return ("SKIPPABLE", f"`if:` reads needs.{c}, which does not run {CLASSIFIER}"), used
                if c not in needs:
                    return ("SKIPPABLE", f"`if:` reads needs.{c} but the job does not need it"), used
                if o not in cls[c][1]:
                    return ("SKIPPABLE", f"`if:` reads output {o!r}, which job {c} does not wire to the classifier step"), used
                atoms += 1
                used.append((c, o))
        if not atoms:
            return ("SKIPPABLE", f"`if: {cond}` reads no classifier output"), used
        if not guard:
            return ("SKIPPABLE", f"`if:` lacks `!cancelled()`: a failed classifier would SKIP the job "
                                 f"(implicit success()) instead of running it"), used

    for n in needs:
        if n in cls:
            if cond is None:
                return ("SKIPPABLE", f"needs classifier {n} without an `if:` holding `!cancelled()`: "
                                     f"a failed classifier skips it"), used
            continue
        if n not in required_jids and cond is None:
            return ("SKIPPABLE", f"needs {n}, which is not itself required: if {n} fails this job is "
                                 f"skipped and reports success"), used
        if n not in required_jids and not re.search(r"!cancelled\(\)|always\(\)", str(cond)):
            return ("SKIPPABLE", f"needs non-required {n} without `!cancelled()`"), used

    m = (job.get("strategy") or {}).get("matrix")
    if m is not None:
        if not isinstance(m, dict) or m.get("exclude") or any(
                not isinstance(v, list) or not v for k, v in m.items() if k != "include") \
                or (set(m) == {"include"} and not m["include"]):
            return ("SKIPPABLE", "a matrix this check cannot prove non-empty (expression, `exclude:`, or empty axis)"), used
    return ("OK", None), used


# --- (c) the classifier fails OPEN -------------------------------------------
# Run the classifier the workflow actually calls, on the events and failures
# where it must run everything, and require every output a required job reads
# to come back "true".
FAIL_OPEN_SCENARIOS = [
    ("push to main", {"EVENT_NAME": "push", "BASE_SHA": ""}),
    ("merge_group", {"EVENT_NAME": "merge_group", "BASE_SHA": "HEAD"}),
    ("pull_request, no base sha", {"EVENT_NAME": "pull_request", "BASE_SHA": ""}),
    ("pull_request, diff fails", {"EVENT_NAME": "pull_request", "BASE_SHA": "0" * 40}),
    ("pull_request, empty diff", {"EVENT_NAME": "pull_request", "BASE_SHA": "HEAD"}),
]


def classifier_fails_open(root, script, outputs):
    """[problems]; empty when every scenario writes every output as true."""
    probs = []
    path = pathlib.Path(root) / script
    if not path.is_file():
        return [f"{script} does not exist"]
    for label, extra in FAIL_OPEN_SCENARIOS:
        with tempfile.TemporaryDirectory() as td:
            out = pathlib.Path(td) / "out"
            out.write_text("")
            env = {"PATH": os.environ.get("PATH", "/usr/bin:/bin"), "HOME": td,
                   "GITHUB_OUTPUT": str(out), **extra}
            r = subprocess.run([sys.executable, str(path)], cwd=root, env=env,
                               capture_output=True, text=True, timeout=300)
            got = {}
            for line in out.read_text().splitlines():
                if "=" in line:
                    k, v = line.split("=", 1)
                    got[k.strip()] = v.strip()
            if r.returncode != 0:
                probs.append(f"{label}: classifier exited {r.returncode}")
            wrong = sorted(o for o in outputs if got.get(o) != "true")
            if wrong:
                probs.append(f"{label}: output(s) {wrong} not 'true' ({ {o: got.get(o) for o in wrong} })")
    return probs



def check(listfile, wfdir, protection=None, root="."):
    contexts = [l.strip() for l in pathlib.Path(listfile).read_text().splitlines() if l.strip()]
    idx = index_jobs(wfdir)
    bad = []

    print(f"  {len(contexts)} required context(s) against {wfdir}\n")
    # job ids, per workflow file, that host a required context (a need on one of
    # these is visible: if it fails, its own required check is red)
    required_jids, classifier_outputs, classifier_where = {}, {}, {}
    for ctx in contexts:
        for fn, _, _, _, jid in idx.get(ctx) or idx.get(re.sub(r"\s*\(.*\)\s*$", "", ctx)) or []:
            required_jids.setdefault(fn, set()).add(jid)
    for ctx in contexts:
        # A matrix job's check is "Name (v1, v2)"; the job itself is named "Name".
        hits = idx.get(ctx) or idx.get(re.sub(r"\s*\(.*\)\s*$", "", ctx)) or []
        if not hits:
            bad.append((ctx, "UNMAPPED", "no job in any workflow produces this name"))
            print(f"    UNMAPPED       {ctx}")
            continue
        for fn, state, detail, wf, jid in hits:
            if state == "OK":
                (state, detail), used = job_skip_state(wf, jid, required_jids.get(fn, set()))
                for c, o in used:
                    script = classifier_jobs(wf)[c][0]
                    classifier_outputs.setdefault(script, set()).add(o)
                    classifier_where.setdefault(script, set()).add(f"{fn}:{c}")
            if state == "OK":
                print(f"    ok             {ctx}   [{fn}]")
            else:
                bad.append((ctx, state, f"{fn}: {detail}"))
                print(f"    {state:<14} {ctx}   [{fn}] -- {detail}")

    if classifier_outputs:
        print()
        for script, outs in sorted(classifier_outputs.items()):
            probs = classifier_fails_open(root, script, outs)
            where = ", ".join(sorted(classifier_where[script]))
            if probs:
                for p in probs:
                    bad.append((script, "FAILS_CLOSED", p))
                    print(f"    FAILS_CLOSED   {script} [{where}] -- {p}")
            else:
                print(f"    fails open     {script}: {len(FAIL_OPEN_SCENARIOS)} scenario(s) x "
                      f"{len(outs)} output(s) {sorted(outs)} all 'true'  [{where}]")

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
        print(f"  FAIL: {len(bad)} problem(s): a required context may not be produced, or may be skipped by something other than the change classifier")
        return 1
    print(f"  PASS: every required context maps to a job that reports on every PR and in the merge queue,")
    print(f"        and can be skipped only by a change classifier that fails open")
    return 0


# --- negative control -------------------------------------------------------
# The gate has to be shown to DISCRIMINATE, not merely to run.  Each planted
# workflow below is the real shape of a way a required context stops arriving.
# Every plant but the merge-queue ones carries a correct `merge_group:`, so each
# is caught for its OWN reason rather than for the missing queue trigger.
MG = "  merge_group:\n"
PLANTS = {
    "dangling-paths": ("the gale#340 shape: a `paths:` key with nothing under it",
                       "on:\n  pull_request:\n    branches: [main]\n    paths:\n" + MG),
    "real-paths":     ("an ordinary path filter",
                       "on:\n  pull_request:\n    branches: [main]\n    paths: ['src/**']\n" + MG),
    "no-pr":          ("push-only, so no PR ever gets a check",
                       "on:\n  push:\n    branches: [main]\n" + MG),
    "types":          ("`types:` that drops synchronize, so pushes to the PR do not re-report",
                       "on:\n  pull_request:\n    branches: [main]\n    types: [opened]\n" + MG),
    "branch":         ("a branches list that excludes main",
                       "on:\n  pull_request:\n    branches: [develop]\n" + MG),
    "no-merge-group": ("PR-only, so the merge queue waits on a check that never arrives",
                       "on:\n  pull_request:\n    branches: [main]\n"),
    "mg-types":       ("a merge_group `types:` that drops checks_requested",
                       "on:\n  pull_request:\n    branches: [main]\n  merge_group:\n    types: [destroyed]\n"),
}


DUMP = None   # --dump-plants DIR: keep each planted fixture, to run another checker on it


def _dump(key, wfd, lst, repo=None):
    if DUMP is None:
        return
    d = pathlib.Path(DUMP) / key
    shutil.copytree(wfd, d / "workflows", dirs_exist_ok=True)
    shutil.copy(lst, d / "list.txt")
    if repo is not None:
        (d / "root").write_text(str(pathlib.Path(DUMP) / "repo"))


def dump_plants(dirname):
    global DUMP
    DUMP = dirname
    pathlib.Path(dirname).mkdir(parents=True, exist_ok=True)
    return self_test()


def self_test():
    ok = True
    with tempfile.TemporaryDirectory() as td:
        td = pathlib.Path(td)
        wfd = td / "workflows"; wfd.mkdir()
        lst = td / "list.txt"
        lst.write_text("The Gate\n")

        clean = "name: clean\non:\n  pull_request:\n    branches: [main]\n  merge_group:\n"
        job = "jobs:\n  g:\n    name: \"The Gate\"\n    runs-on: ubuntu-24.04\n    steps: [{run: 'true'}]\n"

        print("  negative control -- each planted workflow MUST make the gate fail\n")
        for key, (why, trig) in PLANTS.items():
            (wfd / "w.yml").write_text(f"name: {key}\n{trig}{job}")
            _dump(key, wfd, lst)
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

        # (b)/(c): a required job may be skipped ONLY by a fail-open classifier.
        repo = td / "repo"
        (repo / "tools/ci").mkdir(parents=True)
        real = pathlib.Path(__file__).resolve().parent / "ci" / "classify-changes.py"
        shutil.copy(real, repo / CLASSIFIER)
        bad_cls = repo / "tools/ci/bad-classify-changes.py"
        # fails CLOSED on a diff error: the shape (c) exists to catch
        bad_cls.write_text(
            "import os, subprocess\n"
            "out = open(os.environ['GITHUB_OUTPUT'], 'a')\n"
            "ev, base = os.environ.get('EVENT_NAME'), os.environ.get('BASE_SHA', '')\n"
            "r = subprocess.run(['git', 'diff', '--name-only', base + '...HEAD'], capture_output=True)\n"
            "v = 'true' if ev != 'pull_request' or (r.returncode == 0 and r.stdout.strip()) else 'false'\n"
            "out.write(f'zephyr={v}\\n')\n")
        push_cls = repo / "tools/ci/push-classify-changes.py"
        # answers false on push to main
        push_cls.write_text("import os\nopen(os.environ['GITHUB_OUTPUT'], 'a').write("
                            "'zephyr=' + ('false' if os.environ.get('EVENT_NAME') == 'push' else 'true') + '\\n')\n")
        for a_ in (["init", "-q"], ["add", "-A"],
                   ["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false",
                    "commit", "-q", "-m", "fixture"]):
            subprocess.run(["git", *a_], cwd=repo, check=True, capture_output=True)
        if DUMP is not None:
            shutil.copytree(repo, pathlib.Path(DUMP) / "repo", dirs_exist_ok=True)

        def wf(gate_if, needs="changes", script=CLASSIFIER, extra="", gate_name="The Gate", outputs="zephyr"):
            y = (f"{clean}jobs:\n"
                 "  changes:\n    runs-on: ubuntu-24.04\n    outputs:\n"
                 + "".join(f"      {o}: ${{{{ steps.classify.outputs.{o} }}}}\n" for o in outputs.split())
                 + f"    steps:\n      - id: classify\n        run: python3 {script}\n"
                 "  other:\n    runs-on: ubuntu-24.04\n    outputs:\n      x: ${{ steps.s.outputs.x }}\n"
                 "    steps:\n      - id: s\n        run: echo x=false >> $GITHUB_OUTPUT\n"
                 f"  g:\n    name: \"{gate_name}\"\n")
            if needs:
                y += f"    needs: {needs}\n"
            if gate_if is not None:
                y += f"    if: {gate_if}\n"
            return y + f"    runs-on: ubuntu-24.04\n{extra}    steps: [{{run: 'true'}}]\n"

        GOOD = "${{ !cancelled() && needs.changes.outputs.zephyr != 'false' }}"
        SKIP_PLANTS = {
            "if-false": ("a required job with `if: false`", wf("false")),
            "if-event": ("skipped by event type, not by the classifier",
                         wf("${{ !cancelled() && github.event_name != 'merge_group' && needs.changes.outputs.zephyr != 'false' }}")),
            "eq-true": ("`== 'true'`: an empty (crashed) classifier output SKIPS",
                        wf("${{ !cancelled() && needs.changes.outputs.zephyr == 'true' }}")),
            "no-cancelled": ("no `!cancelled()`: a failed classifier skips via implicit success()",
                             wf("needs.changes.outputs.zephyr != 'false'")),
            "not-classifier": ("`if:` reads a job that does not run the classifier",
                               wf("${{ !cancelled() && needs.other.outputs.x != 'false' }}", needs="[changes, other]")),
            "unwired-output": ("`if:` reads an output the classifier job does not wire",
                               wf("${{ !cancelled() && needs.changes.outputs.proofs != 'false' }}")),
            "needs-nonrequired": ("needs a non-required job, no `if:`: its failure skips this one",
                                  wf(None, needs="other")),
            "needs-classifier-no-if": ("needs the classifier with no `if:`", wf(None)),
            "matrix-exclude": ("a matrix whose `exclude:` can remove the job",
                               wf(GOOD, extra="    strategy:\n      matrix:\n        a: [1]\n        exclude:\n          - a: 1\n")),
            "classifier-fails-closed": ("(c) a classifier whose diff-failure path writes false",
                                        wf(GOOD, script="tools/ci/bad-classify-changes.py")),
            "classifier-push-false": ("(c) a classifier that answers false on push to main",
                                      wf(GOOD, script="tools/ci/push-classify-changes.py")),
            "renamed": ("the required job renamed", wf(GOOD, gate_name="The Gate (renamed)")),
        }
        print("  negative control -- each required job that can be skipped by something other"
              " than a fail-open classifier MUST make the gate fail\n")
        for key, (why, y) in SKIP_PLANTS.items():
            (wfd / "w.yml").write_text(y)
            _dump(key, wfd, lst, repo)
            rc = check(lst, wfd, root=repo)
            print(f"  --> {'caught' if rc else 'MISSED'}: {key} ({why})\n")
            if rc == 0:
                ok = False

        (wfd / "w.yml").write_text(wf(GOOD))
        rc = check(lst, wfd, root=repo)
        print(f"  --> {'ok' if rc == 0 else 'BROKEN'}: a required job gated by the real classifier passes\n")
        if rc != 0:
            ok = False
        (wfd / "w.yml").write_text(wf(None, needs=None))
        rc = check(lst, wfd, root=repo)
        print(f"  --> {'ok' if rc == 0 else 'BROKEN'}: a required job with no `if:` passes\n")
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
    ap.add_argument("--dump-plants", metavar="DIR", help=argparse.SUPPRESS)
    a = ap.parse_args()
    if a.dump_plants:
        return dump_plants(a.dump_plants)
    return self_test() if a.self_test else check(a.list, a.workflows, a.protection)


if __name__ == "__main__":
    sys.exit(main())

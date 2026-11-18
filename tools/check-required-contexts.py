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
also checks the ways a required job can be skipped other than through that
classifier (see the (b) block below for the full list):
  (b) its `if:` is absent, or exactly
      `!cancelled() && (needs.<C>.result != 'success' || needs.<C>.outputs.<o> != 'false')`
      with <C> the classifier job, wired exactly, and <o> one of the outputs the
      HAND-MAINTAINED GATE_OUTPUTS map gives that job; no step-level `if:` on
      needs/step outputs; no unobserved `continue-on-error`; no need that can be
      skipped without the classifier form; any matrix provably non-empty;
  (c) the classifier it reads fails OPEN: run on push, merge_group, a PR with no
      base, a failed diff and an empty diff, every output it is read for is true.
What this does NOT prove: that GATE_OUTPUTS maps each job to the output that
really covers its inputs, or that the classifier's input sets are complete.
Those are the classifier's own false-skip controls.

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
# what makes ANY other skip path on a required job a silent bypass. Checked:
#
#  - job `if:`: absent, or EXACTLY
#        !cancelled() && (needs.<C>.result != 'success' || needs.<C>.outputs.<o> != 'false' [|| ...])
#    <C> runs the classifier and is in `needs:`; <o> is wired to the classifier
#    step. `result != 'success'` makes a classifier job that wrote "false" and
#    then failed in a later step RUN the job; `!= 'false'` makes a crashed
#    classifier that wrote nothing run it; `!cancelled()` stops a failed
#    classifier from skipping it through the implicit success().
#  - which <o> a job may read: GATE_OUTPUTS below. That map is HAND-MAINTAINED
#    (it says which classifier output covers which job's inputs); this check
#    proves a job reads only its own outputs, not that the map is right. The
#    classifier's false-skip controls are what test the map's content.
#  - `needs:`: only classifier jobs and other required jobs; and if any need has
#    an `if:` of its own (so it can be skipped), this job must carry the
#    classifier form, or the skip propagates through the implicit success().
#  - no step `if:` reading `needs.*` or `steps.*.outputs` (a step-level skip
#    turns the job green having checked nothing -- the gale#319 pattern).
#  - no `continue-on-error` on the job; on a step only if a later step reads
#    that step's `steps.<id>.outcome`/`conclusion`, or the step is on
#    COE_EXEMPT (hand-maintained, with the reason).
#  - a matrix this check can prove non-empty.
#  - the classifier step: `run:` exactly CLASSIFIER_RUN, `env:` exactly
#    CLASSIFIER_ENV, no `if:`/`continue-on-error` on step or job, and no
#    EVENT_NAME/BASE_SHA/GATE_PROBE_FILES in job- or workflow-level env.

CLASSIFIER = "tools/ci/classify-changes.py"
CLASSIFIER_RUN = f"python3 {CLASSIFIER}"
CLASSIFIER_ENV = {"EVENT_NAME": "${{ github.event_name }}",
                  "BASE_SHA": "${{ github.event.pull_request.base.sha }}"}
CLASSIFIER_ENV_KEYS = {"EVENT_NAME", "BASE_SHA", "GATE_PROBE_FILES", "GITHUB_EVENT_NAME"}
# HAND-MAINTAINED: required job display name -> the classifier outputs it may read.
GATE_OUTPUTS = {
    "Rust Coverage": {"rust_cov"},
    "Zephyr C Coverage": {"zephyr"},
    "LLVM LTO build + size comparison": {"zephyr"},
    "Binary size comparison": {"zephyr"},
    "gust dissolved on Renode (M3: kernel 8K / control 64K / F100)": {"gust_renode"},
    "seam survives lowering on ARM and RISC-V (all thin drivers)": {"cross_arch"},
    "Verus SMT (39 modules)": {"proofs"},
    "Kani BMC (185 harnesses)": {"proofs"},
    "Rocq Proofs (14 files)": {"proofs"},
    "Lean Proofs": {"proofs"},
}
# HAND-MAINTAINED: (workflow file, job id, step name) whose continue-on-error is
# allowed although nothing reads its outcome. Each publishes a trend AFTER the
# job's verdict is computed; it decides nothing.
COE_EXEMPT = {
    ("bazel-tests.yml", "feature-sizes", "Track feature sizes over time"):
        "publishes the size trend (benchmark-action); the gate's verdict is an earlier step",
    ("zephyr-tests.yml", "size-comparison", "Track binary size over time"):
        "publishes the size trend (benchmark-action); the gate's verdict is an earlier step",
}
NEEDS_RESULT = re.compile(r"^needs\.([A-Za-z0-9_-]+)\.result\s*!=\s*'success'$")
ATOM = re.compile(r"^needs\.([A-Za-z0-9_-]+)\.outputs\.([A-Za-z0-9_-]+)\s*!=\s*'false'$")


def classifier_jobs(wf):
    """{job id: (step, {output names wired to that step})} for every job with a
    step that mentions the classifier (exactness is checked separately)."""
    out = {}
    for jid, job in (wf.get("jobs") or {}).items():
        job = job or {}
        for st in job.get("steps") or []:
            st = st or {}
            if CLASSIFIER not in str(st.get("run") or ""):
                continue
            sid = st.get("id")
            outs = set()
            for k, v in (job.get("outputs") or {}).items():
                if sid and re.fullmatch(r"\$\{\{\s*steps\." + re.escape(sid) + r"\.outputs\." + re.escape(k) + r"\s*\}\}", str(v).strip()):
                    outs.add(k)
            out[jid] = (st, outs)
    return out


def classifier_wiring(wf, cjid):
    """[problems] with how classifier job `cjid` runs the classifier."""
    job = (wf.get("jobs") or {}).get(cjid) or {}
    probs = []
    steps = [st for st in job.get("steps") or [] if CLASSIFIER in str((st or {}).get("run") or "")]
    if len(steps) != 1:
        return [f"{cjid}: {len(steps)} steps mention {CLASSIFIER}; exactly one must run it"]
    st = steps[0]
    if str(st.get("run") or "").strip() != CLASSIFIER_RUN:
        probs.append(f"{cjid}: classifier step `run:` is {str(st.get('run')).strip()!r}, not exactly {CLASSIFIER_RUN!r}")
    env = {k: str(v).strip() for k, v in (st.get("env") or {}).items()}
    if env != CLASSIFIER_ENV:
        probs.append(f"{cjid}: classifier step `env:` is {env}, not exactly {CLASSIFIER_ENV}")
    for k in ("if", "continue-on-error"):
        if k in st:
            probs.append(f"{cjid}: classifier step has `{k}:`")
        if k in job:
            probs.append(f"{cjid}: classifier job has `{k}:`")
    for where, e in (("job", job.get("env") or {}), ("workflow", wf.get("env") or {})):
        hit = CLASSIFIER_ENV_KEYS & set(e)
        if hit:
            probs.append(f"{cjid}: {where}-level env sets {sorted(hit)}, which the classifier reads")
    if not st.get("id"):
        probs.append(f"{cjid}: classifier step has no id, so no output can be wired to it")
    return probs


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


def strip_parens(e):
    """Remove ONE pair of parentheses enclosing the whole expression, if any."""
    e = e.strip()
    if not (e.startswith("(") and e.endswith(")")):
        return e, False
    depth = 0
    for i, ch in enumerate(e):
        depth += ch == "("; depth -= ch == ")"
        if depth == 0 and i < len(e) - 1:
            return e, False
    return e[1:-1].strip(), True


def parse_gate_if(cond):
    """(classifier job, [outputs]) if `cond` is exactly the accepted form, else (None, why)."""
    e = str(cond).strip()
    if e.startswith("${{") and e.endswith("}}"):
        e = e[3:-2].strip()
    conj = split_top(e, "&&")
    if len(conj) != 2 or conj[0] != "!cancelled()":
        return None, (f"`if: {cond}` is not `!cancelled() && (needs.<C>.result != 'success' || "
                      f"needs.<C>.outputs.<o> != 'false')`")
    inner, had = strip_parens(conj[1])
    if not had:
        return None, f"`if:` second term `{conj[1]}` is not the parenthesised classifier disjunction"
    terms = [strip_parens(t)[0] for t in split_top(inner, "||")]
    res = [NEEDS_RESULT.match(t) for t in terms]
    atoms = [ATOM.match(t) for t in terms]
    if sum(1 for m in res if m) != 1:
        return None, ("`if:` lacks exactly one `needs.<C>.result != 'success'`: a classifier that "
                      "wrote false and then failed would skip the job")
    for t, r, a in zip(terms, res, atoms):
        if not (r or a):
            return None, (f"`if:` term `{t}` can skip it for a reason other than the classifier")
    cs = {m.group(1) for m in res + atoms if m}
    if len(cs) != 1:
        return None, f"`if:` mixes classifier jobs {sorted(cs)}"
    outs = [a.group(2) for a in atoms if a]
    if not outs:
        return None, "`if:` reads no classifier output"
    return cs.pop(), outs


def job_skip_state(wf, wfname, jid, required_jids):
    """(state, detail), [(classifier job, output)] for one required job."""
    job = (wf.get("jobs") or {}).get(jid) or {}
    jobs = wf.get("jobs") or {}
    name = job.get("name") or jid
    cls = classifier_jobs(wf)
    needs = job.get("needs") or []
    needs = [needs] if isinstance(needs, str) else list(needs)
    cond = job.get("if")
    used = []

    if "continue-on-error" in job:
        return ("SKIPPABLE", "`continue-on-error` on a required job: it reports success whatever fails"), used
    steps = job.get("steps") or []
    for i, st in enumerate(steps):
        st = st or {}
        sif = str(st.get("if", ""))
        if re.search(r"\bneeds\.|\bsteps\.[A-Za-z0-9_-]+\.outputs\b", sif):
            return ("SKIPPABLE", f"step {st.get('name') or i} has `if: {sif}`: a step-level skip reports "
                                 f"the job green having checked nothing"), used
        if "continue-on-error" in st:
            sid = st.get("id")
            later = json.dumps(steps[i + 1:])
            observed = bool(sid) and re.search(r"steps\." + re.escape(sid) + r"\.(outcome|conclusion)", later)
            if not observed and (wfname, jid, st.get("name")) not in COE_EXEMPT:
                return ("SKIPPABLE", f"step {st.get('name') or i} has `continue-on-error` and no later step "
                                     f"reads its outcome (nor is it on COE_EXEMPT)"), used

    if cond is not None:
        if isinstance(cond, bool) or not isinstance(cond, str):
            return ("SKIPPABLE", f"`if: {cond!r}` is not the classifier form"), used
        c, outs = parse_gate_if(cond)
        if c is None:
            return ("SKIPPABLE", outs), used
        if c not in cls:
            return ("SKIPPABLE", f"`if:` reads needs.{c}, which does not run {CLASSIFIER}"), used
        if c not in needs:
            return ("SKIPPABLE", f"`if:` reads needs.{c} but the job does not need it"), used
        probs = classifier_wiring(wf, c)
        if probs:
            return ("SKIPPABLE", "; ".join(probs)), used
        for o in outs:
            if o not in cls[c][1]:
                return ("SKIPPABLE", f"`if:` reads output {o!r}, which job {c} does not wire to the classifier step"), used
        allowed = GATE_OUTPUTS.get(name)
        if allowed is None:
            return ("SKIPPABLE", f"gated by the classifier but not on the hand-maintained GATE_OUTPUTS map"), used
        stray = sorted(set(outs) - allowed)
        if stray:
            return ("SKIPPABLE", f"reads {stray}, another job's classifier output (GATE_OUTPUTS allows {sorted(allowed)})"), used
        used = [(c, o) for o in outs]

    for n in needs:
        if n in cls:
            if cond is None:
                return ("SKIPPABLE", f"needs classifier {n} without the classifier `if:`: a failed "
                                     f"classifier skips it"), used
            continue
        if n not in required_jids:
            return ("SKIPPABLE", f"needs {n}, which is not itself required: if {n} fails or skips, "
                                 f"this job is skipped and reports success"), used
        if (jobs.get(n) or {}).get("if") is not None and cond is None:
            return ("SKIPPABLE", f"needs {n}, which has its own `if:`: when {n} is skipped this job is "
                                 f"skipped too (implicit success()), unless it carries the classifier form"), used

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
                (state, detail), used = job_skip_state(wf, fn, jid, required_jids.get(fn, set()))
                for c, o in used:
                    script = CLASSIFIER
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
    print(f"        and can be skipped only through the classifier wiring this check verifies, reading")
    print(f"        the outputs the hand-maintained GATE_OUTPUTS map assigns it (the map's content is")
    print(f"        tested by the classifier's false-skip controls, not here)")
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
        (d / "root").write_text(str(pathlib.Path(DUMP) / pathlib.Path(repo).name))


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

        # (b)/(c): a required job may be skipped ONLY through the classifier.
        global GATE_OUTPUTS, COE_EXEMPT
        saved_maps = (GATE_OUTPUTS, COE_EXEMPT)
        GATE_OUTPUTS = {"The Gate": {"zephyr"}, "Other Gate": {"zephyr"}}
        COE_EXEMPT = {("w.yml", "g", "trend"): "test exemption"}

        def mkrepo(path, script_body=None):
            (path / "tools/ci").mkdir(parents=True)
            if script_body is None:
                real = pathlib.Path(__file__).resolve().parent / "ci" / "classify-changes.py"
                shutil.copy(real, path / CLASSIFIER)
            else:
                (path / CLASSIFIER).write_text(script_body)
            for a_ in (["init", "-q"], ["add", "-A"],
                       ["-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false",
                        "commit", "-q", "-m", "fixture"]):
                subprocess.run(["git", *a_], cwd=path, check=True, capture_output=True)
            return path

        repo = mkrepo(td / "repo")
        # (c) plants live at the CANONICAL path, so the wiring check passes and
        # only the fail-open run can catch them.
        repo_closed = mkrepo(td / "repo-closed",
            "import os, subprocess\n"
            "out = open(os.environ['GITHUB_OUTPUT'], 'a')\n"
            "ev, base = os.environ.get('EVENT_NAME'), os.environ.get('BASE_SHA', '')\n"
            "r = subprocess.run(['git', 'diff', '--name-only', base + '...HEAD'], capture_output=True)\n"
            "v = 'true' if ev != 'pull_request' or (r.returncode == 0 and r.stdout.strip()) else 'false'\n"
            "out.write(f'zephyr={v}\\n')\n")
        repo_push = mkrepo(td / "repo-push",
            "import os\nopen(os.environ['GITHUB_OUTPUT'], 'a').write("
            "'zephyr=' + ('false' if os.environ.get('EVENT_NAME') == 'push' else 'true') + '\\n')\n")
        if DUMP is not None:
            for r in (repo, repo_closed, repo_push):
                shutil.copytree(r, pathlib.Path(DUMP) / r.name, dirs_exist_ok=True)

        GOOD = "${{ !cancelled() && (needs.changes.result != 'success' || needs.changes.outputs.zephyr != 'false') }}"
        STEP_ENV = ("        env:\n          EVENT_NAME: ${{ github.event_name }}\n"
                    "          BASE_SHA: ${{ github.event.pull_request.base.sha }}\n")

        def wf(gate_if=GOOD, needs="changes", gate_name="The Gate", outputs="zephyr",
               run=CLASSIFIER_RUN, step_env=STEP_ENV, step_extra="", cjob_extra="", wf_extra="",
               gate_extra="", gate_steps="    steps: [{run: 'true'}]\n", other_if=None):
            y = (f"{clean}{wf_extra}jobs:\n"
                 f"  changes:\n    runs-on: ubuntu-24.04\n{cjob_extra}    outputs:\n"
                 + "".join(f"      {o}: ${{{{ steps.classify.outputs.{o} }}}}\n" for o in outputs.split())
                 + f"    steps:\n      - id: classify\n{step_extra}{step_env}        run: {run}\n"
                 "  other:\n    name: \"Other Gate\"\n    runs-on: ubuntu-24.04\n"
                 + (f"    needs: changes\n    if: {other_if}\n" if other_if else "")
                 + "    outputs:\n      x: ${{ steps.s.outputs.x }}\n"
                 "    steps:\n      - id: s\n        run: echo x=false >> $GITHUB_OUTPUT\n"
                 f"  g:\n    name: \"{gate_name}\"\n")
            if needs:
                y += f"    needs: {needs}\n"
            if gate_if is not None:
                y += f"    if: {gate_if}\n"
            return y + f"    runs-on: ubuntu-24.04\n{gate_extra}{gate_steps}"

        OLD = "${{ !cancelled() && needs.changes.outputs.zephyr != 'false' }}"
        SKIP_PLANTS = {
            # the job's own `if:`
            "if-false": ("a required job with `if: false`", wf("false"), repo, False),
            "if-event": ("skipped by event type, not by the classifier",
                         wf("${{ !cancelled() && github.event_name != 'merge_group' && (needs.changes.result != 'success' || needs.changes.outputs.zephyr != 'false') }}"), repo, False),
            "old-form": ("the pre-relay form without `needs.<C>.result != 'success'`: a classifier "
                         "that wrote false then failed skips the job", wf(OLD), repo, False),
            "no-result-term": ("parenthesised but without `needs.<C>.result != 'success'`",
                               wf("${{ !cancelled() && (needs.changes.outputs.zephyr != 'false') }}"), repo, False),
            "eq-true": ("`== 'true'`: an empty (crashed) classifier output SKIPS",
                        wf("${{ !cancelled() && (needs.changes.result != 'success' || needs.changes.outputs.zephyr == 'true') }}"), repo, False),
            "no-cancelled": ("no `!cancelled()`",
                             wf("${{ needs.changes.result != 'success' || needs.changes.outputs.zephyr != 'false' }}"), repo, False),
            "not-classifier": ("`if:` reads a job that does not run the classifier",
                               wf("${{ !cancelled() && (needs.other.result != 'success' || needs.other.outputs.x != 'false') }}",
                                  needs="[changes, other]"), repo, False),
            "unwired-output": ("`if:` reads an output wired to a step other than the classifier's",
                               wf(cjob_extra="", outputs="zephyr").replace(
                                   "zephyr: ${{ steps.classify.outputs.zephyr }}",
                                   "zephyr: ${{ steps.other.outputs.zephyr }}"), repo, False),
            "other-gate-output": ("reads another gate's output (wired, but not this job's on GATE_OUTPUTS)",
                                  wf("${{ !cancelled() && (needs.changes.result != 'success' || needs.changes.outputs.proofs != 'false') }}",
                                     outputs="zephyr proofs"), repo, False),
            # needs
            "needs-nonrequired": ("needs a non-required job", wf(None, needs="other"), repo, False),
            "needs-classifier-no-if": ("needs the classifier with no `if:`", wf(None), repo, False),
            "needs-gated": ("no `if:` but needs a required job that is itself classifier-gated "
                            "(rust-coverage needs zephyr-coverage)", wf(None, needs="other", other_if=GOOD), repo, True),
            # steps
            "step-if-needs": ("a step-level `if:` on needs.*.outputs (the base pattern)",
                              wf(None, needs=None, gate_steps="    steps:\n      - if: needs.changes.outputs.zephyr == 'true'\n        run: 'true'\n"), repo, False),
            "step-if-steps": ("a step-level `if:` on steps.*.outputs",
                              wf(None, needs=None, gate_steps="    steps:\n      - id: q\n        run: echo run=false >> $GITHUB_OUTPUT\n"
                                                             "      - if: steps.q.outputs.run == 'true'\n        run: 'true'\n"), repo, False),
            "coe-job": ("`continue-on-error` on the required job",
                        wf(None, needs=None, gate_extra="    continue-on-error: true\n"), repo, False),
            "coe-step": ("`continue-on-error` on a step nothing observes",
                         wf(None, needs=None, gate_steps="    steps:\n      - id: v\n        continue-on-error: true\n        run: 'false'\n"), repo, False),
            # matrix
            "matrix-exclude": ("a matrix whose `exclude:` can remove the job",
                               wf(gate_extra="    strategy:\n      matrix:\n        a: [1]\n        exclude:\n          - a: 1\n"), repo, False),
            # classifier wiring
            "run-appended": ("classifier `run:` with an appended `echo zephyr=false`",
                             wf(run=f"{CLASSIFIER_RUN} && echo zephyr=false >> $GITHUB_OUTPUT"), repo, False),
            "env-hardcoded-event": ("classifier EVENT_NAME hard-coded",
                                    wf(step_env="        env:\n          EVENT_NAME: pull_request\n"
                                                "          BASE_SHA: ${{ github.event.pull_request.base.sha }}\n"), repo, False),
            "env-gate-probe": ("GATE_PROBE_FILES set on the classify step",
                               wf(step_env=STEP_ENV + "          GATE_PROBE_FILES: docs.txt\n"), repo, False),
            "job-env-gate-probe": ("GATE_PROBE_FILES set in the classifier job's env",
                                   wf(cjob_extra="    env:\n      GATE_PROBE_FILES: docs.txt\n"), repo, False),
            "wf-env-event": ("EVENT_NAME set in workflow-level env",
                             wf(wf_extra="env:\n  EVENT_NAME: pull_request\n"), repo, False),
            "classify-step-if": ("an `if:` on the classify step",
                                 wf(step_extra="        if: github.event_name == 'pull_request'\n"), repo, False),
            "classify-step-coe": ("`continue-on-error` on the classify step",
                                  wf(step_extra="        continue-on-error: true\n"), repo, False),
            # (c)
            "classifier-fails-closed": ("(c) a classifier whose diff-failure path writes false", wf(), repo_closed, False),
            "classifier-push-false": ("(c) a classifier that answers false on push to main", wf(), repo_push, False),
            "renamed": ("the required job renamed", wf(gate_name="The Gate (renamed)"), repo, False),
        }
        print("  negative control -- each required job that can be skipped other than through the"
              " classifier wiring MUST make the gate fail\n")
        for key, (why, y, root_, both) in SKIP_PLANTS.items():
            (wfd / "w.yml").write_text(y)
            lst.write_text("The Gate\nOther Gate\n" if both else "The Gate\n")
            _dump(key, wfd, lst, root_)
            rc = check(lst, wfd, root=root_)
            print(f"  --> {'caught' if rc else 'MISSED'}: {key} ({why})\n")
            if rc == 0:
                ok = False

        lst.write_text("The Gate\n")
        POSITIVE = {
            "the classifier form passes": wf(),
            "two outputs of the same classifier pass (both on GATE_OUTPUTS)": None,
            "a required job with no `if:` passes": wf(None, needs=None),
            "an observed continue-on-error step passes":
                wf(None, needs=None, gate_steps="    steps:\n      - id: v\n        continue-on-error: true\n        run: 'true'\n"
                                                "      - run: test \"${{ steps.v.outcome }}\" = success\n"),
            "an exempt continue-on-error step passes":
                wf(None, needs=None, gate_steps="    steps:\n      - name: trend\n        continue-on-error: true\n        run: 'true'\n"),
        }
        GATE_OUTPUTS["The Gate"] = {"zephyr", "proofs"}
        POSITIVE["two outputs of the same classifier pass (both on GATE_OUTPUTS)"] = wf(
            "${{ !cancelled() && (needs.changes.result != 'success' || needs.changes.outputs.zephyr != 'false' || needs.changes.outputs.proofs != 'false') }}",
            outputs="zephyr proofs")
        for why, y in POSITIVE.items():
            (wfd / "w.yml").write_text(y)
            rc = check(lst, wfd, root=repo)
            print(f"  --> {'ok' if rc == 0 else 'BROKEN'}: {why}\n")
            if rc != 0:
                ok = False
        GATE_OUTPUTS, COE_EXEMPT = saved_maps

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

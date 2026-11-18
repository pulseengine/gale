#!/usr/bin/env python3
"""Change classifier for the heavy CI jobs: which of them does this PR need?

gale's PRs queued about 100 jobs each (2026-11-18); most of that is the Zephyr /
QEMU matrix, coverage, LLVM LTO, Renode and the proof runs, and most PRs touch
none of their inputs. A heavy job skips by a JOB-LEVEL `if:` on an output of this
script. A job skipped that way still reports its check (skipped counts as
success), which a workflow-level `paths:` would not (gale#294).

Two ways to get this wrong, and both are gated:

  1. fail CLOSED by accident (an error that skips a job). So every error path
     answers "run everything": a non-pull_request event (push to main,
     merge_group, schedule, dispatch), no base sha, a failed or empty diff, no
     tomllib, an unparseable manifest. Outputs are written only as "true" or
     "false", and the jobs test `!= 'false'`, so a classifier that crashed
     before writing anything also runs everything.

  2. decide "not affected" when a job's real inputs changed (a missed path, a
     new crate, a file pulled in by `#[path]`). So a job's inputs are DERIVED,
     live, from the build graph at the PR head wherever one exists:
       - Cargo: each job's root manifests, every `path =` dependency
         transitively, every target's source, every `#[path]`, `include!`,
         `include_str!`, `include_bytes!` and `wit_bindgen::generate!(path:)`
         that leaves the crate, and every path literal in a build script;
       - Zephyr: zephyr/module.yml's cmake and kconfig entries, and every
         ${GALE_DIR}/... the module's CMake reads;
       - Bazel: tools/ci/inputs-bazel.json, produced by `bazel query` (see
         --derive-bazel; querying needs the Lean/Rocq toolchains, too heavy for
         a classifier). Its fingerprint of the BUILD/.bzl/MODULE files must
         match the tree (--check-graph), and any change to those files runs
         everything, so a stale snapshot cannot skip a job.
     Only where no graph exists is a path hand-listed (HAND below, each entry
     marked). A path claimed by no job and not on the narrow inert list -- a
     new top-level directory, a new script -- runs EVERYTHING.

Outputs (to $GITHUB_OUTPUT), all "true"/"false":
  gates (what job-level `if:`s read):  zephyr, zephyr_renode, rust_cov, proofs,
                                       gust_renode, cross_arch
  classes (informational, per input):  ci, docs, artifacts, kernel, zephyr_src,
                                       proofs_src, drivers, gust, wit, targets,
                                       unknown, docs_only, artifacts_only

Inputs (env): EVENT_NAME, BASE_SHA. GATE_PROBE_FILES (temper's gate probe): a
file listing changed paths; it replaces the event and the diff.

Modes:
  (default)       classify this change and write the outputs
  --files P...    classify the given paths (as a pull_request) and print
  --self-test     planted-path unit tests on a fixture repo; must all pass
  --check-graph   fast-tier controls on the real tree (cargo metadata agrees
                  with the live walk, every graph input selects its job, named
                  true inputs select their job, docs/artifacts select nothing,
                  the Bazel snapshot is current, inert paths are not inputs)
  --derive-bazel  rerun `bazel query` and rewrite tools/ci/inputs-bazel.json
"""
import glob as globmod
import hashlib
import json
import os
import pathlib
import re
import subprocess
import sys
import tempfile

try:
    import tomllib
except ImportError:  # python < 3.11: the classifier cannot read the graph -> run everything
    tomllib = None

# ── configuration ─────────────────────────────────────────────────────────────

# Any change here could change what every job builds or how CI itself runs.
RUN_ALL_PREFIXES = (".github/", "tools/", ".cargo/", "bzl/")
RUN_ALL_FILES = {"varve.toml", "varve-realms.toml", "Cargo.lock", "rust-toolchain.toml",
                 "rust-toolchain", ".bazelrc", ".bazelversion", ".bazelignore",
                 "MODULE.bazel", "MODULE.bazel.lock", ".gitmodules"}
# ...wherever they live: a build-system file anywhere can re-route any job.
RUN_ALL_BASENAMES = {"BUILD.bazel", "BUILD", "MODULE.bazel", "MODULE.bazel.lock",
                     "Cargo.lock", "rust-toolchain.toml", "rust-toolchain", "WORKSPACE",
                     "WORKSPACE.bazel", ".bazelversion"}
RUN_ALL_SUFFIXES = (".bzl",)

# HAND-MAINTAINED: read only by rivet and the always-run fast gates (rivet
# V-closure, kill-criteria), never by a heavy job. --check-graph fails if any
# graph-derived input of any heavy job lands under one of these.
INERT_PREFIXES = ("artifacts/", "schemas/", ".rivet/", "upstream-reqif/")
INERT_FILES = {"rivet.yaml"}

# The heavy jobs, by the output their `if:` reads.
#   cargo:  manifests the job builds as ROOTS (all targets + dev-deps)
#   cargo_glob: same, discovered the way the job's own script discovers them
#   zephyr: west module manifests the job builds
#   bazel:  key into tools/ci/inputs-bazel.json
#   hand:   HAND-MAINTAINED paths with no build graph (each says why)
GATES = {
    "zephyr": {   # zephyr-tests (3 matrices + Binary size comparison), coverage (Zephyr C), llvm-lto
        "cargo": ["ffi/Cargo.toml"],
        "zephyr": ["zephyr/module.yml"],
        "hand": [],
    },
    "zephyr_renode": {   # renode-tests: the same Zephyr build, run on Renode
        "cargo": ["ffi/Cargo.toml"],
        "zephyr": ["zephyr/module.yml"],
        "hand": ["renode/"],          # HAND: robot/repl files the job copies; no build graph
    },
    "rust_cov": {   # coverage: Rust Coverage (cargo llvm-cov --workspace at the root)
        "cargo": ["Cargo.toml"],
        "hand": [],
    },
    "proofs": {   # formal-verification: Verus, Kani (root + ffi), Rocq, Lean
        "cargo": ["Cargo.toml", "ffi/Cargo.toml"],
        "bazel": "proofs",
        "hand": [],
    },
    "gust_renode": {   # gust-renode: cargo xtask renode-elfs (os/gust) + bazel test in os/gust/renode-test
        "cargo": ["os/gust/Cargo.toml"],
        # HAND: a separate Bazel module (rules_renode); not queried, its MODULE/BUILD run everything
        "hand": ["os/gust/renode-test/"],
    },
    "cross_arch": {   # drv-cross-arch: drivers/build-cross-arch.sh over drivers/*-thin
        "cargo_glob": ["drivers/*-thin/Cargo.toml"],
        # HAND: the job's two scripts (shell + python, no build graph)
        "hand": ["drivers/build-cross-arch.sh", "drivers/check-cross-arch.py"],
    },
}
GATE_NAMES = list(GATES)

# Informational per-class outputs (first match wins after run-all/docs/inert).
CLASS_PREFIXES = [
    ("kernel", ("src/", "plain/", "ffi/", "tests/", "Cargo.toml")),
    ("zephyr_src", ("zephyr/",)),
    ("proofs_src", ("proofs/",)),
    ("drivers", ("drivers/",)),
    ("gust", ("os/gust/", "ports/")),
    ("wit", ("wit/",)),
    ("targets", ("targets/",)),
]
CLASS_NAMES = ["ci", "docs", "artifacts"] + [c for c, _ in CLASS_PREFIXES] + ["unknown"]

BAZEL_SNAPSHOT = "tools/ci/inputs-bazel.json"
# The Bazel targets each bazel-keyed gate runs (copied from the workflow; a
# change to the workflow runs everything and --check-graph compares this list).
BAZEL_TARGETS = {
    "proofs": ["//:verus_test"]
    + [f"//proofs:{m}_proofs_test" for m in
       ("sem", "mutex", "condvar", "msgq", "stack", "pipe", "timer", "event",
        "mem_slab", "poll", "sched", "thread_lifecycle", "executor")]
    + ["//proofs/lean:all"],
}


# ── path helpers ──────────────────────────────────────────────────────────────

def norm(root, p):
    """Repo-relative posix path of p, or None if it leaves the repo."""
    rr = os.path.abspath(root)
    ap = os.path.normpath(os.path.join(rr, p))
    if ap != rr and not ap.startswith(rr + os.sep):
        return None
    return os.path.relpath(ap, rr).replace(os.sep, "/")


def claim_for(root, rel):
    """An existing directory is claimed as a prefix ('d/'), anything else exactly."""
    if rel in (None, "", "."):
        return None
    return rel.rstrip("/") + "/" if os.path.isdir(os.path.join(root, rel)) else rel


def claimed(path, claims):
    for c in claims:
        if c.endswith("/"):
            if path.startswith(c):
                return True
        elif path == c:
            return True
    return False


# ── graph derivation ──────────────────────────────────────────────────────────

class GraphError(Exception):
    pass


STR = r'"((?:[^"\\]|\\.)*)"'
RE_PATH_ATTR = re.compile(r'#\s*\[\s*path\s*=\s*' + STR + r'\s*\]')
RE_INCLUDE = re.compile(r'\binclude(?:_str|_bytes)?!\s*\(\s*(' + STR + r'|concat!\s*\(\s*env!\s*\(\s*"CARGO_MANIFEST_DIR"\s*\)\s*,\s*' + STR + r'\s*\)|[^)]*)\)')
RE_WIT_PATH = re.compile(r'generate!\s*\(\s*\{[^}]*?\bpath\s*:\s*(\[[^\]]*\]|' + STR + r')', re.S)
RE_MOD = re.compile(r'^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+([A-Za-z_][A-Za-z0-9_]*)\s*;', re.M)
RE_LITERAL = re.compile(STR)


def strip_comments(src):
    return lex(src)[0]


def lex(src):
    """(src without comments, [string literal contents]). String and char
    literals are kept intact: a `/*` or `//` inside a string, or a `/*` inside a
    line comment, is not a comment, and a '"' char literal opens no string."""
    out, lits, i, n = [], [], 0, len(src)
    while i < n:
        ch = src[i]
        if src.startswith("//", i):
            j = src.find("\n", i)
            i = n if j < 0 else j
        elif src.startswith("/*", i):
            depth, i = 1, i + 2
            while i < n and depth:
                if src.startswith("/*", i):
                    depth, i = depth + 1, i + 2
                elif src.startswith("*/", i):
                    depth, i = depth - 1, i + 2
                else:
                    i += 1
        elif ch == "r" and re.match(r'r(#*)"', src[i:i + 260]) and (i == 0 or not (src[i - 1].isalnum() or src[i - 1] == "_")):
            hashes = re.match(r'r(#*)"', src[i:]).group(1)
            end = src.find('"' + hashes, i + 2 + len(hashes))
            end = n if end < 0 else end + 1 + len(hashes)
            lits.append(src[i + 2 + len(hashes):end - 1 - len(hashes)])
            out.append(src[i:end])
            i = end
        elif ch == '"':
            j = i + 1
            while j < n and src[j] != '"':
                j += 2 if src[j] == "\\" else 1
            lits.append(src[i + 1:j])
            out.append(src[i:j + 1])
            i = j + 1
        elif ch == "'" and re.match(r"'(\\.|[^\\'])'", src[i:i + 4]):
            m = re.match(r"'(\\.|[^\\'])'", src[i:i + 4])
            out.append(m.group(0))
            i += len(m.group(0))
        else:
            out.append(ch)
            i += 1
    return "".join(out), lits


def scan_rs(root, rel_file, manifest_dir, claims, unresolved):
    """Claim what one .rs file pulls in from elsewhere; return module files to walk."""
    try:
        src = strip_comments(open(os.path.join(root, rel_file), encoding="utf-8", errors="replace").read())
    except OSError:
        return []
    fdir = os.path.dirname(rel_file)
    for m in RE_PATH_ATTR.finditer(src):
        c = claim_for(root, norm(root, os.path.join(fdir, m.group(1))))
        if c:
            claims.add(c)
    for m in RE_INCLUDE.finditer(src):
        if m.group(2) is not None:
            target = os.path.join(fdir, m.group(2))
        elif m.group(3) is not None:
            target = os.path.join(manifest_dir, m.group(3).lstrip("/"))
        else:
            arg = m.group(1)
            if "OUT_DIR" in arg:
                continue   # generated by the build script, whose inputs are scanned
            unresolved.append(f"{rel_file}: include!({arg.strip()[:60]})")
            continue
        c = claim_for(root, norm(root, target))
        if c:
            claims.add(c)
    for m in RE_WIT_PATH.finditer(src):
        for lit in RE_LITERAL.findall(m.group(1)):
            c = claim_for(root, norm(root, os.path.join(manifest_dir, lit)))
            if c:
                claims.add(c)
    # module files this file loads (for a crate root whose directory is not claimed whole)
    stem = os.path.splitext(os.path.basename(rel_file))[0]
    subdir = fdir if stem in ("lib", "main", "mod") else os.path.join(fdir, stem)
    out = []
    for name in RE_MOD.findall(src):
        for cand in (os.path.join(subdir, name + ".rs"), os.path.join(subdir, name, "mod.rs")):
            if os.path.isfile(os.path.join(root, cand)):
                out.append(cand.replace(os.sep, "/"))
    return out


def scan_build_rs(root, rel_file, manifest_dir, claims):
    """Every string literal in a build script that names a path is an input."""
    try:
        _, lits = lex(open(os.path.join(root, rel_file), encoding="utf-8", errors="replace").read())
    except OSError:
        return
    for lit in lits:
        lit = lit.replace("cargo:rerun-if-changed=", "")
        if "/" not in lit and "." not in lit:
            continue
        if any(ch in lit for ch in "{}$ \\") or lit.startswith(("-", "cargo:")):
            continue
        rel = norm(root, os.path.join(manifest_dir, lit))
        if rel is None:
            continue
        if os.path.exists(os.path.join(root, rel)) or "/" in lit:
            c = claim_for(root, rel)
            if c:
                claims.add(c)


def dep_tables(doc):
    for key in ("dependencies", "dev-dependencies", "build-dependencies"):
        yield key, doc.get(key) or {}
    for _, t in (doc.get("target") or {}).items():
        for key in ("dependencies", "dev-dependencies", "build-dependencies"):
            yield key, (t or {}).get(key) or {}
    ws = doc.get("workspace") or {}
    yield "dependencies", ws.get("dependencies") or {}
    for _, t in (doc.get("patch") or {}).items():
        yield "dependencies", t or {}


def walk_cargo(root, manifest_rel, claims, unresolved, packages, as_root):
    """Claim everything one package (and its path deps, transitively) compiles from."""
    key = (manifest_rel, as_root)
    if key in packages:
        return
    packages.add(key)
    if tomllib is None:
        raise GraphError("tomllib unavailable (python < 3.11)")
    try:
        with open(os.path.join(root, manifest_rel), "rb") as f:
            doc = tomllib.load(f)
    except (OSError, tomllib.TOMLDecodeError) as e:
        raise GraphError(f"{manifest_rel}: {e}")
    mdir = os.path.dirname(manifest_rel)
    claims.add(manifest_rel)
    pkg = doc.get("package") or {}

    # Sources. A crate in its own directory is claimed whole. The repo-root
    # package (gale) shares its directory with everything, so its targets are
    # claimed one by one.
    rs_roots = []
    if mdir:
        claims.add(mdir + "/")
        for dp, dns, fns in os.walk(os.path.join(root, mdir)):
            dns[:] = [d for d in dns if d not in ("target", ".git")]
            for fn in fns:
                if fn.endswith(".rs"):
                    rs_roots.append(norm(root, os.path.join(dp, fn)))
    else:
        lib = (doc.get("lib") or {}).get("path") or "src/lib.rs"
        if os.path.exists(os.path.join(root, lib)):
            claims.add(os.path.dirname(lib) + "/")
            rs_roots.append(lib)
        if as_root:
            explicit = []
            for kind in ("bin", "test", "bench", "example"):
                for t in doc.get(kind) or []:
                    if t.get("path"):
                        explicit.append(t["path"])
            auto = {"bin": "src/bin", "test": "tests", "bench": "benches", "example": "examples"}
            for kind, d in auto.items():
                if (pkg.get("auto" + kind + "s") is False) or not os.path.isdir(os.path.join(root, d)):
                    continue
                for e in sorted(os.listdir(os.path.join(root, d))):
                    p = os.path.join(d, e)
                    if e.endswith(".rs"):
                        explicit.append(p)
                    elif os.path.isfile(os.path.join(root, p, "main.rs")):
                        explicit.append(os.path.join(p, "main.rs"))
            if os.path.isfile(os.path.join(root, "src/main.rs")):
                explicit.append("src/main.rs")
            for p in explicit:
                rel = norm(root, p)
                if rel:
                    claims.add(rel)
                    rs_roots.append(rel)
        build = pkg.get("build")
        build = "build.rs" if build is None else build
        if build and os.path.isfile(os.path.join(root, build)):
            claims.add(build)
            rs_roots.append(build)

    # #[path], include!, wit paths; and, for the root package, the module tree.
    seen, todo = set(), list(rs_roots)
    while todo:
        f = todo.pop()
        if f in seen:
            continue
        seen.add(f)
        for sub in scan_rs(root, f, mdir, claims, unresolved):
            claims.add(sub)
            todo.append(sub)
    build = pkg.get("build")
    build = "build.rs" if build is None else build
    if build:
        scan_build_rs(root, norm(root, os.path.join(mdir, build)) or "", mdir, claims)

    for kind, table in dep_tables(doc):
        if kind == "dev-dependencies" and not as_root:
            continue
        for _, spec in table.items():
            if isinstance(spec, dict) and spec.get("path"):
                dep_dir = norm(root, os.path.join(mdir, spec["path"]))
                if dep_dir is None:
                    raise GraphError(f"{manifest_rel}: path dependency {spec['path']} leaves the repo")
                walk_cargo(root, (dep_dir + "/Cargo.toml").lstrip("/") if dep_dir != "." else "Cargo.toml",
                           claims, unresolved, packages, as_root=False)
    ws = doc.get("workspace") or {}
    for pat in ws.get("members") or []:
        for d in sorted(globmod.glob(os.path.join(root, mdir, pat))):
            m = norm(root, os.path.join(d, "Cargo.toml"))
            if m and os.path.isfile(os.path.join(root, m)):
                walk_cargo(root, m, claims, unresolved, packages, as_root)


def walk_zephyr(root, module_rel, claims):
    """A west module: its manifest, its cmake/kconfig entries, and what its CMake reads."""
    try:
        text = open(os.path.join(root, module_rel), encoding="utf-8").read()
    except OSError as e:
        raise GraphError(f"{module_rel}: {e}")
    claims.add(module_rel)
    entries = re.findall(r'^\s*(?:cmake|kconfig|cmake-ext|settings)\s*:\s*(\S+)\s*$', text, re.M)
    if not entries:
        raise GraphError(f"{module_rel}: no cmake/kconfig entry")
    for e in entries:
        rel = norm(root, e)   # module.yml paths are relative to the module root (the repo)
        c = claim_for(root, rel)
        if c:
            claims.add(c)
    for e in entries:
        d = os.path.join(root, e)
        if not os.path.isdir(d):
            continue
        for dp, _, fns in os.walk(d):
            for fn in fns:
                if fn == "CMakeLists.txt" or fn.endswith(".cmake"):
                    src = open(os.path.join(dp, fn), encoding="utf-8", errors="replace").read()
                    for ref in re.findall(r'\$\{GALE_DIR\}/([A-Za-z0-9_./-]+)', src):
                        c = claim_for(root, norm(root, ref))
                        if c:
                            claims.add(c)


def bazel_fingerprint(root, files):
    h = hashlib.sha256()
    for f in sorted(files):
        h.update(f.encode() + b"\0")
        try:
            h.update(open(os.path.join(root, f), "rb").read())
        except OSError:
            h.update(b"<missing>")
        h.update(b"\0")
    return h.hexdigest()


def load_bazel(root):
    try:
        return json.load(open(os.path.join(root, BAZEL_SNAPSHOT), encoding="utf-8"))
    except (OSError, ValueError) as e:
        raise GraphError(f"{BAZEL_SNAPSHOT}: {e}")


def bazel_claims(root, key):
    snap = load_bazel(root)
    files = (snap.get("gates") or {}).get(key, {}).get("files")
    if not files:
        raise GraphError(f"{BAZEL_SNAPSHOT}: no files for {key}")
    out = set()
    for f in files:
        d = os.path.dirname(f)
        out.add(d + "/" if d else f)   # a whole package dir, so a new file in a glob is covered
    return out


def gate_claims(root, cfg=None):
    """{gate: (claims, unresolved)} derived from the tree at `root`. Raises GraphError."""
    cfg = cfg or GATES
    out = {}
    for g, spec in cfg.items():
        claims, unresolved, packages = set(), [], set()
        manifests = list(spec.get("cargo", []))
        for pat in spec.get("cargo_glob", []):
            found = sorted(norm(root, p) for p in globmod.glob(os.path.join(root, pat)))
            if not found:
                raise GraphError(f"{g}: {pat} matches nothing")
            manifests += found
        for m in manifests:
            walk_cargo(root, m, claims, unresolved, packages, as_root=True)
        for z in spec.get("zephyr", []):
            walk_zephyr(root, z, claims)
        if spec.get("bazel"):
            claims |= bazel_claims(root, spec["bazel"])
        for h in spec.get("hand", []):
            claims.add(h)
        out[g] = (claims, unresolved)
    return out


# ── classification ────────────────────────────────────────────────────────────

def is_run_all(p):
    base = p.rsplit("/", 1)[-1]
    return (p.startswith(RUN_ALL_PREFIXES) or p in RUN_ALL_FILES
            or base in RUN_ALL_BASENAMES or p.endswith(RUN_ALL_SUFFIXES))


def docs_named_elsewhere(root, p):
    """A .md/LICENSE file is docs only if no non-markdown file names it."""
    base = p.rsplit("/", 1)[-1]
    r = subprocess.run(["git", "grep", "-qF", "--", base, "--", ":!*.md",
                        ":!tools/ci/classify-changes.py", ":!.github/gate-probe.json"],
                       cwd=root, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    if r.returncode not in (0, 1):
        return True   # git grep itself failed: not provably docs
    return r.returncode == 0


def is_docs(root, p):
    base = p.rsplit("/", 1)[-1]
    if not (p.endswith(".md") or base == "LICENSE" or base.startswith("LICENSE-")):
        return False
    return not docs_named_elsewhere(root, p)


def is_inert(p):
    return p.startswith(INERT_PREFIXES) or p in INERT_FILES


def all_true(reason):
    return ({g: True for g in GATE_NAMES}, {c: True for c in CLASS_NAMES} | {"docs_only": False, "artifacts_only": False}, [reason])


def classify(root, changed, graph=None):
    """(gates, classes, notes). Fails open: any doubt -> every gate true."""
    changed = [c.strip() for c in changed if c.strip()]
    if not changed:
        return all_true("no changed paths")
    if graph is None:
        try:
            graph = gate_claims(root)
        except GraphError as e:
            return all_true(f"build graph unreadable ({e})")
    gates = {g: False for g in GATE_NAMES}
    classes = {c: False for c in CLASS_NAMES}
    notes, run_all = [], None
    for g, (_, unresolved) in graph.items():
        if unresolved:
            gates[g] = True
            notes.append(f"{g}: unresolvable include ({unresolved[0]}) -> always runs")
    for p in changed:
        if is_run_all(p):
            classes["ci"] = True
            run_all = run_all or f"{p} is CI/toolchain/build-system"
            continue
        if is_docs(root, p):
            classes["docs"] = True
            continue
        hits = [g for g, (cl, _) in graph.items() if claimed(p, cl)]
        for g in hits:
            gates[g] = True
        for c, prefixes in CLASS_PREFIXES:
            if p.startswith(prefixes) or p in prefixes:
                classes[c] = True
                break
        if is_inert(p):
            classes["artifacts"] = True
            if hits:   # cannot happen while --check-graph is green; fail open anyway
                run_all = run_all or f"{p} is inert AND an input of {hits}"
            continue
        if not hits:
            classes["unknown"] = True
            run_all = run_all or f"{p} is claimed by no job's build graph"
    if run_all:
        g, c, n = all_true(run_all)
        c.update({k: v for k, v in classes.items() if v})
        return g, c, notes + n
    classes["docs_only"] = all(is_docs(root, p) for p in changed)
    classes["artifacts_only"] = all(is_inert(p) for p in changed)
    return gates, classes, notes


def changed_paths(root, env):
    """(paths or None, reason). None means: run everything."""
    probe = env.get("GATE_PROBE_FILES")
    if probe:
        try:
            return [l for l in open(probe, encoding="utf-8").read().splitlines() if l.strip()], "gate probe"
        except OSError as e:
            return None, f"GATE_PROBE_FILES unreadable ({e})"
    ev = env.get("EVENT_NAME") or env.get("GITHUB_EVENT_NAME") or ""
    if ev != "pull_request":
        return None, f"event {ev or '<none>'} is not pull_request"
    base = (env.get("BASE_SHA") or "").strip()
    if not base:
        return None, "no base sha"
    # --no-renames: a rename is a delete of the old path plus an add of the new
    # one, and BOTH must be classified (with renames on, only the new name shows).
    r = subprocess.run(["git", "diff", "--name-only", "--no-renames", f"{base}...HEAD"],
                       cwd=root, capture_output=True, text=True)
    if r.returncode != 0:
        return None, f"git diff failed against {base}: {r.stderr.strip()[:200]}"
    paths = [l for l in r.stdout.splitlines() if l.strip()]
    if not paths:
        return None, "empty diff"
    return paths, f"{len(paths)} path(s) changed against {base[:12]}"


def emit(gates, classes, out):
    lines = [f"{k}={'true' if v else 'false'}" for k, v in list(gates.items()) + list(classes.items())]
    if out:
        with open(out, "a", encoding="utf-8") as f:
            f.write("\n".join(lines) + "\n")
    return lines


def run(root, env, out):
    paths, why = changed_paths(root, env)
    print(f"classify-changes: {why}")
    if paths is None:
        gates, classes, notes = all_true(why)
    else:
        for p in paths:
            print(f"  changed: {p}")
        gates, classes, notes = classify(root, paths)
    for n in notes:
        print(f"  -> {n}")
    for line in emit(gates, classes, out):
        print(f"  {line}")
    return 0


# ── --check-graph: fast-tier controls on the real tree ────────────────────────

# Named TRUE inputs per job, picked by reading each job, independent of the
# graph walk: if the walk ever loses an edge (a #[path], a build-script object,
# the module.yml), one of these stops selecting its job and the check fails.
TRUE_INPUTS = {
    "zephyr": ["ffi/src/lib.rs", "plain/src/sem.rs", "plain/src/lib.rs", "zephyr/gale_sem.c",
               "zephyr/CMakeLists.txt", "zephyr/Kconfig", "zephyr/module.yml", "ffi/Cargo.toml"],
    "zephyr_renode": ["ffi/src/lib.rs", "plain/src/sem.rs", "zephyr/gale_sem.c", "renode/stm32f4_sem.robot"],
    "rust_cov": ["plain/src/sem.rs", "tests/sem_integration.rs", "benches/sem_bench.rs", "Cargo.toml"],
    "proofs": ["src/sem.rs", "plain/src/sem.rs", "ffi/src/lib.rs", "tests/sem_integration.rs",
               "proofs/sem_proofs.v", "proofs/lean/RingBuf.lean"],
    "gust_renode": ["os/gust/src/main.rs", "os/gust/build.rs", "os/gust/renode-test/gust_wdg.robot",
                    "drivers/wdg-thin/wdg-thin-cm3.o", "drivers/uart-thin/uart-thin-cm3.o",
                    "drivers/os-node/os-time-cm3.o", "targets/generated/gust_target_stm32f100.rs",
                    "ports/arch/armv7m/src/lib.rs", "plain/src/sem.rs"],
    "cross_arch": ["drivers/wdg-thin/src/lib.rs", "drivers/wdg-thin/Cargo.toml", "wit/hal/gust-hal.wit",
                   "targets/generated/gust_target_stm32wb55.rs", "drivers/build-cross-arch.sh",
                   "drivers/check-cross-arch.py"],
}
# Paths that must select NO heavy job.
NO_JOB = ["docs/byo-os-architecture.md", "artifacts/requirements.yaml", "rivet.yaml",
          "schemas/gale.yaml"]


def cargo_metadata_inputs(root, manifest):
    """(kind, path, is_job_root) for every package and target cargo itself
    reports, following path dependencies. A dependency's tests, benches and
    examples are not compiled for the job, so they are marked as such."""
    seen, todo, out = set(), [(manifest, True)], []
    while todo:
        m, is_root = todo.pop()
        if (m, is_root) in seen:
            continue
        seen.add((m, is_root))
        r = subprocess.run(["cargo", "metadata", "--no-deps", "--offline", "--format-version", "1",
                            "--manifest-path", os.path.join(root, m)],
                           cwd=root, capture_output=True, text=True)
        if r.returncode != 0:
            raise GraphError(f"cargo metadata {m}: {r.stderr.strip()[:300]}")
        meta = json.loads(r.stdout)
        for pkg in meta["packages"]:
            pm = norm(root, pkg["manifest_path"])
            if pm != m and is_root is False:
                continue
            out.append(("manifest", pm, is_root))
            for t in pkg["targets"]:
                kinds = set(t.get("kind") or [])
                if is_root or kinds & {"lib", "rlib", "cdylib", "staticlib", "dylib", "proc-macro", "custom-build"}:
                    out.append(("target " + "/".join(sorted(kinds)), norm(root, t["src_path"]), is_root))
            for d in pkg.get("dependencies") or []:
                if d.get("path") and (is_root or d.get("kind") != "dev"):
                    dm = norm(root, os.path.join(d["path"], "Cargo.toml"))
                    if dm:
                        todo.append((dm, False))
    return out


def check_graph(root):
    bad, n = [], 0

    def expect(cond, msg):
        nonlocal n
        n += 1
        if not cond:
            bad.append(msg)

    try:
        graph = gate_claims(root)
    except GraphError as e:
        print(f"  FAIL: the build graph cannot be derived: {e}")
        return 1
    tracked = subprocess.run(["git", "ls-files"], cwd=root, capture_output=True, text=True).stdout.splitlines()

    # 1. Every graph input of every job selects that job (end to end, one-file diffs).
    for g, (claims, unresolved) in graph.items():
        expect(not unresolved, f"{g}: unresolvable include(s) {unresolved[:3]} (job would always run; resolve or record)")
        files = [f for f in tracked if claimed(f, claims) and not is_run_all(f) and not is_docs(root, f)]
        expect(files, f"{g}: graph claims no tracked file -- the walk found nothing")
        for f in files:
            gates, _, _ = classify(root, [f], graph)
            expect(gates[g], f"{g}: graph input {f} does not select it")
        print(f"  {g:14} {len(claims):4} claim(s), {len(files):4} tracked input file(s), all select it")

    # 2. cargo metadata agrees: every package and target cargo reports is claimed.
    n_meta = [0]
    for g, spec in GATES.items():
        manifests = list(spec.get("cargo", []))
        for pat in spec.get("cargo_glob", []):
            manifests += sorted(norm(root, p) for p in globmod.glob(os.path.join(root, pat)))
        for m in manifests:
            try:
                items = cargo_metadata_inputs(root, m)
            except GraphError as e:
                expect(False, str(e))
                continue
            for kind, p, _ in items:
                expect(claimed(p, graph[g][0]),
                       f"{g}: cargo metadata reports {kind} {p} (from {m}) but the walk does not claim it")
                n_meta[0] += 1

    print(f"  cargo metadata: {n_meta[0]} package/target path(s) reported, each checked against the walk")

    # 3. Named true inputs select their job; docs/artifacts select none.
    tracked_set = set(tracked)
    for g, paths in TRUE_INPUTS.items():
        for p in paths:
            expect(p in tracked_set, f"false-skip control {p} ({g}) is not a tracked file -- update TRUE_INPUTS")
            gates, _, _ = classify(root, [p], graph)
            expect(gates[g], f"false-skip control: {p} is a true input of {g} but does not select it")
            # ...and selects it BECAUSE the graph claims it: an unknown path runs
            # everything, which would pass the line above while the edge is lost.
            expect(claimed(p, graph[g][0]), f"false-skip control: {p} selects {g} only as an unknown "
                                            f"path; the graph walk lost this edge")
    for p in NO_JOB:
        expect(p in tracked_set, f"no-job control {p} is not a tracked file -- update NO_JOB")
        gates, classes, _ = classify(root, [p], graph)
        expect(not any(gates.values()), f"{p} selects {[k for k, v in gates.items() if v]} but must select no heavy job")
    gates, _, _ = classify(root, ["docs/byo-os-architecture.md", "artifacts/requirements.yaml"], graph)
    expect(not any(gates.values()), "docs + artifacts change selects a heavy job")

    # 4. Inert prefixes hold no graph input.
    for g, (claims, _) in graph.items():
        for c in claims:
            expect(not is_inert(c) and not any(i.startswith(c) for i in INERT_PREFIXES if c.endswith("/")),
                   f"{g}: graph input {c} overlaps the inert (artifacts) list")

    # 5. The Bazel snapshot is current and covers the workflow's targets.
    try:
        snap = load_bazel(root)
        for key, targets in BAZEL_TARGETS.items():
            ent = (snap.get("gates") or {}).get(key) or {}
            expect(sorted(ent.get("targets") or []) == sorted(targets),
                   f"{BAZEL_SNAPSHOT}: {key} targets differ from BAZEL_TARGETS -- rerun --derive-bazel")
        fp = bazel_fingerprint(root, snap.get("fingerprint_files") or [])
        expect(fp == snap.get("fingerprint"),
               f"{BAZEL_SNAPSHOT} is stale: a BUILD/.bzl/MODULE file changed -- rerun "
               f"`python3 tools/ci/classify-changes.py --derive-bazel` and commit it")
    except GraphError as e:
        expect(False, str(e))

    # 6. Fail-open spot checks on the live script.
    for env in ({"EVENT_NAME": "push"}, {"EVENT_NAME": "merge_group", "BASE_SHA": "HEAD"},
                {"EVENT_NAME": "pull_request", "BASE_SHA": "HEAD"},
                {"EVENT_NAME": "pull_request", "BASE_SHA": "0" * 40},
                {"EVENT_NAME": "pull_request"}):
        paths, why = changed_paths(root, env)
        expect(paths is None, f"{env}: expected run-everything, got paths ({why})")

    print()
    for b in bad:
        print(f"  FAIL: {b}")
    print(f"\n  {n - len(bad)}/{n} graph control(s) hold")
    return 1 if bad else 0


# ── --derive-bazel ────────────────────────────────────────────────────────────

def derive_bazel(root):
    snap = {"comment": "Generated by `python3 tools/ci/classify-changes.py --derive-bazel` "
                       "(bazel query). --check-graph fails when the fingerprint is stale.",
            "gates": {}}
    fp_files = {"MODULE.bazel"}
    for key, targets in BAZEL_TARGETS.items():
        expr = " + ".join(f"deps({t})" for t in targets)
        q = f"kind('source file', {expr}) + buildfiles({expr})"
        r = subprocess.run(["bazel", "query", "--noimplicit_deps", q, "--output=label"],
                           cwd=root, capture_output=True, text=True)
        if r.returncode != 0:
            print(r.stderr[-2000:], file=sys.stderr)
            return 1
        files = []
        for lab in r.stdout.splitlines():
            if not lab.startswith("//"):
                continue   # external repositories
            pkg, _, name = lab[2:].partition(":")
            path = f"{pkg}/{name}" if pkg else name
            files.append(path)
            if path.rsplit("/", 1)[-1] in ("BUILD.bazel", "BUILD") or path.endswith(".bzl"):
                fp_files.add(path)
        snap["gates"][key] = {"targets": targets, "files": sorted(set(files))}
    snap["fingerprint_files"] = sorted(fp_files)
    snap["fingerprint"] = bazel_fingerprint(root, fp_files)
    with open(os.path.join(root, BAZEL_SNAPSHOT), "w", encoding="utf-8") as f:
        json.dump(snap, f, indent=1, sort_keys=True)
        f.write("\n")
    print(f"wrote {BAZEL_SNAPSHOT}: " + ", ".join(f"{k}={len(v['files'])} files" for k, v in snap["gates"].items()))
    return 0


# ── --self-test: planted paths on a fixture repo ──────────────────────────────

FIXTURE = {
    "Cargo.toml": '[package]\nname = "gale"\nversion = "0.1.0"\n[lib]\npath = "plain/src/lib.rs"\n'
                  '[[bench]]\nname = "sem_bench"\nharness = false\n',
    "plain/src/lib.rs": "pub mod sem;\n",
    "plain/src/sem.rs": "pub fn f() {}\n",
    "src/sem.rs": "// verus source\n",
    "tests/sem_integration.rs": "#[test] fn t() {}\n",
    "benches/sem_bench.rs": "fn main() {}\n",
    "benches/engine_control/run.sh": "echo\n",
    "ffi/Cargo.toml": '[package]\nname = "gale-ffi"\nversion = "0.1.0"\n[dependencies]\ngale = { path = ".." }\n',
    "ffi/src/lib.rs": "pub fn g() {}\n",
    "zephyr/module.yml": "name: gale\nbuild:\n  cmake: zephyr\n  kconfig: zephyr/Kconfig\n",
    "zephyr/CMakeLists.txt": "set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)\nset(X ${GALE_DIR}/ffi)\n",
    "zephyr/Kconfig": "config GALE\n",
    "zephyr/gale_sem.c": "int x;\n",
    "renode/sem.robot": "*** x\n",
    "proofs/sem_proofs.v": "Theorem t.\n",
    "proofs/lean/RingBuf.lean": "theorem t\n",
    "proofs/BUILD.bazel": "x\n",
    "os/gust/Cargo.toml": '[package]\nname = "gust"\nversion = "0.1.0"\n[workspace]\n[dependencies]\n'
                          'gale = { path = "../.." }\nport = { path = "../../ports/arch/armv7m" }\n',
    "os/gust/build.rs": 'fn main() { let o = Path::new(&m).join("../../drivers/wdg-thin/wdg-thin-cm3.o"); }\n',
    "os/gust/src/main.rs": '#[path = "../../../targets/generated/t.rs"]\nmod t;\n',
    "os/gust/renode-test/gust_wdg.robot": "x\n",
    "ports/arch/armv7m/Cargo.toml": '[package]\nname = "port"\nversion = "0.1.0"\n',
    "ports/arch/armv7m/src/lib.rs": "\n",
    "targets/generated/t.rs": "pub const A: u32 = 1;\n",
    "targets/stm32.aadl": "x\n",
    "drivers/wdg-thin/Cargo.toml": '[package]\nname = "wdg"\nversion = "0.1.0"\n[workspace]\n',
    "drivers/wdg-thin/src/lib.rs": 'wit_bindgen::generate!({ world: "w", path: "../../wit/hal" });\n',
    "drivers/wdg-thin/wdg-thin-cm3.o": "obj\n",
    "drivers/i2c-thin/Cargo.toml": '[package]\nname = "i2c"\nversion = "0.1.0"\n[workspace]\n',
    "drivers/i2c-thin/src/lib.rs": 'include!("../../../targets/generated/t.rs");\n',
    "drivers/build-cross-arch.sh": "x\n",
    "drivers/check-cross-arch.py": "x\n",
    "drivers/check-graph-env.py": "x\n",
    "wit/hal/gust-hal.wit": "package x;\n",
    "artifacts/requirements.yaml": "artifacts: []\n",
    "rivet.yaml": "x\n",
    "docs/guide.md": "# guide\n",
    "docs/named.md": "# named\n",
    "docs/x.txt": "x\n",
    "scripts/tool.sh": "cat docs/named.md\n",
    "README.md": "# r\n",
    ".github/workflows/w.yml": "x\n",
    "tools/ci/classify-changes.py": "# copy\n",
    "Cargo.lock": "x\n",
    "varve.toml": "x\n",
    "rust-toolchain.toml": "x\n",
}
FIXTURE_GATES = {
    "zephyr": {"cargo": ["ffi/Cargo.toml"], "zephyr": ["zephyr/module.yml"], "hand": []},
    "zephyr_renode": {"cargo": ["ffi/Cargo.toml"], "zephyr": ["zephyr/module.yml"], "hand": ["renode/"]},
    "rust_cov": {"cargo": ["Cargo.toml"], "hand": []},
    "proofs": {"cargo": ["Cargo.toml", "ffi/Cargo.toml"], "hand": ["src/", "proofs/", "plain/"]},
    "gust_renode": {"cargo": ["os/gust/Cargo.toml"], "hand": ["os/gust/renode-test/"]},
    "cross_arch": {"cargo_glob": ["drivers/*-thin/Cargo.toml"],
                   "hand": ["drivers/build-cross-arch.sh", "drivers/check-cross-arch.py"]},
}


def git(root, *args):
    return subprocess.run(["git", *args], cwd=root, capture_output=True, text=True, check=True).stdout.strip()


def make_fixture(td, extra=None):
    for p, body in {**FIXTURE, **(extra or {})}.items():
        fp = os.path.join(td, p)
        os.makedirs(os.path.dirname(fp) or td, exist_ok=True)
        open(fp, "w").write(body)
    git(td, "init", "-q", "-b", "main")
    git(td, "-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false",
        "add", "-A")
    git(td, "-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false",
        "commit", "-q", "-m", "base")
    return git(td, "rev-parse", "HEAD")


def fixture_commit(td, mutate):
    mutate(td)
    git(td, "add", "-A")
    git(td, "-c", "user.email=t@t", "-c", "user.name=t", "-c", "commit.gpgsign=false",
        "commit", "-q", "--allow-empty", "-m", "change")


def self_test():
    global GATES, GATE_NAMES
    saved = (GATES, GATE_NAMES)
    GATES, GATE_NAMES = FIXTURE_GATES, list(FIXTURE_GATES)
    failures, total = [], 0
    ALL = set(GATE_NAMES)

    def case(name, got, want):
        nonlocal total
        total += 1
        sel = {g for g, v in got.items() if v}
        ok = sel == want
        print(f"  {'ok  ' if ok else 'FAIL'} {name}: selects {sorted(sel) or 'nothing'}"
              + ("" if ok else f" (want {sorted(want) or 'nothing'})"))
        if not ok:
            failures.append(name)

    def via_diff(td, base, env_extra=None):
        out = os.path.join(outdir, "github_output")   # outside the fixture, or `git add -A` commits it
        if os.path.exists(out):
            os.remove(out)
        env = {"EVENT_NAME": "pull_request", "BASE_SHA": base, **(env_extra or {})}
        paths, why = changed_paths(td, env)
        gates = all_true(why)[0] if paths is None else classify(td, paths)[0]
        emit(gates, {}, out)
        written = dict(l.split("=", 1) for l in open(out).read().split())
        return {g: written[g] == "true" for g in GATE_NAMES}

    outdir = tempfile.mkdtemp()
    import shutil
    try:
        with tempfile.TemporaryDirectory() as td:
            make_fixture(td)

            def files(*ps):
                return classify(td, list(ps))[0]

            # planted paths per class
            case("docs-only (unnamed .md)", files("docs/guide.md", "README.md"), set())
            case("docs .md named by a script", files("docs/named.md"), ALL)
            case("non-md file under docs/", files("docs/x.txt"), ALL)
            case("artifacts-only (rivet YAML)", files("artifacts/requirements.yaml", "rivet.yaml"), set())
            case("kernel lib (plain/src)", files("plain/src/sem.rs"),
                 {"zephyr", "zephyr_renode", "rust_cov", "proofs", "gust_renode"})
            case("Verus source (src/)", files("src/sem.rs"), {"proofs"})
            case("ffi", files("ffi/src/lib.rs"), {"zephyr", "zephyr_renode", "proofs"})
            case("integration test", files("tests/sem_integration.rs"), {"rust_cov", "proofs"})
            case("root bench target", files("benches/sem_bench.rs"), {"rust_cov", "proofs"})
            case("zephyr module C", files("zephyr/gale_sem.c"), {"zephyr", "zephyr_renode"})
            case("zephyr Kconfig", files("zephyr/Kconfig"), {"zephyr", "zephyr_renode"})
            case("renode robot", files("renode/sem.robot"), {"zephyr_renode"})
            case("proofs", files("proofs/sem_proofs.v"), {"proofs"})
            case("os/gust source", files("os/gust/src/main.rs"), {"gust_renode"})
            case("gust renode-test", files("os/gust/renode-test/gust_wdg.robot"), {"gust_renode"})
            case("path dep of gust (ports/)", files("ports/arch/armv7m/src/lib.rs"), {"gust_renode"})
            case("thin driver source", files("drivers/wdg-thin/src/lib.rs"), {"cross_arch"})
            case("thin driver object (build.rs input of gust)", files("drivers/wdg-thin/wdg-thin-cm3.o"),
                 {"cross_arch", "gust_renode"})
            case("wit via generate!(path:)", files("wit/hal/gust-hal.wit"), {"cross_arch"})
            case("#[path] + include! into targets/", files("targets/generated/t.rs"), {"gust_renode", "cross_arch"})
            case("targets/ file nothing reads", files("targets/stm32.aadl"), ALL)
            case("driver script no job claims", files("drivers/check-graph-env.py"), ALL)
            case("new top-level directory", files("newdir/x.rs"), ALL)
            for p in (".github/workflows/w.yml", "tools/ci/classify-changes.py", "varve.toml",
                      "Cargo.lock", "rust-toolchain.toml", "proofs/BUILD.bazel", "x/defs.bzl"):
                case(f"run-all: {p}", files(p), ALL)
            case("docs + kernel", files("docs/guide.md", "plain/src/sem.rs"),
                 {"zephyr", "zephyr_renode", "rust_cov", "proofs", "gust_renode"})
            case("empty path list", files(), ALL)

            # through the real diff and $GITHUB_OUTPUT
            base = git(td, "rev-parse", "HEAD")
            case("event: empty diff", via_diff(td, base), ALL)
            case("event: diff fails (unknown base)", via_diff(td, "0" * 40), ALL)
            case("event: no base sha", via_diff(td, ""), ALL)
            for ev in ("push", "merge_group", "schedule", "workflow_dispatch", ""):
                case(f"event: {ev or '<none>'}", via_diff(td, base, {"EVENT_NAME": ev}), ALL)

            fixture_commit(td, lambda d: open(os.path.join(d, "docs/guide.md"), "a").write("more\n"))
            case("diff: docs-only edit", via_diff(td, base), set())
            # the same docs-only diff on any other event still runs everything
            for ev in ("push", "merge_group", "schedule", "workflow_dispatch", "pull_request_target"):
                case(f"event: {ev} with a docs-only diff", via_diff(td, base, {"EVENT_NAME": ev}), ALL)
            b2 = git(td, "rev-parse", "HEAD")
            fixture_commit(td, lambda d: os.remove(os.path.join(d, "zephyr/gale_sem.c")))
            case("diff: deleted file", via_diff(td, b2), {"zephyr", "zephyr_renode"})
            b3 = git(td, "rev-parse", "HEAD")
            fixture_commit(td, lambda d: os.rename(os.path.join(d, "renode/sem.robot"),
                                                   os.path.join(d, "docs/sem.md")))
            case("diff: rename out of a claimed dir into docs", via_diff(td, b3), {"zephyr_renode"})
            b4 = git(td, "rev-parse", "HEAD")
            fixture_commit(td, lambda d: os.rename(os.path.join(d, "artifacts/requirements.yaml"),
                                                   os.path.join(d, "drivers/wdg-thin/req.yaml")))
            case("diff: rename from artifacts into a driver", via_diff(td, b4), {"cross_arch"})
            out = os.path.join(outdir, "probe.txt")
            open(out, "w").write("docs/guide.md\n")
            case("gate probe: docs path", via_diff(td, "", {"GATE_PROBE_FILES": out, "EVENT_NAME": ""}), set())
            open(out, "w").write("docs/gate-probe-control.rs\n")
            case("gate probe: .rs under docs/", via_diff(td, "", {"GATE_PROBE_FILES": out, "EVENT_NAME": ""}), ALL)

        # a NEW crate added as a path dependency, with no hand edit, is selected
        with tempfile.TemporaryDirectory() as td:
            make_fixture(td, {"drivers/new-thin/Cargo.toml": '[package]\nname = "n"\nversion = "0.1.0"\n',
                              "drivers/new-thin/src/lib.rs": "pub fn n() {}\n"})
            case("new crate, not yet a dependency", classify(td, ["drivers/new-thin/src/lib.rs"])[0], {"cross_arch"})
        with tempfile.TemporaryDirectory() as td:
            make_fixture(td, {"drivers/new-thin/Cargo.toml": '[package]\nname = "n"\nversion = "0.1.0"\n',
                              "drivers/new-thin/src/lib.rs": "pub fn n() {}\n",
                              "ffi/Cargo.toml": FIXTURE["ffi/Cargo.toml"] + 'n = { path = "../drivers/new-thin" }\n'})
            case("new crate as a path dep of ffi", classify(td, ["drivers/new-thin/src/lib.rs"])[0],
                 {"cross_arch", "zephyr", "zephyr_renode", "proofs"})
        # a crate reaching outside itself with #[path]
        with tempfile.TemporaryDirectory() as td:
            make_fixture(td, {"ffi/src/lib.rs": '#[path = "../../targets/generated/t.rs"]\nmod t;\n'})
            case("#[path] out of ffi", classify(td, ["targets/generated/t.rs"])[0],
                 {"zephyr", "zephyr_renode", "proofs", "gust_renode", "cross_arch"})
        # an include! nobody can resolve: that job always runs
        with tempfile.TemporaryDirectory() as td:
            make_fixture(td, {"ffi/src/lib.rs": 'include!(concat!(env!("SOMEWHERE"), "/x.rs"));\n'})
            case("unresolvable include! in ffi", classify(td, ["docs/guide.md"])[0], {"zephyr", "zephyr_renode", "proofs"})
        # a broken manifest: the graph is unreadable -> everything
        with tempfile.TemporaryDirectory() as td:
            make_fixture(td, {"ffi/Cargo.toml": "[package\n"})
            case("unparseable Cargo.toml", classify(td, ["docs/x.txt"])[0], ALL)
            case("unparseable Cargo.toml, driver change", classify(td, ["drivers/wdg-thin/src/lib.rs"])[0], ALL)
        # no tomllib (python < 3.11)
        global tomllib
        real = tomllib
        tomllib = None
        try:
            with tempfile.TemporaryDirectory() as td:
                make_fixture(td)
                # plain/ is also claimed by a non-cargo source (the Bazel graph),
                # so a walk that silently returned nothing would select only
                # `proofs` here -- a false skip of every cargo-built job.
                case("no tomllib", classify(td, ["plain/src/sem.rs"])[0], ALL)
        finally:
            tomllib = real
    finally:
        GATES, GATE_NAMES = saved
        shutil.rmtree(outdir, ignore_errors=True)

    print(f"\n  {total - len(failures)}/{total} planted case(s) classified as required")
    if failures:
        print("  self-test FAIL: " + "; ".join(failures))
        return 1
    print("  self-test PASS")
    return 0


def main(argv):
    root = os.getcwd()
    if "--self-test" in argv:
        return self_test()
    if "--check-graph" in argv:
        return check_graph(root)
    if "--derive-bazel" in argv:
        return derive_bazel(root)
    if "--files" in argv:
        paths = argv[argv.index("--files") + 1:]
        gates, classes, notes = classify(root, paths)
        for n in notes:
            print(f"  -> {n}")
        for line in emit(gates, classes, None):
            print(f"  {line}")
        return 0
    try:
        return run(root, os.environ, os.environ.get("GITHUB_OUTPUT"))
    except Exception as e:   # never let a crash skip a job: write all-true and fail loudly
        gates, classes, _ = all_true(f"classifier crashed: {e!r}")
        emit(gates, classes, os.environ.get("GITHUB_OUTPUT"))
        print(f"classify-changes: crashed ({e!r}); every output set true", file=sys.stderr)
        return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))

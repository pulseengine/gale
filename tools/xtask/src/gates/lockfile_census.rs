//! Every Cargo.toml and MODULE.bazel is covered by a committed, CURRENT lock
//! file, or is listed as lock-free with a reason (gale#534).
//!
//! lockfile-audit (the sibling gate) audits the locks that are committed. It
//! cannot see a manifest that has no lock, and an audited lock is worth nothing
//! when the build ignores it: a lock that no longer satisfies its manifest is
//! silently re-resolved by every build that does not pass `--locked`. This gate
//! closes both:
//!
//! - CENSUS. Every tracked Cargo.toml must be covered by a tracked Cargo.lock:
//!   its own, or that of the workspace root that claims it (`[workspace]`
//!   `members` globs, minus `exclude`, or `package.workspace`). Every tracked
//!   MODULE.bazel needs a tracked sibling MODULE.bazel.lock. Otherwise the
//!   manifest must be listed in tools/xtask/lockfile-census.toml with an issue
//!   and a reason. FAIL on an unlisted lock-free manifest, on a STALE listing
//!   (the manifest gained a lock, or is no longer tracked), on a duplicated,
//!   malformed or wildcard listing, on a lock with no manifest beside it, and on
//!   a member crate's own Cargo.lock (cargo ignores it in favour of the root's).
//! - CURRENCY. For every tracked Cargo.lock, `cargo metadata --locked
//!   --format-version 1` runs in the lock's directory (so that directory's
//!   rust-toolchain and .cargo/config apply, as they do to its build). It must
//!   exit 0. `--no-deps` is NOT used: it skips resolution, so it passes on a lock
//!   that no longer satisfies the manifest (measured: exit 0 where the full
//!   command exits 101). For every tracked MODULE.bazel.lock,
//!   `bazel mod deps --lockfile_mode=error` runs beside it and must exit 0. Any
//!   failure is FAIL: with every lock current, an unlocked build has nothing to
//!   re-resolve, so `cargo kani` and `cargo fuzz` (which have no `--locked`
//!   flag) build exactly what was audited.
//!
//! Enumeration is `git ls-files`, as in lockfile-audit: an untracked manifest
//! is not shipped. A missing cargo or bazel, or a repository with no Cargo.toml,
//! REFUSES (exit 3).
//!
//! Stated limit: a path dependency that lives inside a workspace root's
//! directory is a member by cargo's rules even when `members` does not name it.
//! This gate does not model that; such a crate is reported as uncovered (a loud
//! false FAIL, not a silent pass). None exists in this tree.

use super::lockfile_audit::ScratchRepo;
use super::object_freshness::is_issue_ref;
use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

pub const NAME: &str = "lockfile-census";

/// The lock-free ledger, relative to the repository root.
pub const LEDGER: &str = "tools/xtask/lockfile-census.toml";

/// How the currency checks are run. Separated so the self-test can run cargo
/// offline and point bazel at a scratch output base.
pub struct Tools {
    pub cargo: String,
    pub bazel: String,
    /// `CARGO_NET_OFFLINE=true`: the plants need no registry.
    pub cargo_offline: bool,
    /// Startup options placed before the bazel command.
    pub bazel_startup: Vec<String>,
}

impl Tools {
    pub fn real() -> Self {
        Tools { cargo: "cargo".into(), bazel: "bazel".into(), cargo_offline: false, bazel_startup: vec![] }
    }
}

// ---------------------------------------------------------------- paths

/// The directory part of a repo-relative path; "" for the root.
pub fn dir_of(p: &str) -> &str {
    p.rfind('/').map(|i| &p[..i]).unwrap_or("")
}

pub fn join(dir: &str, name: &str) -> String {
    if dir.is_empty() {
        name.to_string()
    } else {
        format!("{dir}/{name}")
    }
}

/// Lexically normalise `a/b/../c` -> `a/c`. None when it climbs out of the repo.
pub fn normalize(p: &str) -> Option<String> {
    let mut out: Vec<&str> = Vec::new();
    for seg in p.split('/') {
        match seg {
            "" | "." => {}
            ".." => {
                out.pop()?;
            }
            s => out.push(s),
        }
    }
    Some(out.join("/"))
}

/// `*` and `?` within one path segment, as cargo's `members` globs use them.
fn seg_match(pat: &[u8], s: &[u8]) -> bool {
    match (pat.first(), s.first()) {
        (None, None) => true,
        (Some(b'*'), _) => seg_match(&pat[1..], s) || (!s.is_empty() && seg_match(pat, &s[1..])),
        (Some(b'?'), Some(_)) => seg_match(&pat[1..], &s[1..]),
        (Some(a), Some(b)) if a == b => seg_match(&pat[1..], &s[1..]),
        _ => false,
    }
}

pub fn glob_match(pattern: &str, rel: &str) -> bool {
    let (Some(p), Some(r)) = (normalize(pattern), normalize(rel)) else { return false };
    let ps: Vec<&str> = p.split('/').collect();
    let rs: Vec<&str> = r.split('/').collect();
    ps.len() == rs.len() && ps.iter().zip(&rs).all(|(a, b)| seg_match(a.as_bytes(), b.as_bytes()))
}

// ---------------------------------------------------------------- manifests

/// What the census needs from one Cargo.toml.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Manifest {
    /// A `[workspace]` (or `[workspace.*]`) table is present.
    pub is_root: bool,
    pub members: Vec<String>,
    pub exclude: Vec<String>,
    /// `package.workspace = "..."`, relative to the manifest's directory.
    pub package_workspace: Option<String>,
}

fn strip_comment(line: &str) -> &str {
    let mut in_str = false;
    for (i, c) in line.char_indices() {
        match c {
            '"' => in_str = !in_str,
            '#' if !in_str => return &line[..i],
            _ => {}
        }
    }
    line
}

fn quoted(s: &str) -> Vec<String> {
    s.split('"').skip(1).step_by(2).map(str::to_string).collect()
}

/// A restricted reading of Cargo.toml: section headers, and the `members`,
/// `exclude` and `workspace` keys. Arrays may span lines.
pub fn parse_manifest(text: &str) -> Manifest {
    let mut m = Manifest::default();
    let mut section = String::new();
    let mut open: Option<(&'static str, String)> = None;
    for raw in text.lines() {
        let line = strip_comment(raw).trim();
        if let Some((key, acc)) = open.as_mut() {
            acc.push_str(line);
            acc.push(' ');
            if line.contains(']') {
                let vals = quoted(acc);
                if *key == "members" {
                    m.members = vals;
                } else {
                    m.exclude = vals;
                }
                open = None;
            }
            continue;
        }
        if line.starts_with('[') {
            section = line.trim_matches(|c| c == '[' || c == ']').trim().to_string();
            if section == "workspace" || section.starts_with("workspace.") {
                m.is_root = true;
            }
            continue;
        }
        let Some((k, v)) = line.split_once('=') else { continue };
        let (k, v) = (k.trim(), v.trim());
        if section == "workspace" && (k == "members" || k == "exclude") {
            let key: &'static str = if k == "members" { "members" } else { "exclude" };
            if v.contains(']') {
                if key == "members" {
                    m.members = quoted(v);
                } else {
                    m.exclude = quoted(v);
                }
            } else {
                open = Some((key, format!("{v} ")));
            }
        } else if section == "package" && k == "workspace" {
            m.package_workspace = quoted(v).into_iter().next();
        }
    }
    m
}

/// The tracked Cargo.lock that covers `manifest`, if any: its own, or its
/// workspace root's.
pub fn covering_lock(
    manifest: &str,
    parsed: &BTreeMap<String, Manifest>,
    locks: &BTreeSet<String>,
) -> Option<String> {
    let dir = dir_of(manifest);
    let own = join(dir, "Cargo.lock");
    if locks.contains(&own) {
        return Some(own);
    }
    workspace_root(manifest, parsed).and_then(|root| {
        let l = join(dir_of(&root), "Cargo.lock");
        locks.contains(&l).then_some(l)
    })
}

/// The manifest of the workspace root that claims `manifest` as a member, if
/// that is another manifest. None when the manifest is its own root.
pub fn workspace_root(manifest: &str, parsed: &BTreeMap<String, Manifest>) -> Option<String> {
    let me = parsed.get(manifest)?;
    if me.is_root {
        return None;
    }
    let dir = dir_of(manifest);
    if let Some(ws) = &me.package_workspace {
        let root = join(&normalize(&join(dir, ws))?, "Cargo.toml");
        return parsed.get(&root).filter(|r| r.is_root).map(|_| root);
    }
    // The nearest ancestor with a [workspace] decides, as cargo's search does.
    let mut a = dir;
    while !a.is_empty() {
        a = dir_of(a);
        let cand = join(a, "Cargo.toml");
        let Some(ws) = parsed.get(&cand) else { continue };
        if !ws.is_root {
            continue;
        }
        let rel = if a.is_empty() { dir.to_string() } else { dir[a.len() + 1..].to_string() };
        let excluded = ws.exclude.iter().any(|e| {
            normalize(e).is_some_and(|e| !e.is_empty() && (rel == e || rel.starts_with(&format!("{e}/"))))
        });
        if excluded {
            return None;
        }
        return ws.members.iter().any(|p| glob_match(p, &rel)).then_some(cand);
    }
    None
}

// ---------------------------------------------------------------- ledger

#[derive(Debug, Default, Clone, PartialEq)]
pub struct LockFree {
    pub manifest: String,
    pub issue: String,
    pub reason: String,
    pub line: usize,
}

/// `[[lockfree]]` headers, `key = "string"` lines, comments, blank lines.
/// Every deviation is a problem, not something skipped.
pub fn parse_ledger(text: &str) -> (Vec<LockFree>, Vec<String>) {
    let mut out: Vec<LockFree> = Vec::new();
    let mut bad = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[lockfree]]" {
            out.push(LockFree { line: n, ..LockFree::default() });
            keys.clear();
            continue;
        }
        let Some((k, v)) = line.split_once(" = ") else {
            bad.push(format!("{LEDGER}:{n}: expected `key = \"value\"`, got `{line}`"));
            continue;
        };
        let (k, v) = (k.trim(), v.trim());
        if !(v.len() >= 2 && v.starts_with('"') && v.ends_with('"')) || v[1..v.len() - 1].contains('"') {
            bad.push(format!("{LEDGER}:{n}: value must be one double-quoted string"));
            continue;
        }
        let v = v[1..v.len() - 1].to_string();
        let Some(e) = out.last_mut() else {
            bad.push(format!("{LEDGER}:{n}: key before the first [[lockfree]]"));
            continue;
        };
        if keys.iter().any(|x| x == k) {
            bad.push(format!("{LEDGER}:{n}: key `{k}` repeated in one listing (the last would silently win)"));
            continue;
        }
        keys.push(k.to_string());
        match k {
            "manifest" => e.manifest = v,
            "issue" => e.issue = v,
            "reason" => e.reason = v,
            other => bad.push(format!("{LEDGER}:{n}: unknown key `{other}`")),
        }
    }
    (out, bad)
}

// ---------------------------------------------------------------- census

/// The tracked files the census reads.
#[derive(Debug, Default, Clone)]
pub struct Tree {
    pub cargo: BTreeMap<String, Manifest>,
    pub cargo_locks: BTreeSet<String>,
    pub modules: BTreeSet<String>,
    pub module_locks: BTreeSet<String>,
}

fn is_named(p: &str, name: &str) -> bool {
    p == name || p.ends_with(&format!("/{name}"))
}

/// The outcome of the census, before currency.
#[derive(Debug, Default)]
pub struct Census {
    pub own: usize,
    pub by_root: usize,
    pub listed: Vec<String>,
    pub bad: Vec<String>,
}

pub fn census(t: &Tree, ledger: &[LockFree]) -> Census {
    let mut c = Census::default();
    let listed: BTreeSet<&str> = ledger.iter().map(|e| e.manifest.as_str()).collect();
    let mut covered: BTreeSet<&str> = BTreeSet::new();

    for m in t.cargo.keys() {
        match covering_lock(m, &t.cargo, &t.cargo_locks) {
            Some(l) => {
                covered.insert(m);
                if l == join(dir_of(m), "Cargo.lock") {
                    c.own += 1;
                    // A member's own lock is dead weight: cargo resolves the
                    // whole workspace into the root's lock and never reads it.
                    if let Some(root) = workspace_root(m, &t.cargo) {
                        c.bad.push(format!(
                            "{l}: {m} is a member of the workspace at {root}; cargo ignores this lock -- delete it, or make the crate its own root"
                        ));
                    }
                } else {
                    c.by_root += 1;
                }
            }
            None if listed.contains(m.as_str()) => c.listed.push(m.clone()),
            None => c.bad.push(format!(
                "{m}: no committed Cargo.lock covers it (neither its own nor a workspace root's) -- commit one, or list it in {LEDGER} with an issue and a reason"
            )),
        }
    }
    for m in &t.modules {
        let lock = join(dir_of(m), "MODULE.bazel.lock");
        if t.module_locks.contains(&lock) {
            covered.insert(m);
            c.own += 1;
        } else if listed.contains(m.as_str()) {
            c.listed.push(m.clone());
        } else {
            c.bad.push(format!(
                "{m}: no committed MODULE.bazel.lock beside it -- commit one, or list it in {LEDGER} with an issue and a reason"
            ));
        }
    }
    for l in &t.cargo_locks {
        if !t.cargo.contains_key(&join(dir_of(l), "Cargo.toml")) {
            c.bad.push(format!("{l}: a committed lock with no tracked Cargo.toml beside it"));
        }
    }
    for l in &t.module_locks {
        if !t.modules.contains(&join(dir_of(l), "MODULE.bazel")) {
            c.bad.push(format!("{l}: a committed lock with no tracked MODULE.bazel beside it"));
        }
    }

    let mut seen: BTreeSet<&str> = BTreeSet::new();
    for e in ledger {
        let at = format!("{LEDGER}:{} ({})", e.line, e.manifest);
        for (field, val) in [("manifest", &e.manifest), ("issue", &e.issue), ("reason", &e.reason)] {
            if val.trim().is_empty() {
                c.bad.push(format!("{at}: missing `{field}`"));
            }
        }
        if e.manifest.is_empty() {
            continue;
        }
        if e.manifest.contains(['*', '?', '[', ']', '{', '}']) {
            c.bad.push(format!("{at}: wildcards are not allowed; one listing per manifest"));
            continue;
        }
        if !e.issue.is_empty() && !is_issue_ref(&e.issue) {
            c.bad.push(format!("{at}: issue `{}` is not `#N` or `owner/repo#N`", e.issue));
        }
        if !seen.insert(&e.manifest) {
            c.bad.push(format!("{at}: DUPLICATE listing"));
        }
        let tracked = t.cargo.contains_key(&e.manifest) || t.modules.contains(&e.manifest);
        if !tracked {
            c.bad.push(format!("{at}: STALE -- not a tracked Cargo.toml or MODULE.bazel; remove the listing"));
        } else if covered.contains(e.manifest.as_str()) {
            c.bad.push(format!("{at}: STALE -- a committed lock covers it now; remove the listing"));
        }
    }
    c
}

// ---------------------------------------------------------------- currency

/// Tracked files, via the index. None when git itself fails.
pub fn tracked(repo: &Path) -> Option<Vec<String>> {
    let out = Command::new("git").arg("-C").arg(repo).args(["ls-files", "-z"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        out.stdout
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect(),
    )
}

fn first_error(stderr: &str) -> String {
    let lines: Vec<&str> = stderr.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    lines
        .iter()
        .find(|l| l.starts_with("error") || l.starts_with("ERROR"))
        .or(lines.last())
        .map(|l| l.chars().take(300).collect())
        .unwrap_or_else(|| "(no output)".into())
}

/// One currency check's exit and stderr -> None (current) or why not.
/// `Err` when the tool could not be run at all.
pub fn interpret_currency(kind: &str, lock: &str, exit: Option<i32>, stderr: &str) -> Option<String> {
    match exit {
        Some(0) => None,
        Some(code) => {
            let stale = stderr.contains("--locked was passed")
                || stderr.contains("lockfile_mode=error")
                || stderr.contains("no longer up-to-date");
            let what = if stale {
                "OUT OF DATE: the lock no longer satisfies the manifest"
            } else {
                "could not be verified current"
            };
            Some(format!("{lock}: {what} ({kind} exited {code}): {}", first_error(stderr)))
        }
        None => Some(format!("{lock}: {kind} was killed by a signal; currency not verified")),
    }
}

fn check_cargo(repo: &Path, tools: &Tools, lock: &str) -> Result<Option<String>, String> {
    let dir = repo.join(dir_of(lock));
    let mut c = Command::new(&tools.cargo);
    c.args(["metadata", "--locked", "--format-version", "1", "--manifest-path"])
        .arg(dir.join("Cargo.toml"))
        .current_dir(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    if tools.cargo_offline {
        c.env("CARGO_NET_OFFLINE", "true");
    }
    let out = c.output().map_err(|e| format!("cannot run `{} metadata`: {e}", tools.cargo))?;
    Ok(interpret_currency("cargo metadata --locked", lock, out.status.code(), &String::from_utf8_lossy(&out.stderr)))
}

fn check_bazel(repo: &Path, tools: &Tools, lock: &str) -> Result<Option<String>, String> {
    let dir = repo.join(dir_of(lock));
    let out = Command::new(&tools.bazel)
        .args(&tools.bazel_startup)
        .args(["mod", "deps", "--lockfile_mode=error"])
        .current_dir(&dir)
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("cannot run `{} mod deps`: {e} (install bazelisk)", tools.bazel))?;
    Ok(interpret_currency("bazel mod deps --lockfile_mode=error", lock, out.status.code(), &String::from_utf8_lossy(&out.stderr)))
}

/// Read the tree from the index and the working copy.
fn read_tree(repo: &Path, files: &[String]) -> Result<Tree, String> {
    let mut t = Tree::default();
    for p in files {
        if is_named(p, "Cargo.toml") {
            let text = std::fs::read_to_string(repo.join(p)).map_err(|e| format!("{p}: {e}"))?;
            t.cargo.insert(p.clone(), parse_manifest(&text));
        } else if is_named(p, "Cargo.lock") {
            t.cargo_locks.insert(p.clone());
        } else if is_named(p, "MODULE.bazel") {
            t.modules.insert(p.clone());
        } else if is_named(p, "MODULE.bazel.lock") {
            t.module_locks.insert(p.clone());
        }
    }
    Ok(t)
}

pub fn run(repo: &Path, target: Option<&str>) -> Verdict {
    let root: PathBuf = target.map(PathBuf::from).unwrap_or_else(|| repo.to_path_buf());
    run_with(&root, &Tools::real())
}

pub fn run_with(repo: &Path, tools: &Tools) -> Verdict {
    let Some(files) = tracked(repo) else {
        return Verdict::Refused(format!("git ls-files failed in {} -- the gate cannot answer", repo.display()));
    };
    let t = match read_tree(repo, &files) {
        Ok(t) => t,
        Err(m) => return Verdict::Refused(format!("cannot read a tracked manifest: {m}")),
    };
    if t.cargo.is_empty() {
        return Verdict::Refused("git ls-files lists no Cargo.toml -- a census of nothing is not a pass".into());
    }
    let Ok(text) = std::fs::read_to_string(repo.join(LEDGER)) else {
        return Verdict::Refused(format!("{LEDGER} is missing -- the gate cannot tell a listing from a failure"));
    };
    let (ledger, mut bad) = parse_ledger(&text);
    let c = census(&t, &ledger);
    bad.extend(c.bad);

    let mut current = 0usize;
    let mut checked = 0usize;
    let paired_cargo: Vec<&String> =
        t.cargo_locks.iter().filter(|l| t.cargo.contains_key(&join(dir_of(l), "Cargo.toml"))).collect();
    let paired_bazel: Vec<&String> =
        t.module_locks.iter().filter(|l| t.modules.contains(&join(dir_of(l), "MODULE.bazel"))).collect();
    for l in &paired_cargo {
        checked += 1;
        match check_cargo(repo, tools, l) {
            Ok(None) => current += 1,
            Ok(Some(m)) => bad.push(m),
            Err(m) => return Verdict::Refused(m),
        }
    }
    for l in &paired_bazel {
        checked += 1;
        match check_bazel(repo, tools, l) {
            Ok(None) => current += 1,
            Ok(Some(m)) => bad.push(m),
            Err(m) => return Verdict::Refused(m),
        }
    }

    let mut lines = vec![
        format!(
            "census: {} Cargo.toml + {} MODULE.bazel tracked; {} covered by their own lock, {} by a workspace root's lock, {} listed lock-free",
            t.cargo.len(),
            t.modules.len(),
            c.own,
            c.by_root,
            c.listed.len()
        ),
        format!(
            "currency: {current} of {checked} lock(s) current ({} Cargo.lock via `cargo metadata --locked`, {} MODULE.bazel.lock via `bazel mod deps --lockfile_mode=error`)",
            paired_cargo.len(),
            paired_bazel.len()
        ),
    ];
    for m in &c.listed {
        lines.push(format!("lock-free (listed): {m}"));
    }
    if bad.is_empty() {
        Verdict::Pass(lines)
    } else {
        lines.push(format!("FAIL: {} problem(s):", bad.len()));
        lines.extend(bad.into_iter().map(|b| format!("  {b}")));
        Verdict::Fail(lines)
    }
}

// ---------------------------------------------------------------- self-test

fn pkg(name: &str, version: &str, extra: &str) -> String {
    format!("[package]\nname = \"{name}\"\nversion = \"{version}\"\nedition = \"2021\"\n{extra}")
}

/// app depends on ../dep by path at `=0.2.0`; dep is its own root at 0.2.0.
fn app_manifest() -> String {
    pkg("app", "0.1.0", "\n[workspace]\n\n[dependencies]\ndep = { path = \"../dep\", version = \"=0.2.0\" }\n")
}
fn app_lock(dep_version: &str) -> String {
    format!(
        "version = 3\n\n[[package]]\nname = \"app\"\nversion = \"0.1.0\"\ndependencies = [\n \"dep\",\n]\n\n[[package]]\nname = \"dep\"\nversion = \"{dep_version}\"\n"
    )
}
const DEP_LOCK: &str = "version = 3\n\n[[package]]\nname = \"dep\"\nversion = \"0.2.0\"\n";

pub fn listing(manifest: &str) -> String {
    format!("[[lockfree]]\nmanifest = \"{manifest}\"\nissue = \"#534\"\nreason = \"fixture\"\n")
}

fn tree(cargo: &[(&str, &str)], locks: &[&str], modules: &[&str], mlocks: &[&str]) -> Tree {
    Tree {
        cargo: cargo.iter().map(|(p, t)| (p.to_string(), parse_manifest(t))).collect(),
        cargo_locks: locks.iter().map(|s| s.to_string()).collect(),
        modules: modules.iter().map(|s| s.to_string()).collect(),
        module_locks: mlocks.iter().map(|s| s.to_string()).collect(),
    }
}

fn lf(m: &str) -> LockFree {
    LockFree { manifest: m.into(), issue: "#534".into(), reason: "r".into(), line: 1 }
}

/// Checks that need neither cargo, bazel nor the network. `cargo test` runs
/// these; `--self-test` runs them and the tool-backed plants.
pub fn pure_checks(ck: &mut dyn FnMut(&str, bool)) {
    // manifest reading
    let ws = parse_manifest("[workspace]\nmembers = [\n  \"crates/*\", # all\n  \"tools/x\",\n]\nexclude = [\"crates/skip\"]\n");
    ck("manifest: multi-line members + exclude", ws.is_root && ws.members == ["crates/*", "tools/x"] && ws.exclude == ["crates/skip"]);
    ck("manifest: [workspace.dependencies] makes a root", parse_manifest("[workspace.dependencies]\na = \"1\"\n").is_root);
    ck("manifest: a plain package is not a root", !parse_manifest(&pkg("a", "0.1.0", "")).is_root);
    ck("manifest: `# [workspace]` in a comment is not a root", !parse_manifest("# [workspace]\n[package]\nname = \"a\"\n").is_root);
    ck(
        "manifest: package.workspace is read",
        parse_manifest("[package]\nname = \"a\"\nworkspace = \"../..\"\n").package_workspace.as_deref() == Some("../.."),
    );
    ck("glob: crates/* matches crates/a", glob_match("crates/*", "crates/a"));
    ck("glob: crates/* does not match crates/a/b", !glob_match("crates/*", "crates/a/b"));
    ck("glob: ./tools/x matches tools/x", glob_match("./tools/x", "tools/x"));

    // coverage through a workspace root, honouring members and exclude
    let root_ws = "[workspace]\nmembers = [\"crates/*\"]\nexclude = [\"crates/skip\"]\n";
    let member = pkg("m", "0.1.0", "");
    let t = tree(
        &[
            ("ws/Cargo.toml", root_ws),
            ("ws/crates/a/Cargo.toml", &member),
            ("ws/crates/skip/Cargo.toml", &member),
            ("ws/other/Cargo.toml", &member),
        ],
        &["ws/Cargo.lock"],
        &[],
        &[],
    );
    ck(
        "coverage: a members-glob member is covered by the root's lock",
        covering_lock("ws/crates/a/Cargo.toml", &t.cargo, &t.cargo_locks).as_deref() == Some("ws/Cargo.lock"),
    );
    ck("coverage: an excluded crate is not covered", covering_lock("ws/crates/skip/Cargo.toml", &t.cargo, &t.cargo_locks).is_none());
    ck("coverage: a non-member under the root is not covered", covering_lock("ws/other/Cargo.toml", &t.cargo, &t.cargo_locks).is_none());
    let c = census(&t, &[]);
    ck("census: excluded + non-member, unlisted -> 2 problems", c.bad.len() == 2 && c.own == 1 && c.by_root == 1);
    let c = census(&t, &[lf("ws/crates/skip/Cargo.toml"), lf("ws/other/Cargo.toml")]);
    ck("census: both listed -> clean, 2 listed", c.bad.is_empty() && c.listed.len() == 2);
    let t2 = tree(
        &[("ws/Cargo.toml", "[workspace]\n"), ("ws/sub/Cargo.toml", &pkg("s", "0.1.0", "workspace = \"..\"\n"))],
        &["ws/Cargo.lock"],
        &[],
        &[],
    );
    ck(
        "coverage: package.workspace points at a root -> covered",
        covering_lock("ws/sub/Cargo.toml", &t2.cargo, &t2.cargo_locks).as_deref() == Some("ws/Cargo.lock"),
    );
    let t3 = tree(&[("ws/Cargo.toml", root_ws), ("ws/crates/a/Cargo.toml", &member)], &["ws/Cargo.lock", "ws/crates/a/Cargo.lock"], &[], &[]);
    ck("census: a member's own lock (ignored by cargo) -> problem", census(&t3, &[]).bad.iter().any(|m| m.contains("cargo ignores")));

    // listings
    let t4 = tree(&[("a/Cargo.toml", &member), ("b/Cargo.toml", &member)], &["a/Cargo.lock"], &["MODULE.bazel"], &[]);
    ck("census: unlisted lock-free Cargo.toml -> problem", census(&t4, &[lf("MODULE.bazel")]).bad.len() == 1);
    ck("census: MODULE.bazel without lock or listing -> problem", census(&t4, &[lf("b/Cargo.toml")]).bad.len() == 1);
    ck("census: everything listed -> clean", census(&t4, &[lf("b/Cargo.toml"), lf("MODULE.bazel")]).bad.is_empty());
    let both = [lf("b/Cargo.toml"), lf("MODULE.bazel")];
    let with = |e: LockFree| census(&t4, &[both[0].clone(), both[1].clone(), e]).bad;
    ck("listing: a manifest that has a lock -> STALE", with(lf("a/Cargo.toml")).iter().any(|m| m.contains("STALE")));
    ck("listing: a manifest that is gone -> STALE", with(lf("gone/Cargo.toml")).iter().any(|m| m.contains("STALE")));
    ck("listing: duplicate -> problem", with(lf("b/Cargo.toml")).iter().any(|m| m.contains("DUPLICATE")));
    ck("listing: wildcard -> problem", with(lf("*/Cargo.toml")).iter().any(|m| m.contains("wildcard")));
    ck("listing: missing reason -> problem", with(LockFree { reason: String::new(), ..lf("c/Cargo.toml") }).len() >= 2);
    ck("listing: issue not #N -> problem", with(LockFree { issue: "TODO".into(), ..lf("b/Cargo.toml") }).iter().any(|m| m.contains("issue")));
    let t5 = tree(&[("a/Cargo.toml", &member)], &["a/Cargo.lock", "z/Cargo.lock"], &[], &["y/MODULE.bazel.lock"]);
    ck("census: locks with no manifest beside them -> 2 problems", census(&t5, &[]).bad.len() == 2);

    // ledger parser
    let (e, b) = parse_ledger(&listing("a/Cargo.toml"));
    ck("ledger: well-formed listing parses", e.len() == 1 && b.is_empty());
    let (_, b) = parse_ledger(&format!("{}issue = \"#1\"\n", listing("a/Cargo.toml")));
    ck("ledger: repeated key -> problem", b.len() == 1);
    let (_, b) = parse_ledger("[[lockfree]]\nbogus = \"x\"\n");
    ck("ledger: unknown key -> problem", b.len() == 1);

    // currency interpretation
    ck("currency: exit 0 -> current", interpret_currency("cargo", "Cargo.lock", Some(0), "").is_none());
    let stale = interpret_currency(
        "cargo",
        "Cargo.lock",
        Some(101),
        "error: cannot update the lock file Cargo.lock because --locked was passed to prevent this\n",
    );
    ck("currency: --locked refusal -> OUT OF DATE", stale.as_deref().is_some_and(|m| m.contains("OUT OF DATE")));
    ck(
        "currency: any other failure -> not verified (still a problem)",
        interpret_currency("cargo", "Cargo.lock", Some(101), "error: failed to download\n").is_some_and(|m| m.contains("not be verified")),
    );
    ck("currency: killed -> problem", interpret_currency("cargo", "Cargo.lock", None, "").is_some());

    // refusals that need no tool
    let none = Tools { cargo: "gale-no-such-cargo".into(), bazel: "gale-no-such-bazel".into(), cargo_offline: true, bazel_startup: vec![] };
    ck(
        "a directory that is not a git repository -> REFUSED",
        matches!(run_with(&std::env::temp_dir().join("gale-lockfile-census-no-such-dir"), &none), Verdict::Refused(_)),
    );
    match ScratchRepo::new("census-empty", &[("README", "x\n"), (LEDGER, "")]) {
        Ok(s) => ck("no tracked Cargo.toml -> REFUSED", matches!(run_with(&s.0, &none), Verdict::Refused(_))),
        Err(m) => ck(&format!("scratch repo: {m}"), false),
    }
    match ScratchRepo::new("census-nocargo", &[("a/Cargo.toml", &member), ("a/src/lib.rs", ""), ("a/Cargo.lock", DEP_LOCK), (LEDGER, "")]) {
        Ok(s) => ck("cargo missing -> REFUSED", matches!(run_with(&s.0, &none), Verdict::Refused(_))),
        Err(m) => ck(&format!("scratch repo: {m}"), false),
    }
}

fn is_fail_with(v: &Verdict, needle: &str) -> bool {
    matches!(v, Verdict::Fail(l) if l.iter().any(|x| x.contains(needle)))
}

fn probe(out: &mut Vec<(String, bool)>, what: &str, r: Result<Verdict, String>, want: &dyn Fn(&Verdict) -> bool) {
    match r {
        Ok(v) => {
            let got = want(&v);
            out.push((what.to_string(), got));
            if !got {
                out.push((format!("  (got: {})", v.to_string().replace('\n', " | ")), false));
            }
        }
        Err(m) => out.push((format!("{what}: scratch repo: {m}"), false)),
    }
}

pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut res: Vec<(String, bool)> = Vec::new();
    pure_checks(&mut |what: &str, got: bool| res.push((what.to_string(), got)));

    // Tool-backed plants: the real cargo (offline: path dependencies only) and
    // the real bazel, against scratch git repositories.
    let tools = Tools { cargo_offline: true, ..Tools::real() };
    let app = app_manifest();
    let dep = pkg("dep", "0.2.0", "\n[workspace]\n");
    let base = |app_lock_text: String, extra: Vec<(&'static str, String)>, ledger: String| -> Vec<(String, String)> {
        let mut f: Vec<(String, String)> = vec![
            ("app/Cargo.toml".into(), app.clone()),
            ("app/src/lib.rs".into(), String::new()),
            ("app/Cargo.lock".into(), app_lock_text),
            ("dep/Cargo.toml".into(), dep.clone()),
            ("dep/src/lib.rs".into(), String::new()),
            ("dep/Cargo.lock".into(), DEP_LOCK.into()),
            (LEDGER.into(), ledger),
        ];
        f.extend(extra.into_iter().map(|(p, t)| (p.to_string(), t)));
        f
    };
    let scenario = |tag: &str, files: Vec<(String, String)>, tools: &Tools| -> Result<Verdict, String> {
        let refs: Vec<(&str, &str)> = files.iter().map(|(p, t)| (p.as_str(), t.as_str())).collect();
        let s = ScratchRepo::new(tag, &refs)?;
        Ok(run_with(&s.0, tools))
    };

    let ok = scenario("census-ok", base(app_lock("0.2.0"), vec![], String::new()), &tools);
    if let Ok(Verdict::Refused(m)) = &ok {
        return Verdict::Refused(format!("the tool-backed plants cannot run: {m}"));
    }
    probe(&mut res, "positive control: current locks, nothing lock-free -> PASS", ok, &|v| {
        matches!(v, Verdict::Pass(l) if l.iter().any(|x| x.contains("currency: 2 of 2")))
    });
    probe(
        &mut res,
        "plant (i): app requires dep =0.2.0, its lock has dep 0.1.0 -> FAIL (OUT OF DATE)",
        scenario("census-i", base(app_lock("0.1.0"), vec![], String::new()), &tools),
        &|v| is_fail_with(v, "app/Cargo.lock: OUT OF DATE"),
    );
    let stray = pkg("stray", "0.1.0", "\n[workspace]\n");
    probe(
        &mut res,
        "plant (ii): an unlisted lock-free Cargo.toml -> FAIL",
        scenario("census-ii", base(app_lock("0.2.0"), vec![("stray/Cargo.toml", stray.clone())], String::new()), &tools),
        &|v| is_fail_with(v, "stray/Cargo.toml: no committed Cargo.lock covers it"),
    );
    probe(
        &mut res,
        "positive control: the same crate, listed -> PASS",
        scenario("census-ii-ok", base(app_lock("0.2.0"), vec![("stray/Cargo.toml", stray)], listing("stray/Cargo.toml")), &tools),
        &|v| matches!(v, Verdict::Pass(_)),
    );
    probe(
        &mut res,
        "plant (iii): a listing for a manifest that has a lock -> FAIL (STALE)",
        scenario("census-iii", base(app_lock("0.2.0"), vec![], listing("app/Cargo.toml")), &tools),
        &|v| is_fail_with(v, "STALE -- a committed lock covers it now"),
    );
    probe(
        &mut res,
        "plant (iii'): a listing for a manifest that is gone -> FAIL (STALE)",
        scenario("census-iii-gone", base(app_lock("0.2.0"), vec![], listing("gone/Cargo.toml")), &tools),
        &|v| is_fail_with(v, "STALE -- not a tracked"),
    );
    probe(
        &mut res,
        "plant (iv): a MODULE.bazel with no lock and no listing -> FAIL",
        scenario("census-iv", base(app_lock("0.2.0"), vec![("MODULE.bazel", "module(name = \"plant\")\n".into())], String::new()), &tools),
        &|v| is_fail_with(v, "MODULE.bazel: no committed MODULE.bazel.lock"),
    );

    // plant (v): a MODULE.bazel.lock that lacks a dependency the module
    // declares, run through the real bazel in a throwaway output base. The
    // Bazel version is the repository's own, so no second download.
    let ob = std::env::temp_dir().join(format!("gale-lockcensus-bazel-ob-{}", std::process::id()));
    let bazel_tools = Tools {
        cargo_offline: true,
        bazel_startup: vec!["--batch".into(), format!("--output_base={}", ob.display())],
        ..Tools::real()
    };
    let bv = std::fs::read_to_string(repo.join(".bazelversion")).unwrap_or_default();
    let module = "module(name = \"plant\")\n\nbazel_dep(name = \"platforms\", version = \"0.0.11\")\n";
    let empty_lock = "{\n  \"lockFileVersion\": 26,\n  \"registryFileHashes\": {},\n  \"selectedYankedVersions\": {},\n  \"moduleExtensions\": {},\n  \"facts\": {}\n}\n";
    let mut extra = vec![("MODULE.bazel", module.to_string()), ("MODULE.bazel.lock", empty_lock.to_string())];
    if !bv.trim().is_empty() {
        extra.push((".bazelversion", bv));
    }
    probe(
        &mut res,
        "plant (v): a MODULE.bazel.lock missing a declared bazel_dep -> FAIL",
        scenario("census-v", base(app_lock("0.2.0"), extra, String::new()), &bazel_tools),
        &|v| is_fail_with(v, "MODULE.bazel.lock: OUT OF DATE"),
    );
    // Best effort: bazel leaves read-only files in its output base.
    let _ = Command::new("chmod").arg("-R").arg("u+w").arg(&ob).status();
    let _ = std::fs::remove_dir_all(&ob);

    let all_ok = res.iter().all(|(_, g)| *g);
    let mut lines: Vec<String> =
        res.into_iter().map(|(w, g)| if w.starts_with("  (got") { w } else { format!("{w}: {}", if g { "ok" } else { "WRONG" }) }).collect();
    if all_ok {
        lines.push("PASS".into());
        Verdict::Pass(lines)
    } else {
        Verdict::Fail(lines)
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn pure_checks_hold() {
        let mut wrong = Vec::new();
        super::pure_checks(&mut |what: &str, got: bool| {
            if !got {
                wrong.push(what.to_string());
            }
        });
        assert!(wrong.is_empty(), "wrong: {wrong:?}");
    }
}

//! Every committed Cargo.lock is audited for RustSec vulnerabilities (gale#534).
//!
//! supply-chain.yml ran `cargo deny check advisories` once, against the root
//! package. The repository commits 45 lock files; the other 44 were never
//! audited, and one of them (drivers/gustos-hostrun) carried 19 RustSec
//! vulnerabilities while CI stayed green. This gate audits EVERY lock file git
//! tracks.
//!
//! How:
//! - Enumerate with `git ls-files` (a path that is `Cargo.lock` or ends in
//!   `/Cargo.lock`), never a filesystem glob: an untracked lock is not shipped,
//!   and a glob would also see build directories. Zero lock files REFUSES.
//! - Audit each with `cargo audit --json -f <lock>`. cargo-deny is not used per
//!   lock: it needs a manifest and `cargo metadata`, which for the nested bench
//!   crates means their own targets, toolchains and git sources. cargo-audit
//!   reads the lock file alone.
//! - The verdict comes from the parsed JSON AND the exit code, and they must
//!   agree (exit 1 iff `vulnerabilities.count > 0`, `count == list.len()`,
//!   `found == (count > 0)`). Any disagreement, any exit other than 0/1, output
//!   that is not the expected JSON, an empty advisory database, or a missing
//!   tool REFUSES (exit 3) -- an audit that did not complete is not a pass. A
//!   failure to fetch the database surfaces as exit 1 with no JSON: refused.
//! - cargo-audit's own ignore list (`settings.ignore`) and severity threshold
//!   must be empty: an `audit.toml` somewhere would otherwise hide advisories
//!   from the ledger below.
//!
//! Exceptions live in tools/xtask/lockfile-audit.toml, one `[[exception]]` per
//! (lock file, advisory id), each with an issue and an expiry at most
//! MAX_EXPIRY_DAYS out. FAIL on: a vulnerability with no exception; an
//! exception that is expired, malformed, duplicated, beyond the cap, or uses a
//! wildcard; and a STALE exception (the advisory no longer appears in that lock,
//! or the lock is no longer committed), so the ledger can only shrink.
//!
//! Unmaintained / unsound / other informational warnings are REPORTED per lock
//! and in total, and do not fail. They are not exploitable defects, most have
//! no fix a lock bump can apply, and a gate that is red on them would fill its
//! ledger with entries that dilute the one signal it exists for. The root
//! package's cargo-deny step still applies the org policy to its own lock.
//!
//! Blind spot, stated: a lock file is audited as committed. A crate that builds
//! without its lock (a library, or `cargo install` without `--locked`) resolves
//! newer versions this gate never sees.

use super::object_freshness::{is_issue_ref, parse_day, today, ymd};
use super::target_reference::{parse_json, Json};
use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const NAME: &str = "lockfile-audit";

/// The exception ledger, relative to the repository root.
pub const LEDGER: &str = "tools/xtask/lockfile-audit.toml";

/// The furthest an exception's expiry may lie in the future.
const MAX_EXPIRY_DAYS: i64 = 90;

/// Below this many days to the earliest expiry, CI prints a warning.
const EXPIRY_WARN_DAYS: i64 = 14;

/// How the audit tool is invoked. `program args.. --json [--no-fetch] -f <lock>`.
pub struct Tool {
    pub program: String,
    pub args: Vec<String>,
}

impl Tool {
    pub fn cargo_audit() -> Self {
        Tool { program: "cargo".into(), args: vec!["audit".into()] }
    }
}

/// One lock file's audit, as parsed from the tool's JSON.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct Report {
    /// advisory id -> the affected `name@version`s in this lock.
    pub vulns: BTreeMap<String, BTreeSet<String>>,
    /// warning kind (unmaintained, unsound, ...) -> count. Reported, not failed.
    pub warnings: BTreeMap<String, usize>,
    pub db_advisories: u64,
    pub db_commit: String,
}

fn num(j: Option<&Json>) -> Option<f64> {
    match j {
        Some(Json::Num(n)) => Some(*n),
        _ => None,
    }
}

/// The tool's exit status and stdout -> a report, or why the audit cannot be
/// trusted. Separated from I/O so the self-test can plant disagreements.
pub fn interpret(exit: Option<i32>, stdout: &str) -> Result<Report, String> {
    let Some(code) = exit else {
        return Err("the audit tool was killed by a signal".into());
    };
    if code != 0 && code != 1 {
        return Err(format!("the audit tool exited {code}: it did not complete"));
    }
    let j = parse_json(stdout.trim()).map_err(|e| {
        format!("exit {code} with no parseable JSON report ({e}): the audit did not complete (advisory database unavailable?)")
    })?;
    let db = j.get("database").ok_or("JSON report has no `database`")?;
    let db_advisories = num(db.get("advisory-count")).ok_or("JSON report has no `database.advisory-count`")?;
    if db_advisories < 1.0 {
        return Err("the advisory database is empty: an audit against it is vacuous".into());
    }
    let db_commit = db.get("last-commit").and_then(Json::str).unwrap_or("?").to_string();
    let settings = j.get("settings").ok_or("JSON report has no `settings`")?;
    match settings.get("ignore") {
        Some(Json::Arr(v)) if v.is_empty() => {}
        Some(Json::Arr(v)) => {
            return Err(format!(
                "the audit tool ignored {} advisor(ies) through its own config (audit.toml): exceptions belong in {LEDGER}",
                v.len()
            ))
        }
        _ => return Err("JSON report has no `settings.ignore` array".into()),
    }
    match settings.get("severity") {
        Some(Json::Null) | None => {}
        Some(_) => return Err("the audit tool ran with a severity threshold: advisories below it are hidden".into()),
    }
    let v = j.get("vulnerabilities").ok_or("JSON report has no `vulnerabilities`")?;
    let found = match v.get("found") {
        Some(Json::Bool(b)) => *b,
        _ => return Err("JSON report has no `vulnerabilities.found`".into()),
    };
    let count = num(v.get("count")).ok_or("JSON report has no `vulnerabilities.count`")?;
    let list = match v.get("list") {
        Some(Json::Arr(l)) => l,
        _ => return Err("JSON report has no `vulnerabilities.list`".into()),
    };
    if count < 0.0 || count.fract() != 0.0 || count as usize != list.len() {
        return Err(format!("the report disagrees with itself: count {count} but {} listed", list.len()));
    }
    if found != (count > 0.0) {
        return Err(format!("the report disagrees with itself: found={found} but count {count}"));
    }
    if (code == 1) != (count > 0.0) {
        return Err(format!(
            "exit code and report disagree: exit {code} with {count} vulnerabilit(ies) -- neither is trusted"
        ));
    }
    let mut vulns: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    for item in list {
        let id = item.get("advisory").and_then(|a| a.get("id")).and_then(Json::str);
        let pkg = item.get("package");
        let name = pkg.and_then(|p| p.get("name")).and_then(Json::str);
        let ver = pkg.and_then(|p| p.get("version")).and_then(Json::str);
        match (id, name, ver) {
            (Some(id), Some(n), Some(ver)) if !id.is_empty() => {
                vulns.entry(id.to_string()).or_default().insert(format!("{n}@{ver}"));
            }
            _ => return Err("a vulnerability entry lacks advisory.id / package.name / package.version".into()),
        }
    }
    let mut warnings = BTreeMap::new();
    match j.get("warnings") {
        Some(Json::Obj(kinds)) => {
            for (k, list) in kinds {
                let Json::Arr(l) = list else { return Err(format!("`warnings.{k}` is not an array")) };
                warnings.insert(k.clone(), l.len());
            }
        }
        _ => return Err("JSON report has no `warnings` object".into()),
    }
    Ok(Report { vulns, warnings, db_advisories: db_advisories as u64, db_commit })
}

fn audit_one(repo: &Path, tool: &Tool, lock: &str, fetch: bool) -> Result<Report, String> {
    let mut c = Command::new(&tool.program);
    c.args(&tool.args).arg("--json");
    if !fetch {
        c.arg("--no-fetch");
    }
    c.arg("-f").arg(repo.join(lock)).current_dir(repo);
    let out = c
        .output()
        .map_err(|e| format!("cannot run `{} {}`: {e} (install cargo-audit)", tool.program, tool.args.join(" ")))?;
    let stdout = String::from_utf8_lossy(&out.stdout);
    interpret(out.status.code(), &stdout).map_err(|m| {
        let err = String::from_utf8_lossy(&out.stderr);
        match err.lines().map(str::trim).find(|l| !l.is_empty()) {
            Some(first) => format!("{m}; stderr: {first}"),
            None => m,
        }
    })
}

// ---------------------------------------------------------------- ledger

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Exception {
    pub lockfile: String,
    pub advisory: String,
    pub issue: String,
    pub expires: String,
    pub reason: String,
    /// 1-based line of the `[[exception]]` header, for messages.
    pub line: usize,
}

/// Parse the restricted TOML subset the ledger uses: `[[exception]]` headers,
/// `key = "string"` lines, comments, blank lines. Every deviation is a problem
/// (the gate FAILS on a malformed ledger), not something silently skipped.
pub fn parse_ledger(text: &str) -> (Vec<Exception>, Vec<String>) {
    let mut out: Vec<Exception> = Vec::new();
    let mut bad = Vec::new();
    let mut keys: Vec<String> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[exception]]" {
            out.push(Exception { line: n, ..Exception::default() });
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
            bad.push(format!("{LEDGER}:{n}: key before the first [[exception]]"));
            continue;
        };
        if keys.iter().any(|x| x == k) {
            bad.push(format!("{LEDGER}:{n}: key `{k}` repeated in one exception (the last would silently win)"));
            continue;
        }
        keys.push(k.to_string());
        match k {
            "lockfile" => e.lockfile = v,
            "advisory" => e.advisory = v,
            "issue" => e.issue = v,
            "expires" => e.expires = v,
            "reason" => e.reason = v,
            other => bad.push(format!("{LEDGER}:{n}: unknown key `{other}`")),
        }
    }
    (out, bad)
}

/// `RUSTSEC-YYYY-NNNN`, exactly. Rules out `RUSTSEC-2026-*` and friends.
pub fn is_advisory_id(s: &str) -> bool {
    let p: Vec<&str> = s.split('-').collect();
    p.len() == 3
        && p[0] == "RUSTSEC"
        && p[1].len() == 4
        && p[2].len() == 4
        && p[1].bytes().chain(p[2].bytes()).all(|b| b.is_ascii_digit())
}

fn has_wildcard(s: &str) -> bool {
    s.contains(['*', '?', '[', ']', '{', '}'])
}

/// Every exception on its own: well-formed, unique, unexpired, within the cap,
/// naming a committed lock file. Does not look at audit results.
pub fn check_exceptions(exs: &[Exception], locks: &[String], today: i64) -> Vec<String> {
    let mut bad = Vec::new();
    let mut seen: BTreeSet<(&str, &str)> = BTreeSet::new();
    for e in exs {
        let at = format!("{LEDGER}:{} ({} {})", e.line, e.lockfile, e.advisory);
        for (field, val) in [
            ("lockfile", &e.lockfile),
            ("advisory", &e.advisory),
            ("issue", &e.issue),
            ("expires", &e.expires),
            ("reason", &e.reason),
        ] {
            if val.trim().is_empty() {
                bad.push(format!("{at}: missing `{field}`"));
            }
        }
        if has_wildcard(&e.lockfile) || has_wildcard(&e.advisory) {
            bad.push(format!("{at}: wildcards are not allowed; one exception per (lock file, advisory)"));
        } else if !e.advisory.is_empty() && !is_advisory_id(&e.advisory) {
            bad.push(format!("{at}: advisory `{}` is not RUSTSEC-YYYY-NNNN", e.advisory));
        }
        if !e.lockfile.is_empty() && !has_wildcard(&e.lockfile) && !locks.iter().any(|l| l == &e.lockfile) {
            bad.push(format!("{at}: STALE -- `{}` is not a committed lock file; remove the exception", e.lockfile));
        }
        if !e.issue.is_empty() && !is_issue_ref(&e.issue) {
            bad.push(format!("{at}: issue `{}` is not `#N` or `owner/repo#N`", e.issue));
        }
        if !e.expires.is_empty() {
            match parse_day(&e.expires) {
                None => bad.push(format!("{at}: expires `{}` is not a real YYYY-MM-DD date", e.expires)),
                Some(d) if today > d => bad.push(format!(
                    "{at}: EXPIRED on {} ({}): fix the dependency, or re-decide the exception",
                    e.expires, e.issue
                )),
                Some(d) if d - today > MAX_EXPIRY_DAYS => bad.push(format!(
                    "{at}: expires {} is more than {MAX_EXPIRY_DAYS} days out -- a permanent excuse",
                    e.expires
                )),
                Some(_) => {}
            }
        }
        if !seen.insert((&e.lockfile, &e.advisory)) {
            bad.push(format!("{at}: DUPLICATE exception for the same (lock file, advisory)"));
        }
    }
    bad
}

/// The decision, separated from I/O. `reports` holds every audited lock.
pub fn judge(reports: &BTreeMap<String, Report>, exs: &[Exception], locks: &[String], today: i64) -> Vec<String> {
    let mut bad = check_exceptions(exs, locks, today);
    for (lock, r) in reports {
        for (id, pkgs) in &r.vulns {
            if !exs.iter().any(|e| &e.lockfile == lock && &e.advisory == id) {
                let p: Vec<&str> = pkgs.iter().map(String::as_str).collect();
                bad.push(format!("{lock}: {id} ({}) is a vulnerability with no exception", p.join(", ")));
            }
        }
    }
    for e in exs {
        if let Some(r) = reports.get(&e.lockfile) {
            if !e.advisory.is_empty() && !r.vulns.contains_key(&e.advisory) {
                bad.push(format!(
                    "{LEDGER}:{}: STALE -- {} no longer reports {}; remove the exception",
                    e.line, e.lockfile, e.advisory
                ));
            }
        }
    }
    bad
}

/// Tracked lock files, via the index. None when git itself fails.
pub fn enumerate(repo: &Path) -> Option<Vec<String>> {
    let out = Command::new("git").arg("-C").arg(repo).args(["ls-files", "-z"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        out.stdout
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .filter(|p| p == "Cargo.lock" || p.ends_with("/Cargo.lock"))
            .collect(),
    )
}

pub fn run(repo: &Path, target: Option<&str>) -> Verdict {
    // `--module DIR` audits another git repository (the CLI form of the plants).
    let root: PathBuf = target.map(PathBuf::from).unwrap_or_else(|| repo.to_path_buf());
    let v = run_with(&root, &Tool::cargo_audit(), today(), true);
    if let Some(w) = expiry_warning(&root, today()) {
        // A workflow command must start its own line on stdout.
        println!("{w}");
    }
    v
}

fn expiry_warning(repo: &Path, today: i64) -> Option<String> {
    let text = std::fs::read_to_string(repo.join(LEDGER)).ok()?;
    let (exs, _) = parse_ledger(&text);
    let earliest = exs.iter().filter_map(|e| parse_day(&e.expires)).filter(|d| *d >= today).min()?;
    (earliest - today < EXPIRY_WARN_DAYS).then(|| {
        format!(
            "::warning::lockfile-audit: an exception expires on {} ({} day(s)); after that this check fails until the dependency is fixed or the exception is re-decided",
            ymd(earliest * 86_400),
            earliest - today
        )
    })
}

/// The whole gate against `repo`. `fetch` lets the FIRST audit refresh the
/// advisory database; the rest read the same copy.
pub fn run_with(repo: &Path, tool: &Tool, today: i64, fetch: bool) -> Verdict {
    let Some(locks) = enumerate(repo) else {
        return Verdict::Refused(format!("git ls-files failed in {} -- the gate cannot answer", repo.display()));
    };
    if locks.is_empty() {
        return Verdict::Refused("git ls-files lists no Cargo.lock -- an audit of nothing is not a pass".into());
    }
    let Ok(text) = std::fs::read_to_string(repo.join(LEDGER)) else {
        return Verdict::Refused(format!("{LEDGER} is missing -- the gate cannot tell an exception from a failure"));
    };
    let (exs, ledger_bad) = parse_ledger(&text);

    let mut reports: BTreeMap<String, Report> = BTreeMap::new();
    let mut refused: Vec<String> = Vec::new();
    for (i, lock) in locks.iter().enumerate() {
        match audit_one(repo, tool, lock, fetch && i == 0) {
            Ok(r) => {
                reports.insert(lock.clone(), r);
            }
            Err(m) => refused.push(format!("{lock}: {m}")),
        }
        // Without a database the rest cannot run either; one line says so.
        if i == 0 && !refused.is_empty() {
            break;
        }
    }
    if !refused.is_empty() {
        return Verdict::Refused(format!(
            "audited {} of {} lock file(s); the audit could not complete:\n  {}",
            reports.len(),
            locks.len(),
            refused.join("\n  ")
        ));
    }

    let mut bad = ledger_bad;
    bad.extend(judge(&reports, &exs, &locks, today));

    let (db_n, db_c) = reports.values().next().map(|r| (r.db_advisories, r.db_commit.clone())).unwrap_or_default();
    let mut lines = vec![format!(
        "audited {} of {} lockfiles (advisory db: {db_n} advisories at {})",
        reports.len(),
        locks.len(),
        &db_c[..db_c.len().min(12)]
    )];
    let mut tot_v = 0;
    let mut tot_w: BTreeMap<&str, usize> = BTreeMap::new();
    for (lock, r) in &reports {
        let ex = r.vulns.keys().filter(|id| exs.iter().any(|e| &e.lockfile == lock && &e.advisory == *id)).count();
        tot_v += r.vulns.len();
        let w: Vec<String> = r.warnings.iter().filter(|(_, n)| **n > 0).map(|(k, n)| format!("{k} {n}")).collect();
        for (k, n) in &r.warnings {
            *tot_w.entry(k).or_default() += n;
        }
        let v = if r.vulns.is_empty() {
            "0 vulnerabilities".to_string()
        } else {
            format!("{} vulnerabilit(ies), {ex} excepted", r.vulns.len())
        };
        let w = if w.is_empty() { String::new() } else { format!("; reported, not failed: {}", w.join(", ")) };
        lines.push(format!("{lock}: {v}{w}"));
    }
    let w: Vec<String> = tot_w.iter().map(|(k, n)| format!("{k} {n}")).collect();
    lines.push(format!(
        "total: {tot_v} distinct (lock, advisory) vulnerabilit(ies), {} exception(s); warnings (not failed): {}",
        exs.len(),
        if w.is_empty() { "none".into() } else { w.join(", ") }
    ));
    if bad.is_empty() {
        Verdict::Pass(lines)
    } else {
        lines.push(format!("FAIL: {} problem(s):", bad.len()));
        lines.extend(bad.into_iter().map(|b| format!("  {b}")));
        Verdict::Fail(lines)
    }
}

// ---------------------------------------------------------------- self-test

/// A lock file pinning smallvec 1.6.0: RUSTSEC-2021-0003 (buffer overflow in
/// `insert_many`, patched in 1.6.1), an advisory standing since 2021. Written to
/// a scratch repository at test time -- never committed as a Cargo.lock the
/// real-tree run would audit.
pub const VULNERABLE_LOCK: &str = "version = 3\n\n[[package]]\nname = \"fixture\"\nversion = \"0.1.0\"\ndependencies = [\n \"smallvec\",\n]\n\n[[package]]\nname = \"smallvec\"\nversion = \"1.6.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\n";
pub const VULNERABLE_ID: &str = "RUSTSEC-2021-0003";
/// A lock file with no dependencies at all.
pub const CLEAN_LOCK: &str = "version = 3\n\n[[package]]\nname = \"fixture\"\nversion = \"0.1.0\"\n";

/// A scratch git repository that removes itself.
pub struct ScratchRepo(pub PathBuf);

impl ScratchRepo {
    /// `files`: (relative path, content). Every file is `git add`ed.
    pub fn new(tag: &str, files: &[(&str, &str)]) -> Result<Self, String> {
        let p = std::env::temp_dir().join(format!(
            "gale-xtask-lockaudit-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        ));
        std::fs::create_dir_all(&p).map_err(|e| format!("scratch dir: {e}"))?;
        let s = ScratchRepo(p);
        let git = |args: &[&str]| -> Result<(), String> {
            let o = Command::new("git").arg("-C").arg(&s.0).args(args).output().map_err(|e| format!("git: {e}"))?;
            if o.status.success() {
                Ok(())
            } else {
                Err(format!("git {args:?}: {}", String::from_utf8_lossy(&o.stderr).trim()))
            }
        };
        git(&["init", "-q"])?;
        for (rel, content) in files {
            let f = s.0.join(rel);
            if let Some(d) = f.parent() {
                std::fs::create_dir_all(d).map_err(|e| format!("{rel}: {e}"))?;
            }
            std::fs::write(&f, content).map_err(|e| format!("{rel}: {e}"))?;
            git(&["add", "--", rel])?;
        }
        Ok(s)
    }
}

impl Drop for ScratchRepo {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

pub fn ledger_entry(lock: &str, id: &str, issue: &str, expires: &str) -> String {
    format!(
        "[[exception]]\nlockfile = \"{lock}\"\nadvisory = \"{id}\"\nissue = \"{issue}\"\nexpires = \"{expires}\"\nreason = \"fixture\"\n"
    )
}

fn report_json(exit_found: bool, count: usize, listed: usize, ignore: &str) -> String {
    let item = r#"{"advisory":{"id":"RUSTSEC-2021-0003"},"package":{"name":"smallvec","version":"1.6.0"}}"#;
    let list: Vec<&str> = std::iter::repeat_n(item, listed).collect();
    format!(
        r#"{{"database":{{"advisory-count":1555,"last-commit":"abc"}},"lockfile":{{"dependency-count":2}},"settings":{{"severity":null,"ignore":[{ignore}]}},"vulnerabilities":{{"found":{exit_found},"count":{count},"list":[{}]}},"warnings":{{"unmaintained":[{{}}],"unsound":[]}}}}"#,
        list.join(",")
    )
}

fn rep(lock_vulns: &[(&str, &[&str])]) -> BTreeMap<String, Report> {
    lock_vulns
        .iter()
        .map(|(l, ids)| {
            let mut r = Report::default();
            for id in *ids {
                r.vulns.entry(id.to_string()).or_default().insert("x@1".into());
            }
            (l.to_string(), r)
        })
        .collect()
}

/// The checks that need neither the audit tool nor the network. `cargo test`
/// runs these; `--self-test` runs them and the tool-backed plants.
pub fn pure_checks(ck: &mut dyn FnMut(&str, bool)) {
    let t = parse_day("2026-11-20").unwrap_or(0);
    let day = |d: i64| ymd((t + d) * 86_400);

    // interpret: the tool's exit and its report must agree.
    ck("interpret: clean report, exit 0 -> report", interpret(Some(0), &report_json(false, 0, 0, "")).is_ok());
    let r = interpret(Some(1), &report_json(true, 1, 1, ""));
    ck(
        "interpret: one vuln, exit 1 -> report naming it, warnings counted",
        r.as_ref().is_ok_and(|r| r.vulns.contains_key(VULNERABLE_ID) && r.warnings.get("unmaintained") == Some(&1)),
    );
    ck("interpret: exit 0 but count 1 -> refused", interpret(Some(0), &report_json(true, 1, 1, "")).is_err());
    ck("interpret: exit 1 but count 0 -> refused", interpret(Some(1), &report_json(false, 0, 0, "")).is_err());
    ck("interpret: count 2 but 1 listed -> refused", interpret(Some(1), &report_json(true, 2, 1, "")).is_err());
    ck("interpret: found=false but count 1 -> refused", interpret(Some(1), &report_json(false, 1, 1, "")).is_err());
    ck("interpret: exit 1, no JSON (db fetch failed) -> refused", interpret(Some(1), "").is_err());
    ck("interpret: exit 101 (no such command) -> refused", interpret(Some(101), "").is_err());
    ck("interpret: killed by a signal -> refused", interpret(None, &report_json(false, 0, 0, "")).is_err());
    ck(
        "interpret: the tool's own ignore list is non-empty -> refused",
        interpret(Some(0), &report_json(false, 0, 0, "\"RUSTSEC-2021-0003\"")).is_err(),
    );
    ck(
        "interpret: empty advisory database -> refused",
        interpret(Some(0), &report_json(false, 0, 0, "").replace("1555", "0")).is_err(),
    );

    // ledger parser
    let good = ledger_entry("a/Cargo.lock", VULNERABLE_ID, "#534", &day(30));
    let (e, b) = parse_ledger(&good);
    ck("ledger: well-formed entry parses", e.len() == 1 && b.is_empty());
    let (_, b) = parse_ledger(&format!("{good}expires = \"{}\"\n", day(10)));
    ck("ledger: a key repeated in one exception -> problem", b.len() == 1);
    let (_, b) = parse_ledger("[[exception]]\nlockfile = a/Cargo.lock\n");
    ck("ledger: unquoted value -> problem", b.len() == 1);
    let (_, b) = parse_ledger("[[exception]]\nbogus = \"x\"\n");
    ck("ledger: unknown key -> problem", b.len() == 1);
    let (_, b) = parse_ledger("lockfile = \"a/Cargo.lock\"\n");
    ck("ledger: key before any [[exception]] -> problem", b.len() == 1);

    // exceptions on their own
    let locks = vec!["a/Cargo.lock".to_string(), "b/Cargo.lock".to_string()];
    let ex = |lock: &str, id: &str, issue: &str, exp: &str| Exception {
        lockfile: lock.into(),
        advisory: id.into(),
        issue: issue.into(),
        expires: exp.into(),
        reason: "r".into(),
        line: 1,
    };
    let ok = ex("a/Cargo.lock", VULNERABLE_ID, "#534", &day(30));
    ck("exception: valid", check_exceptions(std::slice::from_ref(&ok), &locks, t).is_empty());
    ck("exception: expiring today is still valid", check_exceptions(&[ex("a/Cargo.lock", VULNERABLE_ID, "#1", &day(0))], &locks, t).is_empty());
    ck(
        "exception: expired yesterday -> fail",
        check_exceptions(&[ex("a/Cargo.lock", VULNERABLE_ID, "#1", &day(-1))], &locks, t).iter().any(|m| m.contains("EXPIRED")),
    );
    ck("exception: exactly at the 90-day cap -> valid", check_exceptions(&[ex("a/Cargo.lock", VULNERABLE_ID, "#1", &day(90))], &locks, t).is_empty());
    ck("exception: 91 days out -> fail", check_exceptions(&[ex("a/Cargo.lock", VULNERABLE_ID, "#1", &day(91))], &locks, t).len() == 1);
    ck("exception: not a real date -> fail", check_exceptions(&[ex("a/Cargo.lock", VULNERABLE_ID, "#1", "2026-02-30")], &locks, t).len() == 1);
    ck("exception: issue not #N -> fail", check_exceptions(&[ex("a/Cargo.lock", VULNERABLE_ID, "TODO", &day(5))], &locks, t).len() == 1);
    ck("exception: missing reason -> fail", check_exceptions(&[Exception { reason: String::new(), ..ok.clone() }], &locks, t).len() == 1);
    ck("exception: duplicate -> fail", check_exceptions(&[ok.clone(), ok.clone()], &locks, t).iter().any(|m| m.contains("DUPLICATE")));
    ck("exception: wildcard advisory -> fail", check_exceptions(&[ex("a/Cargo.lock", "RUSTSEC-2021-*", "#1", &day(5))], &locks, t).iter().any(|m| m.contains("wildcard")));
    ck("exception: wildcard lock file -> fail", check_exceptions(&[ex("*/Cargo.lock", VULNERABLE_ID, "#1", &day(5))], &locks, t).iter().any(|m| m.contains("wildcard")));
    ck("exception: advisory not RUSTSEC-YYYY-NNNN -> fail", check_exceptions(&[ex("a/Cargo.lock", "GHSA-xxxx", "#1", &day(5))], &locks, t).len() == 1);
    ck("exception: lock file not committed -> stale", check_exceptions(&[ex("z/Cargo.lock", VULNERABLE_ID, "#1", &day(5))], &locks, t).iter().any(|m| m.contains("STALE")));

    // judge: findings against the ledger
    let vuln = rep(&[("a/Cargo.lock", &[VULNERABLE_ID]), ("b/Cargo.lock", &[])]);
    let clean = rep(&[("a/Cargo.lock", &[]), ("b/Cargo.lock", &[])]);
    ck("judge: vulnerability with no exception -> fail", judge(&vuln, &[], &locks, t).len() == 1);
    ck("judge: vulnerability with a valid exception -> pass", judge(&vuln, std::slice::from_ref(&ok), &locks, t).is_empty());
    ck(
        "judge: the exception names the wrong lock -> fail (vuln) + stale",
        judge(&vuln, &[ex("b/Cargo.lock", VULNERABLE_ID, "#1", &day(5))], &locks, t).len() == 2,
    );
    ck("judge: STALE exception (advisory gone from the lock) -> fail", judge(&clean, std::slice::from_ref(&ok), &locks, t).iter().any(|m| m.contains("STALE")));
    ck("judge: nothing found, nothing excepted -> pass", judge(&clean, &[], &locks, t).is_empty());
    ck("advisory id: RUSTSEC-2021-0003 ok", is_advisory_id(VULNERABLE_ID));
    ck("advisory id: RUSTSEC-2021-003 refused", !is_advisory_id("RUSTSEC-2021-003"));

    // plant (d): an empty enumeration refuses, before any tool runs. A lock
    // file under any other name is not enumerated.
    let tool = Tool { program: "gale-lockfile-audit-no-such-tool".into(), args: vec![] };
    match ScratchRepo::new("empty", &[("README", "x\n"), ("fixtures/vulnerable.lock", VULNERABLE_LOCK), (LEDGER, "")]) {
        Ok(s) => ck(
            "plant (d): no committed Cargo.lock -> REFUSED",
            matches!(run_with(&s.0, &tool, t, false), Verdict::Refused(_)),
        ),
        Err(m) => ck(&format!("plant (d): scratch repo: {m}"), false),
    }
    ck(
        "a directory that is not a git repository -> REFUSED",
        matches!(run_with(&std::env::temp_dir().join("gale-lockfile-audit-no-such-dir"), &tool, t, false), Verdict::Refused(_)),
    );
}

fn is_fail_with(v: &Verdict, needle: &str) -> bool {
    matches!(v, Verdict::Fail(l) if l.iter().any(|x| x.contains(needle)))
}

/// One tool-backed plant: record whether `want` holds, and what came back if not.
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

pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut res: Vec<(String, bool)> = Vec::new();
    pure_checks(&mut |what: &str, got: bool| res.push((what.to_string(), got)));

    // The tool-backed plants: the real cargo-audit against scratch repositories.
    let t = today();
    let tool = Tool::cargo_audit();
    let lock = "app/Cargo.lock";
    let scenario = |tag: &str, content: &str, ledger: &str, fetch: bool| -> Result<Verdict, String> {
        let s = ScratchRepo::new(tag, &[(lock, content), (LEDGER, ledger)])?;
        Ok(run_with(&s.0, &tool, t, fetch))
    };
    // (a) first, and the only one that fetches the database.
    let a = scenario("a", VULNERABLE_LOCK, "", true);
    if let Ok(Verdict::Refused(m)) = &a {
        return Verdict::Refused(format!("the tool-backed plants cannot run: {m}"));
    }
    probe(&mut res, "plant (a): smallvec 1.6.0, no exception -> FAIL naming RUSTSEC-2021-0003", a, &|v| {
        is_fail_with(v, VULNERABLE_ID)
    });
    let soon = ymd((t + 10) * 86_400);
    probe(
        &mut res,
        "positive control: the same lock with a valid exception -> PASS",
        scenario("ok", VULNERABLE_LOCK, &ledger_entry(lock, VULNERABLE_ID, "#534", &soon), false),
        &|v| matches!(v, Verdict::Pass(_)),
    );
    let past = ymd((t - 1) * 86_400);
    probe(
        &mut res,
        "plant (b): an expired exception -> FAIL",
        scenario("b", VULNERABLE_LOCK, &ledger_entry(lock, VULNERABLE_ID, "#534", &past), false),
        &|v| is_fail_with(v, "EXPIRED"),
    );
    probe(
        &mut res,
        "plant (c): a stale exception (clean lock) -> FAIL",
        scenario("c", CLEAN_LOCK, &ledger_entry(lock, VULNERABLE_ID, "#534", &soon), false),
        &|v| is_fail_with(v, "STALE"),
    );
    probe(&mut res, "positive control: clean lock, empty ledger -> PASS", scenario("clean", CLEAN_LOCK, "", false), &|v| {
        matches!(v, Verdict::Pass(l) if l.iter().any(|x| x.contains("audited 1 of 1")))
    });
    let missing = Tool { program: "gale-lockfile-audit-no-such-tool".into(), args: vec![] };
    probe(
        &mut res,
        "the audit tool is missing -> REFUSED",
        ScratchRepo::new("tool", &[(lock, CLEAN_LOCK), (LEDGER, "")]).map(|s| run_with(&s.0, &missing, t, false)),
        &|v| matches!(v, Verdict::Refused(_)),
    );
    let nodb = std::env::temp_dir().join(format!("gale-lockfile-audit-nodb-{}", std::process::id()));
    let nodb_tool = Tool { program: "cargo".into(), args: vec!["audit".into(), "--db".into(), nodb.display().to_string()] };
    probe(
        &mut res,
        "the advisory database cannot be read -> REFUSED",
        ScratchRepo::new("nodb", &[(lock, VULNERABLE_LOCK), (LEDGER, "")]).map(|s| run_with(&s.0, &nodb_tool, t, false)),
        &|v| matches!(v, Verdict::Refused(_)),
    );

    let ok = res.iter().all(|(_, g)| *g);
    let mut lines: Vec<String> =
        res.into_iter().map(|(w, g)| if w.starts_with("  (got") { w } else { format!("{w}: {}", if g { "ok" } else { "WRONG" }) }).collect();
    if ok {
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

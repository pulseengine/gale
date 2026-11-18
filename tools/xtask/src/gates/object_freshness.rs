//! A committed `.o` must be the build of the sources and toolchain it is
//! recorded against -- a CONTENT property, checked by hash (gale#506).
//!
//! Four objects went stale for 5-7 weeks unnoticed (gale#334): nothing rebuilds
//! a committed object when its inputs change. The first version of this gate
//! compared git commit DATES, and that could not hold a single object:
//! - all 19 gated objects were date-stale and all 19 were on a known-stale
//!   ledger, so a further source edit could not change the verdict;
//! - 13 bytes of garbage committed as `wdg-thin-cm3.o` read `ok`, because a new
//!   commit date is all the comparison saw.
//!
//! Now each gated object's `[[binary]]` entry in `binaries.toml` records
//! - `sha256`: the committed object's bytes;
//! - `inputs_sha256`: a digest of every file in its input set (below);
//! - `toolchain`: the toolchain pin, derived from varve.toml + rust-toolchain.toml.
//!
//! FAIL when, for any gated object:
//! - its bytes do not hash to `sha256` (no ledger entry excuses this);
//! - its inputs no longer digest to `inputs_sha256`, or the pin is not `toolchain`
//!   (no ledger entry excuses this either: a known-stale object is accepted at ONE
//!   recorded state of its sources, not for whatever they become);
//! - it carries `stale_issue`/`stale_expires` (KNOWN-STALE: the object is NOT a
//!   build of the recorded inputs) and the expiry has passed, lies more than
//!   MAX_EXPIRY_DAYS out, or either field is missing or malformed;
//! - it claims to be fresh (no stale fields) but git dates contradict the claim:
//!   the object was last committed before one of its inputs or pin files was.
//!   That cross-check is what catches a record that was updated without a rebuild.
//!
//! Stated blind spots: the gate does not rebuild, so "fresh" means the record
//! says the object was built from these inputs and nothing contradicts it, not
//! that a rebuild reproduces the bytes. rustc is a channel (`stable`), not a
//! version pin, so a rustc release changes nothing the gate sees. Objects on
//! UNCOVERED have no input set; binary-ledger pins their bytes and nothing
//! judges their freshness.

use super::binary_ledger::{parse, Entry};
use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const NAME: &str = "object-freshness";

const REL: &str = "drivers";

/// Builder -> the object it writes. Only builders whose output is COMMITTED.
/// build-reloc-cores.sh takes its target as an argument, and
/// build-dissolve-gustos.sh's object is on UNCOVERED below.
///
/// This list previously said build-iso-core.sh "writes to a temp dir". It does
/// not: its `OUT` defaults to the drivers directory, so it writes the COMMITTED
/// `iso-core-fused-cm3.o` in place. That false premise is why the object was
/// left out of this list, and the `**/` glob in committed_objects (see its note)
/// is why the census could not catch the omission. Two wrong things lined up,
/// and the gate was green because neither was visible on its own.
const BUILDERS: &[(&str, &str)] = &[
    ("build-os-ts.sh", "os-node/os-ts-cm3.o"),
    ("build-os-tl.sh", "os-node/os-tl-cm3.o"),
    ("build-os-time.sh", "os-node/os-time-cm3.o"),
    ("build-breadth.sh", "breadth/breadth-cm3.o"),
    ("build-iso-core.sh", "iso-core-fused-cm3.o"),
];

/// Inputs a builder reaches through a shell VARIABLE, which `inputs_for`'s
/// `$HERE/<name>` scan cannot see: build-breadth.sh builds `$HERE/$d-comp` for
/// d in gpio timer spi uart, and build-iso-core.sh passes its three crates to
/// build-reloc-cores.sh as arguments. Without these, an edit to gpio-comp or
/// hm-thin changed no input of the object built from it (gale#506).
/// Each is a crate directory under drivers/, contributing its CRATE_BUILD_FILES.
const EXTRA_CRATES: &[(&str, &[&str])] = &[
    ("build-breadth.sh", &["gpio-comp", "timer-comp", "spi-comp", "uart-comp"]),
    ("build-iso-core.sh", &["hm-thin", "mpu-thin", "switch-thin"]),
];

/// Always an input: the seam definitions every builder consumes. Repo-relative,
/// like the toolchain files: the seams moved to the root `wit/` in rework step 2c.
const ALWAYS: &[&str] = &["wit/hal", "wit/os"];


/// The input list is deliberately NARROW. A first draft used the whole crate
/// directory and flagged gpio-thin over an edited RESULTS.md -- a doc change
/// does not invalidate an object. A second draft added wit-os for everything
/// and flagged all 13 thin drivers, which do not reference wit-os at all. Each
/// widening produced false positives, so the sets are per class.
const CRATE_BUILD_FILES: &[&str] = &["src", "Cargo.toml", "Cargo.lock", ".cargo"];
const THIN_SEAMS: &[&str] = &["wit/hal"];
const OS_SEAMS: &[&str] = &["wit/hal", "wit/os"];

/// Composed/fused objects: produced from a component GRAPH rather than one
/// crate, so "the directory next to it" is not their input set. Listed, not
/// gated -- a new object here fails the census rather than escaping silently.
const UNCOVERED: &[&str] = &[
    "os-node/exec-cm3.o",
    "os-node/gustos-dissolved-cm3.o",
    // The frozen synth#757 repro pair: refreshing them destroys the bug
    // reproduction. Listed so the census sees them.
    "os-node/repro-757/os-tl-buggy.o",
    "os-node/repro-757/os-tl-fixed.o",
];

/// The binary ledger. Every gated object's freshness record lives in its
/// `[[binary]]` entry there (gale#506): the object's `sha256`, the digest of its
/// inputs (`inputs_sha256`), the toolchain pin (`toolchain`), and -- only for a
/// KNOWN-STALE object -- `stale_issue` and `stale_expires`.
const LEDGER: &str = "binaries.toml";

/// The toolchain pin files. The toolchain string an entry records is DERIVED
/// from them (see `toolchain`), so a comment edit in varve.toml does not stale
/// anything, while a layer, digest or rustc channel change stales everything.
const TOOLCHAIN_FILES: &[&str] = &["varve.toml", "rust-toolchain.toml"];

/// The furthest a KNOWN-STALE expiry may lie in the future. Without a cap,
/// `stale_expires = "9999-12-31"` is the permanent excuse the expiry exists to
/// prevent.
const MAX_EXPIRY_DAYS: i64 = 183;

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Unix timestamp of the last commit touching `relpath`.
fn last_commit_epoch(repo: &Path, relpath: &str) -> Option<i64> {
    super::last_content_epoch(repo, relpath)
}

fn ymd(epoch: i64) -> String {
    // Civil-from-days (Howard Hinnant). Dates here are only ever displayed, but
    // shelling out to `date` per object would be 24 extra processes.
    let days = epoch.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// Every committed .o under the drivers directory, at ANY depth, any name.
/// From git, not a filesystem glob: a glob reached one level and one suffix
/// (the repro-757 pair escaped on both counts), and a filesystem walk would
/// pick up uncommitted build output, which is the opposite error.
/// NOTE the TWO pathspecs. git's `**/` requires at least one directory level,
/// so `drivers/**/*.o` silently excludes any object committed at the ROOT of
/// `drivers/` -- which is where `iso-core-fused-cm3.o` lives. It was therefore
/// invisible to this gate from the day it was written: never gated, and unable
/// to be flagged by the census below, because the census can only report on
/// what this function returns. Measured: `drivers/**/*.o` = 24 files,
/// `drivers/*.o` = 25. The missing one is the object behind the Renode
/// isolation gate.
fn committed_objects(repo: &Path) -> Option<Vec<String>> {
    let out = git(
        repo,
        &["ls-files", "--", &format!("{REL}/*.o"), &format!("{REL}/**/*.o")],
    )?;
    Some(
        out.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| l.trim_start_matches(&format!("{REL}/")).to_string())
            .collect(),
    )
}

/// Derive a builder's input set from the script itself: the scripts reference
/// their sources as `$HERE/<dir>`. Deriving rather than hardcoding means a new
/// dependency is picked up automatically.
fn inputs_for(repo: &Path, script: &str) -> Vec<String> {
    let text = std::fs::read_to_string(repo.join(REL).join(script)).unwrap_or_default();
    let mut dirs: BTreeSet<String> = BTreeSet::new();
    let bytes = text.as_bytes();
    let needle = b"$HERE/";
    let mut i = 0usize;
    while i + needle.len() <= bytes.len() {
        if &bytes[i..i + needle.len()] == needle {
            let mut j = i + needle.len();
            let start = j;
            // `.` is accepted so a reference to a sibling SCRIPT is captured as
            // the script, not as a directory that does not exist. Before this,
            // `"$HERE/build-reloc-cores.sh"` yielded `build-reloc-cores`, a path
            // with no file behind it, which then resolved to no commit date and
            // was silently dropped — so the script that actually builds
            // iso-core-fused-cm3.o's components was not one of its inputs.
            // A derived path that resolves to nothing is the same defect this
            // repo found in the fixture gate (gale#417).
            while j < bytes.len()
                && (bytes[j].is_ascii_lowercase()
                    || bytes[j].is_ascii_digit()
                    || bytes[j] == b'-'
                    || bytes[j] == b'.')
            {
                j += 1;
            }
            if j > start {
                let d = &text[start..j];
                // os-node is the OUTPUT directory, not an input; excluding it
                // stops the object from being compared against itself.
                // Trailing '.' from e.g. "$HERE/foo." is not a real name.
                let d = d.trim_end_matches('.');
                if d != "os-node" && !d.is_empty() {
                    dirs.insert(d.to_string());
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    let mut out = vec![format!("{REL}/{script}")];
    out.extend(dirs.iter().map(|d| format!("{REL}/{d}")));
    for (s, crates) in EXTRA_CRATES {
        if *s == script {
            for c in *crates {
                out.extend(CRATE_BUILD_FILES.iter().map(|b| format!("{REL}/{c}/{b}")));
            }
        }
    }
    out.extend(ALWAYS.iter().map(|s| s.to_string()));
    out
}

/// Every gated object (drivers-relative) and its input paths (repo-relative),
/// after the census. Err is the verdict to return.
fn subjects(repo: &Path) -> Result<BTreeMap<String, Vec<String>>, Verdict> {
    if git(repo, &["rev-parse", "--git-dir"]).is_none() {
        return Err(Verdict::Refused("not a git repository".into()));
    }
    let Some(found) = committed_objects(repo) else {
        return Err(Verdict::Refused("git ls-files failed — the gate cannot answer".into()));
    };
    if found.is_empty() {
        return Err(Verdict::Refused("no committed objects found — the sweep would be vacuous".into()));
    }

    let builder_outputs: BTreeSet<&str> = BUILDERS.iter().map(|(_, o)| *o).collect();
    let uncovered: BTreeSet<&str> = UNCOVERED.iter().copied().collect();

    // Objects that live beside their own crate, with per-class inputs. Only a
    // real crate directory is auto-covered: without that check a composed
    // artifact dropped into a non-crate directory would be silently mis-gated
    // against the wrong inputs instead of flagged by the census.
    let mut gated: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for (script, obj) in BUILDERS {
        gated.insert(obj.to_string(), inputs_for(repo, script));
    }
    for key in &found {
        if builder_outputs.contains(key.as_str()) || uncovered.contains(key.as_str()) {
            continue;
        }
        let p = PathBuf::from(key);
        let Some(dir) = p.parent().and_then(|d| d.to_str()) else { continue };
        if dir.is_empty() || !repo.join(REL).join(dir).join("Cargo.toml").is_file() {
            continue;
        }
        let leaf = dir.rsplit('/').next().unwrap_or(dir);
        let seams = if leaf.ends_with("-thin") { THIN_SEAMS } else { OS_SEAMS };
        let mut inputs: Vec<String> =
            CRATE_BUILD_FILES.iter().map(|b| format!("{REL}/{dir}/{b}")).collect();
        inputs.extend(seams.iter().map(|s| s.to_string()));
        gated.insert(key.clone(), inputs);
    }

    // Census: an object that is neither gated nor listed must not pass silently.
    let known: BTreeSet<&str> = gated
        .keys()
        .map(String::as_str)
        .chain(uncovered.iter().copied())
        .collect();
    let stray: Vec<&String> = found.iter().filter(|f| !known.contains(f.as_str())).collect();
    if !stray.is_empty() {
        let mut lines = vec![format!("FAIL: {} committed object(s) neither gated nor listed:", stray.len())];
        lines.extend(stray.iter().map(|s| format!("  {s}")));
        return Err(Verdict::Fail(lines));
    }
    let found_set: BTreeSet<&str> = found.iter().map(String::as_str).collect();
    let vanished: Vec<&str> = uncovered.iter().copied().filter(|u| !found_set.contains(u)).collect();
    if !vanished.is_empty() {
        let mut lines = vec!["FAIL: UNCOVERED lists object(s) that no longer exist:".to_string()];
        lines.extend(vanished.iter().map(|v| format!("  {v}")));
        return Err(Verdict::Fail(lines));
    }
    Ok(gated)
}

/// Every file under the input paths, repo-relative and sorted: tracked files
/// AND untracked-but-not-ignored ones, read from the working tree. Untracked
/// files count because a new source file changes the build before it is
/// committed; ignored ones (target/) do not.
fn input_files(repo: &Path, inputs: &[String], outputs: &BTreeSet<String>) -> Option<Vec<String>> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-files", "-z", "--cached", "--others", "--exclude-standard", "--"])
        .args(inputs)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let files: BTreeSet<String> = out
        .stdout
        .split(|b| *b == 0)
        .filter(|s| !s.is_empty())
        .map(|s| String::from_utf8_lossy(s).into_owned())
        .filter(|f| repo.join(f).is_file() && !outputs.contains(f))
        .collect();
    Some(files.into_iter().collect())
}

/// The digest of an input set: SHA-256 over `path NUL sha256(content) LF` for
/// each file, sorted by path. Paths are part of it, so a rename is a change.
pub fn fold_digest(files: &[(String, String)]) -> String {
    let mut sorted: Vec<&(String, String)> = files.iter().collect();
    sorted.sort();
    let mut buf = Vec::new();
    for (p, h) in sorted {
        buf.extend_from_slice(p.as_bytes());
        buf.push(0);
        buf.extend_from_slice(h.as_bytes());
        buf.push(b'\n');
    }
    crate::sha256::hex(&buf)
}

/// (digest, file count) of an object's input set, from the working tree.
/// Committed objects are OUTPUTS: a builder's input directory can hold one
/// (build-breadth.sh names `$HERE/breadth`, where its own object lands), and an
/// object must not be an input of itself. `outputs` is every committed object,
/// repo-relative.
fn inputs_digest(repo: &Path, inputs: &[String], outputs: &BTreeSet<String>) -> Result<(String, usize), String> {
    let files = input_files(repo, inputs, outputs).ok_or("git ls-files failed on the input set")?;
    if files.is_empty() {
        return Err("the input set resolves to no files — a digest of nothing is vacuous".into());
    }
    let mut hashed = Vec::with_capacity(files.len());
    for f in &files {
        let bytes = std::fs::read(repo.join(f)).map_err(|e| format!("{f}: {e}"))?;
        hashed.push((f.clone(), crate::sha256::hex(&bytes)));
    }
    Ok((fold_digest(&hashed), files.len()))
}

/// `key = "value"` inside `[section]` of a flat TOML file, None if absent.
fn toml_value(text: &str, section: &str, key: &str) -> Option<String> {
    let mut inside = false;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            inside = line == format!("[{section}]");
            continue;
        }
        if !inside {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            let v = v.trim();
            if k.trim() == key && v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
                return Some(v[1..v.len() - 1].to_string());
            }
        }
    }
    None
}

/// The toolchain pin, an input to EVERY gated object (gale#438): every object
/// here is a synth output, so the pin decides its bytes as much as any source
/// does (iso-core-fused-cm3.o: 15094 B committed, 15050 B rebuilt from the
/// pin). The varve layer + digest fixes synth and loom; rust-toolchain.toml
/// names only a rustc CHANNEL, and the string says so.
pub fn toolchain(repo: &Path) -> Result<String, String> {
    let varve = std::fs::read_to_string(repo.join(TOOLCHAIN_FILES[0]))
        .map_err(|e| format!("{}: {e}", TOOLCHAIN_FILES[0]))?;
    let rust = std::fs::read_to_string(repo.join(TOOLCHAIN_FILES[1]))
        .map_err(|e| format!("{}: {e}", TOOLCHAIN_FILES[1]))?;
    let get = |text: &str, file: &str, key: &str| {
        toml_value(text, "toolchain", key).ok_or(format!("{file}: no [toolchain] {key}"))
    };
    Ok(format!(
        "varve {} {} {}; rustc channel {}",
        get(&varve, TOOLCHAIN_FILES[0], "realm")?,
        get(&varve, TOOLCHAIN_FILES[0], "layer")?,
        get(&varve, TOOLCHAIN_FILES[0], "digest")?,
        get(&rust, TOOLCHAIN_FILES[1], "channel")?,
    ))
}

/// Days since 1970-01-01 of a civil date (Howard Hinnant's days_from_civil).
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// `YYYY-MM-DD` -> days since the epoch; None unless it round-trips, so
/// `2026-02-30` is refused rather than read as March 2nd.
fn parse_day(s: &str) -> Option<i64> {
    let p: Vec<&str> = s.split('-').collect();
    if p.len() != 3 || p[0].len() != 4 || p[1].len() != 2 || p[2].len() != 2 {
        return None;
    }
    let (y, m, d) = (p[0].parse().ok()?, p[1].parse().ok()?, p[2].parse().ok()?);
    let days = days_from_civil(y, m, d);
    (ymd(days * 86_400) == s).then_some(days)
}

fn today() -> i64 {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    secs.div_euclid(86_400)
}

/// `#N` or `owner/repo#N`.
fn is_issue_ref(s: &str) -> bool {
    let Some((repo, n)) = s.rsplit_once('#') else { return false };
    let num_ok = !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit());
    let repo_ok = repo.is_empty()
        || repo.split_once('/').is_some_and(|(o, r)| {
            let ok = |x: &str| {
                !x.is_empty() && x.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_' || b == b'.')
            };
            ok(o) && ok(r)
        });
    num_ok && repo_ok
}

/// What a gated object currently is, measured from the tree.
pub struct Observed<'a> {
    pub object_sha256: &'a str,
    pub inputs_sha256: &'a str,
    pub toolchain: &'a str,
    pub today: i64,
    /// git dates contradict a freshness claim: the object's last content commit
    /// predates the last content commit of an input or pin file. None when not
    /// measured (the entry is ledgered, so it claims nothing).
    pub dated_older_than_inputs: Option<bool>,
}

/// The decision for ONE gated object, separated from I/O so the self-test can
/// drive it. Ok(None) = fresh; Ok(Some((issue, expires))) = known-stale and
/// accepted; Err = the reasons it fails.
pub fn judge(e: &Entry, o: &Observed) -> Result<Option<(String, String)>, Vec<String>> {
    let mut bad = Vec::new();
    if o.object_sha256 != e.sha256 {
        bad.push(format!(
            "bytes hash to {} but the record says {} — this is not the object the entry describes",
            o.object_sha256, e.sha256
        ));
    }
    match &e.inputs_sha256 {
        None => bad.push("no `inputs_sha256` recorded".into()),
        Some(r) if r != o.inputs_sha256 => bad.push(format!(
            "inputs changed since the record ({r} -> {}): rebuild the object, or re-record it as known-stale",
            o.inputs_sha256
        )),
        Some(_) => {}
    }
    match &e.toolchain {
        None => bad.push("no `toolchain` recorded".into()),
        Some(r) if r != o.toolchain => bad.push(format!(
            "toolchain pin changed since the record (`{r}` -> `{}`)",
            o.toolchain
        )),
        Some(_) => {}
    }
    let ledgered = match (&e.stale_issue, &e.stale_expires) {
        (None, None) => None,
        (Some(i), Some(x)) => {
            if !is_issue_ref(i) {
                bad.push(format!("stale_issue `{i}` is not `#N` or `owner/repo#N`"));
            }
            match parse_day(x) {
                None => bad.push(format!("stale_expires `{x}` is not a YYYY-MM-DD date")),
                Some(d) if o.today > d => bad.push(format!(
                    "known-stale ledger entry EXPIRED on {x} ({i}): regenerate the object, or re-decide the expiry"
                )),
                Some(d) if d - o.today > MAX_EXPIRY_DAYS => bad.push(format!(
                    "stale_expires {x} is more than {MAX_EXPIRY_DAYS} days out — a permanent excuse"
                )),
                Some(_) => {}
            }
            Some((i.clone(), x.clone()))
        }
        _ => {
            bad.push("a known-stale entry needs BOTH `stale_issue` and `stale_expires`".into());
            None
        }
    };
    if ledgered.is_none() && o.dated_older_than_inputs == Some(true) {
        bad.push(
            "the entry claims the object is fresh, but the object was last committed before one of its \
             inputs or pin files — a record updated without a rebuild"
                .into(),
        );
    }
    if bad.is_empty() {
        Ok(ledgered)
    } else {
        Err(bad)
    }
}

fn is_freshness_entry(e: &Entry) -> bool {
    e.inputs_sha256.is_some() || e.toolchain.is_some() || e.stale_issue.is_some() || e.stale_expires.is_some()
}

/// Measured state of every gated object: (repo-relative path, input paths,
/// object sha256, inputs digest, input file count).
type Measured = Vec<(String, Vec<String>, String, String, usize)>;

fn measure(repo: &Path) -> Result<Measured, Verdict> {
    let gated = subjects(repo)?;
    let outputs: BTreeSet<String> = committed_objects(repo)
        .ok_or_else(|| Verdict::Refused("git ls-files failed — the gate cannot answer".into()))?
        .into_iter()
        .map(|o| format!("{REL}/{o}"))
        .collect();
    let mut out = Vec::new();
    for (obj, inputs) in gated {
        let path = format!("{REL}/{obj}");
        let bytes = std::fs::read(repo.join(&path))
            .map_err(|e| Verdict::Refused(format!("{path}: cannot read the committed object: {e}")))?;
        let (digest, n) =
            inputs_digest(repo, &inputs, &outputs).map_err(|m| Verdict::Refused(format!("{path}: {m}")))?;
        out.push((path, inputs, crate::sha256::hex(&bytes), digest, n));
    }
    Ok(out)
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let measured = match measure(repo) {
        Ok(m) => m,
        Err(v) => return v,
    };
    let tc = match toolchain(repo) {
        Ok(t) => t,
        Err(m) => return Verdict::Refused(m),
    };
    let Ok(text) = std::fs::read_to_string(repo.join(LEDGER)) else {
        return Verdict::Refused(format!("{LEDGER} is missing — the gate cannot answer"));
    };
    let entries = match parse(&text) {
        Ok(e) => e,
        Err(m) => return Verdict::Refused(format!("{LEDGER}: {m}")),
    };
    let today = today();

    let mut lines = vec![format!("toolchain: {tc}"), format!("today (UTC): {}", ymd(today * 86_400))];
    lines.push(format!("{:<26} {:>6}  verdict", "object", "inputs"));
    let mut fails: Vec<String> = Vec::new();
    let (mut fresh, mut stale) = (0usize, 0usize);
    let mut earliest: Option<String> = None;
    for (path, inputs, obj_sha, digest, n) in &measured {
        let short = path.rsplit('/').next().unwrap_or(path);
        let Some(e) = entries.iter().find(|e| &e.path == path) else {
            fails.push(format!("{path}: gated object with no [[binary]] entry in {LEDGER}"));
            lines.push(format!("{short:<26} {n:>6}  FAIL"));
            continue;
        };
        // The date cross-check is only for a freshness CLAIM; a ledgered entry
        // already says the object is not a build of its inputs.
        let dated = if e.stale_issue.is_none() && e.stale_expires.is_none() {
            let Some(obj_t) = last_commit_epoch(repo, path) else {
                return Verdict::Refused(format!("no commit date for {path} — cannot cross-check its freshness claim"));
            };
            let newest = inputs
                .iter()
                .map(String::as_str)
                .chain(TOOLCHAIN_FILES.iter().copied())
                .filter_map(|i| last_commit_epoch(repo, i))
                .max();
            let Some(newest) = newest else {
                return Verdict::Refused(format!("no commit date for any input of {path}"));
            };
            Some(newest > obj_t)
        } else {
            None
        };
        let o = Observed {
            object_sha256: obj_sha,
            inputs_sha256: digest,
            toolchain: &tc,
            today,
            dated_older_than_inputs: dated,
        };
        match judge(e, &o) {
            Ok(None) => {
                fresh += 1;
                lines.push(format!("{short:<26} {n:>6}  fresh"));
            }
            Ok(Some((issue, exp))) => {
                stale += 1;
                if earliest.as_ref().is_none_or(|x| &exp < x) {
                    earliest = Some(exp.clone());
                }
                lines.push(format!("{short:<26} {n:>6}  KNOWN-STALE {issue}, expires {exp}"));
            }
            Err(why) => {
                lines.push(format!("{short:<26} {n:>6}  FAIL"));
                fails.extend(why.into_iter().map(|w| format!("{path}: {w}")));
            }
        }
    }
    // A freshness record on an object this gate does not judge is a claim
    // nobody checks.
    let gated: BTreeSet<&str> = measured.iter().map(|m| m.0.as_str()).collect();
    for e in entries.iter().filter(|e| is_freshness_entry(e) && !gated.contains(e.path.as_str())) {
        fails.push(format!("{}: has freshness fields but is not a gated object — nothing judges them", e.path));
    }

    lines.push(format!(
        "{} gated object(s): {fresh} fresh, {stale} known-stale{}, {} failing; {} UNCOVERED object(s) are hash-pinned by binary-ledger only",
        measured.len(),
        earliest.map(|x| format!(" (earliest expiry {x})")).unwrap_or_default(),
        measured.len() - fresh - stale,
        UNCOVERED.len()
    ));
    if fails.is_empty() {
        Verdict::Pass(lines)
    } else {
        lines.push(format!("FAIL: {} problem(s):", fails.len()));
        lines.extend(fails.into_iter().map(|f| format!("  {f}")));
        Verdict::Fail(lines)
    }
}

/// `xtask freshness-record`: the measured `sha256`, `inputs_sha256` and
/// `toolchain` of every gated object, as the lines its `[[binary]]` entry
/// carries. It prints; it does not edit binaries.toml, because recording an
/// object as fresh is a claim that it was REBUILT from these inputs, which only
/// the person who rebuilt it can make.
pub fn record(repo: &Path) -> Result<Vec<String>, String> {
    let measured = measure(repo).map_err(|v| v.to_string())?;
    let tc = toolchain(repo)?;
    let mut out = Vec::new();
    for (path, _, obj_sha, digest, n) in measured {
        out.push(format!("path = \"{path}\""));
        out.push(format!("sha256 = \"{obj_sha}\""));
        out.push(format!("inputs_sha256 = \"{digest}\"  # {n} input file(s)"));
        out.push(format!("toolchain = \"{tc}\""));
        out.push(String::new());
    }
    Ok(out)
}

/// Every rule of `judge` must be able to fire, and must not fire on the
/// control. Each planted input changes ONE thing from a passing baseline.
pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };
    ck("sha256 implementation matches FIPS 180-4 vectors", crate::sha256::self_check());

    let obj = crate::sha256::hex(b"the object");
    let inp = fold_digest(&[("drivers/x/src/lib.rs".into(), crate::sha256::hex(b"fn x() {}"))]);
    let tc = "varve r 1 sha256:00; rustc channel stable".to_string();
    let today = parse_day("2026-11-18").unwrap_or(0);
    let base = Entry {
        path: "drivers/x/x-cm3.o".into(),
        sha256: obj.clone(),
        inputs_sha256: Some(inp.clone()),
        toolchain: Some(tc.clone()),
        ..Entry::default()
    };
    let ledgered = |issue: &str, exp: &str| Entry {
        stale_issue: Some(issue.into()),
        stale_expires: Some(exp.into()),
        ..base.clone()
    };
    let seen = |o: &str, i: &str, t: &str, day: i64, dated: Option<bool>| {
        (o.to_string(), i.to_string(), t.to_string(), day, dated)
    };
    let j = |e: &Entry, s: &(String, String, String, i64, Option<bool>)| {
        judge(
            e,
            &Observed {
                object_sha256: &s.0,
                inputs_sha256: &s.1,
                toolchain: &s.2,
                today: s.3,
                dated_older_than_inputs: s.4,
            },
        )
    };
    let good = seen(&obj, &inp, &tc, today, Some(false));
    let garbage = crate::sha256::hex(b"13 bytes junk");
    let edited = fold_digest(&[("drivers/x/src/lib.rs".into(), crate::sha256::hex(b"fn x() { }"))]);

    ck("control: recorded object, inputs and pin match: fresh", j(&base, &good) == Ok(None));
    ck("planted garbage object: fail", j(&base, &seen(&garbage, &inp, &tc, today, Some(false))).is_err());
    ck(
        "planted garbage object on a KNOWN-STALE entry: still fail",
        j(&ledgered("#506", "2026-12-31"), &seen(&garbage, &inp, &tc, today, None)).is_err(),
    );
    ck("one-byte source edit: fail", j(&base, &seen(&obj, &edited, &tc, today, Some(false))).is_err());
    ck(
        "one-byte source edit on a KNOWN-STALE entry: still fail",
        j(&ledgered("#506", "2026-12-31"), &seen(&obj, &edited, &tc, today, None)).is_err(),
    );
    ck(
        "toolchain pin moved: fail",
        j(&base, &seen(&obj, &inp, "varve r 2 sha256:11; rustc channel stable", today, Some(false))).is_err(),
    );
    ck(
        "KNOWN-STALE within its expiry: accepted, and reported as stale",
        j(&ledgered("#506", "2026-12-31"), &seen(&obj, &inp, &tc, today, None))
            == Ok(Some(("#506".into(), "2026-12-31".into()))),
    );
    ck(
        "KNOWN-STALE on its expiry day: still accepted",
        j(&ledgered("#506", "2026-11-18"), &seen(&obj, &inp, &tc, today, None)).is_ok(),
    );
    ck(
        "KNOWN-STALE one day past its expiry: fail",
        j(&ledgered("#506", "2026-11-17"), &seen(&obj, &inp, &tc, today, None)).is_err(),
    );
    ck(
        "KNOWN-STALE expiry beyond the cap: fail",
        j(&ledgered("#506", "9999-12-31"), &seen(&obj, &inp, &tc, today, None)).is_err(),
    );
    ck(
        "KNOWN-STALE with no issue: fail",
        j(&Entry { stale_expires: Some("2026-12-31".into()), ..base.clone() }, &good).is_err(),
    );
    ck(
        "KNOWN-STALE with no expiry: fail",
        j(&Entry { stale_issue: Some("#506".into()), ..base.clone() }, &good).is_err(),
    );
    ck("KNOWN-STALE with a malformed issue: fail", j(&ledgered("soon", "2026-12-31"), &good).is_err());
    ck("KNOWN-STALE with an impossible date: fail", j(&ledgered("#506", "2026-02-30"), &good).is_err());
    ck(
        "KNOWN-STALE naming another repo's issue: accepted",
        j(&ledgered("pulseengine/synth#1", "2026-12-31"), &seen(&obj, &inp, &tc, today, None)).is_ok(),
    );
    ck(
        "fresh claim contradicted by git dates: fail",
        j(&base, &seen(&obj, &inp, &tc, today, Some(true))).is_err(),
    );
    ck("no inputs_sha256 recorded: fail", j(&Entry { inputs_sha256: None, ..base.clone() }, &good).is_err());
    ck("no toolchain recorded: fail", j(&Entry { toolchain: None, ..base.clone() }, &good).is_err());
    ck(
        "digest depends on the path, not only the bytes",
        fold_digest(&[("a".into(), "h".into())]) != fold_digest(&[("b".into(), "h".into())]),
    );
    ck(
        "digest does not depend on listing order",
        fold_digest(&[("a".into(), "1".into()), ("b".into(), "2".into())])
            == fold_digest(&[("b".into(), "2".into()), ("a".into(), "1".into())]),
    );
    ck("ymd epoch 0", ymd(0) == "1970-01-01");
    ck("ymd 1787000000", ymd(1_787_000_000) == "2026-08-17");
    ck("parse_day round-trips 2026-11-18", parse_day("2026-11-18").map(|d| ymd(d * 86_400)).as_deref() == Some("2026-11-18"));
    ck("parse_day 1970-01-01 is day 0", parse_day("1970-01-01") == Some(0));
    ck("parse_day 2024-02-29 (leap) accepted", parse_day("2024-02-29").is_some());

    // Against the real tree: the measurement must not be vacuous.
    match toolchain(repo) {
        Ok(t) => ck("toolchain pin derived from varve.toml + rust-toolchain.toml", t.starts_with("varve ") && t.contains("sha256:")),
        Err(_) => ck("toolchain pin could not be derived", false),
    }
    if let Some(found) = committed_objects(repo) {
        // The enumeration half that was once broken: a root-level object must be
        // seen, a nested one still seen, and the sweep must be objects only.
        ck("root-level committed object is enumerated", found.iter().any(|f| f == "iso-core-fused-cm3.o"));
        ck("nested committed object is still enumerated", found.iter().any(|f| f == "os-node/os-tl-cm3.o"));
        ck(
            "enumeration is objects only, not the whole tree",
            found.iter().all(|f| f.ends_with(".o")) && found.len() < 100,
        );
    } else {
        ck("committed_objects could not run", false);
    }
    // Variable-path builder inputs (EXTRA_CRATES) must reach the derived set.
    ck(
        "build-breadth.sh's inputs include the gpio-comp crate it builds through $d",
        inputs_for(repo, "build-breadth.sh").iter().any(|i| i == "drivers/gpio-comp/src"),
    );
    ck(
        "build-iso-core.sh's inputs include the hm-thin crate it passes as an argument",
        inputs_for(repo, "build-iso-core.sh").iter().any(|i| i == "drivers/hm-thin/src"),
    );
    let none = BTreeSet::new();
    let own: BTreeSet<String> = ["drivers/breadth/breadth-cm3.o".to_string()].into_iter().collect();
    match (
        input_files(repo, &["drivers/breadth".to_string()], &none),
        input_files(repo, &["drivers/breadth".to_string()], &own),
    ) {
        (Some(with), Some(without)) => ck(
            "an object is not an input of itself",
            with.iter().any(|f| f.ends_with("breadth-cm3.o")) && !without.iter().any(|f| f.ends_with(".o")),
        ),
        _ => ck("input_files could not run", false),
    }
    match inputs_digest(repo, &["drivers/wdg-thin/src".to_string(), "wit/hal".to_string()], &none) {
        Ok((_, n)) => ck("a thin driver's input set resolves to files (not vacuous)", n >= 2),
        Err(_) => ck("a thin driver's input set could not be digested", false),
    }
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

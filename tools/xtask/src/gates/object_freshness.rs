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
//! - its bytes are not a relocatable ELF for ARM;
//! - it claims to be fresh (no stale fields) but git dates contradict the claim:
//!   the object was last committed before one of its input files was. That
//!   cross-check is what catches a record that was updated without a rebuild;
//!   in a shallow clone it cannot be answered, so the gate REFUSES.
//!
//! The input set of an object: its builder script (or, for a crate-adjacent
//! object, the synth/loom/meld command lines in its RESULTS.md), the WIT seams,
//! its crate's build files, every crate reached through `path =` dependencies
//! (transitively), and every file pulled in by a `#[path]`/`include!` that
//! leaves its crate, which must be listed in PATH_INCLUDES or the gate REFUSES.
//!
//! Stated blind spots: the gate does not rebuild, so "recorded-fresh" means the
//! record says the object was built from these inputs and nothing contradicts
//! it, not that a rebuild reproduces the bytes. rustc is a channel (`stable`),
//! not a version pin, so a rustc release changes nothing the gate sees. Cargo
//! config in parent directories (the repo root `.cargo/`) and registry
//! dependencies (pinned by each crate's Cargo.lock) are the only cargo inputs
//! not followed. Objects on UNCOVERED have no input set; binary-ledger pins
//! their bytes and nothing judges their freshness.

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
/// Each is a crate directory under drivers/, followed like any crate (`crate_closure`).
const EXTRA_CRATES: &[(&str, &[&str])] = &[
    ("build-breadth.sh", &["gpio-comp", "timer-comp", "spi-comp", "uart-comp"]),
    ("build-iso-core.sh", &["hm-thin", "mpu-thin", "switch-thin"]),
];

/// Always an input: the seam definitions every builder consumes. Repo-relative,
/// like the toolchain files: the seams moved to the root `wit/` in rework step 2c.
const ALWAYS: &[&str] = &["wit/hal", "wit/os"];


/// The input list is deliberately NARROW. A first draft used the whole crate
/// directory and flagged gpio-thin over an edited RESULTS.md -- a doc change
/// does not invalidate an object (its synth/loom command lines do; see
/// `build_commands`). A second draft added wit-os for everything and flagged
/// all 13 thin drivers, which do not reference wit-os at all.
///
/// The crate being built contributes TOP_CRATE_FILES; a crate it reaches
/// through a `path =` dependency contributes DEP_CRATE_FILES. `wit` is there
/// because hm/mpu/switch-thin `generate!` against a local `wit/` world.
const TOP_CRATE_FILES: &[&str] = &["src", "Cargo.toml", "Cargo.lock", ".cargo", "wit", "build.rs"];
const DEP_CRATE_FILES: &[&str] = &["src", "Cargo.toml", "wit", "build.rs"];
const OPTIONAL_CRATE_FILES: &[&str] = &[".cargo", "wit", "build.rs", "Cargo.lock"];

/// A `#[path]` or `include!` that reaches OUTSIDE its crate directory, listed:
/// (crate directory, the file it pulls in). Each becomes an input of every
/// object that crate is built into; an unlisted one makes the gate REFUSE.
const PATH_INCLUDES: &[(&str, &str)] = &[
    // exec-provider compiles the executor verbatim (`#[path]`), and os-ts links
    // exec-provider.
    ("drivers/exec-provider", "plain/src/executor.rs"),
];

/// The substrings that mark a build command line in a RESULTS.md.
const BUILD_COMMAND_MARKERS: &[&str] = &["synth compile", "loom optimize", "meld fuse"];

/// Days before the earliest known-stale expiry at which the gate starts warning.
const EXPIRY_WARN_DAYS: i64 = 21;
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
/// from every setting of their `[toolchain]` tables (see `toolchain`), so a
/// comment edit does not stale anything, while a layer, digest, channel or
/// components change stales everything. They are not dated by the fresh-claim
/// cross-check: the string comparison covers them.
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
    let own_dir = BUILDERS
        .iter()
        .find(|(s, _)| *s == script)
        .and_then(|(_, o)| o.rsplit_once('/').map(|(d, _)| d))
        .unwrap_or("");
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
                // The builder's OUTPUT directory (os-node, breadth) is not an
                // input; excluding it stops the object from being compared
                // against itself, and stops an input that holds only the
                // object from tripping the unresolved-input refusal.
                // Trailing '.' from e.g. "$HERE/foo." is not a real name.
                let d = d.trim_end_matches('.');
                if d != "os-node" && d != own_dir && !d.is_empty() {
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
    out.extend(ALWAYS.iter().map(|s| s.to_string()));
    out
}

/// `dir/x`, with the repo root spelled "".
fn join(dir: &str, x: &str) -> String {
    if dir.is_empty() {
        x.to_string()
    } else {
        format!("{dir}/{x}")
    }
}

/// `rel` resolved against the repo-relative directory `base`, lexically. None
/// if it climbs above the repository root.
fn normalize(base: &str, rel: &str) -> Option<String> {
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for c in rel.split('/') {
        match c {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            c => parts.push(c),
        }
    }
    Some(parts.join("/"))
}

/// Whether repo-relative `f` is `p` or lies under it ("" is the whole repo).
fn under(f: &str, p: &str) -> bool {
    p.is_empty() || f == p || f.starts_with(&format!("{p}/"))
}

/// The value of every `path = "..."` key in a dependency table of a Cargo.toml
/// (`[dependencies]`, `[build-dependencies]`, `[target.*.dependencies]`, and
/// their `[dependencies.foo]` forms; inline tables included). Dev-dependencies
/// are not compiled into the object and are skipped.
pub fn cargo_path_deps(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut deps = false;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            deps = line.contains("dependencies") && !line.contains("dev-dependencies");
            continue;
        }
        if !deps {
            continue;
        }
        let b = line.as_bytes();
        let mut i = 0;
        while let Some(k) = line[i..].find("path") {
            let at = i + k;
            i = at + 4;
            let word_start = at == 0 || !(b[at - 1].is_ascii_alphanumeric() || b[at - 1] == b'_' || b[at - 1] == b'-');
            if !word_start {
                continue;
            }
            let rest = line[i..].trim_start();
            let Some(rest) = rest.strip_prefix('=') else { continue };
            let rest = rest.trim_start();
            let Some(rest) = rest.strip_prefix('"') else { continue };
            if let Some(end) = rest.find('"') {
                out.push(rest[..end].to_string());
            }
        }
    }
    out
}

/// A crate and every crate it reaches through `path =` dependencies,
/// transitively. Returns (input paths, crate directories). The top crate
/// contributes TOP_CRATE_FILES; a dependency contributes DEP_CRATE_FILES (its
/// lock file and .cargo are not read when it is built as a dependency); both
/// contribute the directory of a `[lib] path`, which is how the root `gale`
/// crate builds from plain/src.
fn crate_closure(repo: &Path, top: &str) -> Result<(Vec<String>, Vec<String>), String> {
    let mut inputs = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut stack = vec![(top.to_string(), true)];
    while let Some((dir, is_top)) = stack.pop() {
        if !seen.insert(dir.clone()) {
            continue;
        }
        let manifest = join(&dir, "Cargo.toml");
        let text = std::fs::read_to_string(repo.join(&manifest))
            .map_err(|_| format!("crate `{dir}` has no readable {manifest}"))?;
        let files = if is_top { TOP_CRATE_FILES } else { DEP_CRATE_FILES };
        inputs.extend(files.iter().map(|f| join(&dir, f)));
        if let Some(lib) = toml_value(&text, "lib", "path") {
            let lib = normalize(&dir, &lib).ok_or(format!("{manifest}: [lib] path leaves the repository"))?;
            let lib_dir = lib.rsplit_once('/').map(|(d, _)| d.to_string()).unwrap_or_default();
            inputs.push(if lib_dir.is_empty() { lib } else { lib_dir });
        }
        for p in cargo_path_deps(&text) {
            let dep = normalize(&dir, &p).ok_or(format!("{manifest}: path dependency `{p}` leaves the repository"))?;
            stack.push((dep, false));
        }
    }
    Ok((inputs, seen.into_iter().collect()))
}

/// The `#[path = "..."]` and `include!`/`include_str!`/`include_bytes!` targets
/// in one Rust source file, resolved against its directory. Err for a
/// non-literal include, which cannot be resolved without expanding macros.
pub fn rust_includes(file: &str, text: &str) -> Result<Vec<String>, String> {
    let dir = file.rsplit_once('/').map(|(d, _)| d).unwrap_or("");
    let mut out = Vec::new();
    for (n, raw) in text.lines().enumerate() {
        let line = raw.trim_start();
        if line.starts_with("//") {
            continue;
        }
        let mut lits: Vec<&str> = Vec::new();
        if let Some(rest) = line.strip_prefix("#[path") {
            let rest = rest.trim_start().strip_prefix('=').map(str::trim_start);
            match rest.and_then(|r| r.strip_prefix('"')).and_then(|r| r.split_once('"')) {
                Some((lit, _)) => lits.push(lit),
                None => return Err(format!("{file}:{}: unparseable #[path]", n + 1)),
            }
        }
        for mac in ["include!(", "include_str!(", "include_bytes!("] {
            let mut s = line;
            while let Some(k) = s.find(mac) {
                let rest = s[k + mac.len()..].trim_start();
                match rest.strip_prefix('"').and_then(|r| r.split_once('"')) {
                    Some((lit, _)) => lits.push(lit),
                    None => return Err(format!("{file}:{}: non-literal {mac}..) cannot be resolved", n + 1)),
                }
                s = &s[k + mac.len()..];
            }
        }
        for l in lits {
            out.push(normalize(dir, l).ok_or(format!("{file}:{}: `{l}` leaves the repository", n + 1))?);
        }
    }
    Ok(out)
}

/// What one gated object is built from.
struct Spec {
    /// Repo-relative paths whose files are hashed (directories or files).
    inputs: Vec<String>,
    /// Crate directories whose Rust sources are scanned for `#[path]`/`include!`.
    crates: Vec<String>,
    /// Where the object's synth/loom/meld command lines are written down, for an
    /// object with no builder script (the crate-adjacent ones).
    build_doc: Option<String>,
}

fn add_crate(repo: &Path, dir: &str, spec: &mut Spec) -> Result<(), String> {
    let (inputs, crates) = crate_closure(repo, dir)?;
    spec.inputs.extend(inputs);
    spec.crates.extend(crates);
    Ok(())
}

/// Every gated object (drivers-relative) and what it is built from, after the
/// census. Err is the verdict to return.
fn subjects(repo: &Path) -> Result<BTreeMap<String, Spec>, Verdict> {
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
    let refuse = |obj: &str, m: String| Verdict::Refused(format!("{REL}/{obj}: {m}"));

    let mut gated: BTreeMap<String, Spec> = BTreeMap::new();
    for (script, obj) in BUILDERS {
        let mut spec = Spec { inputs: inputs_for(repo, script), crates: Vec::new(), build_doc: None };
        // A derived input that is a crate brings its path dependencies.
        for i in spec.inputs.clone() {
            if repo.join(&i).join("Cargo.toml").is_file() {
                add_crate(repo, &i, &mut spec).map_err(|m| refuse(obj, m))?;
            }
        }
        for (s, crates) in EXTRA_CRATES {
            if s == script {
                for c in *crates {
                    add_crate(repo, &format!("{REL}/{c}"), &mut spec).map_err(|m| refuse(obj, m))?;
                }
            }
        }
        gated.insert(obj.to_string(), spec);
    }
    // Objects that live beside their own crate. Only a real crate directory is
    // auto-covered: a composed artifact dropped into a non-crate directory is
    // flagged by the census instead of mis-gated against the wrong inputs.
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
        let mut spec = Spec {
            inputs: seams.iter().map(|s| s.to_string()).collect(),
            crates: Vec::new(),
            build_doc: Some(format!("{REL}/{dir}/RESULTS.md")),
        };
        add_crate(repo, &format!("{REL}/{dir}"), &mut spec).map_err(|m| refuse(key, m))?;
        gated.insert(key.clone(), spec);
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
        .args(inputs.iter().map(|i| if i.is_empty() { "." } else { i.as_str() }))
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

/// Inputs that may legitimately be absent: a crate's `.cargo` (only the raw
/// `extern "C"` crates have one), its `wit/` (only crates that generate against
/// a local world), its `build.rs`, and a lock file. When one appears, it is
/// picked up.
fn is_optional_input(i: &str) -> bool {
    let leaf = i.rsplit('/').next().unwrap_or(i);
    OPTIONAL_CRATE_FILES.contains(&leaf)
}

/// The newest of a set of commit dates, failing CLOSED: a file with no commit
/// date (untracked, or never committed under this path) is an Err naming it,
/// never skipped. Skipping it -- a `filter_map` -- let an object be judged
/// fresh against the inputs that happened to be dated.
pub fn newest_dated(dated: &[(String, Option<i64>)]) -> Result<i64, String> {
    let mut newest: Option<i64> = None;
    for (path, t) in dated {
        let Some(t) = t else {
            return Err(format!("no commit date for input `{path}` — cannot cross-check a freshness claim"));
        };
        newest = Some(newest.map_or(*t, |n| n.max(*t)));
    }
    newest.ok_or_else(|| "no inputs to date".to_string())
}

/// The fresh-claim cross-check's comparison: the object is older than an input.
pub fn older_than(obj_t: i64, newest_input_t: i64) -> bool {
    newest_input_t > obj_t
}

/// Whether git dates contradict a freshness claim: the object's last content
/// commit predates that of one of its input FILES. Each file is dated on its
/// own; a directory dated as one path through `git log --follow` does not
/// reliably see a file added inside it. Fails closed through `newest_dated`.
///
/// The toolchain pin files are NOT dated: the recorded `toolchain` string
/// already covers every key that can change a build, and dating the files made
/// a comment-only commit to varve.toml contradict every fresh claim. Nor is
/// the build-command extract of RESULTS.md: it is not a file, and the
/// document's date moves with every prose edit.
fn dated_older_than_inputs(repo: &Path, obj: &str, files: &[String]) -> Result<bool, String> {
    let obj_t = last_commit_epoch(repo, obj).ok_or(format!("no commit date for {obj}"))?;
    let dated: Vec<(String, Option<i64>)> =
        files.iter().map(|f| (f.clone(), last_commit_epoch(repo, f))).collect();
    Ok(older_than(obj_t, newest_dated(&dated)?))
}

/// A fresh claim is cross-checked against git history, and a shallow clone has
/// none worth the name: every file's "last commit" is the one commit there is,
/// so an unrebuilt object reads as no older than its inputs. REFUSE rather than
/// answer `fresh` from a truncated history.
pub fn shallow_guard(is_shallow: bool, fresh_claims: usize) -> Result<(), String> {
    if is_shallow && fresh_claims > 0 {
        return Err(format!(
            "shallow clone with {fresh_claims} fresh claim(s): the date cross-check needs full history (fetch-depth: 0)"
        ));
    }
    Ok(())
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

/// The synth / loom / meld command lines written in a build document, in
/// order. The thin drivers have no builder script: RESULTS.md is the only
/// record of the flags they were dissolved with. Hashing only these lines
/// makes a flag edit an input change while a prose edit is not.
pub fn build_commands(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim)
        .filter(|l| BUILD_COMMAND_MARKERS.iter().any(|m| l.contains(m)))
        .map(String::from)
        .collect()
}

/// A file pulled in by `#[path]`/`include!` may lie inside its crate, or be
/// listed in `table`; anything else leaves the crate unseen.
pub fn include_allowed(krate: &str, target: &str, table: &[(&str, &str)]) -> bool {
    under(target, krate) || table.iter().any(|(c, t)| *c == krate && *t == target)
}

/// An object's input set, measured from the working tree.
struct Inputs {
    digest: String,
    files: Vec<String>,
    commands: usize,
}

/// Committed objects are OUTPUTS: a builder's input directory can hold one,
/// and an object must not be an input of itself. `outputs` is every committed
/// object, repo-relative.
///
/// Every declared input path must resolve to at least one file, or the gate
/// REFUSES and names it: a path that resolves to nothing contributes nothing to
/// the digest or the dates, which is how a derived input silently dropped out
/// before (gale#417). OPTIONAL_CRATE_FILES are the stated exceptions.
///
/// A `#[path]` or `include!` in a crate's sources that leaves the crate
/// directory REFUSES unless PATH_INCLUDES lists it; a listed one becomes an
/// input. That is how plain/src/executor.rs reaches os-ts-cm3.o.
fn measure_inputs(repo: &Path, spec: &Spec, outputs: &BTreeSet<String>) -> Result<Inputs, String> {
    let mut files = input_files(repo, &spec.inputs, outputs).ok_or("git ls-files failed on the input set")?;
    if files.is_empty() {
        return Err("the input set resolves to no files — a digest of nothing is vacuous".into());
    }
    for i in spec.inputs.iter().filter(|i| !is_optional_input(i)) {
        if !files.iter().any(|f| under(f, i)) {
            return Err(format!("declared input `{i}` resolves to no file — refusing rather than dropping it"));
        }
    }
    let mut extra: BTreeSet<String> = BTreeSet::new();
    for c in &spec.crates {
        for f in files.iter().filter(|f| under(f, c) && f.ends_with(".rs")) {
            let text = std::fs::read_to_string(repo.join(f)).map_err(|e| format!("{f}: {e}"))?;
            for t in rust_includes(f, &text)? {
                if !include_allowed(c, &t, PATH_INCLUDES) {
                    return Err(format!(
                        "{f} pulls in `{t}` from outside its crate ({c}) — list it in PATH_INCLUDES so it becomes an input"
                    ));
                }
                if !repo.join(&t).is_file() {
                    return Err(format!("{f} pulls in `{t}`, which is not a file"));
                }
                extra.insert(t);
            }
        }
    }
    files.extend(extra);
    files.sort();
    files.dedup();
    let mut hashed = Vec::with_capacity(files.len() + 1);
    for f in &files {
        let bytes = std::fs::read(repo.join(f)).map_err(|e| format!("{f}: {e}"))?;
        hashed.push((f.clone(), crate::sha256::hex(&bytes)));
    }
    let mut commands = 0;
    if let Some(doc) = &spec.build_doc {
        let text = std::fs::read_to_string(repo.join(doc))
            .map_err(|_| format!("{doc} is missing — it is the only record of this object's build flags"))?;
        let cmds = build_commands(&text);
        if cmds.is_empty() {
            return Err(format!("{doc} records no synth/loom/meld command line — the build flags are unknown"));
        }
        commands = cmds.len();
        hashed.push((format!("{doc}#build-commands"), crate::sha256::hex(cmds.join("\n").as_bytes())));
    }
    Ok(Inputs { digest: fold_digest(&hashed), files, commands })
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

/// Every `key = value` of `[section]`, comments and spacing stripped, sorted
/// by key. Err on a line it cannot read as one key and one value (a multi-line
/// array, say), so a setting cannot slip out of the string unseen.
pub fn toml_section(text: &str, section: &str) -> Result<String, String> {
    let mut inside = false;
    let mut kv: Vec<(String, String)> = Vec::new();
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            inside = line == format!("[{section}]");
            continue;
        }
        if !inside || line.is_empty() {
            continue;
        }
        let Some((k, v)) = line.split_once('=') else {
            return Err(format!("[{section}]: cannot read `{line}` as key = value"));
        };
        // Quotes are dropped so the string can sit inside a TOML string.
        let v: String = v.replace('"', "").split_whitespace().collect::<Vec<_>>().join(" ");
        if v.starts_with('[') && !v.ends_with(']') {
            return Err(format!("[{section}]: multi-line value for `{}` is not supported", k.trim()));
        }
        kv.push((k.trim().to_string(), v));
    }
    if kv.is_empty() {
        return Err(format!("no [{section}] settings"));
    }
    kv.sort();
    Ok(kv.into_iter().map(|(k, v)| format!("{k}={v}")).collect::<Vec<_>>().join(" "))
}

/// The toolchain pin, an input to EVERY gated object (gale#438): every object
/// here is a synth output, so the pin decides its bytes as much as any source
/// does (iso-core-fused-cm3.o: 15094 B committed, 15050 B rebuilt from the
/// pin). Every setting of both `[toolchain]` tables is in the string: the varve
/// layer + digest fix synth and loom; rust-toolchain.toml contributes its
/// channel AND its components/targets/profile. A comment does not.
pub fn toolchain(repo: &Path) -> Result<String, String> {
    let mut parts = Vec::new();
    for f in TOOLCHAIN_FILES {
        let text = std::fs::read_to_string(repo.join(f)).map_err(|e| format!("{f}: {e}"))?;
        parts.push(format!("{f}: {}", toml_section(&text, "toolchain").map_err(|m| format!("{f}: {m}"))?));
    }
    Ok(parts.join("; "))
}

/// Why `bytes` is not a relocatable ELF for the expected machine, if it is not.
/// Garbage whose hash was re-recorded passes the hash check; it does not pass
/// this.
pub fn elf_problem(bytes: &[u8], want_riscv: bool) -> Option<String> {
    if bytes.len() < 20 {
        return Some(format!("{} bytes is too short for an ELF header", bytes.len()));
    }
    if &bytes[..4] != b"\x7fELF" {
        return Some("not an ELF file (bad magic)".into());
    }
    let rd = |o: usize| match bytes[5] {
        1 => Some(u16::from_le_bytes([bytes[o], bytes[o + 1]])),
        2 => Some(u16::from_be_bytes([bytes[o], bytes[o + 1]])),
        _ => None,
    };
    let (Some(ty), Some(machine)) = (rd(16), rd(18)) else {
        return Some("ELF with an invalid data encoding".into());
    };
    if ty != 1 {
        return Some(format!("ELF type {ty}, not ET_REL (1) — not a relocatable object"));
    }
    let (want, name) = if want_riscv { (243, "RISC-V") } else { (40, "ARM") };
    if machine != want {
        return Some(format!("ELF machine {machine}, expected {name} ({want})"));
    }
    None
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

/// A CI warning when the earliest known-stale expiry is under EXPIRY_WARN_DAYS
/// away: once it passes, this required check blocks every PR.
pub fn expiry_warning(earliest: i64, today: i64) -> Option<String> {
    (earliest - today < EXPIRY_WARN_DAYS).then(|| {
        format!(
            "::warning::object-freshness: a known-stale ledger entry expires on {} ({} day(s)); after that this required check fails every PR until the objects are rebuilt and re-validated, or the date is re-decided",
            ymd(earliest * 86_400),
            earliest - today
        )
    })
}

/// What a gated object currently is, measured from the tree.
pub struct Observed<'a> {
    pub object_sha256: &'a str,
    pub inputs_sha256: &'a str,
    pub toolchain: &'a str,
    pub today: i64,
    /// Why the committed bytes are not a relocatable ELF of the right machine.
    pub elf_problem: Option<String>,
    /// git dates contradict a freshness claim: the object's last content commit
    /// predates the last content commit of an input file. None when not
    /// measured (the entry is ledgered, so it claims nothing).
    pub dated_older_than_inputs: Option<bool>,
}

/// The decision for ONE gated object, separated from I/O so the self-test can
/// drive it. Ok(None) = recorded fresh; Ok(Some((issue, expires))) = known-stale
/// and accepted; Err = the reasons it fails. No entry at all is a failure.
pub fn judge(e: Option<&Entry>, o: &Observed) -> Result<Option<(String, String)>, Vec<String>> {
    let Some(e) = e else {
        return Err(vec![format!("gated object with no [[binary]] entry in {LEDGER}")]);
    };
    let mut bad = Vec::new();
    if o.object_sha256 != e.sha256 {
        bad.push(format!(
            "bytes hash to {} but the record says {} — this is not the object the entry describes",
            o.object_sha256, e.sha256
        ));
    }
    if let Some(p) = &o.elf_problem {
        bad.push(format!("not a relocatable object: {p}"));
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
                None => bad.push(format!("stale_expires `{x}` is malformed: not a real YYYY-MM-DD date")),
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
             input files — a record updated without a rebuild"
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

/// Freshness fields on an entry this gate does not judge: a claim nobody checks.
pub fn orphan_records(entries: &[Entry], gated: &BTreeSet<&str>) -> Vec<String> {
    entries
        .iter()
        .filter(|e| is_freshness_entry(e) && !gated.contains(e.path.as_str()))
        .map(|e| format!("{}: has freshness fields but is not a gated object — nothing judges them", e.path))
        .collect()
}

/// One gated object, measured.
struct Measured {
    path: String,
    sha256: String,
    elf_problem: Option<String>,
    inputs: Inputs,
}

fn measure(repo: &Path) -> Result<Vec<Measured>, Verdict> {
    let gated = subjects(repo)?;
    let outputs: BTreeSet<String> = committed_objects(repo)
        .ok_or_else(|| Verdict::Refused("git ls-files failed — the gate cannot answer".into()))?
        .into_iter()
        .map(|o| format!("{REL}/{o}"))
        .collect();
    let mut out = Vec::new();
    for (obj, spec) in gated {
        let path = format!("{REL}/{obj}");
        let bytes = std::fs::read(repo.join(&path))
            .map_err(|e| Verdict::Refused(format!("{path}: cannot read the committed object: {e}")))?;
        let inputs =
            measure_inputs(repo, &spec, &outputs).map_err(|m| Verdict::Refused(format!("{path}: {m}")))?;
        out.push(Measured { sha256: crate::sha256::hex(&bytes), elf_problem: elf_problem(&bytes, false), path, inputs });
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
    let find = |p: &str| entries.iter().find(|e| e.path == p);
    let fresh_claims = measured
        .iter()
        .filter(|m| find(&m.path).is_some_and(|e| e.stale_issue.is_none() && e.stale_expires.is_none()))
        .count();
    let Some(shallow) = git(repo, &["rev-parse", "--is-shallow-repository"]) else {
        return Verdict::Refused("git rev-parse --is-shallow-repository failed".into());
    };
    if let Err(m) = shallow_guard(shallow == "true", fresh_claims) {
        return Verdict::Refused(m);
    }

    let mut lines = vec![format!("toolchain: {tc}"), format!("today (UTC): {}", ymd(today * 86_400))];
    lines.push(format!("{:<26} {:>6}  verdict", "object", "inputs"));
    let mut fails: Vec<String> = Vec::new();
    let (mut fresh, mut stale) = (0usize, 0usize);
    let mut earliest: Option<(i64, String)> = None;
    for m in &measured {
        let n = m.inputs.files.len() + m.inputs.commands.min(1);
        let short = m.path.rsplit('/').next().unwrap_or(&m.path);
        let e = find(&m.path);
        // The date cross-check is only for a freshness CLAIM; a ledgered entry
        // already says the object is not a build of its inputs.
        let dated = match e {
            Some(e) if e.stale_issue.is_none() && e.stale_expires.is_none() => {
                match dated_older_than_inputs(repo, &m.path, &m.inputs.files) {
                    Ok(b) => Some(b),
                    Err(msg) => return Verdict::Refused(format!("{}: {msg}", m.path)),
                }
            }
            _ => None,
        };
        let o = Observed {
            object_sha256: &m.sha256,
            inputs_sha256: &m.inputs.digest,
            toolchain: &tc,
            today,
            elf_problem: m.elf_problem.clone(),
            dated_older_than_inputs: dated,
        };
        match judge(e, &o) {
            Ok(None) => {
                fresh += 1;
                lines.push(format!("{short:<26} {n:>6}  recorded-fresh (not rebuilt)"));
            }
            Ok(Some((issue, exp))) => {
                stale += 1;
                if let Some(d) = parse_day(&exp) {
                    if earliest.as_ref().is_none_or(|(x, _)| d < *x) {
                        earliest = Some((d, exp.clone()));
                    }
                }
                lines.push(format!("{short:<26} {n:>6}  KNOWN-STALE {issue}, expires {exp}"));
            }
            Err(why) => {
                lines.push(format!("{short:<26} {n:>6}  FAIL"));
                fails.extend(why.into_iter().map(|w| format!("{}: {w}", m.path)));
            }
        }
    }
    let gated: BTreeSet<&str> = measured.iter().map(|m| m.path.as_str()).collect();
    fails.extend(orphan_records(&entries, &gated));
    if let Some(w) = earliest.as_ref().and_then(|(d, _)| expiry_warning(*d, today)) {
        // A workflow command must start its own line on stdout.
        println!("{w}");
    }

    lines.push(format!(
        "{} gated object(s): {fresh} recorded-fresh (not rebuilt), {stale} known-stale{}, {} failing; {} UNCOVERED object(s) are hash-pinned by binary-ledger only",
        measured.len(),
        earliest.map(|(_, x)| format!(" (earliest expiry {x})")).unwrap_or_default(),
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
    for m in measured {
        out.push(format!("path = \"{}\"", m.path));
        out.push(format!("sha256 = \"{}\"", m.sha256));
        out.push(format!(
            "inputs_sha256 = \"{}\"  # {} file(s), {} build command line(s)",
            m.inputs.digest,
            m.inputs.files.len(),
            m.inputs.commands
        ));
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
    let tc = "varve.toml: layer=\"1\"; rust-toolchain.toml: channel=\"stable\"".to_string();
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
    struct Seen {
        o: String,
        i: String,
        t: String,
        day: i64,
        elf: Option<String>,
        dated: Option<bool>,
    }
    let seen = |o: &str, i: &str, t: &str, dated: Option<bool>| Seen {
        o: o.into(),
        i: i.into(),
        t: t.into(),
        day: today,
        elf: None,
        dated,
    };
    let j = |e: Option<&Entry>, s: &Seen| {
        judge(
            e,
            &Observed {
                object_sha256: &s.o,
                inputs_sha256: &s.i,
                toolchain: &s.t,
                today: s.day,
                elf_problem: s.elf.clone(),
                dated_older_than_inputs: s.dated,
            },
        )
    };
    let good = seen(&obj, &inp, &tc, Some(false));
    let stale_seen = seen(&obj, &inp, &tc, None);
    let garbage = crate::sha256::hex(b"13 bytes junk");
    let edited = fold_digest(&[("drivers/x/src/lib.rs".into(), crate::sha256::hex(b"fn x() { }"))]);
    let errs = |r: Result<Option<(String, String)>, Vec<String>>, needle: &str| {
        r.is_err_and(|v| v.iter().any(|m| m.contains(needle)))
    };

    ck("control: recorded object, inputs and pin match: recorded-fresh", j(Some(&base), &good) == Ok(None));
    ck("planted garbage object: fail", errs(j(Some(&base), &seen(&garbage, &inp, &tc, Some(false))), "bytes hash to"));
    ck(
        "planted garbage object on a KNOWN-STALE entry: still fail",
        errs(j(Some(&ledgered("#506", "2026-12-31")), &seen(&garbage, &inp, &tc, None)), "bytes hash to"),
    );
    ck(
        "garbage whose hash was re-recorded: fails the ELF check",
        errs(j(Some(&base), &Seen { elf: elf_problem(b"13 bytes junk", false), ..seen(&obj, &inp, &tc, Some(false)) }), "not a relocatable object"),
    );
    ck("one-byte source edit: fail", errs(j(Some(&base), &seen(&obj, &edited, &tc, Some(false))), "inputs changed"));
    ck(
        "one-byte source edit on a KNOWN-STALE entry: still fail",
        errs(j(Some(&ledgered("#506", "2026-12-31")), &seen(&obj, &edited, &tc, None)), "inputs changed"),
    );
    ck("toolchain pin moved: fail", errs(j(Some(&base), &seen(&obj, &inp, "other", Some(false))), "toolchain pin changed"));
    ck(
        "KNOWN-STALE within its expiry: accepted, and reported as stale",
        j(Some(&ledgered("#506", "2026-12-31")), &stale_seen) == Ok(Some(("#506".into(), "2026-12-31".into()))),
    );
    ck("KNOWN-STALE on its expiry day: still accepted", j(Some(&ledgered("#506", "2026-11-18")), &stale_seen).is_ok());
    ck(
        "KNOWN-STALE one day past its expiry: fail",
        errs(j(Some(&ledgered("#506", "2026-11-17")), &stale_seen), "EXPIRED"),
    );
    // The cap, at its boundary: 2026-11-18 + 183 = 2027-05-20.
    ck("KNOWN-STALE expiry exactly at the cap (183 days): accepted", j(Some(&ledgered("#506", "2027-05-20")), &stale_seen).is_ok());
    ck(
        "KNOWN-STALE expiry one day past the cap (184 days): fail",
        errs(j(Some(&ledgered("#506", "2027-05-21")), &stale_seen), "permanent excuse"),
    );
    ck(
        "KNOWN-STALE with no issue: fail",
        errs(j(Some(&Entry { stale_expires: Some("2026-12-31".into()), ..base.clone() }), &good), "BOTH"),
    );
    ck(
        "KNOWN-STALE with no expiry: fail",
        errs(j(Some(&Entry { stale_issue: Some("#506".into()), ..base.clone() }), &good), "BOTH"),
    );
    ck("KNOWN-STALE with a malformed issue: fail", errs(j(Some(&ledgered("soon", "2026-12-31")), &stale_seen), "stale_issue"));
    // A FUTURE impossible date: a past one would fail as expired even if the
    // date parser accepted it.
    ck(
        "KNOWN-STALE with a future impossible date (2027-02-30): fail as malformed",
        errs(j(Some(&ledgered("#506", "2027-02-30")), &stale_seen), "malformed"),
    );
    ck(
        "KNOWN-STALE naming another repo's issue: accepted",
        j(Some(&ledgered("pulseengine/synth#1", "2026-12-31")), &stale_seen).is_ok(),
    );
    ck(
        "fresh claim contradicted by git dates: fail",
        errs(j(Some(&base), &seen(&obj, &inp, &tc, Some(true))), "claims the object is fresh"),
    );
    ck("gated object with no ledger entry: fail", errs(j(None, &good), "no [[binary]] entry"));
    ck("no inputs_sha256 recorded: fail", j(Some(&Entry { inputs_sha256: None, ..base.clone() }), &good).is_err());
    ck("no toolchain recorded: fail", j(Some(&Entry { toolchain: None, ..base.clone() }), &good).is_err());
    let gated: BTreeSet<&str> = ["drivers/x/x-cm3.o"].into_iter().collect();
    let orphan = Entry { path: "os/y.o".into(), ..ledgered("#506", "2026-12-31") };
    ck("freshness fields on an ungated object: fail", orphan_records(&[base.clone(), orphan], &gated).len() == 1);
    ck("plain entry on an ungated object: no complaint", orphan_records(&[Entry { path: "os/y.o".into(), ..Entry::default() }], &gated).is_empty());

    // The date comparison, in its direction: older object than input = stale.
    ck("dating: object older than its newest input is contradicted", older_than(100, 200));
    ck("dating: object newer than its inputs is not", !older_than(200, 100));
    ck("dating: same commit is not", !older_than(100, 100));
    let d = |p: &str, t: Option<i64>| (p.to_string(), t);
    ck("dating: newest of dated inputs", newest_dated(&[d("a", Some(1)), d("b", Some(5))]) == Ok(5));
    ck(
        "dating: an undated input (untracked file) REFUSES, never skipped",
        newest_dated(&[d("a", Some(9)), d("drivers/x/src/new.rs", None)]).is_err_and(|m| m.contains("drivers/x/src/new.rs")),
    );
    ck("dating: no inputs at all REFUSES", newest_dated(&[]).is_err());
    ck("shallow clone with a fresh claim REFUSES", shallow_guard(true, 1).is_err());
    ck("shallow clone with only known-stale entries is answerable", shallow_guard(true, 0).is_ok());
    ck("full clone with fresh claims is answerable", shallow_guard(false, 3).is_ok());
    ck("expiry warning at 20 days", expiry_warning(today + 20, today).is_some_and(|w| w.starts_with("::warning::")));
    ck("no expiry warning at 21 days", expiry_warning(today + 21, today).is_none());

    // ELF sanity.
    let mut elf = vec![0u8; 20];
    elf[..4].copy_from_slice(b"\x7fELF");
    elf[5] = 1;
    elf[16] = 1;
    elf[18] = 40;
    ck("ELF: ARM ET_REL accepted", elf_problem(&elf, false).is_none());
    ck("ELF: ARM where RISC-V is expected: fail", elf_problem(&elf, true).is_some());
    let mut exec = elf.clone();
    exec[16] = 2;
    ck("ELF: ET_EXEC is not a relocatable object", elf_problem(&exec, false).is_some());
    ck("ELF: 13 bytes of garbage: fail", elf_problem(b"13 bytes junk", false).is_some());

    ck(
        "digest depends on the path, not only the bytes",
        fold_digest(&[("a".into(), "h".into())]) != fold_digest(&[("b".into(), "h".into())]),
    );
    ck(
        "digest does not depend on listing order",
        fold_digest(&[("a".into(), "1".into()), ("b".into(), "2".into())])
            == fold_digest(&[("b".into(), "2".into()), ("a".into(), "1".into())]),
    );

    // Input derivation, as pure functions.
    ck(
        "Cargo path dependencies: inline, table and target forms; dev-dependencies skipped",
        cargo_path_deps(
            "[dependencies]\ngale = { path = \"../..\" }\nx = \"1\"\n[dependencies.m]\npath = \"../m\"\n\
             [target.'cfg(a)'.dependencies]\nt = { version = \"1\", path = \"../t\" }\n[dev-dependencies]\nd = { path = \"../d\" }\n\
             [lib]\npath = \"src/lib.rs\"\n",
        ) == vec!["../..".to_string(), "../m".to_string(), "../t".to_string()],
    );
    ck("normalize: ../.. from a crate is the repo root", normalize("drivers/mpu-thin", "../..").as_deref() == Some(""));
    ck("normalize: climbing above the root is refused", normalize("drivers", "../../x").is_none());
    ck(
        "#[path] resolves against the file's directory",
        rust_includes("drivers/e/src/lib.rs", "#[path = \"../../../plain/src/executor.rs\"]\nmod ex;\n")
            == Ok(vec!["plain/src/executor.rs".to_string()]),
    );
    ck(
        "include! / include_str! resolve; doc comments do not",
        rust_includes("d/c/src/lib.rs", "//! #[path = \"x\"]\ninclude!(\"../gen.rs\");\nconst W: &str = include_str!(\"../wit/w.wit\");\n")
            == Ok(vec!["d/c/gen.rs".to_string(), "d/c/wit/w.wit".to_string()]),
    );
    ck(
        "an unlisted #[path] leaving its crate is not allowed",
        !include_allowed("drivers/wdg-thin", "drivers/uart-thin/src/lib.rs", PATH_INCLUDES),
    );
    ck("the listed executor #[path] is allowed", include_allowed("drivers/exec-provider", "plain/src/executor.rs", PATH_INCLUDES));
    ck("a file inside its own crate is allowed", include_allowed("drivers/c", "drivers/c/gen.rs", &[]));
    ck("a non-literal include! REFUSES", rust_includes("a/b.rs", "include!(concat!(env!(\"OUT_DIR\"), \"/x.rs\"));").is_err());
    ck(
        "build commands: flag lines kept, prose dropped",
        build_commands("Prose about synth 0.52.0.\n    loom optimize <w> --passes inline | synth compile --target cortex-m3\n")
            == vec!["loom optimize <w> --passes inline | synth compile --target cortex-m3".to_string()],
    );
    ck(
        "toolchain: every [toolchain] setting, comments ignored",
        toml_section("[toolchain]\n# c\nchannel = \"stable\"  # x\ncomponents = [\"rustfmt\", \"clippy\"]\n[other]\nk = 1\n", "toolchain").as_deref()
            == Ok("channel=stable components=[rustfmt, clippy]"),
    );
    ck("toolchain: a multi-line array REFUSES", toml_section("[toolchain]\ncomponents = [\n\"a\",\n]\n", "toolchain").is_err());

    ck("ymd epoch 0", ymd(0) == "1970-01-01");
    ck("ymd 1787000000", ymd(1_787_000_000) == "2026-08-17");
    ck("parse_day round-trips 2026-11-18", parse_day("2026-11-18").map(|d| ymd(d * 86_400)).as_deref() == Some("2026-11-18"));
    ck("parse_day 1970-01-01 is day 0", parse_day("1970-01-01") == Some(0));
    ck("parse_day 2024-02-29 (leap) accepted", parse_day("2024-02-29").is_some());

    // Against the real tree: the measurement must not be vacuous, and the
    // derivations must reach what reviewers found missing.
    match toolchain(repo) {
        Ok(t) => ck(
            "toolchain pin derived from both files, components included",
            t.contains("sha256:") && t.contains("channel=") && t.contains("components="),
        ),
        Err(_) => ck("toolchain pin could not be derived", false),
    }
    ck(
        "dating against the repo: committed inputs date",
        dated_older_than_inputs(repo, "drivers/wdg-thin/wdg-thin-cm3.o", &["drivers/wdg-thin/src/lib.rs".into()]).is_ok(),
    );
    ck(
        "dating against the repo: an uncommitted input REFUSES, naming it",
        dated_older_than_inputs(
            repo,
            "drivers/wdg-thin/wdg-thin-cm3.o",
            &["drivers/wdg-thin/src/lib.rs".into(), "drivers/wdg-thin/src/never-committed.rs".into()],
        )
        .is_err_and(|m| m.contains("never-committed.rs")),
    );
    let none = BTreeSet::new();
    let spec = |inputs: &[&str]| Spec { inputs: inputs.iter().map(|s| s.to_string()).collect(), crates: Vec::new(), build_doc: None };
    ck(
        "a declared input that resolves to nothing REFUSES, naming it",
        measure_inputs(repo, &spec(&["drivers/wdg-thin/src", "drivers/no-such-input"]), &none)
            .is_err_and(|m| m.contains("drivers/no-such-input")),
    );
    let with_doc = Spec { build_doc: Some("drivers/wdg-thin/RESULTS.md".into()), ..spec(&["drivers/wdg-thin/src"]) };
    match (measure_inputs(repo, &with_doc, &none), measure_inputs(repo, &spec(&["drivers/wdg-thin/src"]), &none)) {
        (Ok(w), Ok(wo)) => ck(
            "the RESULTS.md build command lines are hashed into the digest",
            w.commands > 0 && w.digest != wo.digest,
        ),
        _ => ck("the RESULTS.md build command lines could not be measured", false),
    }
    ck(
        "a crate's absent .cargo is an optional input",
        measure_inputs(repo, &spec(&["drivers/wdg-thin/src", "drivers/wdg-thin/.cargo"]), &none).is_ok(),
    );
    match subjects(repo) {
        Ok(s) => {
            let has = |obj: &str, input: &str| s.get(obj).is_some_and(|sp| sp.inputs.iter().any(|i| i == input));
            let crate_of = |obj: &str, c: &str| s.get(obj).is_some_and(|sp| sp.crates.iter().any(|i| i == c));
            ck("hm-thin's local wit/ world is an input", has("hm-thin/hm-thin-cm3.o", "drivers/hm-thin/wit"));
            ck("iso-core reaches hm-thin's wit/ too", has("iso-core-fused-cm3.o", "drivers/hm-thin/wit"));
            ck("mpu-thin reaches the root gale crate (path = \"../..\")", crate_of("mpu-thin/mpu-thin-cm3.o", "") && has("mpu-thin/mpu-thin-cm3.o", "src"));
            ck("the root crate's [lib] path dir (plain/src) is an input of mpu-thin", has("mpu-thin/mpu-thin-cm3.o", "plain/src"));
            ck("time-provider's path dependency os-time-math reaches os-time", has("os-node/os-time-cm3.o", "drivers/os-time-math/src"));
            ck("exec-provider is scanned for #[path] as part of os-ts", crate_of("os-node/os-ts-cm3.o", "drivers/exec-provider"));
            ck("breadth reaches the gpio-comp crate it builds through $d", has("breadth/breadth-cm3.o", "drivers/gpio-comp/src"));
            ck("iso-core reaches the hm-thin crate it passes as an argument", has("iso-core-fused-cm3.o", "drivers/hm-thin/src"));
            ck("a thin driver's flags come from its RESULTS.md", s.get("wdg-thin/wdg-thin-cm3.o").is_some_and(|sp| sp.build_doc.is_some()));
            let outputs: BTreeSet<String> = committed_objects(repo).unwrap_or_default().into_iter().map(|o| format!("{REL}/{o}")).collect();
            ck(
                "os-ts's measured inputs include the #[path]-included executor",
                s.get("os-node/os-ts-cm3.o")
                    .and_then(|sp| measure_inputs(repo, sp, &outputs).ok())
                    .is_some_and(|m| m.files.iter().any(|f| f == "plain/src/executor.rs")),
            );
        }
        Err(_) => ck("subjects could not be derived", false),
    }
    if let Some(found) = committed_objects(repo) {
        ck("root-level committed object is enumerated", found.iter().any(|f| f == "iso-core-fused-cm3.o"));
        ck("nested committed object is still enumerated", found.iter().any(|f| f == "os-node/os-tl-cm3.o"));
        ck("enumeration is objects only, not the whole tree", found.iter().all(|f| f.ends_with(".o")) && found.len() < 100);
    } else {
        ck("committed_objects could not run", false);
    }
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
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

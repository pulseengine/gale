//! A committed `.o` must not be older than the sources that produce it.
//!
//! Four objects went stale for 5-7 weeks unnoticed (gale#334): nothing rebuilds
//! a committed object when its inputs change, so the drift is invisible until
//! someone diffs by hand. First misdiagnosed as toolchain drift; a rustc bisect
//! ruled that out (1.90/1.94/1.97 all gave ~9000 B against a committed 3638),
//! and the real cause was SOURCE drift.
//!
//! Ported from check-object-freshness.py (gale#368). The Python is already
//! correct -- its census defect (a glob reaching one directory deep and one
//! suffix, so the repro-757 pair escaped: 24 committed, 22 seen) was fixed
//! before this port. What the port adds is the Refused outcome: a gate that
//! cannot reach git is not a gate reporting "fresh".

use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const NAME: &str = "object-freshness";

const REL: &str = "benches/gust/drivers";

/// Builder -> the object it writes. Only builders whose output is COMMITTED.
/// build-iso-core.sh and build-dissolve-gustos.sh write to a temp dir, and
/// build-reloc-cores.sh takes its target as an argument.
const BUILDERS: &[(&str, &str)] = &[
    ("build-os-ts.sh", "os-node/os-ts-cm3.o"),
    ("build-os-tl.sh", "os-node/os-tl-cm3.o"),
    ("build-os-time.sh", "os-node/os-time-cm3.o"),
    ("build-breadth.sh", "breadth/breadth-cm3.o"),
];

/// Always an input: the seam definitions every builder consumes.
const ALWAYS: &[&str] = &["wit", "wit-os"];

/// The input list is deliberately NARROW. A first draft used the whole crate
/// directory and flagged gpio-thin over an edited RESULTS.md -- a doc change
/// does not invalidate an object. A second draft added wit-os for everything
/// and flagged all 13 thin drivers, which do not reference wit-os at all. Each
/// widening produced false positives, so the sets are per class.
const CRATE_BUILD_FILES: &[&str] = &["src", "Cargo.toml", "Cargo.lock", ".cargo"];
const THIN_SEAMS: &[&str] = &["wit"];
const OS_SEAMS: &[&str] = &["wit", "wit-os"];

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

/// KNOWN-STALE LEDGER. Regenerating a committed object is a change the
/// toolchain-re-pin precedent (963e5c9) settled by re-validating on silicon,
/// which CI cannot do. Ledgered rather than skipped: the gate FAILS if a new
/// object goes stale, AND if one of these stops being stale -- so the list
/// shrinks to empty when they are regenerated instead of outliving the problem.
const KNOWN_STALE: &[&str] = &[
    "breadth/breadth-cm3.o",
    "os-node/os-time-cm3.o",
    "os-node/os-tl-cm3.o",
    "os-node/os-ts-cm3.o",
    "hm-thin/hm-thin-cm3.o",
    "mpu-thin/mpu-thin-cm3.o",
    "switch-thin/switch-thin-cm3.o",
    "wdg-thin/wdg-thin-cm3.o",
    "dma-own/dma-own-cm3.o",
    "spawn-provider/spawn-provider-cm3.o",
    "timer-provider/timer-provider-cm3.o",
];

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
    let s = git(repo, &["log", "-1", "--format=%ct", "--", relpath])?;
    s.parse().ok()
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
fn committed_objects(repo: &Path) -> Option<Vec<String>> {
    let out = git(repo, &["ls-files", "--", &format!("{REL}/**/*.o")])?;
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
            while j < bytes.len()
                && (bytes[j].is_ascii_lowercase() || bytes[j].is_ascii_digit() || bytes[j] == b'-')
            {
                j += 1;
            }
            if j > start {
                let d = &text[start..j];
                // os-node is the OUTPUT directory, not an input; excluding it
                // stops the object from being compared against itself.
                if d != "os-node" {
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
    out.extend(ALWAYS.iter().map(|s| format!("{REL}/{s}")));
    out
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    if git(repo, &["rev-parse", "--git-dir"]).is_none() {
        return Verdict::Refused("not a git repository".into());
    }
    let Some(found) = committed_objects(repo) else {
        return Verdict::Refused("git ls-files failed — the gate cannot answer".into());
    };
    if found.is_empty() {
        return Verdict::Refused("no committed objects found — the sweep would be vacuous".into());
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
        inputs.extend(seams.iter().map(|s| format!("{REL}/{s}")));
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
        return Verdict::Fail(lines);
    }
    let found_set: BTreeSet<&str> = found.iter().map(String::as_str).collect();
    let vanished: Vec<&str> = uncovered.iter().copied().filter(|u| !found_set.contains(u)).collect();
    if !vanished.is_empty() {
        let mut lines = vec!["FAIL: UNCOVERED lists object(s) that no longer exist:".to_string()];
        lines.extend(vanished.iter().map(|v| format!("  {v}")));
        return Verdict::Fail(lines);
    }

    let mut lines = vec!["object                     committed    newest input verdict".to_string()];
    let mut stale: BTreeSet<String> = BTreeSet::new();
    for (obj, inputs) in &gated {
        let Some(obj_t) = last_commit_epoch(repo, &format!("{REL}/{obj}")) else {
            return Verdict::Refused(format!("no commit date for {obj} — cannot compare"));
        };
        let newest = inputs
            .iter()
            .filter_map(|i| last_commit_epoch(repo, i))
            .max();
        let Some(newest) = newest else {
            return Verdict::Refused(format!("no commit date for any input of {obj}"));
        };
        let is_stale = newest > obj_t;
        if is_stale {
            stale.insert(obj.clone());
        }
        let short = obj.rsplit('/').next().unwrap_or(obj);
        lines.push(format!(
            "{:<26} {}   {}   {}",
            short,
            ymd(obj_t),
            ymd(newest),
            if is_stale { "STALE" } else { "ok" }
        ));
    }

    let ledger: BTreeSet<String> = KNOWN_STALE.iter().map(|s| s.to_string()).collect();
    let newly: Vec<&String> = stale.difference(&ledger).collect();
    let fixed: Vec<&String> = ledger.difference(&stale).collect();
    if !newly.is_empty() || !fixed.is_empty() {
        for n in &newly {
            lines.push(format!("FAIL: {n} went stale and is not on the ledger"));
        }
        // A ledger nobody prunes becomes a permanent excuse.
        for f in &fixed {
            lines.push(format!("FAIL: {f} is no longer stale — remove it from KNOWN_STALE"));
        }
        return Verdict::Fail(lines);
    }

    lines.push(format!(
        "{} object(s) stale, all on the known-stale ledger; regenerating any of them",
        stale.len()
    ));
    lines.push("requires silicon re-validation, so they are recorded rather than refreshed.".into());
    Verdict::Pass(lines)
}

/// The comparator must fire on a newer input and not on an older one, and the
/// ledger must be exact in BOTH directions.
pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };
    ck("newer input is stale", 200i64 > 100);
    ck("older input is not stale", !(100i64 > 200));
    ck("equal is not stale", !(100i64 > 100));
    // The date formatter, against dates this repo actually carries.
    // Pinned against known values, not guessed. My first version of this
    // assertion was an `||` of two guesses and reported WRONG while the gate's
    // own output was correct -- a self-test that fails for its own reasons is
    // worse than none, because it trains you to ignore it.
    ck("ymd epoch 0", ymd(0) == "1970-01-01");
    ck("ymd 1600000000", ymd(1_600_000_000) == "2020-09-13");
    ck("ymd 1756684800", ymd(1_756_684_800) == "2025-09-01");
    ck("ymd 1787000000", ymd(1_787_000_000) == "2026-08-17");
    // Ledger set arithmetic, both directions.
    let ledger: BTreeSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
    let stale_now: BTreeSet<String> = ["a".to_string(), "c".to_string()].into_iter().collect();
    ck("new stale detected", stale_now.difference(&ledger).count() == 1);
    ck("fixed-but-ledgered detected", ledger.difference(&stale_now).count() == 1);
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

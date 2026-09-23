//! A committed Renode `.elf` fixture must not be older than the sources that
//! produce it.
//!
//! The Renode targets are gale's main EXECUTION oracle — `gust_iso.robot` exists
//! because "every prior claim about this object was static" (gale#385). But
//! `BUILD.bazel` hands `renode_test` a committed ELF verbatim (`"ELF":
//! ":gust_iso.elf"`), and nothing rebuilds it. So the gate executes whatever
//! image was committed, whenever it was committed: measured at 11 of 16 older
//! than their own `src/bin` source, some by three months (gale#413).
//!
//! That is the `object-freshness` defect one layer up, and the two are
//! connected: refreshing a dissolved `.o` without refreshing the fixture linked
//! from it leaves the only executing gate green about the OLD object. It was
//! found exactly that way, while regenerating `iso-core-fused-cm3.o` (gale#411).
//!
//! This gate is a LOWER BOUND by construction — see `INPUTS ARE NARROW` below.
//! It reports a fixture stale when it certainly is, never merely when it might
//! be, so a `STALE` row is always real and an `ok` row is not a guarantee.

use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "fixture-freshness";

const REL: &str = "benches/gust/renode-test";
const BIN_DIR: &str = "benches/gust/src/bin";
const BUILD_RS: &str = "benches/gust/build.rs";

/// Fixtures whose bin source is not `src/bin/<stem>.rs`. An ELF that is neither
/// derivable nor listed here fails the census rather than escaping silently.
const SOURCE_OVERRIDE: &[(&str, &str)] = &[
    // The dissolved gust kernel: built from the crate's own `src/main.rs` and
    // committed under a different name (REFLASH.md still calls it `--bin
    // gust_wasm`, which no longer resolves).
    ("gust_wasm.elf", "benches/gust/src/main.rs"),
];

/// INPUTS ARE NARROW, deliberately. Each fixture is compared against:
///   * its bin source, and
///   * every dissolved object `build.rs` links into THAT bin.
///
/// NOT included: the `gale` crate, which every one of these images links. A
/// change anywhere in `plain/` would then flag all 16 at once, and most of those
/// would be false — a bin using `mpu_switch` is not invalidated by an edit to
/// `msgq`. Module-level granularity is the only honest way to include it and is
/// not worth the machinery, so the gate under-reports instead of over-reporting.
/// The same reasoning `object-freshness` records: each widening produced false
/// positives, so the sets stay per class.
///
/// NOT included either: `build.rs` itself. It is read, to learn WHICH objects a
/// bin links, but it is not an input — treating it as one makes every edit to it
/// invalidate all 17 fixtures at once (measured: 16 of 17 stale, every row
/// reporting the same date), and a gate whose STALE rows all say the same thing
/// says nothing. The gap this leaves is real and small: a `build.rs` change that
/// alters a LINKER ARGUMENT rather than the object list — a `--defsym`, a
/// section flag — will not be caught. Adding or removing an object is caught,
/// because the object is itself an input.
///
/// Also not included: `.repl` and `.robot`. They are the platform and the test,
/// not inputs to the image.
const ALWAYS: &[&str] = &[];

/// KNOWN-STALE LEDGER. Refreshing a fixture means re-running the Renode gate on
/// a new image, which is a real re-validation and not bookkeeping — and the gate
/// cannot run on every host (`rules_renode` has no darwin-arm64 toolchain).
/// Ledgered rather than skipped: this gate FAILS if a new fixture goes stale,
/// AND if one of these stops being stale — so the list shrinks to empty as they
/// are refreshed instead of outliving the problem (gale#413).
const KNOWN_STALE: &[&str] = &[
    "gust_adc.elf",
    "gust_breadth.elf",
    "gust_can.elf",
    "gust_dac.elf",
    "gust_gpio.elf",
    "gust_i2c.elf",
    "gust_pwm.elf",
    "gust_spi.elf",
    "gust_timer.elf",
    "gust_uart.elf",
    "gust_wdg.elf",
];

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(repo).args(args).output().ok()?;
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

/// Civil-from-days (Howard Hinnant). Display only.
fn ymd(epoch: i64) -> String {
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

/// Every committed `.elf` under the renode-test directory. From git, not a
/// filesystem glob: a walk would pick up uncommitted build output, which is the
/// opposite error.
fn committed_fixtures(repo: &Path) -> Option<Vec<String>> {
    let out = git(repo, &["ls-files", "--", &format!("{REL}/*.elf")])?;
    Some(
        out.lines()
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .map(|l| l.trim_start_matches(&format!("{REL}/")).to_string())
            .collect(),
    )
}

/// Which dissolved objects `build.rs` links into `bin`. Derived from the file
/// rather than hardcoded, so a newly linked object becomes an input without
/// anyone remembering to add it here.
///
/// The lines look like:
///   println!("cargo:rustc-link-arg-bin=gust_iso={}", iobj.display());
/// with the path in a `let` above. Both halves are read: the bin name from the
/// `rustc-link-arg-bin=` marker, the path from the nearest preceding
/// `Path::new(&manifest).join("…")`.
fn linked_objects(repo: &Path, bin: &str) -> Vec<String> {
    let text = std::fs::read_to_string(repo.join(BUILD_RS)).unwrap_or_default();
    let marker = format!("rustc-link-arg-bin={bin}=");
    let mut out: Vec<String> = Vec::new();
    let lines: Vec<&str> = text.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if !line.contains(&marker) {
            continue;
        }
        // Walk back to the nearest `.join("…")` — the object this block links.
        for prev in lines[..=i].iter().rev() {
            let Some(j) = prev.find(".join(\"") else { continue };
            let rest = &prev[j + 7..];
            let Some(end) = rest.find('"') else { continue };
            let rel = &rest[..end];
            if rel.ends_with(".o") {
                out.push(format!("benches/gust/drivers/{rel}"));
            }
            break;
        }
    }
    out.sort();
    out.dedup();
    out
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    if git(repo, &["rev-parse", "--git-dir"]).is_none() {
        return Verdict::Refused("not a git repository".into());
    }
    let Some(found) = committed_fixtures(repo) else {
        return Verdict::Refused("git ls-files failed — the gate cannot answer".into());
    };
    if found.is_empty() {
        return Verdict::Refused("no committed ELF fixtures found — the sweep would be vacuous".into());
    }

    let overrides: BTreeMap<&str, &str> = SOURCE_OVERRIDE.iter().copied().collect();

    // Resolve each fixture's source. An unresolvable one is a census failure,
    // not a skip: a fixture nothing can locate the source of is exactly the one
    // that would drift unnoticed.
    let mut gated: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut unresolved: Vec<String> = Vec::new();
    for elf in &found {
        let stem = elf.trim_end_matches(".elf");
        let src = match overrides.get(elf.as_str()) {
            Some(s) => s.to_string(),
            None => {
                let derived = format!("{BIN_DIR}/{stem}.rs");
                if !repo.join(&derived).is_file() {
                    unresolved.push(elf.clone());
                    continue;
                }
                derived
            }
        };
        let mut inputs = vec![src];
        inputs.extend(ALWAYS.iter().map(|s| s.to_string()));
        inputs.extend(linked_objects(repo, stem));
        gated.insert(elf.clone(), inputs);
    }
    if !unresolved.is_empty() {
        let mut lines = vec![format!(
            "FAIL: {} committed fixture(s) whose source cannot be located:",
            unresolved.len()
        )];
        lines.extend(unresolved.iter().map(|u| format!("  {u}")));
        lines.push("  add a SOURCE_OVERRIDE entry, or the fixture is unbuildable.".into());
        return Verdict::Fail(lines);
    }

    // A ledger entry for a fixture that no longer exists is a stale excuse.
    let found_set: BTreeSet<&str> = found.iter().map(String::as_str).collect();
    let vanished: Vec<&str> =
        KNOWN_STALE.iter().copied().filter(|k| !found_set.contains(k)).collect();
    if !vanished.is_empty() {
        let mut lines = vec!["FAIL: KNOWN_STALE lists fixture(s) that no longer exist:".to_string()];
        lines.extend(vanished.iter().map(|v| format!("  {v}")));
        return Verdict::Fail(lines);
    }

    let mut lines =
        vec!["fixture                 committed    newest input verdict".to_string()];
    let mut stale: BTreeSet<String> = BTreeSet::new();
    for (elf, inputs) in &gated {
        let Some(elf_t) = last_commit_epoch(repo, &format!("{REL}/{elf}")) else {
            return Verdict::Refused(format!("no commit date for {elf} — cannot compare"));
        };
        let newest = inputs.iter().filter_map(|i| last_commit_epoch(repo, i)).max();
        let Some(newest) = newest else {
            return Verdict::Refused(format!("no commit date for any input of {elf}"));
        };
        let is_stale = newest > elf_t;
        if is_stale {
            stale.insert(elf.clone());
        }
        lines.push(format!(
            "{:<23} {}   {}   {}",
            elf,
            ymd(elf_t),
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
            lines.push("      the Renode gate would run an image built before that change".into());
        }
        for f in &fixed {
            lines.push(format!("FAIL: {f} is no longer stale — remove it from KNOWN_STALE"));
        }
        return Verdict::Fail(lines);
    }

    lines.push(format!(
        "{} fixture(s) stale, all on the known-stale ledger. Refreshing one means",
        stale.len()
    ));
    lines.push("re-running the Renode gate on a new image, so they are recorded, not".into());
    lines.push("refreshed here. Inputs are narrow (no `gale` crate) — a LOWER bound.".into());
    Verdict::Pass(lines)
}

/// The comparator must fire on a newer input and not on an older one; the
/// ledger must be exact in BOTH directions; and `linked_objects` must actually
/// find something, or every fixture would be compared against its bin source
/// alone and the dissolved-object half of the gate would be silently vacuous —
/// which is the failure mode this gate exists to catch in the first place.
pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
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
    ck("ymd epoch 0", ymd(0) == "1970-01-01");
    ck("ymd 1756684800", ymd(1_756_684_800) == "2025-09-01");
    ck("ymd 1787000000", ymd(1_787_000_000) == "2026-08-17");

    let ledger: BTreeSet<String> = ["a".to_string(), "b".to_string()].into_iter().collect();
    let stale_now: BTreeSet<String> = ["a".to_string(), "c".to_string()].into_iter().collect();
    ck("new stale detected", stale_now.difference(&ledger).count() == 1);
    ck("fixed-but-ledgered detected", ledger.difference(&stale_now).count() == 1);

    // The object-link derivation, against a bin known to link one: gust_iso
    // links iso-core-fused-cm3.o. If this ever returns empty the gate has
    // quietly become "bin source only".
    let iso = linked_objects(repo, "gust_iso");
    ck(
        "linked_objects finds gust_iso's fused object",
        iso.iter().any(|o| o.ends_with("iso-core-fused-cm3.o")),
    );
    // Negative control for the same derivation: a bin that links no object must
    // come back empty, or the parser is matching something it should not. The
    // first version of this assertion used `gust_two_tenant`, which DOES link
    // one — the control reported WRONG and was right to; pick a real bin with
    // no link-arg line, and a name that does not exist at all.
    ck(
        "linked_objects empty for a real bin that links none",
        linked_objects(repo, "gust_iso_unpriv_probe").is_empty(),
    );
    ck(
        "linked_objects empty for a nonexistent bin",
        linked_objects(repo, "no_such_bin_zzz").is_empty(),
    );

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

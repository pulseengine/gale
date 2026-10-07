//! Every committed AADL model must parse with the pinned spar.
//!
//! The models under `safety/aadl/` carry headers naming the analyses they exist
//! to feed -- `spar parse`, `spar instance`, `spar analyze` (latency, resource
//! budgets, EMV2 fault propagation). None of those can run on a file that does
//! not parse, and **nothing in CI ran even the first one**: a grep of the
//! workflows and of this gate directory for `safety/aadl`, `spar parse` or
//! `spar analyze` returned nothing before this gate existed (gale#441).
//!
//! What that cost: both safety models are non-conformant AADL and have been for
//! an unknown length of time. `system implementation` declares `thread`
//! subcomponents directly, which AADL v2 permits only inside a `process`
//! implementation. An earlier spar accepted it; the pinned one correctly does
//! not. Because no gate existed, the day it started failing is not recoverable
//! from CI history -- which is the real defect. A model nobody parses is prose.
//!
//! The 7 target models under `benches/gust/targets/` DO parse and are the
//! source for generated constants and the linker map (REQ-TARGET-MODEL-001), so
//! the point of this gate is as much to keep them parsing as to surface the two
//! that do not.
//!
//! LEDGER, both directions, same shape as object-freshness and
//! fixture-freshness: a newly-unparseable model fails, AND a ledgered model
//! that starts parsing fails until it is removed from the ledger -- so the list
//! shrinks to empty when the models are fixed instead of outliving the problem.
//!
//! REFUSES rather than passes when spar is absent or when the sweep finds no
//! models at all: a green produced by having nothing to check is the failure
//! mode this repo keeps finding (gale#438).

use crate::verdict::Verdict;
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "aadl-parse";

/// Models known not to parse. Each entry is a debt with a reason, not an
/// exemption -- see gale#441. Fixing one means wrapping its threads in a
/// `process` and re-running the analyses its header promises, which is a change
/// of its own rather than a line in this list.
const KNOWN_UNPARSEABLE: &[&str] = &[
    // `system implementation Handoff_System.impl` declares `producer` and
    // `consumer` as `thread` subcomponents directly.
    "safety/aadl/semaphore.aadl",
    // Same shape.
    "safety/aadl/mutex.aadl",
];

fn git(repo: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git").arg("-C").arg(repo).args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Committed `.aadl`, from git rather than a filesystem walk: the claim is about
/// COMMITTED models, and a walk would also sweep scratch copies.
///
/// TWO pathspecs on purpose. git's `**/` requires at least one directory level,
/// so `**/*.aadl` alone would miss a model committed at the repository root --
/// the bug that left one object ungated for the life of `object-freshness`
/// (gale#438). There is no root-level model today; the point is that there
/// cannot be one that this gate fails to see.
fn committed_models(repo: &Path) -> Option<Vec<String>> {
    let out = git(repo, &["ls-files", "--", "*.aadl", "**/*.aadl"])?;
    let mut v: Vec<String> = out
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    v.sort();
    v.dedup();
    Some(v)
}

/// Which spar to run. `$SPAR` first, so CI and the bench scripts can point at
/// the pinned layer's binary explicitly, exactly as build-iso-core.sh does for
/// `$MELD`/`$SYNTH`.
fn spar_bin() -> String {
    std::env::var("SPAR").unwrap_or_else(|_| "spar".to_string())
}

/// Establish that the resolved binary is a spar that MEETS THE CLI CONVENTION,
/// not merely something spawnable called "spar".
///
/// This check exists because its absence immediately produced a wrong verdict.
/// The first version of this gate probed availability with
/// `Command::new("spar").arg("--version").output().is_err()`, which is true only
/// when the process cannot be SPAWNED — it says nothing about whether the
/// command succeeded. On this machine bare `spar` resolves to
/// `~/.cargo/bin/spar`, which answers `--version` with
/// `Unknown command: --version` and a non-zero status, and which ACCEPTS both
/// non-conformant safety models that the pinned spar 0.40.0 rejects. So the gate
/// swept 9 models, called all 9 ok, and was measuring with the wrong binary.
/// Only the ledger's fixed-but-ledgered direction caught it.
///
/// Per pulseengine-cli-conventions, `--version` prints `<binary> <semver>` and
/// exits 0. Anything else is refused by name rather than trusted.
fn spar_version(bin: &str) -> Result<String, String> {
    let out = Command::new(bin)
        .arg("--version")
        .output()
        .map_err(|e| format!("{bin}: cannot run ({e})"))?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    let first = text.lines().next().unwrap_or("").trim().to_string();
    if !out.status.success() || !first.starts_with("spar ") {
        return Err(format!(
            "`{bin}` does not answer `--version` as a spar should (got {first:?}, \
             exit {:?}). This is the stale-shadow trap: a binary earlier on PATH \
             than the pinned layer can ACCEPT models the pin rejects, so the gate \
             would report a verdict for the wrong tool. Set $SPAR to the pinned \
             binary (`varve which spar`) or put the shim directory first on PATH.",
            out.status.code()
        ));
    }
    Ok(first)
}

fn spar_parses(bin: &str, repo: &Path, rel: &str) -> Result<bool, String> {
    let out = Command::new(bin)
        .arg("parse")
        .arg(rel)
        .current_dir(repo)
        .output()
        .map_err(|e| format!("{bin}: {e}"))?;
    Ok(out.status.success())
}

pub fn run(repo: &Path, target: Option<&str>) -> Verdict {
    let bin = spar_bin();
    let ver = match spar_version(&bin) {
        Ok(v) => v,
        Err(e) => return Verdict::Refused(e),
    };

    // `--module` aims the gate at one file, so it can be pointed at a crafted
    // model instead of only at the set it already knows the verdict for. Without
    // that a control can only ever confirm the status quo (the reason
    // data-overlap and graph-env carry the same flag).
    if let Some(t) = target {
        return match spar_parses(&bin, repo, t) {
            Err(e) => Verdict::Refused(e),
            Ok(true) => Verdict::Pass(vec![format!("ok   {t}: spar parse accepted it")]),
            Ok(false) => Verdict::Fail(vec![format!("FAIL {t}: spar parse rejected it")]),
        };
    }

    let Some(models) = committed_models(repo) else {
        return Verdict::Refused("git ls-files failed — the gate cannot answer".into());
    };
    if models.is_empty() {
        return Verdict::Refused(
            "no committed .aadl models found — the sweep would be vacuous".into(),
        );
    }

    let mut lines = vec![format!("{:<44} verdict", "model")];
    let mut broken: Vec<String> = Vec::new();
    for m in &models {
        match spar_parses(&bin, repo, m) {
            Err(e) => return Verdict::Refused(e),
            Ok(true) => lines.push(format!("  {m:<42} ok")),
            Ok(false) => {
                lines.push(format!("  {m:<42} PARSE FAILS"));
                broken.push(m.clone());
            }
        }
    }

    let ledger: Vec<String> = KNOWN_UNPARSEABLE.iter().map(|s| s.to_string()).collect();
    let newly: Vec<&String> = broken.iter().filter(|b| !ledger.contains(b)).collect();
    let fixed: Vec<&String> = ledger.iter().filter(|l| !broken.contains(l)).collect();

    // A ledger entry naming a model that no longer exists is stale bookkeeping
    // and would quietly shrink the gate's reach.
    let vanished: Vec<&String> = ledger.iter().filter(|l| !models.contains(l)).collect();

    let mut fail = Vec::new();
    for n in &newly {
        fail.push(format!("FAIL: {n} no longer parses and is not on the ledger"));
    }
    for f in &fixed {
        if models.contains(f) {
            fail.push(format!(
                "FAIL: {f} parses now — remove it from KNOWN_UNPARSEABLE so the ledger \
                 shrinks instead of outliving the problem"
            ));
        }
    }
    for v in &vanished {
        fail.push(format!("FAIL: ledger names {v}, which is not a committed model"));
    }

    if !fail.is_empty() {
        lines.extend(fail);
        return Verdict::Fail(lines);
    }
    lines.push(format!(
        "{ver}; {} committed model(s); {} parse, {} on the known-unparseable ledger (gale#441). \
         A PARSE FAILS row is always real; an `ok` row says the file parses, not that \
         its analyses were run.",
        models.len(),
        models.len() - broken.len(),
        broken.len()
    ));
    Verdict::Pass(lines)
}

/// The gate must be OBSERVED to reject a bad model and accept a good one. Either
/// half misbehaving means it cannot distinguish, and a sweep that is merely
/// green proves nothing about either.
pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let bin = spar_bin();
    let mut lines = Vec::new();
    match spar_version(&bin) {
        Ok(v) => lines.push(format!("resolved {bin} -> {v}")),
        Err(e) => return Verdict::Refused(e),
    }
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };

    // The matched pair, against real committed models: one that must parse and
    // one that must not. Both halves, because a gate that flags everything and a
    // gate that flags nothing are both green on a one-sided test.
    let good = "benches/gust/targets/stm32g474.aadl";
    let bad = "safety/aadl/semaphore.aadl";
    ck(
        "a conforming target model is accepted",
        matches!(spar_parses(&bin, repo, good), Ok(true)),
    );
    ck(
        "a thread-in-system model is rejected",
        matches!(spar_parses(&bin, repo, bad), Ok(false)),
    );

    // The enumeration, against the real tree -- the half that was missing from
    // object-freshness's self-test and left it green on 20 of 21 objects.
    if let Some(found) = committed_models(repo) {
        ck("enumeration is not vacuous", !found.is_empty());
        ck(
            "enumeration finds the target models",
            found.iter().any(|f| f == good),
        );
        ck(
            "enumeration finds the safety models",
            found.iter().any(|f| f == bad),
        );
        // Negative control for the three above: a sweep returning the whole tree
        // would satisfy them while gating nothing.
        ck(
            "enumeration is .aadl only",
            found.iter().all(|f| f.ends_with(".aadl")),
        );
    } else {
        ck("committed_models could not run", false);
    }

    // Ledger arithmetic, both directions.
    let ledger: Vec<String> = vec!["a".into(), "b".into()];
    let broken_now: Vec<String> = vec!["a".into(), "c".into()];
    ck(
        "newly-unparseable detected",
        broken_now.iter().filter(|b| !ledger.contains(b)).count() == 1,
    );
    ck(
        "fixed-but-ledgered detected",
        ledger.iter().filter(|l| !broken_now.contains(l)).count() == 1,
    );

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

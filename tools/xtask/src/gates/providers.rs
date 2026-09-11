//! gust:os syscall-seam drift gate (gale#214).
//!
//! The gust:os providers implement WIT Guest traits generated from
//! `drivers/wit-os/gust-os.wit` via `wit_bindgen::generate!`. If the WIT gains a
//! method but a Guest impl is not updated, the provider crate fails to compile
//! (E0046 "not all trait items implemented") -- but NOTHING in CI built these
//! crates, so the break shipped GREEN and only surfaced when the dissolve path
//! was exercised by hand (#202 added time.resolution() to the WIT without the
//! time-provider impl, silently breaking gust_os_tl/ts; #213 fixed it).
//!
//! Cargo-only: no meld/loom/synth/qemu. The compile IS the oracle, because
//! wit-bindgen turns the WIT into the Guest trait.
//!
//! Ported from check-providers.sh (gale#368). The shell was sound -- the verdict
//! rode `cargo build`'s real exit status through an `if`, not through a pipe, and
//! discovery was a glob rather than a hand-written list (a hand-written list had
//! silently narrowed before: app-timer was added and the gate kept reporting
//! "all 8 in sync" while never building it, the same shape as the Lean gate's
//! enumerated target list in gale#292). What the port adds is Refused: a missing
//! cargo, or zero discovered crates, is not a passing gate.

use crate::verdict::Verdict;
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "providers";

const REL: &str = "benches/gust/drivers";

/// DISCOVERED, not enumerated: every `app-*` and `*-provider` directory holding
/// a Cargo.toml. Globbing means a new app/provider crate is gated by
/// construction rather than by somebody remembering to add it.
fn discover(dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let Ok(rd) = std::fs::read_dir(dir) else { return out };
    for e in rd.filter_map(|e| e.ok()) {
        let p = e.path();
        if !p.is_dir() {
            continue;
        }
        let Some(name) = p.file_name().and_then(|n| n.to_str()) else { continue };
        if !(name.starts_with("app-") || name.ends_with("-provider")) {
            continue;
        }
        if p.join("Cargo.toml").is_file() {
            out.push(name.to_string());
        }
    }
    out.sort();
    out
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let here = repo.join(REL);
    if !here.is_dir() {
        return Verdict::Refused(format!("no drivers directory at {}", here.display()));
    }
    if Command::new("cargo").arg("--version").output().is_err() {
        return Verdict::Refused("cargo not on PATH".into());
    }

    let crates = discover(&here);
    // The shell guarded this too: zero discovered crates is a vacuous pass, not
    // a clean one.
    if crates.is_empty() {
        return Verdict::Refused("no app-*/-provider crates discovered".into());
    }

    let mut lines = Vec::new();
    let mut failed: Vec<String> = Vec::new();
    for c in &crates {
        let out = Command::new("cargo")
            .current_dir(here.join(c))
            .args(["build", "--release", "--target", "wasm32-unknown-unknown"])
            .output();
        match out {
            Ok(o) if o.status.success() => lines.push(format!("{c:<16} ok")),
            Ok(o) => {
                lines.push(format!("{c:<16} FAILED"));
                let err = String::from_utf8_lossy(&o.stderr);
                for l in err.lines().rev().take(12).collect::<Vec<_>>().into_iter().rev() {
                    lines.push(format!("    {l}"));
                }
                failed.push(c.clone());
            }
            Err(e) => return Verdict::Refused(format!("cargo not runnable for {c}: {e}")),
        }
    }

    if !failed.is_empty() {
        lines.push(format!("gust:os provider drift gate FAILED: {}", failed.join(" ")));
        lines.push("A WIT change in drivers/wit-os/gust-os.wit likely outran a Guest impl".into());
        lines.push("(E0046). Update the provider's impl to match the WIT interface.".into());
        return Verdict::Fail(lines);
    }
    lines.push(format!("all {} provider/app crate(s) compile against the current WIT", crates.len()));
    Verdict::Pass(lines)
}

/// Discovery is the thing that can go quietly wrong here -- an enumerated list
/// narrows silently, and a glob that matches nothing passes vacuously. Both
/// directions are pinned.
pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };

    let here = repo.join(REL);
    let found = discover(&here);
    ck("discovery finds crates", !found.is_empty());
    ck(
        "every discovered crate has a Cargo.toml",
        found.iter().all(|c| here.join(c).join("Cargo.toml").is_file()),
    );
    ck(
        "only app-* / *-provider are discovered",
        found.iter().all(|c| c.starts_with("app-") || c.ends_with("-provider")),
    );
    // A directory without a Cargo.toml must NOT be discovered -- otherwise the
    // gate would try to build it and fail for the wrong reason.
    let scratch = std::env::temp_dir().join(format!("gale-prov-st-{}", std::process::id()));
    let _ = std::fs::create_dir_all(scratch.join("app-not-a-crate"));
    ck("dir without Cargo.toml is skipped", discover(&scratch).is_empty());
    let _ = std::fs::remove_dir_all(&scratch);
    // An empty tree discovers nothing, which run() turns into Refused.
    let empty = std::env::temp_dir().join(format!("gale-prov-empty-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&empty);
    ck("empty tree discovers nothing", discover(&empty).is_empty());
    let _ = std::fs::remove_dir_all(&empty);

    lines.push(format!("discovered: {}", found.join(" ")));
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

//! Every generated per-target WIT world must RESOLVE against the real gust:hal
//! seam -- its imports must name interfaces that actually exist in
//! drivers/wit/gust-hal.wit. Catches a generated world drifting from the seam.
//!
//! Ported from check-wit-resolve.sh (gale#368). The shell version was correctly
//! written -- the verdict rode `wasm-tools`' real exit status through an `if`,
//! not through a pipe -- but it had NO negative control: nothing established
//! that it could go red. A gate nobody has watched fail is a gate nobody knows
//! works. `--self-test` supplies the matched pair.

use crate::verdict::Verdict;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const NAME: &str = "wit-resolve";

/// A scratch directory that removes itself. The shell version leaked one on
/// every early `exit 1` path -- it only ran `rm -rf` on the success branch and
/// on one of the two failure branches.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> std::io::Result<Self> {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "gale-xtask-{tag}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        fs::create_dir_all(p.join("deps/hal"))?;
        Ok(Scratch(p))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// Does one world resolve against one hal? `Ok(true/false)` is a verdict;
/// `Err` means the check could not be performed at all.
fn resolves(world: &Path, hal: &Path, tag: &str) -> Result<bool, String> {
    let s = Scratch::new(tag).map_err(|e| format!("scratch dir: {e}"))?;
    fs::copy(world, s.path().join("world.wit")).map_err(|e| format!("copy world: {e}"))?;
    fs::copy(hal, s.path().join("deps/hal/gust-hal.wit"))
        .map_err(|e| format!("copy hal: {e}"))?;

    let out = Command::new("wasm-tools")
        .args(["component", "wit"])
        .arg(s.path())
        .output()
        .map_err(|e| format!("wasm-tools not runnable: {e}"))?;
    Ok(out.status.success())
}

pub fn run(repo: &Path, _target: Option<&str>) -> Verdict {
    let hal = repo.join("benches/gust/drivers/wit/gust-hal.wit");
    let gen = repo.join("benches/gust/targets/generated");

    if !hal.is_file() {
        return Verdict::Refused(format!("gust:hal seam not found at {}", hal.display()));
    }
    // Missing tool is REFUSED, never a pass. This is the collapse the shell
    // idiom `wasm-tools ... 2>/dev/null || true` produces silently.
    if Command::new("wasm-tools").arg("--version").output().is_err() {
        return Verdict::Refused("wasm-tools not on PATH".into());
    }

    let mut worlds: Vec<PathBuf> = match fs::read_dir(&gen) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.starts_with("world-") && n.ends_with(".wit"))
            })
            .collect(),
        Err(e) => return Verdict::Refused(format!("{}: {e}", gen.display())),
    };
    worlds.sort();

    // An empty set is REFUSED, not a vacuous pass -- the same reason
    // check-providers.sh guards its discovery with `-gt 0`.
    if worlds.is_empty() {
        return Verdict::Refused(format!("no generated worlds in {}", gen.display()));
    }

    let mut lines = Vec::new();
    let mut failed = false;
    for w in &worlds {
        let base = w.file_name().unwrap_or_default().to_string_lossy().to_string();
        match resolves(w, &hal, "resolve") {
            Ok(true) => lines.push(format!("{base}: resolves against gust:hal")),
            Ok(false) => {
                lines.push(format!("{base}: does NOT resolve against gust:hal"));
                failed = true;
            }
            Err(e) => return Verdict::Refused(format!("{base}: {e}")),
        }
    }
    if failed {
        Verdict::Fail(lines)
    } else {
        lines.push(format!("{} world(s) checked", worlds.len()));
        Verdict::Pass(lines)
    }
}

/// The negative control the shell version never had: a matched pair. A world
/// naming an interface the seam does not export MUST be rejected, and a world
/// naming only real ones MUST be accepted. If either half misbehaves the gate
/// cannot distinguish, and reports so rather than claiming a clean run.
pub fn self_test(repo: &Path, _target: Option<&str>) -> Verdict {
    let hal = repo.join("benches/gust/drivers/wit/gust-hal.wit");
    if !hal.is_file() {
        return Verdict::Refused(format!("gust:hal seam not found at {}", hal.display()));
    }
    if Command::new("wasm-tools").arg("--version").output().is_err() {
        return Verdict::Refused("wasm-tools not on PATH".into());
    }

    let s = match Scratch::new("selftest") {
        Ok(s) => s,
        Err(e) => return Verdict::Refused(format!("scratch dir: {e}")),
    };

    let bad = s.path().join("world-planted-bad.wit");
    let good = s.path().join("world-planted-good.wit");
    // Spelled exactly like a generated world -- versioned package, versioned
    // import. An unversioned import is rejected by wasm-tools for a reason that
    // has nothing to do with the property under test, which would make the
    // "good" half a false negative and the control worthless.
    let bad_src = "package gale:selftest@0.1.0;\n\nworld planted {\n  \
                   import gust:hal/this-interface-does-not-exist@0.1.0;\n}\n";
    let good_src = "package gale:selftest@0.1.0;\n\nworld planted {\n  \
                    import gust:hal/mmio@0.1.0;\n}\n";
    if let Err(e) = fs::write(&bad, bad_src).and_then(|_| fs::write(&good, good_src)) {
        return Verdict::Refused(format!("planting: {e}"));
    }

    let mut lines = Vec::new();
    let mut broken = false;

    match resolves(&bad, &hal, "st-bad") {
        Ok(false) => lines.push("planted bad world REJECTED (control bites)".into()),
        Ok(true) => {
            lines.push("planted bad world was ACCEPTED — the gate cannot distinguish".into());
            broken = true;
        }
        Err(e) => return Verdict::Refused(format!("bad-world control: {e}")),
    }
    match resolves(&good, &hal, "st-good") {
        Ok(true) => lines.push("planted good world ACCEPTED (no false positive)".into()),
        Ok(false) => {
            lines.push("planted good world was REJECTED — the gate fails clean input".into());
            broken = true;
        }
        Err(e) => return Verdict::Refused(format!("good-world control: {e}")),
    }

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

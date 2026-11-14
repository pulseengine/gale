//! Every generated per-target WIT world, and every gust:bus package (wit/bus,
//! gale#511), must RESOLVE against the real gust:hal
//! seam; every device-driver package (wit/sensor) must resolve against gust:hal
//! plus gust:bus. The seam -- its imports must name interfaces that actually exist in
//! wit/hal/gust-hal.wit. Catches a generated world drifting from the seam.
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
    resolves_with(world, &[("hal", hal)], tag)
}

/// Like `resolves`, with each `(name, file)` copied to `deps/<name>/`.
fn resolves_with(world: &Path, deps: &[(&str, &Path)], tag: &str) -> Result<bool, String> {
    let s = Scratch::new(tag).map_err(|e| format!("scratch dir: {e}"))?;
    fs::copy(world, s.path().join("world.wit")).map_err(|e| format!("copy world: {e}"))?;
    for (name, file) in deps {
        let dir = s.path().join("deps").join(name);
        fs::create_dir_all(&dir).map_err(|e| format!("deps/{name}: {e}"))?;
        let base = file.file_name().ok_or_else(|| format!("deps/{name}: no file name"))?;
        fs::copy(file, dir.join(base)).map_err(|e| format!("copy {name}: {e}"))?;
    }

    let out = Command::new("wasm-tools")
        .args(["component", "wit"])
        .arg(s.path())
        .output()
        .map_err(|e| format!("wasm-tools not runnable: {e}"))?;
    Ok(out.status.success())
}

pub fn run(repo: &Path, _target: Option<&str>) -> Verdict {
    let hal = repo.join("wit/hal/gust-hal.wit");
    let gen = repo.join("targets/generated");

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
    // gust:bus (gale#511) imports gust:hal/mmio@0.1.0 in its driver world, so it
    // must resolve against the same seam. Swept by directory, not by a name list,
    // and an empty sweep is REFUSED like the generated worlds above.
    let bus = repo.join("wit/bus");
    let mut bus_pkgs: Vec<PathBuf> = match fs::read_dir(&bus) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "wit"))
            .collect(),
        Err(e) => return Verdict::Refused(format!("{}: {e}", bus.display())),
    };
    bus_pkgs.sort();
    if bus_pkgs.is_empty() {
        return Verdict::Refused(format!("no .wit package in {}", bus.display()));
    }
    for w in &bus_pkgs {
        let base = w.file_name().unwrap_or_default().to_string_lossy().to_string();
        match resolves(w, &hal, "resolve-bus") {
            Ok(true) => lines.push(format!("wit/bus/{base}: resolves against gust:hal")),
            Ok(false) => {
                lines.push(format!("wit/bus/{base}: does NOT resolve against gust:hal"));
                failed = true;
            }
            Err(e) => return Verdict::Refused(format!("wit/bus/{base}: {e}")),
        }
    }

    // Device-driver packages (gale#511: gust:sen66) import gust:bus, which imports
    // gust:hal, so they resolve against both. Swept by directory; empty is REFUSED.
    let bus_wit = bus.join("gust-bus.wit");
    if !bus_wit.is_file() {
        return Verdict::Refused(format!("gust:bus not found at {}", bus_wit.display()));
    }
    let sensor = repo.join("wit/sensor");
    let mut dev_pkgs: Vec<PathBuf> = match fs::read_dir(&sensor) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "wit"))
            .collect(),
        Err(e) => return Verdict::Refused(format!("{}: {e}", sensor.display())),
    };
    dev_pkgs.sort();
    if dev_pkgs.is_empty() {
        return Verdict::Refused(format!("no .wit package in {}", sensor.display()));
    }
    for w in &dev_pkgs {
        let base = w.file_name().unwrap_or_default().to_string_lossy().to_string();
        match resolves_with(w, &[("hal", &hal), ("bus", &bus_wit)], "resolve-dev") {
            Ok(true) => lines.push(format!("wit/sensor/{base}: resolves against gust:hal + gust:bus")),
            Ok(false) => {
                lines.push(format!("wit/sensor/{base}: does NOT resolve against gust:hal + gust:bus"));
                failed = true;
            }
            Err(e) => return Verdict::Refused(format!("wit/sensor/{base}: {e}")),
        }
    }

    if failed {
        Verdict::Fail(lines)
    } else {
        lines.push(format!(
            "{} world(s), {} gust:bus package(s) and {} device package(s) checked",
            worlds.len(),
            bus_pkgs.len(),
            dev_pkgs.len()
        ));
        Verdict::Pass(lines)
    }
}

/// The negative control the shell version never had: a matched pair. A world
/// naming an interface the seam does not export MUST be rejected, and a world
/// naming only real ones MUST be accepted. If either half misbehaves the gate
/// cannot distinguish, and reports so rather than claiming a clean run.
pub fn self_test(repo: &Path, _target: Option<&str>) -> Verdict {
    let hal = repo.join("wit/hal/gust-hal.wit");
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

    // The same matched pair for the device-package path (deps hal + bus): a world
    // naming a gust:bus interface that does not exist MUST be rejected.
    let bus_wit = repo.join("wit/bus/gust-bus.wit");
    if !bus_wit.is_file() {
        return Verdict::Refused(format!("gust:bus not found at {}", bus_wit.display()));
    }
    let dbad = s.path().join("dev-planted-bad.wit");
    let dgood = s.path().join("dev-planted-good.wit");
    let dbad_src = "package gale:selftest@0.1.0;\n\nworld planted {\n  \
                    import gust:bus/this-interface-does-not-exist@0.1.0;\n}\n";
    let dgood_src = "package gale:selftest@0.1.0;\n\nworld planted {\n  \
                     import gust:bus/i2c-master@0.1.0;\n}\n";
    if let Err(e) = fs::write(&dbad, dbad_src).and_then(|_| fs::write(&dgood, dgood_src)) {
        return Verdict::Refused(format!("planting: {e}"));
    }
    let deps = [("hal", hal.as_path()), ("bus", bus_wit.as_path())];
    match resolves_with(&dbad, &deps, "st-dbad") {
        Ok(false) => lines.push("planted bad device world REJECTED (control bites)".into()),
        Ok(true) => {
            lines.push("planted bad device world was ACCEPTED — the gate cannot distinguish".into());
            broken = true;
        }
        Err(e) => return Verdict::Refused(format!("bad-device control: {e}")),
    }
    match resolves_with(&dgood, &deps, "st-dgood") {
        Ok(true) => lines.push("planted good device world ACCEPTED (no false positive)".into()),
        Ok(false) => {
            lines.push("planted good device world was REJECTED — the gate fails clean input".into());
            broken = true;
        }
        Err(e) => return Verdict::Refused(format!("good-device control: {e}")),
    }

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

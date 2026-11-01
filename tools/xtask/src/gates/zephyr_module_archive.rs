//! The Zephyr module archive is reproducible, self-contained, and named.
//!
//! gale#463: varve layers carry gale as a verified Zephyr module by consuming
//! `gale-zephyr-module-<version>.tar.gz` from the cosign-signed release. Three
//! properties make that archive worth signing, and each is checked here rather
//! than assumed:
//!
//!   * REPRODUCIBLE -- two builds from the same commit are byte-identical. A
//!     signature over an archive nobody can rebuild bit-for-bit is a signature
//!     over an accident of one runner.
//!   * SELF-CONTAINED -- the extracted archive builds the module's crate on its
//!     own. "Exactly the module tree" is a claim about a transitive closure, and
//!     the first draft of the path list was wrong: the root Cargo.toml declares
//!     `[[bench]] sem_bench`, and cargo refuses to parse the manifest without
//!     that file. Only building from the extraction finds that class.
//!   * NAMED -- `zephyr/module.yml` carries `name: gale`, so a consumer's module
//!     name-collision check does not depend on what directory it unpacked into.
//!
//! The archive itself is produced by scripts/zephyr-module-archive.sh, the one
//! place the path list lives; this gate only runs it and judges the result.

use crate::verdict::Verdict;
use std::path::{Path, PathBuf};
use std::process::Command;

pub const NAME: &str = "zephyr-module-archive";

const SCRIPT: &str = "scripts/zephyr-module-archive.sh";
const VERSION: &str = "check";

struct Scratch(PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Self> {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "gale-xtask-zmod-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&p)?;
        Ok(Scratch(p))
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn run_cmd(cmd: &mut Command) -> Result<String, String> {
    let out = cmd.output().map_err(|e| format!("{cmd:?}: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "{cmd:?} exited {}: {}",
            out.status,
            String::from_utf8_lossy(&out.stderr).trim().chars().take(600).collect::<String>()
        ));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Build the archive into `dir` via the script; return its path.
fn build(repo: &Path, dir: &Path) -> Result<PathBuf, String> {
    let printed = run_cmd(
        Command::new("bash")
            .arg(repo.join(SCRIPT))
            .arg(VERSION)
            .arg(dir)
            .current_dir(repo),
    )?;
    let p = PathBuf::from(printed.lines().last().unwrap_or(""));
    if !p.is_file() {
        return Err(format!("script reported {} but no such file", p.display()));
    }
    Ok(p)
}

/// Does `module.yml` text declare the module name `gale` at top level?
fn names_gale(module_yml: &str) -> bool {
    module_yml.lines().any(|l| {
        let t = l.trim_end();
        // top level only: an indented `name:` belongs to a nested mapping
        !t.starts_with(' ') && t.strip_prefix("name:").map(|v| v.trim().trim_matches('"')) == Some("gale")
    })
}

/// Extract `archive` into `into` and build the module crate from it, with a
/// target dir inside the scratch so no cached artifact from the real checkout
/// can stand in for a missing file.
fn builds_standalone(archive: &Path, into: &Path) -> Result<(), String> {
    run_cmd(Command::new("tar").arg("xzf").arg(archive).arg("-C").arg(into))?;
    let root = into.join(format!("gale-zephyr-module-{VERSION}"));
    run_cmd(
        Command::new("cargo")
            .args(["check", "--quiet", "--manifest-path", "ffi/Cargo.toml", "--target-dir"])
            .arg(into.join("target"))
            .current_dir(&root),
    )
    .map(|_| ())
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    if !repo.join(SCRIPT).is_file() {
        return Verdict::Refused(format!("{SCRIPT} is missing -- nothing produces the archive"));
    }
    let s = match Scratch::new() {
        Ok(s) => s,
        Err(e) => return Verdict::Refused(format!("scratch dir: {e}")),
    };
    let (a_dir, b_dir, x_dir) = (s.path().join("a"), s.path().join("b"), s.path().join("x"));
    for d in [&a_dir, &b_dir, &x_dir] {
        if let Err(e) = std::fs::create_dir_all(d) {
            return Verdict::Refused(format!("scratch dir: {e}"));
        }
    }
    let a = match build(repo, &a_dir) {
        Ok(p) => p,
        Err(e) => return Verdict::Refused(format!("first build: {e}")),
    };
    let b = match build(repo, &b_dir) {
        Ok(p) => p,
        Err(e) => return Verdict::Refused(format!("second build: {e}")),
    };
    let (Ok(ab), Ok(bb)) = (std::fs::read(&a), std::fs::read(&b)) else {
        return Verdict::Refused("cannot read the built archives".into());
    };

    let mut lines = vec![format!("archive: {} bytes", ab.len())];
    let mut fail = Vec::new();
    if ab != bb {
        fail.push("NOT REPRODUCIBLE: two builds from the same commit differ".to_string());
    } else {
        lines.push("reproducible: two builds byte-identical".into());
    }
    match std::fs::read_to_string(repo.join("zephyr/module.yml")) {
        Ok(t) if names_gale(&t) => lines.push("named: zephyr/module.yml declares `name: gale`".into()),
        Ok(_) => fail.push("zephyr/module.yml has no top-level `name: gale`".into()),
        Err(e) => return Verdict::Refused(format!("zephyr/module.yml: {e}")),
    }
    match builds_standalone(&a, &x_dir) {
        Ok(()) => lines.push("self-contained: ffi builds from the extracted archive alone".into()),
        Err(e) => fail.push(format!("NOT SELF-CONTAINED: the extracted archive does not build: {e}")),
    }

    if fail.is_empty() {
        Verdict::Pass(lines)
    } else {
        let mut out = vec![format!("FAIL: {} problem(s):", fail.len())];
        out.extend(fail.into_iter().map(|f| format!("  {f}")));
        out.extend(lines);
        Verdict::Fail(out)
    }
}

/// Each case is a way the gate could pass while wrong. The expensive one is the
/// real negative control: an archive MISSING a file the build needs must fail
/// the standalone build -- otherwise "self-contained: ok" could be produced by a
/// build that silently read files from somewhere else.
pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };

    ck("top-level `name: gale` is recognised", names_gale("name: gale\nbuild:\n  cmake: zephyr\n"));
    ck("a quoted name is recognised", names_gale("name: \"gale\"\n"));
    ck("a missing name is rejected", !names_gale("build:\n  cmake: zephyr\n"));
    ck("a nested name is not the module's", !names_gale("build:\n  name: gale\n"));
    ck("another module's name is rejected", !names_gale("name: galeX\n"));

    // Negative control on the standalone build, against the real tree: an
    // archive without benches/sem_bench.rs (the file the first path list
    // forgot) must NOT build.
    match Scratch::new() {
        Ok(s) => {
            let arc = s.path().join("short.tar.gz");
            let x = s.path().join("x");
            let made = std::fs::create_dir_all(&x).is_ok()
                && Command::new("bash")
                    .arg("-c")
                    .arg(format!(
                        "set -o pipefail; git archive --format=tar --prefix=gale-zephyr-module-{VERSION}/ HEAD -- \
                         zephyr ffi/Cargo.toml ffi/Cargo.lock ffi/include ffi/src Cargo.toml Cargo.lock plain \
                         rust-toolchain.toml LICENSE | gzip -n > '{}'",
                        arc.display()
                    ))
                    .current_dir(repo)
                    .status()
                    .map(|st| st.success())
                    .unwrap_or(false);
            ck("negative control archive could be built", made);
            if made {
                ck(
                    "an archive missing a build input does NOT build standalone",
                    builds_standalone(&arc, &x).is_err(),
                );
            }
        }
        Err(_) => ck("scratch dir for the negative control", false),
    }

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

//! Planted-input oracle for the object-freshness and binary-ledger gates (gale#506).
//!
//! Each test clones the repository (with the working tree's uncommitted diff
//! applied), plants ONE bad input in the clone's working tree, and runs the
//! built `xtask` binary against it. Nothing is committed: both gates read
//! content, so a working-tree plant is what CI would see after a commit.
//!
//! On the date-comparing gate these went the wrong way: 13 bytes of garbage as
//! `wdg-thin-cm3.o` passed both gates, a source edit passed because the object
//! was already on the known-stale ledger, and there was no expiry to pass.

use std::path::{Path, PathBuf};
use std::process::Command;

const WDG_OBJ: &str = "drivers/wdg-thin/wdg-thin-cm3.o";
const WDG_SRC: &str = "drivers/wdg-thin/src/lib.rs";

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").canonicalize().expect("repo root")
}

fn git(dir: &Path, args: &[&str]) -> std::process::Output {
    let out = Command::new("git").arg("-C").arg(dir).args(args).output().expect("git runs");
    assert!(out.status.success(), "git {args:?} failed: {}", String::from_utf8_lossy(&out.stderr));
    out
}

/// A clone of the repo at HEAD plus the working tree's tracked changes.
fn clone(name: &str) -> PathBuf {
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("freshness-oracle-{name}"));
    if dir.exists() {
        std::fs::remove_dir_all(&dir).expect("clear old clone");
    }
    let src = repo();
    let out = Command::new("git")
        .args(["clone", "-q", "--shared"])
        .arg(&src)
        .arg(&dir)
        .output()
        .expect("git clone runs");
    assert!(out.status.success(), "clone failed: {}", String::from_utf8_lossy(&out.stderr));
    let diff = git(&src, &["diff", "HEAD", "--binary"]).stdout;
    if !diff.is_empty() {
        let patch = dir.join(".oracle.patch");
        std::fs::write(&patch, &diff).expect("write patch");
        git(&dir, &["apply", "--whitespace=nowarn", ".oracle.patch"]);
        std::fs::remove_file(&patch).expect("remove patch");
    }
    dir
}

fn gate(dir: &Path, name: &str) -> (i32, String) {
    let out = Command::new(env!("CARGO_BIN_EXE_xtask"))
        .args(["check", name])
        .current_dir(dir)
        .output()
        .expect("xtask runs");
    let text = format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr));
    (out.status.code().unwrap_or(-1), text)
}

/// Rewrite the lines of ONE `[[binary]]` entry of binaries.toml.
fn edit_entry(dir: &Path, path: &str, f: impl Fn(&str) -> Option<String>) {
    let p = dir.join("binaries.toml");
    let text = std::fs::read_to_string(&p).expect("binaries.toml");
    let mut inside = false;
    let mut out = Vec::new();
    for line in text.lines() {
        if line.trim() == "[[binary]]" {
            inside = false;
        }
        if line.trim() == format!("path = \"{path}\"") {
            inside = true;
        }
        if inside {
            if let Some(l) = f(line) {
                out.push(l);
            }
        } else {
            out.push(line.to_string());
        }
    }
    std::fs::write(&p, out.join("\n") + "\n").expect("write binaries.toml");
}

fn plant_garbage(dir: &Path) {
    std::fs::write(dir.join(WDG_OBJ), b"13 bytes junk").expect("plant garbage");
}

fn assert_refuses(dir: &Path, name: &str, names: &str) {
    let (rc, text) = gate(dir, name);
    assert_eq!(rc, 3, "{name} must REFUSE (exit 3), got {rc}:\n{text}");
    assert!(text.contains(names), "{name} refused, but without naming `{names}`:\n{text}");
}

fn assert_fails(dir: &Path, name: &str, why: &str) {
    let (rc, text) = gate(dir, name);
    assert_eq!(rc, 1, "{name} must FAIL (exit 1), got {rc}:\n{text}");
    assert!(
        text.lines().any(|l| l.contains("wdg-thin-cm3.o") && l.contains(why)),
        "{name} failed, but not for `{why}` on wdg-thin-cm3.o:\n{text}"
    );
}

/// (d) The control: the unchanged tree passes both gates. Without it, every
/// test below could pass on a gate that fails everything.
#[test]
fn control_unchanged_tree_passes() {
    let dir = clone("control");
    for g in ["object-freshness", "binary-ledger"] {
        let (rc, text) = gate(&dir, g);
        assert_eq!(rc, 0, "{g} must pass on the unchanged tree:\n{text}");
    }
}

/// (a) Garbage bytes under a gated name, with the real ledger.
#[test]
fn garbage_object_fails_with_real_ledger() {
    let dir = clone("garbage-real");
    plant_garbage(&dir);
    assert_fails(&dir, "object-freshness", "bytes hash to");
    assert_fails(&dir, "binary-ledger", "sha256");
}

/// (a) Garbage bytes with the object's KNOWN-STALE ledger lines removed.
#[test]
fn garbage_object_fails_with_emptied_ledger() {
    let dir = clone("garbage-empty");
    plant_garbage(&dir);
    edit_entry(&dir, WDG_OBJ, |l| (!l.starts_with("stale_")).then(|| l.to_string()));
    assert_fails(&dir, "object-freshness", "bytes hash to");
    assert_fails(&dir, "binary-ledger", "sha256");
}

/// (b) A one-line source edit without a rebuild. The object is on the
/// known-stale ledger, which must not excuse further drift.
#[test]
fn source_edit_without_rebuild_fails() {
    let dir = clone("source-edit");
    let p = dir.join(WDG_SRC);
    let mut src = std::fs::read_to_string(&p).expect("wdg-thin source");
    src.push_str("// one-line edit, no rebuild\n");
    std::fs::write(&p, src).expect("edit source");
    assert_fails(&dir, "object-freshness", "inputs changed");
}

/// (c) A known-stale entry whose expiry has passed.
#[test]
fn expired_ledger_entry_fails() {
    let dir = clone("expired");
    edit_entry(&dir, WDG_OBJ, |l| {
        Some(if l.starts_with("stale_expires") { "stale_expires = \"2020-01-01\"".to_string() } else { l.to_string() })
    });
    assert_fails(&dir, "object-freshness", "EXPIRED");
}

/// An input with no commit date must REFUSE, not be skipped. wdg-thin is made a
/// FRESH claim (its stale lines removed), so the date cross-check runs, and an
/// untracked source file is added to its input dir. Skipping undated inputs
/// (a `filter_map`) let the newest date come from the rest.
#[test]
fn undated_input_of_a_fresh_claim_refuses() {
    let dir = clone("undated-input");
    edit_entry(&dir, WDG_OBJ, |l| (!l.starts_with("stale_")).then(|| l.to_string()));
    std::fs::write(dir.join("drivers/wdg-thin/src/planted.rs"), "// untracked\n").expect("plant file");
    assert_refuses(&dir, "object-freshness", "drivers/wdg-thin/src/planted.rs");
}

/// A declared input that resolves to no file must REFUSE, naming it.
#[test]
fn input_that_does_not_exist_refuses() {
    let dir = clone("missing-input");
    let p = dir.join("drivers/build-os-ts.sh");
    let mut sh = std::fs::read_to_string(&p).expect("builder");
    sh.push_str("# planted: \"$HERE/no-such-input\"\n");
    std::fs::write(&p, sh).expect("edit builder");
    assert_refuses(&dir, "object-freshness", "drivers/no-such-input");
}

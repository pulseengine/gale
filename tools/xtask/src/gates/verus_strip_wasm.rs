//! verus-strip compiled to a WASI p2 component must behave byte-for-byte like
//! the native binary.
//!
//! gale's own tooling is meant to run as components on a wasm runtime. The
//! first one moved is verus-strip. This gate shows the move is safe for every
//! input gale has: for each `src/*.rs` it runs the native binary and the
//! component (under `--runtime`, e.g. wasmtime) and compares stdout bytes and
//! exit code. It also compares the `--check src plain/src` report and exit code.
//!
//! Verdicts: 0 every file was compared and all were identical; 1 at least one
//! difference (named, with the first differing byte offset); 3 refused -- the
//! inputs are empty or short, or a binary/runtime could not be spawned or died
//! by a signal. A refusal is never a pass.
//!
//! The runtime runs with the input root as its working directory and
//! `--dir .::.`, and both sides get the same relative paths. A report that
//! prints paths then reads the same on both sides, whatever the root is.

use crate::verdict::Verdict;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

pub const NAME: &str = "verus-strip-wasm";

/// The three paths the gate needs, set by `main` from
/// `--native`, `--wasm` and `--runtime`.
pub struct Opts {
    pub native: PathBuf,
    pub wasm: PathBuf,
    pub runtime: PathBuf,
}

static OPTS: OnceLock<Opts> = OnceLock::new();

pub fn set_opts(o: Opts) {
    let _ = OPTS.set(o);
}

fn opts() -> Result<&'static Opts, String> {
    OPTS.get()
        .ok_or_else(|| "needs --native <bin> --wasm <component.wasm> --runtime <wasmtime>".into())
}

/// One run's observable behaviour.
struct Out {
    stdout: Vec<u8>,
    code: i32,
}

/// Spawn failure or death by signal is an `Err`: the run did not answer.
fn exec(mut cmd: Command, what: &str) -> Result<Out, String> {
    let o = cmd
        .output()
        .map_err(|e| format!("cannot spawn {what}: {e}"))?;
    match o.status.code() {
        Some(code) => Ok(Out {
            stdout: o.stdout,
            code,
        }),
        None => Err(format!("{what} was killed by a signal ({})", o.status)),
    }
}

fn native(bin: &Path, root: &Path, args: &[&str]) -> Result<Out, String> {
    let mut c = Command::new(bin);
    c.current_dir(root).args(args);
    exec(c, &format!("native {}", bin.display()))
}

fn wasm(rt: &Path, comp: &Path, root: &Path, args: &[&str]) -> Result<Out, String> {
    let mut c = Command::new(rt);
    c.current_dir(root)
        .args(["run", "-S", "cli-exit-with-code", "--dir", ".::."])
        .arg(comp)
        .args(args);
    exec(c, &format!("runtime {}", rt.display()))
}

/// Offset of the first differing byte, or `None` if equal.
fn first_diff(a: &[u8], b: &[u8]) -> Option<usize> {
    if a == b {
        return None;
    }
    Some(
        a.iter()
            .zip(b)
            .position(|(x, y)| x != y)
            .unwrap_or(a.len().min(b.len())),
    )
}

/// Entries of `const FILES` in tools/verus-strip/tests/gate.rs: the floor on
/// how many inputs a run must compare.
fn gate_list_len(repo: &Path) -> Result<usize, String> {
    let p = repo.join("tools/verus-strip/tests/gate.rs");
    let t = std::fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    let start = t
        .find("const FILES")
        .ok_or("gate.rs has no `const FILES`")?;
    let body = &t[start..];
    let end = body
        .find("];")
        .ok_or("gate.rs `const FILES` is unterminated")?;
    let n = body[..end].matches('"').count() / 2;
    if n == 0 {
        return Err("gate.rs `const FILES` parsed as empty -- the floor would be vacuous".into());
    }
    Ok(n)
}

/// Sorted `src/*.rs` file names under `root`.
fn inputs(root: &Path) -> Result<Vec<String>, String> {
    let d = root.join("src");
    let rd = std::fs::read_dir(&d).map_err(|e| format!("{}: {e}", d.display()))?;
    let mut v: Vec<String> = rd
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".rs"))
        .collect();
    v.sort();
    Ok(v)
}

/// The comparison. `nroot` is what the native binary reads, `wroot` what the
/// component reads; they are the same tree except in the planted-input test.
fn compare(o: &Opts, nroot: &Path, wroot: &Path, floor: usize) -> Verdict {
    let files = match inputs(nroot) {
        Ok(f) => f,
        Err(e) => return Verdict::Refused(e),
    };
    if files.is_empty() || files.len() < floor {
        return Verdict::Refused(format!(
            "{} input(s) under {}/src, need at least {} (and more than 0)",
            files.len(),
            nroot.display(),
            floor.max(1)
        ));
    }
    let mut lines = Vec::new();
    let mut same = 0usize;
    let mut differ = 0usize;
    let mut check = |label: &str, nargs: &[&str], wargs: &[&str]| -> Result<(), String> {
        let n = native(&o.native, nroot, nargs)?;
        let w = wasm(&o.runtime, &o.wasm, wroot, wargs)?;
        if n.code != w.code {
            differ += 1;
            lines.push(format!(
                "DIFF {label}: exit code native={} wasm={}",
                n.code, w.code
            ));
        } else if let Some(at) = first_diff(&n.stdout, &w.stdout) {
            differ += 1;
            lines.push(format!(
                "DIFF {label}: stdout differs at byte {at} (native {} B, wasm {} B)",
                n.stdout.len(),
                w.stdout.len()
            ));
        } else {
            same += 1;
        }
        Ok(())
    };
    for f in &files {
        let rel = format!("src/{f}");
        if let Err(e) = check(&rel, &[&rel], &[&rel]) {
            return Verdict::Refused(e);
        }
    }
    let ck = ["--check", "src", "plain/src"];
    if let Err(e) = check("--check src plain/src", &ck, &ck) {
        return Verdict::Refused(e);
    }
    let compared = files.len() + 1;
    lines.push(format!(
        "{compared} compared ({} src files + the --check report): {same} identical, {differ} differ",
        files.len()
    ));
    if differ == 0 && same == compared {
        Verdict::Pass(lines)
    } else {
        Verdict::Fail(lines)
    }
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let o = match opts() {
        Ok(o) => o,
        Err(e) => return Verdict::Refused(e),
    };
    let floor = match gate_list_len(repo) {
        Ok(n) => n,
        Err(e) => return Verdict::Refused(e),
    };
    compare(o, repo, repo, floor)
}

/// Copy `src/<names>` and `plain/src/` of `repo` into `dst`.
fn stage(repo: &Path, dst: &Path, names: &[&str]) -> Result<(), String> {
    let io = |e: std::io::Error| format!("staging {}: {e}", dst.display());
    std::fs::create_dir_all(dst.join("src")).map_err(io)?;
    std::fs::create_dir_all(dst.join("plain/src")).map_err(io)?;
    for n in names {
        std::fs::copy(repo.join("src").join(n), dst.join("src").join(n)).map_err(io)?;
        let p = repo.join("plain/src").join(n);
        if p.exists() {
            std::fs::copy(p, dst.join("plain/src").join(n)).map_err(io)?;
        }
    }
    Ok(())
}

pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };

    ck("first_diff equal", first_diff(b"abc", b"abc").is_none());
    ck("first_diff mid", first_diff(b"abc", b"abd") == Some(2));
    ck("first_diff prefix", first_diff(b"ab", b"abc") == Some(2));
    ck(
        "gate.rs list parses non-empty",
        gate_list_len(repo).is_ok_and(|n| n > 0),
    );

    let o = match opts() {
        Ok(o) => o,
        Err(e) => return Verdict::Refused(format!("self-test plants run the real binaries: {e}")),
    };
    let tmp = std::env::temp_dir().join(format!("gale-xtask-vswasm-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&tmp);
    let names = ["sem.rs", "mutex.rs", "error.rs"];
    let (good, bad, empty) = (tmp.join("good"), tmp.join("bad"), tmp.join("empty"));
    let staged = stage(repo, &good, &names)
        .and_then(|_| stage(repo, &bad, &names))
        .and_then(|_| std::fs::create_dir_all(empty.join("src")).map_err(|e| e.to_string()))
        .and_then(|_| {
            // The planted input: the component reads a perturbed sem.rs. The
            // item goes INSIDE `verus! { }`: verus-strip emits only that block,
            // so text appended after it would not change the output at all.
            let p = bad.join("src/sem.rs");
            let t = std::fs::read_to_string(&p).map_err(|e| e.to_string())?;
            let at = t
                .rfind("} // verus!")
                .ok_or("sem.rs has no `} // verus!` to plant before")?;
            let t = format!(
                "{}pub fn planted_difference() -> u32 {{ 7 }}\n\n{}",
                &t[..at],
                &t[at..]
            );
            std::fs::write(&p, t).map_err(|e| e.to_string())
        });
    if let Err(e) = staged {
        let _ = std::fs::remove_dir_all(&tmp);
        return Verdict::Refused(e);
    }

    let show = |v: &Verdict| format!("exit {}", v.code());

    // Positive control: identical staged inputs pass.
    let v = compare(o, &good, &good, names.len());
    ck(
        &format!("control: identical inputs PASS ({})", show(&v)),
        matches!(v, Verdict::Pass(_)),
    );

    // (a) the component gets a perturbed copy of one input: must differ.
    let v = compare(o, &good, &bad, names.len());
    let names_file = matches!(&v, Verdict::Fail(l) if l.iter().any(|x| x.contains("src/sem.rs") && x.contains("byte")));
    ck(
        &format!(
            "(a) perturbed input on the wasm side FAILS naming src/sem.rs ({})",
            show(&v)
        ),
        names_file,
    );

    // (b) a runtime that does not exist must refuse, not pass or fail.
    let missing = Opts {
        native: o.native.clone(),
        wasm: o.wasm.clone(),
        runtime: tmp.join("no-such-runtime"),
    };
    let v = compare(&missing, &good, &good, names.len());
    ck(
        &format!("(b) missing runtime REFUSES ({})", show(&v)),
        matches!(v, Verdict::Refused(_)),
    );

    // (c) an empty source dir must refuse.
    let v = compare(o, &empty, &empty, 0);
    ck(
        &format!("(c) empty src dir REFUSES ({})", show(&v)),
        matches!(v, Verdict::Refused(_)),
    );

    // (d) fewer inputs than the floor must refuse.
    let v = compare(o, &good, &good, names.len() + 1);
    ck(
        &format!("(d) inputs below the floor REFUSE ({})", show(&v)),
        matches!(v, Verdict::Refused(_)),
    );

    let _ = std::fs::remove_dir_all(&tmp);
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_diff_offsets() {
        assert_eq!(first_diff(b"", b""), None);
        assert_eq!(first_diff(b"x", b""), Some(0));
        assert_eq!(first_diff(b"abc", b"abd"), Some(2));
    }

    #[test]
    fn missing_binaries_refuse() {
        let o = Opts {
            native: "/nonexistent/native".into(),
            wasm: "/nonexistent/x.wasm".into(),
            runtime: "/nonexistent/rt".into(),
        };
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
        assert!(matches!(compare(&o, &root, &root, 1), Verdict::Refused(_)));
    }
}

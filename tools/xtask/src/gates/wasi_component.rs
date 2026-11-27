//! Shared plumbing for the gates that run one of gale's tools both natively and
//! as a WASI p2 component and compare the two (verus-strip-wasm,
//! gust-target-gen-wasm).
//!
//! Only the parts that must not drift between those gates live here: the
//! three paths from `--native`/`--wasm`/`--runtime`, anchoring them before any
//! spawn, the spawn itself (a spawn failure or death by signal is a refusal,
//! never a verdict), the first-differing-byte helper and the wasmtime command.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;

/// The three paths the gates need, set by `main` from
/// `--native`, `--wasm` and `--runtime`.
pub struct Opts {
    pub native: PathBuf,
    pub wasm: PathBuf,
    pub runtime: PathBuf,
}

static OPTS: OnceLock<Opts> = OnceLock::new();

/// Every spawn runs with some other directory as its working directory, so a
/// relative `--native target/release/verus-strip` would resolve there and not
/// where it was given. Paths with a separator are made absolute against the
/// caller's directory; a bare name (`wasmtime`) is left for the PATH lookup.
pub fn anchor(p: PathBuf) -> PathBuf {
    if p.components().count() > 1 || p.is_absolute() {
        std::path::absolute(&p).unwrap_or(p)
    } else {
        p
    }
}

pub fn set_opts(o: Opts) {
    let _ = OPTS.set(Opts {
        native: anchor(o.native),
        wasm: anchor(o.wasm),
        runtime: anchor(o.runtime),
    });
}

pub fn opts() -> Result<&'static Opts, String> {
    OPTS.get()
        .ok_or_else(|| "needs --native <bin> --wasm <component.wasm> --runtime <wasmtime>".into())
}

/// One run's observable behaviour.
pub struct Out {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: i32,
}

/// Spawn failure or death by signal is an `Err`: the run did not answer.
pub fn exec(mut cmd: Command, what: &str) -> Result<Out, String> {
    let o = cmd
        .output()
        .map_err(|e| format!("cannot spawn {what}: {e}"))?;
    match o.status.code() {
        Some(code) => Ok(Out {
            stdout: o.stdout,
            stderr: o.stderr,
            code,
        }),
        None => Err(format!("{what} was killed by a signal ({})", o.status)),
    }
}

/// `<rt> run -S cli-exit-with-code --dir <host>::<guest>... <comp>`, run in
/// `cwd`. The caller appends the component's own arguments.
pub fn wasmtime(rt: &Path, comp: &Path, cwd: &Path, dirs: &[(&Path, &str)]) -> Command {
    let mut c = Command::new(rt);
    c.current_dir(cwd).args(["run", "-S", "cli-exit-with-code"]);
    for (host, guest) in dirs {
        c.arg("--dir").arg(format!("{}::{guest}", host.display()));
    }
    c.arg(comp);
    c
}

/// Offset of the first differing byte, or `None` if equal.
pub fn first_diff(a: &[u8], b: &[u8]) -> Option<usize> {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_diff_offsets() {
        assert_eq!(first_diff(b"", b""), None);
        assert_eq!(first_diff(b"x", b""), Some(0));
        assert_eq!(first_diff(b"abc", b"abd"), Some(2));
    }

    /// CI passes `target/release/verus-strip`; the spawns run elsewhere, so a
    /// relative path must be anchored first (gale#556, first run).
    #[test]
    fn relative_paths_are_anchored_bare_names_are_not() {
        let rel = anchor("target/release/verus-strip".into());
        assert!(rel.is_absolute());
        assert!(rel.ends_with("target/release/verus-strip"));
        assert_eq!(anchor("wasmtime".into()), PathBuf::from("wasmtime"));
        assert_eq!(anchor("/opt/rt".into()), PathBuf::from("/opt/rt"));
    }

    #[test]
    fn wasmtime_command_shape() {
        let c = wasmtime(
            Path::new("/rt"),
            Path::new("/c.wasm"),
            Path::new("/"),
            &[(Path::new("."), "."), (Path::new("/o"), "out")],
        );
        let a: Vec<String> = c
            .get_args()
            .map(|s| s.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            a,
            [
                "run",
                "-S",
                "cli-exit-with-code",
                "--dir",
                ".::.",
                "--dir",
                "/o::out",
                "/c.wasm"
            ]
        );
    }
}

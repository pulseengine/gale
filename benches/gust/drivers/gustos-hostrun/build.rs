//! Stamp the RESOLVED engine version into the binary. The harness prints "which
//! engine executed the composite" as part of its evidence, and `wasmtime` exposes no
//! version constant of its own, so the version is read from the lockfile that
//! actually pinned it — not from the `"42"` requirement in Cargo.toml, which would
//! silently misreport any patch bump.
use std::path::Path;

fn main() {
    let lock = Path::new(env!("CARGO_MANIFEST_DIR")).join("Cargo.lock");
    println!("cargo:rerun-if-changed={}", lock.display());
    let mut version = "unknown".to_string();
    if let Ok(text) = std::fs::read_to_string(&lock) {
        let mut in_wasmtime = false;
        for line in text.lines() {
            if line.trim() == "[[package]]" {
                in_wasmtime = false;
            } else if line.trim() == "name = \"wasmtime\"" {
                in_wasmtime = true;
            } else if in_wasmtime {
                if let Some(v) = line.trim().strip_prefix("version = ") {
                    version = v.trim_matches('"').to_string();
                    break;
                }
            }
        }
    }
    println!("cargo:rustc-env=WASMTIME_VERSION={version}");
}

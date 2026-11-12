//! Build the Renode test images from source: `cargo xtask renode-elfs`.
//!
//! The Renode targets in os/gust/renode-test/BUILD.bazel each run an image named
//! by `"ELF": ":<stem>.elf"`. Those images used to be committed. On 2026-11-11
//! only 1 of the 17 (`gust_dma`) matched what current main builds, in any of the
//! four oracle configurations, so the gates were exercising binaries the source
//! no longer produces. They are now built by this command, in CI before
//! `bazel test` and locally before any Renode run, and are not committed.
//!
//! Mapping: `<stem>.elf` is the release build of bin `<stem>` in os/gust, with
//! default features, except `gust_wasm.elf`, which is the crate's main binary
//! (`gust`, src/main.rs).

use std::path::{Path, PathBuf};
use std::process::Command;

pub const BENCH: &str = "os/gust";
pub const RENODE: &str = "os/gust/renode-test";
const TARGET: &str = "thumbv7m-none-eabi";

/// Every image a renode_test target names, in BUILD.bazel order, deduplicated.
/// Any `":<name>.elf"` label counts, whatever the variable is called: the
/// 2-core target passes two images as `ELF0`/`ELF1`, which a match on
/// `"ELF": ":` alone silently skipped.
pub fn elf_stems(build: &str) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for part in build.split("\":").skip(1) {
        let label = part.split('"').next().unwrap_or("");
        if let Some(stem) = label.strip_suffix(".elf") {
            if !stem.is_empty() && !out.iter().any(|s| s == stem) {
                out.push(stem.to_string());
            }
        }
    }
    out
}

/// The cargo bin that produces `<stem>.elf`.
pub fn bin_for(stem: &str) -> &str {
    if stem == "gust_wasm" {
        "gust"
    } else {
        stem
    }
}

/// The source file of that bin, relative to the repository root.
pub fn source_for(stem: &str) -> PathBuf {
    if stem == "gust_wasm" {
        PathBuf::from(BENCH).join("src/main.rs")
    } else {
        PathBuf::from(BENCH).join("src/bin").join(format!("{stem}.rs"))
    }
}

pub fn run(repo: &Path) -> i32 {
    let build = match std::fs::read_to_string(repo.join(RENODE).join("BUILD.bazel")) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("renode-elfs: cannot read {RENODE}/BUILD.bazel: {e}");
            return 3;
        }
    };
    let stems = elf_stems(&build);
    if stems.is_empty() {
        eprintln!("renode-elfs: BUILD.bazel names no ELF -- nothing to build would mean a vacuous gate");
        return 3;
    }
    let missing: Vec<&String> = stems.iter().filter(|s| !repo.join(source_for(s)).is_file()).collect();
    if !missing.is_empty() {
        for s in missing {
            eprintln!("renode-elfs: no source for {s}.elf (expected {})", source_for(s).display());
        }
        return 1;
    }
    let mut cmd = Command::new("cargo");
    cmd.current_dir(repo.join(BENCH)).args(["build", "--release"]);
    for s in &stems {
        cmd.args(["--bin", bin_for(s)]);
    }
    eprintln!("renode-elfs: building {} image(s) in {BENCH}", stems.len());
    match cmd.status() {
        Ok(st) if st.success() => {}
        Ok(st) => {
            eprintln!("renode-elfs: cargo build failed ({st})");
            return 1;
        }
        Err(e) => {
            eprintln!("renode-elfs: cannot run cargo: {e}");
            return 3;
        }
    }
    let out_dir = repo.join(BENCH).join("target").join(TARGET).join("release");
    for s in &stems {
        let from = out_dir.join(bin_for(s));
        let to = repo.join(RENODE).join(format!("{s}.elf"));
        if let Err(e) = std::fs::copy(&from, &to) {
            eprintln!("renode-elfs: copy {} -> {}: {e}", from.display(), to.display());
            return 1;
        }
        println!("{s}.elf <- {}", bin_for(s));
    }
    0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stems_are_read_in_order_and_deduplicated() {
        let b = "renode_test(\n name = \"a\",\n variables_with_label = {\"ELF\": \":gust_wasm.elf\", \"REPL\": \":x.repl\"},\n)\n\
                 renode_test(\n name = \"b\",\n variables_with_label = {\"ELF\": \":gust_uart.elf\"},\n)\n\
                 renode_test(\n name = \"c\",\n variables_with_label = {\"ELF\": \":gust_uart.elf\"},\n)\n";
        assert_eq!(elf_stems(b), vec!["gust_wasm", "gust_uart"]);
    }

    #[test]
    fn numbered_image_variables_are_read() {
        let b = "renode_test(\n name = \"two\",\n variables_with_label = {\"ELF0\": \":gust_switch_renode.elf\", \"ELF1\": \":gust_estimator_part.elf\", \"REPL\": \":x.repl\"},\n)\n";
        assert_eq!(elf_stems(b), vec!["gust_switch_renode", "gust_estimator_part"]);
    }

    #[test]
    fn main_binary_maps_to_main_rs() {
        assert_eq!(bin_for("gust_wasm"), "gust");
        assert_eq!(source_for("gust_wasm"), PathBuf::from("os/gust/src/main.rs"));
        assert_eq!(bin_for("gust_iso"), "gust_iso");
        assert_eq!(source_for("gust_iso"), PathBuf::from("os/gust/src/bin/gust_iso.rs"));
    }
}

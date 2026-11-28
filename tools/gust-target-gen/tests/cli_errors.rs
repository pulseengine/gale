//! Planted bad inputs: every user-facing failure must exit with its documented
//! code and an `error: ...` line naming the problem, never a panic (exit 101).
//!
//! Exit codes (see `--help` and README.md):
//!   0 success, 1 input rejected, 2 usage error, 3 I/O error.

use std::path::PathBuf;
use std::process::{Command, Output};

const BIN: &str = env!("CARGO_BIN_EXE_gust-target-gen");
const ITEMS: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/golden/f100.items.json");
const BOARD: &str = "STM32F100::Board.vldiscovery";

/// A fresh, empty scratch directory for one test.
fn scratch(name: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("cli_errors").join(name);
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn run(args: &[&str]) -> Output {
    Command::new(BIN).args(args).output().expect("spawn gust-target-gen")
}

/// Assert exit `code`, no panic, and every `needles` substring on stderr.
fn expect_fail(out: &Output, code: i32, needles: &[&str]) {
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(!stderr.contains("panicked at"), "panicked:\n{stderr}");
    assert_eq!(out.status.code(), Some(code), "exit code; stderr:\n{stderr}");
    assert!(stderr.contains("error: "), "no `error: ` line:\n{stderr}");
    for n in needles {
        assert!(stderr.contains(n), "stderr lacks `{n}`:\n{stderr}");
    }
}

#[test]
fn no_args_is_usage_error() {
    expect_fail(&run(&[]), 2, &["--items", "Usage:"]);
}

#[test]
fn unknown_flag_is_usage_error() {
    let d = scratch("unknown_flag");
    let out = d.join("out");
    let o = run(&["--items", ITEMS, "--board", BOARD, "--out", out.to_str().unwrap(), "--bogus"]);
    expect_fail(&o, 2, &["--bogus", "Usage:"]);
    assert!(!out.exists(), "a usage error must not write anything");
}

#[test]
fn flag_without_value_is_usage_error() {
    expect_fail(&run(&["--board", BOARD, "--out", "x", "--items"]), 2, &["--items"]);
}

#[test]
fn missing_board_is_usage_error() {
    expect_fail(&run(&["--items", ITEMS, "--out", "x"]), 2, &["--board"]);
}

#[test]
fn missing_items_file_is_io_error() {
    let d = scratch("missing_items");
    let missing = d.join("no-such.json");
    let o = run(&["--items", missing.to_str().unwrap(), "--board", BOARD, "--out", d.join("out").to_str().unwrap()]);
    expect_fail(&o, 3, &["no-such.json"]);
}

#[test]
fn malformed_json_is_input_error() {
    let d = scratch("malformed_json");
    let bad = d.join("bad.json");
    std::fs::write(&bad, "{ not json").unwrap();
    let o = run(&["--items", bad.to_str().unwrap(), "--board", BOARD, "--out", d.join("out").to_str().unwrap()]);
    expect_fail(&o, 1, &["malformed", "bad.json"]);
}

#[test]
fn unknown_board_is_input_error() {
    let d = scratch("unknown_board");
    let o = run(&["--items", ITEMS, "--board", "NOPE::Board.x", "--out", d.join("out").to_str().unwrap()]);
    expect_fail(&o, 1, &["NOPE"]);
    assert!(!d.join("out").exists(), "a rejected model must not write anything");
}

#[test]
fn board_without_package_is_input_error() {
    let d = scratch("board_no_pkg");
    let o = run(&["--items", ITEMS, "--board", "Board.vldiscovery", "--out", d.join("out").to_str().unwrap()]);
    expect_fail(&o, 1, &["Board.vldiscovery"]);
}

#[test]
fn malformed_reference_is_input_error() {
    let d = scratch("malformed_reference");
    let tsv = d.join("bad.tsv");
    std::fs::write(&tsv, "USART1 0x40013800\n").unwrap(); // space, not TAB
    let o = run(&[
        "--items", ITEMS, "--board", BOARD, "--out", d.join("out").to_str().unwrap(),
        "--reference", tsv.to_str().unwrap(),
    ]);
    expect_fail(&o, 1, &["bad.tsv", "line 1"]);
}

#[test]
fn unwritable_out_is_io_error() {
    // --out below a regular file: create_dir_all fails for every user, root included.
    let d = scratch("unwritable_out");
    let file = d.join("a-file");
    std::fs::write(&file, "").unwrap();
    let out = file.join("out");
    let o = run(&["--items", ITEMS, "--board", BOARD, "--out", out.to_str().unwrap()]);
    expect_fail(&o, 3, &["a-file"]);
}

#[test]
fn version_exits_zero() {
    for flag in ["--version", "-V"] {
        let o = run(&[flag]);
        assert_eq!(o.status.code(), Some(0), "{flag}");
        assert_eq!(
            String::from_utf8_lossy(&o.stdout),
            format!("gust-target-gen {}\n", env!("CARGO_PKG_VERSION")),
            "{flag}"
        );
    }
}

#[test]
fn help_exits_zero_and_documents_exit_codes() {
    for flag in ["--help", "-h"] {
        let o = run(&[flag]);
        assert_eq!(o.status.code(), Some(0), "{flag}");
        let s = String::from_utf8_lossy(&o.stdout);
        assert!(s.contains("Usage:") && s.contains("Exit codes:"), "{flag}:\n{s}");
    }
}

#[test]
fn good_input_still_succeeds() {
    let d = scratch("good");
    let out = d.join("out");
    let o = run(&["--items", ITEMS, "--board", BOARD, "--out", out.to_str().unwrap()]);
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stderr));
    assert!(out.join("gust_target_stm32f100.rs").is_file());
}

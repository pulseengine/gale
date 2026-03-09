"""Public API for the Rust testing Bazel rules.

Usage in BUILD.bazel:
    load("//bzl:defs.bzl", "cargo_test", "clippy_test", "rustfmt_test", ...)
"""

load("//bzl/private:cargo.bzl",
    _cargo_bench_test = "cargo_bench_test",
    _cargo_fuzz_test = "cargo_fuzz_test",
    _cargo_mutants_test = "cargo_mutants_test",
    _cargo_test = "cargo_test",
    _clippy_test = "clippy_test",
    _kani_test = "kani_test",
    _miri_test = "miri_test",
    _rustfmt_test = "rustfmt_test",
    _sanitizer_test = "sanitizer_test",
)
load("//bzl/private:precommit.bzl",
    _pre_commit_install = "pre_commit_install",
)

cargo_test = _cargo_test
rustfmt_test = _rustfmt_test
clippy_test = _clippy_test
miri_test = _miri_test
kani_test = _kani_test
sanitizer_test = _sanitizer_test
cargo_fuzz_test = _cargo_fuzz_test
cargo_bench_test = _cargo_bench_test
cargo_mutants_test = _cargo_mutants_test
pre_commit_install = _pre_commit_install

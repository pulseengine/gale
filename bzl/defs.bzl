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

load("//bzl/private:verus_to_rocq.bzl",
    _verus_strip = "verus_strip",
    _verus_strip_gate = "verus_strip_gate",
    _rocq_module = "rocq_module",
)

verus_strip = _verus_strip
verus_strip_gate = _verus_strip_gate
rocq_module = _rocq_module
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

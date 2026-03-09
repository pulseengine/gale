"""Bazel rules for Rust testing tools (cargo test, clippy, fmt, miri, kani, fuzz, sanitizers, bench, mutants).

All rules follow the same pattern as rules_verus: generate a test script,
run with no-sandbox to access host cargo/rustup, resolve the real workspace
from runfiles symlinks.
"""

def _cargo_preamble(toolchain = ""):
    """Common shell preamble: find real HOME, cargo, rustup, workspace."""
    return """\
#!/bin/bash
set -euo pipefail

# Locate real HOME (Bazel overrides it inside sandbox)
REAL_HOME=$(eval echo ~$(id -un 2>/dev/null) 2>/dev/null || echo "${{HOME:-/root}}")
if [ -d "$REAL_HOME/.rustup" ]; then
    export HOME="$REAL_HOME"
fi
for p in "$HOME/.cargo/bin" "$HOME/.rustup/shims" "/usr/local/bin"; do
    [ -d "$p" ] && export PATH="$p:$PATH"
done

# Resolve workspace directory from runfiles Cargo.toml symlink
MANIFEST="$TEST_SRCDIR/$TEST_WORKSPACE/{manifest_short_path}"
if [ -L "$MANIFEST" ]; then
    MANIFEST="$(readlink -f "$MANIFEST")"
fi
WS="$(dirname "$MANIFEST")"
cd "$WS"
""" + ("""\
# Select Rust toolchain
TOOLCHAIN="{toolchain}"
""".format(toolchain = toolchain) if toolchain else "")

def _detect_target():
    """Shell snippet to detect host target triple."""
    return """\
ARCH="$(uname -m)"
OS="$(uname -s)"
if [ "$OS" = "Darwin" ]; then
    if [ "$ARCH" = "arm64" ]; then TARGET="aarch64-apple-darwin"
    else TARGET="x86_64-apple-darwin"; fi
else TARGET="x86_64-unknown-linux-gnu"; fi
"""

# =============================================================================
# cargo_test — standard unit + integration tests
# =============================================================================

def _cargo_test_impl(ctx):
    manifest = ctx.file.manifest
    script_content = _cargo_preamble().format(
        manifest_short_path = manifest.short_path,
    ) + """\
echo "=== cargo test ==="
exec cargo test {extra_args}
""".format(extra_args = " ".join(ctx.attr.extra_args))

    script = ctx.actions.declare_file(ctx.label.name + "_test.sh")
    ctx.actions.write(output = script, content = script_content, is_executable = True)

    runfiles = ctx.runfiles(files = [manifest] + ctx.files.srcs + ctx.files.data)
    return [
        DefaultInfo(executable = script, runfiles = runfiles),
        testing.ExecutionInfo({"no-sandbox": "1"}),
    ]

cargo_test = rule(
    implementation = _cargo_test_impl,
    attrs = {
        "manifest": attr.label(
            allow_single_file = ["Cargo.toml"],
            mandatory = True,
            doc = "Path to Cargo.toml",
        ),
        "srcs": attr.label_list(allow_files = True, doc = "Source files"),
        "data": attr.label_list(allow_files = True, doc = "Additional data files"),
        "extra_args": attr.string_list(default = ["--lib", "--tests"], doc = "Extra cargo test args"),
    },
    test = True,
    doc = "Run cargo test on the plain Rust crate.",
)

# =============================================================================
# rustfmt_test — cargo fmt --check
# =============================================================================

def _rustfmt_test_impl(ctx):
    manifest = ctx.file.manifest
    script_content = _cargo_preamble().format(
        manifest_short_path = manifest.short_path,
    ) + """\
echo "=== cargo fmt --check ==="
exec cargo fmt --check
"""

    script = ctx.actions.declare_file(ctx.label.name + "_test.sh")
    ctx.actions.write(output = script, content = script_content, is_executable = True)

    runfiles = ctx.runfiles(files = [manifest] + ctx.files.srcs)
    return [
        DefaultInfo(executable = script, runfiles = runfiles),
        testing.ExecutionInfo({"no-sandbox": "1"}),
    ]

rustfmt_test = rule(
    implementation = _rustfmt_test_impl,
    attrs = {
        "manifest": attr.label(allow_single_file = ["Cargo.toml"], mandatory = True),
        "srcs": attr.label_list(allow_files = True),
    },
    test = True,
    doc = "Check Rust formatting with cargo fmt --check.",
)

# =============================================================================
# clippy_test — cargo clippy -D warnings
# =============================================================================

def _clippy_test_impl(ctx):
    manifest = ctx.file.manifest
    extra = " ".join(ctx.attr.extra_args) if ctx.attr.extra_args else "-D warnings"
    script_content = _cargo_preamble().format(
        manifest_short_path = manifest.short_path,
    ) + """\
echo "=== cargo clippy ==="
exec cargo clippy --all-targets -- {extra}
""".format(extra = extra)

    script = ctx.actions.declare_file(ctx.label.name + "_test.sh")
    ctx.actions.write(output = script, content = script_content, is_executable = True)

    runfiles = ctx.runfiles(files = [manifest] + ctx.files.srcs + ctx.files.data)
    return [
        DefaultInfo(executable = script, runfiles = runfiles),
        testing.ExecutionInfo({"no-sandbox": "1"}),
    ]

clippy_test = rule(
    implementation = _clippy_test_impl,
    attrs = {
        "manifest": attr.label(allow_single_file = ["Cargo.toml"], mandatory = True),
        "srcs": attr.label_list(allow_files = True),
        "data": attr.label_list(allow_files = True),
        "extra_args": attr.string_list(default = ["-D", "warnings"], doc = "Clippy args after --"),
    },
    test = True,
    doc = "Run cargo clippy with -D warnings.",
)

# =============================================================================
# miri_test — cargo +nightly miri test
# =============================================================================

def _miri_test_impl(ctx):
    manifest = ctx.file.manifest
    script_content = _cargo_preamble().format(
        manifest_short_path = manifest.short_path,
    ) + """\
echo "=== cargo miri test ==="
exec cargo +nightly miri test {extra_args}
""".format(extra_args = " ".join(ctx.attr.extra_args))

    script = ctx.actions.declare_file(ctx.label.name + "_test.sh")
    ctx.actions.write(output = script, content = script_content, is_executable = True)

    runfiles = ctx.runfiles(files = [manifest] + ctx.files.srcs + ctx.files.data)
    return [
        DefaultInfo(executable = script, runfiles = runfiles),
        testing.ExecutionInfo({"no-sandbox": "1"}),
    ]

miri_test = rule(
    implementation = _miri_test_impl,
    attrs = {
        "manifest": attr.label(allow_single_file = ["Cargo.toml"], mandatory = True),
        "srcs": attr.label_list(allow_files = True),
        "data": attr.label_list(allow_files = True),
        "extra_args": attr.string_list(default = ["--lib", "--tests"]),
    },
    test = True,
    doc = "Run cargo miri test for memory safety / UB detection.",
)

# =============================================================================
# kani_test — cargo kani (bounded model checking)
# =============================================================================

def _kani_test_impl(ctx):
    manifest = ctx.file.manifest
    script_content = _cargo_preamble().format(
        manifest_short_path = manifest.short_path,
    ) + """\
echo "=== cargo kani ==="
exec cargo kani {extra_args}
""".format(extra_args = " ".join(ctx.attr.extra_args))

    script = ctx.actions.declare_file(ctx.label.name + "_test.sh")
    ctx.actions.write(output = script, content = script_content, is_executable = True)

    runfiles = ctx.runfiles(files = [manifest] + ctx.files.srcs + ctx.files.data)
    return [
        DefaultInfo(executable = script, runfiles = runfiles),
        testing.ExecutionInfo({"no-sandbox": "1"}),
    ]

kani_test = rule(
    implementation = _kani_test_impl,
    attrs = {
        "manifest": attr.label(allow_single_file = ["Cargo.toml"], mandatory = True),
        "srcs": attr.label_list(allow_files = True),
        "data": attr.label_list(allow_files = True),
        "extra_args": attr.string_list(default = ["--tests"]),
    },
    test = True,
    doc = "Run Kani bounded model checking.",
)

# =============================================================================
# sanitizer_test — cargo test with ASan/TSan/LSan
# =============================================================================

def _sanitizer_test_impl(ctx):
    manifest = ctx.file.manifest
    sanitizer = ctx.attr.sanitizer
    script_content = _cargo_preamble().format(
        manifest_short_path = manifest.short_path,
    ) + _detect_target() + """\
echo "=== {sanitizer} ==="
RUSTFLAGS="-Zsanitizer={sanitizer}" exec cargo +nightly test \\
    --target "$TARGET" --lib --tests
""".format(sanitizer = sanitizer)

    script = ctx.actions.declare_file(ctx.label.name + "_test.sh")
    ctx.actions.write(output = script, content = script_content, is_executable = True)

    runfiles = ctx.runfiles(files = [manifest] + ctx.files.srcs + ctx.files.data)
    return [
        DefaultInfo(executable = script, runfiles = runfiles),
        testing.ExecutionInfo({"no-sandbox": "1"}),
    ]

sanitizer_test = rule(
    implementation = _sanitizer_test_impl,
    attrs = {
        "manifest": attr.label(allow_single_file = ["Cargo.toml"], mandatory = True),
        "srcs": attr.label_list(allow_files = True),
        "data": attr.label_list(allow_files = True),
        "sanitizer": attr.string(
            mandatory = True,
            values = ["address", "thread", "leak", "memory"],
            doc = "Which sanitizer to enable",
        ),
    },
    test = True,
    doc = "Run cargo test with a sanitizer enabled.",
)

# =============================================================================
# cargo_fuzz_test — cargo fuzz run with bounded duration
# =============================================================================

def _cargo_fuzz_test_impl(ctx):
    manifest = ctx.file.manifest
    script_content = _cargo_preamble().format(
        manifest_short_path = manifest.short_path,
    ) + """\
echo "=== cargo fuzz: {target} ({duration}s) ==="
exec cargo +nightly fuzz run {target} -- -max_total_time={duration}
""".format(
        target = ctx.attr.fuzz_target,
        duration = ctx.attr.duration_secs,
    )

    script = ctx.actions.declare_file(ctx.label.name + "_test.sh")
    ctx.actions.write(output = script, content = script_content, is_executable = True)

    runfiles = ctx.runfiles(files = [manifest] + ctx.files.srcs + ctx.files.data)
    return [
        DefaultInfo(executable = script, runfiles = runfiles),
        testing.ExecutionInfo({"no-sandbox": "1"}),
    ]

cargo_fuzz_test = rule(
    implementation = _cargo_fuzz_test_impl,
    attrs = {
        "manifest": attr.label(allow_single_file = ["Cargo.toml"], mandatory = True),
        "srcs": attr.label_list(allow_files = True),
        "data": attr.label_list(allow_files = True),
        "fuzz_target": attr.string(mandatory = True, doc = "Name of the fuzz target binary"),
        "duration_secs": attr.int(default = 60, doc = "Max fuzzing duration in seconds"),
    },
    test = True,
    doc = "Run a cargo-fuzz target for a bounded duration.",
)

# =============================================================================
# cargo_bench_test — cargo bench (verify benchmarks compile and run)
# =============================================================================

def _cargo_bench_test_impl(ctx):
    manifest = ctx.file.manifest
    script_content = _cargo_preamble().format(
        manifest_short_path = manifest.short_path,
    ) + """\
echo "=== cargo bench ==="
exec cargo bench {extra_args}
""".format(extra_args = " ".join(ctx.attr.extra_args))

    script = ctx.actions.declare_file(ctx.label.name + "_test.sh")
    ctx.actions.write(output = script, content = script_content, is_executable = True)

    runfiles = ctx.runfiles(files = [manifest] + ctx.files.srcs + ctx.files.data)
    return [
        DefaultInfo(executable = script, runfiles = runfiles),
        testing.ExecutionInfo({"no-sandbox": "1"}),
    ]

cargo_bench_test = rule(
    implementation = _cargo_bench_test_impl,
    attrs = {
        "manifest": attr.label(allow_single_file = ["Cargo.toml"], mandatory = True),
        "srcs": attr.label_list(allow_files = True),
        "data": attr.label_list(allow_files = True),
        "extra_args": attr.string_list(default = []),
    },
    test = True,
    doc = "Run cargo bench.",
)

# =============================================================================
# cargo_mutants_test — mutation testing
# =============================================================================

def _cargo_mutants_test_impl(ctx):
    manifest = ctx.file.manifest
    script_content = _cargo_preamble().format(
        manifest_short_path = manifest.short_path,
    ) + """\
echo "=== cargo mutants ==="
exec cargo mutants {extra_args}
""".format(extra_args = " ".join(ctx.attr.extra_args))

    script = ctx.actions.declare_file(ctx.label.name + "_test.sh")
    ctx.actions.write(output = script, content = script_content, is_executable = True)

    runfiles = ctx.runfiles(files = [manifest] + ctx.files.srcs + ctx.files.data)
    return [
        DefaultInfo(executable = script, runfiles = runfiles),
        testing.ExecutionInfo({"no-sandbox": "1"}),
    ]

cargo_mutants_test = rule(
    implementation = _cargo_mutants_test_impl,
    attrs = {
        "manifest": attr.label(allow_single_file = ["Cargo.toml"], mandatory = True),
        "srcs": attr.label_list(allow_files = True),
        "data": attr.label_list(allow_files = True),
        "extra_args": attr.string_list(default = []),
    },
    test = True,
    doc = "Run cargo-mutants mutation testing.",
)

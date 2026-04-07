"""Rules for Verus → Rocq translation pipeline.

Chain: src/*.rs → [verus-strip] → standalone .rs → [coq_of_rust] → .v → [rocq] → .vo
No intermediate files on disk — Bazel handles caching.
"""

load("@rules_rocq_rust//coq_of_rust:defs.bzl", "coq_of_rust_library")
load("@rules_rocq_rust//rocq:defs.bzl", "rocq_library")

# Local verus-strip binary — gale's version with extended stripping logic
# (let ghost, inline proof{}, assume_specification, --standalone mode).
_LOCAL_STRIP_TOOL = "//tools/verus-strip:verus-strip"

def verus_strip(name, srcs, **kwargs):
    """Strip Verus annotations from a list of source files.

    Uses the local //tools/verus-strip:verus-strip (which has extended
    stripping logic: let ghost, inline proof{}, assume_specification).

    Args:
        name: Target name.
        srcs: List of Verus-annotated .rs source file labels.
        **kwargs: Additional kwargs forwarded to the underlying rule.
    """
    _verus_strip_rule(
        name = name,
        srcs = srcs,
        **kwargs
    )

def _verus_strip_rule_impl(ctx):
    """Strip Verus annotations from source files, producing plain Rust."""
    strip_tool = ctx.executable._strip_tool
    srcs = ctx.files.srcs
    outputs = []

    for src in srcs:
        out = ctx.actions.declare_file(src.basename)
        outputs.append(out)

        ctx.actions.run(
            executable = strip_tool,
            arguments = [src.path, "-o", out.path],
            inputs = [src],
            outputs = [out],
            mnemonic = "VerusStrip",
            progress_message = "Stripping Verus annotations from %s" % src.short_path,
        )

    return [
        DefaultInfo(
            files = depset(outputs),
            runfiles = ctx.runfiles(files = outputs),
        ),
    ]

_verus_strip_rule = rule(
    implementation = _verus_strip_rule_impl,
    attrs = {
        "srcs": attr.label_list(
            allow_files = [".rs"],
            mandatory = True,
            doc = "Verus-annotated Rust source files to strip.",
        ),
        "_strip_tool": attr.label(
            default = _LOCAL_STRIP_TOOL,
            executable = True,
            cfg = "exec",
            doc = "The verus-strip binary.",
        ),
    },
    doc = "Strip Verus verification annotations from Rust source files, producing plain Rust.",
)

def _verus_strip_gate_impl(ctx):
    """Test that stripping verus_srcs produces output matching plain_srcs."""
    strip_tool = ctx.executable._strip_tool
    verus_srcs = ctx.files.verus_srcs
    plain_srcs = ctx.files.plain_srcs

    script_lines = [
        "#!/bin/bash",
        "set -euo pipefail",
        "FAILURES=0",
        "CHECKED=0",
    ]

    runfiles_list = verus_srcs + plain_srcs

    plain_by_name = {}
    for f in plain_srcs:
        plain_by_name[f.basename] = f

    for verus_src in verus_srcs:
        plain_src = plain_by_name.get(verus_src.basename)
        if not plain_src:
            fail("No matching plain source for %s" % verus_src.basename)

        script_lines.append("CHECKED=$((CHECKED + 1))")
        script_lines.append(
            'STRIPPED=$("{strip_tool}" "{verus}")'.format(
                strip_tool = strip_tool.short_path,
                verus = verus_src.short_path,
            ),
        )
        script_lines.append(
            'EXPECTED=$(cat "{plain}")'.format(plain = plain_src.short_path),
        )
        script_lines.append('if [ "$STRIPPED" = "$EXPECTED" ]; then')
        script_lines.append('  echo "  OK    %s"' % verus_src.basename)
        script_lines.append("else")
        script_lines.append('  echo "  DIFF  %s"' % verus_src.basename)
        script_lines.append(
            '  diff <(echo "$STRIPPED") "{plain}" | head -20 || true'.format(
                plain = plain_src.short_path,
            ),
        )
        script_lines.append("  FAILURES=$((FAILURES + 1))")
        script_lines.append("fi")

    script_lines.extend([
        "echo",
        'echo "Checked $CHECKED files: $((CHECKED - FAILURES)) OK, $FAILURES diverged"',
        'if [ "$FAILURES" -gt 0 ]; then exit 1; fi',
    ])

    script_content = "\n".join(script_lines) + "\n"
    test_script = ctx.actions.declare_file(ctx.label.name + "_test.sh")
    ctx.actions.write(
        output = test_script,
        content = script_content,
        is_executable = True,
    )

    runfiles = ctx.runfiles(files = runfiles_list + [strip_tool])

    return [
        DefaultInfo(
            executable = test_script,
            runfiles = runfiles,
        ),
    ]

_verus_strip_gate_test = rule(
    implementation = _verus_strip_gate_impl,
    attrs = {
        "verus_srcs": attr.label_list(
            allow_files = [".rs"],
            mandatory = True,
            doc = "Verus-annotated Rust source files.",
        ),
        "plain_srcs": attr.label_list(
            allow_files = [".rs"],
            mandatory = True,
            doc = "Expected plain Rust source files (stripped output should match these).",
        ),
        "_strip_tool": attr.label(
            default = _LOCAL_STRIP_TOOL,
            executable = True,
            cfg = "exec",
            doc = "The verus-strip binary.",
        ),
    },
    test = True,
    doc = "Test that stripping Verus annotations from verus_srcs produces output matching plain_srcs.",
)

def verus_strip_gate(name, verus_srcs, plain_srcs, **kwargs):
    """Test that stripping verus_srcs produces output matching plain_srcs.

    Uses the local //tools/verus-strip:verus-strip (gale's version with
    extended stripping logic: let ghost, inline proof{}, assume_specification).

    Args:
        name: Target name (will be a test target).
        verus_srcs: List of Verus-annotated .rs source file labels.
        plain_srcs: List of expected plain .rs source file labels.
        **kwargs: Additional kwargs forwarded to the underlying rule.
    """
    _verus_strip_gate_test(
        name = name,
        verus_srcs = verus_srcs,
        plain_srcs = plain_srcs,
        **kwargs
    )

def _verus_strip_standalone_impl(ctx):
    """Strip Verus annotations and produce a standalone Rust file.

    Uses the local verus-strip tool with --standalone to inline cross-module
    stubs (crate:: imports) so coq_of_rust can translate without resolving deps.
    """
    src = ctx.file.src
    out = ctx.actions.declare_file(ctx.attr.module + ".rs")

    ctx.actions.run(
        executable = ctx.executable._verus_strip,
        arguments = ["--standalone", src.path, "-o", out.path],
        inputs = [src],
        outputs = [out],
        mnemonic = "VerusStripStandalone",
        progress_message = "Stripping Verus annotations (standalone) from %s" % src.short_path,
    )

    return [DefaultInfo(files = depset([out]))]

_verus_strip_standalone_rule = rule(
    implementation = _verus_strip_standalone_impl,
    attrs = {
        "src": attr.label(allow_single_file = [".rs"]),
        "module": attr.string(mandatory = True),
        "_verus_strip": attr.label(
            default = _LOCAL_STRIP_TOOL,
            executable = True,
            cfg = "exec",
        ),
    },
    doc = "Strip Verus annotations with --standalone mode, inlining cross-module stubs for coq_of_rust.",
)

def rocq_module(name, src, rocq_of_rust_lib = None):
    """Generate stripped .rs, translate to Rocq, compile.

    Args:
        name: Module name (e.g., "sem")
        src: Verus-annotated source file label (e.g., "//:src/sem.rs")
        rocq_of_rust_lib: RocqOfRust library target
    """
    if rocq_of_rust_lib == None:
        rocq_of_rust_lib = "@rocq_of_rust_source//:rocq_of_rust_main"

    # Step 1: Strip Verus annotations → standalone .rs (with inlined stubs)
    _verus_strip_standalone_rule(
        name = name + "_stripped",
        src = src,
        module = name,
    )

    # Step 2: Translate standalone .rs → .v
    coq_of_rust_library(
        name = name + "_translated",
        rust_sources = [":" + name + "_stripped"],
        edition = "2021",
    )

    # Step 3: Compile .v with Rocq
    rocq_library(
        name = name + "_compiled",
        srcs = [":" + name + "_translated"],
        deps = [rocq_of_rust_lib],
        extra_flags = ["-impredicative-set"],
    )

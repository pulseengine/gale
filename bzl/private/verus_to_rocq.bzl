"""Rules for Verus → Rocq translation pipeline.

Chain: src/*.rs → [verus-strip] → standalone .rs → [coq_of_rust] → .v → [rocq] → .vo
No intermediate files on disk — Bazel handles caching.
"""

load("@rules_rocq_rust//coq_of_rust:defs.bzl", "coq_of_rust_library")
load("@rules_rocq_rust//rocq:defs.bzl", "rocq_library")
load("@rules_verus//verus:defs.bzl", _rv_verus_strip = "verus_strip", _rv_verus_strip_test = "verus_strip_test")

# Local verus-strip binary — gale's version with extended stripping logic
# (let ghost, inline proof{}, assume_specification, --standalone mode).
_LOCAL_STRIP_TOOL = "//tools/verus-strip:verus-strip"

def verus_strip(name, srcs, **kwargs):
    """Strip Verus annotations from a list of source files.

    Delegates to @rules_verus//verus:defs.bzl#verus_strip, overriding the
    strip tool to use the local //tools/verus-strip:verus-strip (which has
    newer stripping logic: let ghost, inline proof{}, assume_specification).

    Args:
        name: Target name.
        srcs: List of Verus-annotated .rs source file labels.
        **kwargs: Additional kwargs forwarded to the underlying rule.
    """
    _rv_verus_strip(
        name = name,
        srcs = srcs,
        _strip_tool = _LOCAL_STRIP_TOOL,
        **kwargs
    )

def verus_strip_gate(name, verus_srcs, plain_srcs, **kwargs):
    """Test that stripping verus_srcs produces output matching plain_srcs.

    Delegates to @rules_verus//verus:defs.bzl#verus_strip_test, overriding
    the strip tool to use the local //tools/verus-strip:verus-strip.

    Args:
        name: Target name (will be a test target).
        verus_srcs: List of Verus-annotated .rs source file labels.
        plain_srcs: List of expected plain .rs source file labels.
        **kwargs: Additional kwargs forwarded to the underlying rule.
    """
    _rv_verus_strip_test(
        name = name,
        verus_srcs = verus_srcs,
        plain_srcs = plain_srcs,
        _strip_tool = _LOCAL_STRIP_TOOL,
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

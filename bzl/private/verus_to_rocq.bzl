"""Rules for Verus → Rocq translation pipeline.

Chain: src/*.rs → [verus-strip] → standalone .rs → [coq_of_rust] → .v → [rocq] → .vo
No intermediate files on disk — Bazel handles caching.
"""

load("@rules_rocq_rust//coq_of_rust:defs.bzl", "coq_of_rust_library")
load("@rules_rocq_rust//rocq:defs.bzl", "rocq_library")

def _verus_strip_impl(ctx):
    """Strip Verus annotations and produce a standalone Rust file."""
    src = ctx.file.src
    out = ctx.actions.declare_file(ctx.attr.module + ".rs")

    ctx.actions.run(
        executable = ctx.executable._verus_strip,
        arguments = ["--standalone", src.path, "-o", out.path],
        inputs = [src],
        outputs = [out],
        mnemonic = "VerusStrip",
        progress_message = "Stripping Verus annotations from %s" % src.short_path,
    )

    return [DefaultInfo(files = depset([out]))]

verus_strip = rule(
    implementation = _verus_strip_impl,
    attrs = {
        "src": attr.label(allow_single_file = [".rs"]),
        "module": attr.string(mandatory = True),
        "_verus_strip": attr.label(
            default = "//tools/verus-strip:verus-strip",
            executable = True,
            cfg = "exec",
        ),
    },
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

    # Step 1: Strip Verus annotations → standalone .rs
    verus_strip(
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

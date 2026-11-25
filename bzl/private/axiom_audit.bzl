"""Axiom-audit manifests (docs/design/axioms-trusted-audit.md, REQ-HONESTY-AXIOMS-001).

These rules run NO checker and decide nothing. Each writes a manifest naming
the compiled objects of a set of proof libraries, the load paths needed to read
them, and the checker binary of the pinned toolchain, and makes those objects
(and every object they load) outputs of the target, so `bazel build` puts them
on disk. `cargo xtask check axioms-audit` reads the manifest, runs the checker
itself, and judges the output. Keeping the verdict in Rust and the checker run
outside the action cache means a recorded log is never the evidence.

Manifest lines are tab-separated, every path relative to the execution root:

  rocq:  checker <path>
         lib <label> <src> <module> <flag,flag,...>
         q <label> <physical dir> <logical prefix>      (in load order)
  lean:  lean <path>
         path <LEAN_PATH entry>
         mod <label> <src> <module> <olean>

What this cannot see: a library not listed in `libs` (the gate holds the
listed sources equal to the tracked proof files), and how the objects were
built (that is the rule that built them).
"""

load("@rules_lean//lean:defs.bzl", "LeanInfo")
load("@rules_rocq_rust//rocq/private:rocq.bzl", "RocqInfo")

# ── Rocq ────────────────────────────────────────────────────────────────────

_RocqAuditInfo = provider(
    doc = "Every compiled .vo a rocq_library loads, transitively (RocqInfo carries only the direct ones).",
    fields = {"vo": "depset of File", "flags": "this target's own coqc extra_flags (list of string)"},
)

def _rocq_audit_aspect_impl(target, ctx):
    trans = [d[_RocqAuditInfo].vo for d in getattr(ctx.rule.attr, "deps", []) if _RocqAuditInfo in d]
    direct = target[RocqInfo].compiled.to_list() if RocqInfo in target and target[RocqInfo].compiled else []
    flags = getattr(ctx.rule.attr, "extra_flags", [])
    return [_RocqAuditInfo(vo = depset(direct, transitive = trans), flags = list(flags))]

_rocq_audit_aspect = aspect(
    implementation = _rocq_audit_aspect_impl,
    attr_aspects = ["deps"],
)

def _rocq_axiom_audit_impl(ctx):
    rocq_info = ctx.toolchains["@rules_rocq_rust//rocq:toolchain_type"].rocq_info
    checker = [f for f in rocq_info.coq_tools.to_list() if f.basename == "rocqchk"]
    if len(checker) != 1:
        fail("the Rocq toolchain has %d binaries named rocqchk, expected 1" % len(checker))
    lines = ["checker\t" + checker[0].path]
    files = [checker[0]]
    stdlib = rocq_info.stdlib_path
    for lib in ctx.attr.libs:
        info = lib[RocqInfo]
        if not info.compiled:
            fail("%s has no compiled objects (no Rocq toolchain?)" % lib.label)
        files.extend(lib[_RocqAuditInfo].vo.to_list())
        label = str(lib.label)
        srcs = info.sources.to_list()
        if len(srcs) != 1:
            fail("%s: one source per audited library, found %d" % (label, len(srcs)))
        logical = info.include_paths[-1][1]
        module = logical + "." + srcs[0].basename[:-2]
        flags = ",".join(lib[_RocqAuditInfo].flags)
        lines.append("\t".join(["lib", label, srcs[0].path, module, flags]))
        if stdlib:
            lines.append("\t".join(["q", label, stdlib, "Stdlib"]))
        for (_, log, phys) in rocq_info.extra_libs:
            lines.append("\t".join(["q", label, phys, log]))
        seen = {}
        for (phys, log) in info.include_paths:
            if (phys, log) not in seen:
                seen[(phys, log)] = True
                lines.append("\t".join(["q", label, phys, log]))
    files.extend(rocq_info.stdlib.to_list())
    for (lib_files, _, _) in rocq_info.extra_libs:
        files.extend(lib_files)
    out = ctx.actions.declare_file(ctx.label.name + ".manifest")
    ctx.actions.write(out, "\n".join(lines) + "\n")
    return [DefaultInfo(files = depset([out] + files))]

rocq_axiom_audit = rule(
    implementation = _rocq_axiom_audit_impl,
    attrs = {
        "libs": attr.label_list(providers = [RocqInfo], aspects = [_rocq_audit_aspect], doc = "rocq_library targets, one .v each"),
    },
    toolchains = ["@rules_rocq_rust//rocq:toolchain_type"],
)

# ── Lean ────────────────────────────────────────────────────────────────────

_LeanAuditInfo = provider(
    doc = "LEAN_PATH entries and files of a lean_library's dependencies, transitively.",
    fields = {"paths": "depset of string", "files": "depset of File"},
)

def _lean_audit_aspect_impl(target, ctx):
    paths = []
    files = []
    for d in getattr(ctx.rule.attr, "deps", []):
        if LeanInfo in d:
            paths.append(depset(d[LeanInfo].lean_path_entries))
            files.append(d[LeanInfo].all_files)
        if _LeanAuditInfo in d:
            paths.append(d[_LeanAuditInfo].paths)
            files.append(d[_LeanAuditInfo].files)
    return [_LeanAuditInfo(paths = depset(transitive = paths), files = depset(transitive = files))]

_lean_audit_aspect = aspect(
    implementation = _lean_audit_aspect_impl,
    attr_aspects = ["deps"],
)

def _lean_axiom_audit_impl(ctx):
    tc = ctx.toolchains["@rules_lean//lean:toolchain_type"].lean_toolchain_info
    lines = ["lean\t" + tc.lean.path]
    paths = {}
    files = [tc.lean] + list(tc.all_files)
    mods = []
    for lib in ctx.attr.libs:
        info = lib[LeanInfo]
        for p in lib[_LeanAuditInfo].paths.to_list() + info.lean_path_entries:
            paths[p] = True
        files.extend(info.oleans)
        files.extend(lib[_LeanAuditInfo].files.to_list())
        if len(info.srcs) != len(info.oleans):
            fail("%s: %d sources, %d oleans" % (lib.label, len(info.srcs), len(info.oleans)))
        for src, olean in zip(info.srcs, info.oleans):
            root = info.lean_path_entries[0].rstrip("/")
            rel = olean.path[len(root) + 1:] if olean.path.startswith(root + "/") else fail("%s outside %s" % (olean.path, root))
            module = rel[:-len(".olean")].replace("/", ".")
            mods.append("\t".join(["mod", str(lib.label), src.path, module, olean.path]))
    for p in paths:
        lines.append("path\t" + p)
    lines.extend(mods)
    out = ctx.actions.declare_file(ctx.label.name + ".manifest")
    ctx.actions.write(out, "\n".join(lines) + "\n")
    return [DefaultInfo(files = depset([out] + files))]

lean_axiom_audit = rule(
    implementation = _lean_axiom_audit_impl,
    attrs = {
        "libs": attr.label_list(providers = [LeanInfo], aspects = [_lean_audit_aspect], doc = "lean_library targets"),
    },
    toolchains = ["@rules_lean//lean:toolchain_type"],
)

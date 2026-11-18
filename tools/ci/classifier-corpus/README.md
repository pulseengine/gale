# Change-classifier corpus

Language-neutral test cases for the CI change classifier: which heavy jobs a
change must run. The cases are the specification for the shared Rust classifier
that the pulseengine repos will pin through the varve layer. They were written
against `tools/ci/classify-changes.py`, the Python reference on this branch,
which will not land: the Rust tool replaces it and must pass this corpus.

74 cases: 1 from lane L7, 3 from loom, 58 from reference self-test, 3 from relay, 5 from review of ac9b592, 4 from rivet.

## Layout

- `spec.json`: the contract. Gate (job) outputs, class outputs, the run-all and
  inert lists, the docs rule, the unknown-path rule, the fail-open triggers, and
  the fixture's job->input configuration (`gates`).
- `fixture-default.json`: the file tree every case starts from (path -> content).
- `cases/NNN-slug.json`: one case each.

## Case schema

```
id, source, description, notes?        free text
tree:  { base: "default", set?: {path: content}, delete?: [path] }
gates?: a replacement for spec.json `gates` (same shape)
input: one of
  { mode: "paths", event: "pull_request", paths: [...], inject?: {...} }
        the change is exactly this list of paths
  { mode: "git", event: <name or "">, base: "parent"|"empty"|"invalid"|"none",
    change: [{op: "append", path, text} | {op: "delete", path} | {op: "rename", from, to}] }
        commit the tree, apply `change` as a second commit, diff BASE...HEAD;
        base "parent" = the first commit, "empty" = HEAD itself, "invalid" =
        forty zeros, "none" = no base given
  { mode: "probe", paths: [...] }
        the paths arrive through GATE_PROBE_FILES, with no event
check?: { kind: "bazel-srcs" } | { kind: "depinfo", gate, files: {path: content} }
        a check-mode case; see below
expect:
  selected: [gate outputs that must be true, all others false] | "all"
  warning?: true     the run must emit a GitHub `::warning::`
  check?: "pass" | "fail",  error?: <code>
```

`selected` is exact. A tool that selects more than this is safe but wastes
runners. A tool that selects less has a false skip: a required check goes green
without running.

### Gate inputs in the fixture (`spec.json` `gates`)

- `cargo`: manifests the job builds as roots.
- `cargo_glob`: roots found by glob.
- `zephyr`: west `module.yml` files.
- `hand`: hand-listed paths; a trailing `/` means the whole directory.
- `bazel_build` (rivet cases only): BUILD files whose `srcs` are read directly.

In the default fixture, `proofs.hand` (`src/`, `proofs/`, `plain/`) stands in for
the Bazel graph.

### Check-mode cases

- `bazel-srcs` (rivet): every `srcs` in a `bazel_build` file must be a literal
  list of package-relative labels. `glob()`, a variable, or an `@external` label
  is the error `bazel-srcs-not-literal`. The check fails, and classification
  selects all.
- `depinfo` (loom): a rustc dep-info (`.d`) file for `gate`. Every
  repository-relative path it names must be claimed by that gate (`check: pass`),
  or the cross-check fails. Absolute paths outside the repository (toolchain,
  registry) are ignored.

## Reference results

The Python reference passes 61 of the 74 cases. The 7 check-mode
cases are not in the reference (n/a). The remaining 6 are the review's plants
and one of relay's edges, and they fail as follows:

| case | source | reference result | |
|---|---|---|---|
| `059-cfg-attr-path` | review of ac9b592 | FALSE SKIP (misses proofs,zephyr,zephyr_renode) | got gust_renode |
| `060-generate-nested-braces` | review of ac9b592 | over-selects (safe) | got cross_arch,gust_renode,proofs,rust_cov,zephyr,zephyr_renode |
| `061-incbin-global-asm` | review of ac9b592 | FALSE SKIP (misses proofs,zephyr,zephyr_renode) | got cross_arch |
| `062-build-rs-join-dotdot` | review of ac9b592 | FALSE SKIP (misses proofs,zephyr,zephyr_renode) | got cross_arch,gust_renode |
| `063-runtime-read-test-file` | review of ac9b592 | FALSE SKIP (misses proofs,rust_cov) | got cross_arch |
| `067-package-metadata-component` | relay | FALSE SKIP (misses cross_arch) | got gust_renode |

None of these shapes occurs in a crate that a gated job builds. A grep of the
gale tree at this commit finds no `cfg_attr(.. path = ..)`, `.incbin`,
`.join("..")`, `[package.metadata.component]` or nested-brace `generate!`. The
only runtime file reads are in `drivers/gustos-hostrun`, which no gated job
builds. This matches the review's finding of no live false skip.

## Notes for the Rust tool (from the review of ac9b592)

- **The inert list rests on review.** `artifacts/`, `schemas/`, `.rivet/`,
  `upstream-reqif/` and `rivet.yaml` are declared to be read only by rivet and
  the always-run fast gates. No build graph proves this. The only mechanical
  guard is that no graph-derived input may sit under an inert path. A heavy job
  that starts reading one of these paths must be added to the job's inputs by
  hand.
- **The crash handler must emit `::warning::` and still select all.** The
  reference selects all on a crash, but reports it only on stderr
  (`forced-crash`). A crash that only costs time is easy to miss.
- **Cache `git grep` for docs classification.** The docs rule runs one
  `git grep` per changed `.md` file. With 1,200 `.md` files in a diff that took
  50 s. Grep once for all basenames, or build the basename index once.
- **Below Python 3.11, or with no TOML parser, everything runs**
  (`no-toml-parser`). This is moot in Rust, but the rule stays: if a manifest
  cannot be parsed, select all.
- Two edges a case cannot pin down well:
  - The job->output map that workflows wire (`GATE_OUTPUTS` in
    `tools/check-required-contexts.py`) is hand-maintained. The corpus tests
    each job's inputs, not which output a workflow reads.
  - Bazel queries are slow (Lean pulls its whole library). The reference used a
    committed `bazel query` snapshot with a fingerprint. Rivet's rule
    (`bazel-srcs-*`) instead makes BUILD files readable without Bazel.

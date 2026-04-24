# Rivet variant handling: design study for Gale

Status: research, 2026-04-23. Baseline: `rivet 0.4.3 (2d1a16b2)`.
Prototype files in this worktree: `artifacts/feature-model.yaml`,
`artifacts/variants/bindings.yaml`, `artifacts/variants/*.yaml`.

## 1. What rivet offers today

Rivet v0.4.3 ships a first-class product-line subsystem under
`rivet variant *`, backed by three YAML documents:

1. **Feature model** (problem space) — a FODA-style feature tree with
   group types (`mandatory`, `optional`, `alternative`, `or`, `leaf`),
   typed-ish per-feature `attributes`, and s-expression cross-tree
   constraints (`implies`, `excludes`, `(and …)`, `(or …)`, plus the
   rivet artifact-query palette).
2. **Variant configuration** — a user-level selection
   (`name:` + `selects: [feature, …]`).
3. **Binding model** — `bindings: { feature: { artifacts: [ID,…], source: [glob,…] } }`.

Driving commands, from rivet's own help output:

- `rivet variant init <name>` — scaffolds starter FM + bindings.
- `rivet variant list --model FM` — tree dump.
- `rivet variant check --model FM --variant V` — PASS/FAIL solve.
- `rivet variant check-all --model FM --binding B` — iterates
  every variant declared inline under `variants:` in the binding file.
- `rivet variant solve ... --binding B` — solved feature set plus
  bound artifact IDs and origin tags (`mandatory | selected |
  implied by`).
- `rivet variant features ... --format env|cargo|cmake|cpp-header|bazel|make|json`
  — emits the effective feature set as a build-system fragment.
  This is the key capability for CI driving: it produces
  `export RIVET_FEATURE_SEM=1`, `RIVET_ATTR_SEM_KCONFIG=CONFIG_GALE_KERNEL_SEM`,
  etc., directly consumable by a shell step.
- `rivet variant manifest ... --binding B` — enumerates the source globs
  that participate in this variant (answers "what went into the build?").
- `rivet variant explain ... [feature]` — audit trail of why each feature
  was selected or suppressed.
- `rivet validate --model FM --variant V --binding B` — runs normal
  artifact validation *restricted* to the bound artifact set; JSON
  output carries a `variant: { name, resolved_artifacts, bound_artifacts }`
  block.

Docs: `/Users/r/git/pulseengine/rivet/docs/feature-model-schema.md`,
`feature-model-bindings.md`, `pure-variants-comparison.md`.

### Smallest valid feature-model + variant that rivet accepts

```yaml
# fm.yaml
kind: feature-model
root: gale
features:
  gale:
    group: or
    children: [sem, mutex]
  sem:   { group: leaf }
  mutex: { group: leaf }
```

```yaml
# v.yaml
name: sem-only
selects: [sem]
```

`rivet variant check --model fm.yaml --variant v.yaml` reports `PASS`.
`rivet variant features --model fm.yaml --variant v.yaml --format env`
prints `export RIVET_FEATURE_SEM=1` plus the mandatory root.

## 2. Gale's variant axes

| Axis | Cardinality | Source of truth today | Example values |
|------|-------------|------------------------|----------------|
| Primitives (`CONFIG_GALE_KERNEL_*`) | 33, each on/off | `zephyr/Kconfig`, `ffi/Cargo.toml` `[features]`, `zephyr/gale_overlay.conf` | `SEM`, `MUTEX`, `CONDVAR`, `MSGQ`, `TIMER`, `MEM_SLAB`, `HEAP`, `SCHED`, `SMP_STATE`, … |
| Board | 6, pick one | `.github/workflows/{zephyr-tests,renode-tests,llvm-lto}.yml` matrices | `qemu_cortex_m3`, `qemu_cortex_m33`, `qemu_x86_64`, `stm32f4_disco`, `nucleo_l552ze_q`, `qemu_cortex_r5` |
| Toolchain | 5, pick one | `.github/workflows/llvm-lto.yml`, `zephyr-tests.yml` | GCC `-Os`, GCC `-O2`, LLVM no-LTO, LLVM+LTO, Clang/libc++ |
| Overlays | 5+, optional subset | `zephyr/gale_*overlay.conf`, `benches/engine_control/prj-*.conf` | `gale_overlay`, `gale_smp_overlay`, `gale_lto_overlay`, `gale_profiling_overlay`, `ctf_tracing` |
| Heap hardening | 3, pick at most one | proposed `CONFIG_SYS_HEAP_HARDENING_{BASIC,MODERATE,FULL}` | — |

Secondary axes worth modelling later: cargo profile (`release` vs
`release-lto`), userspace on/off (depends on `CONFIG_USERSPACE`),
fuzz/sanitiser overlays, STPA UCA coverage targets.

## 3. Proposed model structure

- `artifacts/feature-model.yaml` — single top-level FM for Gale.
  Living alongside the existing artifact YAMLs keeps "one project,
  one variant space" and lets `rivet validate --model artifacts/feature-model.yaml`
  work without extra flags. **Note:** rivet's generic-yaml loader today
  tries to parse every `artifacts/**/*.yaml` as an artifact file and
  emits a `WARN ... unknown field kind/name/variants` on the FM,
  binding, and variant files. Proposal: either move the FM/variants
  under a nested directory that `rivet.yaml` excludes, or add an
  explicit include/exclude in `rivet.yaml`'s `sources:` block (the
  latter requires confirming rivet supports exclude patterns — quick
  CLI check shows no flag, needs a follow-up issue).
- `artifacts/variants/bindings.yaml` — combined bindings + named
  `variants:` block for `check-all`.
- `artifacts/variants/<name>.yaml` — one file per named variant, so a
  PR that touches a single CI lane touches a single file and reviewers
  can see the intent diff.

Representative 40-line excerpt from `artifacts/feature-model.yaml`:

```yaml
kind: feature-model
root: gale
features:
  gale:
    group: mandatory
    children: [primitives, board, toolchain, overlays]

  primitives:
    group: or
    children: [sem, mutex, condvar, msgq, timer]
  sem:
    group: leaf
    attributes:
      kconfig: "CONFIG_GALE_KERNEL_SEM"
      cargo_feature: "sem"
      ztest_path: "tests/kernel/semaphore/semaphore"
  mutex:
    group: leaf
    attributes:
      kconfig: "CONFIG_GALE_KERNEL_MUTEX"
      cargo_feature: "mutex"

  board:
    group: alternative
    children: [qemu_cortex_m3, qemu_x86_64, stm32f4_disco]
  qemu_cortex_m3:
    group: leaf
    attributes: { west_board: "qemu_cortex_m3", runner: "qemu", smp_capable: "false" }
  qemu_x86_64:
    group: leaf
    attributes: { west_board: "qemu_x86_64", runner: "qemu", smp_capable: "true" }
  stm32f4_disco:
    group: leaf
    attributes: { west_board: "stm32f4_disco", runner: "renode" }

  toolchain:
    group: alternative
    children: [gcc_os, llvm_lto]

constraints:
  - (implies gale_lto_overlay llvm_lto)
  - (implies gale_smp_overlay qemu_x86_64)
```

Attribute names (`kconfig`, `cargo_feature`, `west_board`,
`overlay_conf`, `ztest_path`, `runner`, …) are the CI-driver contract:
the workflow step reads them back via `rivet variant features`.

## 4. Binding to existing artifacts

```yaml
bindings:
  sem:
    artifacts: [SWREQ-SEM-P01, SWREQ-SEM-P02, UV-SEM-001, IV-SEM-001, FV-SEM-001]
    source:    ["src/kernel/sem.rs", "ffi/src/sem.rs", "proofs/sem/**"]
```

`rivet variant solve --binding artifacts/variants/bindings.yaml` then
prints `Bound artifacts (6): SWREQ-SEM-P01 …` for the `sem-m3-llvm-lto`
variant — the coverage answer to "which artifacts does this lane
exercise?". With `rivet validate --variant ...`, the JSON block
`"variant": { "bound_artifacts": 6, "resolved_artifacts": 6 }`
confirms all bound IDs exist in the project.

## 5. CI driver feasibility

**Verdict: PARTIAL.** Rivet can already emit the selection into seven
build-system formats, and the `attributes` scheme lets us carry every
piece of metadata a CI step needs (Kconfig symbol, west board, overlay
path, ztest path, cargo feature). A workflow step like

```sh
eval "$(rivet variant features --model ... --variant $V --format env)"
west build -b "$RIVET_ATTR_QEMU_CORTEX_M3_WEST_BOARD" \
  -- -DEXTRA_CONF_FILE="$RIVET_ATTR_GALE_OVERLAY_OVERLAY_CONF"
```

is possible *today* per variant. Evidence: `rivet variant features
--format env` on `sem-m3-gcc` emits 21 well-formed `export` lines
(see log from empirical probe).

What does not exist today:

- No `rivet list --variant V` flag to emit the filtered artifact set
  (the `filter` s-expression route does not know about variants).
- No way to ask "which CI lanes cover feature X?" — the binding only
  maps features to artifact IDs, not to variant names. A GH Actions
  matrix generator has to iterate `rivet variant check-all` output
  and filter on attributes client-side.
- No `rivet variant ls` that emits just the declared variant names
  (needed to feed `strategy.matrix.include:` without hand-parsing
  `check-all` text).
- Attributes are untyped scalar strings; a boolean `smp_capable`
  round-trips as the string `"false"` in env emit, which shells
  happily treat as truthy.

## 6. Gaps found in rivet itself

The pure::variants comparison (rivet's own
`docs/pure-variants-comparison.md`) already enumerates five gaps. The
ones that bite Gale specifically:

1. **No typed attributes** (their Gap 1). `smp_capable: "false"` vs
   `false` vs `0` all parse; a CI driver relies on the string form.
2. **No `rivet list --variant` or `rivet variant artifacts` subcommand**
   that prints the flattened ID list of a solved variant. Today
   `rivet variant solve` prints it to text; JSON-shaped output for CI
   consumption requires either parsing that text or invoking `rivet
   validate --format json` and scraping `variant.bound_artifacts`.
3. **No variant matrix emitter**. To drive a GH Actions matrix from the
   variant set we need `rivet variant matrix --format github-actions`
   that outputs `{ "include": [ {name, board, toolchain, …}, … ] }`.
4. **No exclude pattern in `rivet.yaml` `sources:`** (or it is not
   documented). Placing FM/variants under `artifacts/` makes the
   generic-yaml loader warn. Workaround: nest under a path not
   enumerated in `sources:`, or use `artifacts/variants/` and rely on
   tolerant warnings — but that pollutes every validate run.
5. **No group cardinality ranges** (their Gap 4). "Pick 2-of-3
   overlays for the hardening test" is not expressible; has to be
   encoded as an `or` group plus a cross-tree constraint.

## 7. Suggested follow-up issues

1. **rivet**: add `rivet variant matrix --format {github-actions,json}`
   that emits the named-variant list with their attribute projection —
   the one missing piece for end-to-end CI driving.
2. **rivet**: support `sources: { include: [...], exclude: [...] }` in
   `rivet.yaml`, or auto-skip files whose `kind:` is `feature-model` /
   whose top-level key is `bindings:` / `variants:` / `selects:`.
3. **gale**: once (1) lands, replace the hand-enumerated
   `strategy.matrix.include:` blocks in `zephyr-tests.yml`,
   `llvm-lto.yml`, and `renode-tests.yml` with a single
   `rivet variant matrix` invocation in a setup job.

## 8. Files left behind in this worktree

All uncommitted.

- `artifacts/feature-model.yaml` — prototype Gale feature model
  (30 features, 5 constraints, 5 representative primitives).
- `artifacts/variants/bindings.yaml` — feature→artifact binding +
  6 named variants for `rivet variant check-all`.
- `artifacts/variants/sem-m3-gcc.yaml` — baseline CI lane (qemu_cortex_m3 + GCC).
- `artifacts/variants/sem-m3-llvm-lto.yaml` — LLVM+LTO lane.
- `artifacts/variants/sem-smp-x86.yaml` — SMP lane.
- `artifacts/variants/sem-renode-stm32f4.yaml` — Renode HIL lane.
- `docs/research/rivet-variant-handling.md` — this document.

Every prototype has been validated with live rivet invocations:
`rivet variant list`, `check`, `check-all`, `solve --binding`,
`features --format {env,cmake,bazel}`, `manifest`, and
`validate --model --variant --binding --format json`. No rivet state
was mutated; all commands are read-only. The known warnings about
rivet's artifact loader treating FM/variant YAMLs as artifacts is
benign for `validate` (result stays `PASS`) but noisy — captured as
follow-up issue #2 above.

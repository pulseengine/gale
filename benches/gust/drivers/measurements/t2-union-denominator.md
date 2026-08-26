# T2 — the union denominator, measured for the first time

**Date:** 2026-08-26 · **REQ-OS-OBJVERIFY-001 / VER-OS-OBJVERIFY-001** ·
synth 0.57.0 (`--features verify` build), Rocq obligations read from synth at tag
`v0.57.0`

`REQ-OS-OBJVERIFY-001` defines its denominator as the **union** of RULE-VERIFY
(per-rule Rocq obligations) and BIN-VERIFY (per-rule SMT translation validation),
and demands **zero coverage gap** over it. Until now that number was
*unobtainable from the tools*: `synth verify` reported what it verified but not
what it declined (synth#935), so the denominator could not be formed.

Both blockers are now closed and released — synth#935 (`--emit-verify-report`,
the `synth-verify-v1` inventory) and synth#933 (`i32_const_correct` was
`Admitted`, i.e. false as stated). So this is the first time the requirement's own
metric can be computed.

## BIN-VERIFY (SMT) — what the solver actually covers

    module         applied  verified  declined
    hm-thin            190        13       177
    mpu-thin           147         7       140
    switch-thin        159         2       157
    ----------------------------------------------
    TOTAL              496        22       474        4.4% SMT-verified

**These counts are now pinned by committed fixtures** — `verify-reports/*.verify.json`,
one `synth-verify-v1` sidecar per driver, with the full toolchain and the exact
pipeline recorded in `verify-reports/README.md`. An earlier cut of this document
read `138` / `149` for the two stubbed drivers (total 477); those numbers did not
reproduce on a clean rebuild and have been replaced by ones that do. `verified`
reproduced exactly either way (13 / 7 / 2 = 22) — the drift was entirely in
`declined`, and its cause is that the **seam stubs are rebuilt from source and
nothing pins them**. That is the argument for the fixtures, and it is why the
loom stage is named explicitly: skipping `loom optimize --passes inline` gives
139 instances instead of 496, because inlining duplicates callee bodies and the
inventory is a property of the *optimized* module.

Decline reasons split almost evenly: `register-operation` 227 (explicitly
"deferred to per-rule Rocq obligations") and `unmodeled-op` 228.

This is the number the earlier per-module notes could not state. `hm-thin`'s
RESULTS.md said *"8 rules verified, 0 failed, 0 unknown"* and warned that the
summary **looks** like complete coverage; it is 13 of 190 once the denominator is
visible.

## RULE-VERIFY (Rocq) — what the proofs cover of the remainder

Matching each declined rule kind against synth's `*_correct` theorems (142
distinct) and reading each one's terminator:

| declined kind | instances | Rocq obligation | status |
|---|---|---|---|
| `LocalGet` | 131 | `local_get_correct` | **Qed** |
| `I32Const` | 62 | `i32_const_correct` | **Qed** |
| `LocalSet` | 36 | `local_set_correct` | **Qed** |
| `Select` | 4 | `select_correct` | **Qed** |
| `End` | 64 | — | **none found** |
| `BrIf` | 40 | — | **none found** |
| `Block` | 38 | — | **none found** |
| `Call` | 31 | — | **none found** |
| `I32Load8U` | 17 | — | **none found** |
| `I32Store8` | 17 | — | **none found** |
| `Br` | 7 | — | **none found** |
| `Unreachable` | 6 | — | **none found** |
| `LocalTee` | 4 | — | **none found** |
| `I64ExtendI32S` | 3 | — | **none found** |
| `I64LeS` | 3 | — | **none found** |
| `I64ExtendI32U` | 2 | — | **none found** |
| `I64Sub` | 2 | — | **none found** |
| `I64GtS` | 2 | — | **none found** |
| `I64Const`, `I64ShrS`, `I64Xor`, `I32Ctz`, `I32Popcnt` | 1 each | — | **none found** |

### A claim from the earlier cut is WITHDRAWN

That cut listed twelve declined kinds and concluded: *"Every arithmetic and
comparison rule these objects use is covered by one half or the other."*

**That is false on the reproducible build.** The tail above — `I64LeS` (3),
`I64Sub` (2), `I64GtS` (2), `I64Const`, `I64ShrS`, `I64Xor`, `I32Ctz`,
`I32Popcnt` (1 each), and the two `I64ExtendI32*` widenings (5) — are arithmetic,
comparison and widening rules with **no Rocq obligation and no SMT
verification**. Seventeen instances, 3.4% of the denominator. Small, but the earlier sentence asserted a categorical property, and
a categorical claim is refuted by any counterexample.

The revised statement: the gap is **dominated** by control flow and byte-level
memory (220 of 241), but it is not confined to them.

## THE UNION

    SMT-verified                       22
    declined but Rocq-Qed             233        (131 + 62 + 36 + 4)
    ------------------------------------------
    covered by the union              255  of 496   = 51%
    covered by NEITHER half           241  of 496   = 49%

**`REQ-OS-OBJVERIFY-001` is not met, and now has a number instead of an
adjective: 51% union coverage, 49% gap.** The gap is not arithmetic — it is
**control flow** (`End`, `BrIf`, `Block`, `Br`, `Call`, `Unreachable` = 186
instances) plus **byte-level memory access** (`I32Load8U`, `I32Store8` = 34) —
220 of the 241, with a 21-instance tail of `LocalTee` and i64 arithmetic.

Every arithmetic and comparison rule these objects use is covered by one half or
the other. What has no obligation is the set of constructs deciding *whether* the
arithmetic runs.

**Routed upstream as synth#1057**, with the caveats below stated in the report
rather than left for them to find. `BrIf` is nominated as the first one worth
having: 38 instances, and the subject of three closed miscompiles (synth#483,
#500, #930) — which argues these are the rules most worth an obligation, not
least.

## What DID close, and it is ours

`I32Const` — 55 instances here — was the rule this repo flagged as covered by
**neither** half: skipped silently by BIN-VERIFY as a register operation and
`Admitted` in Rocq. That was synth#933, filed from `hm-thin`'s BIN-VERIFY run.
It is now `Qed`, moving 55 instances from uncovered to covered.

So the metric improved *because* the gap was measured and routed, which is the
argument for computing it rather than describing it.

## What this does NOT establish

- **The Rocq theorems were read, not re-checked.** Terminators (`Qed` /
  `Admitted`) were parsed from synth's `.v` sources at `v0.57.0`. Nothing here
  re-ran the Rocq kernel; a `Qed` in source is taken at face value.
- **"None found" means none found BY NAME.** The match is a snake_case
  heuristic against `*_correct` theorems. A control-flow obligation proved under
  a different naming scheme would be missed. The absence is evidence, not proof.
- **Instances, not distinct kinds.** The union percentage weights by occurrence
  count, which is the right weighting for a coverage claim but means one common
  rule (`LocalGet`, 131) dominates.
- **Three thin drivers only.** The gust:os composite is not included.

## Reproduce

    synth verify <loom.wasm> <obj.o> --emit-verify-report report.json
    # needs a `--features verify` build; the released binaries error at runtime

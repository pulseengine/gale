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
    mpu-thin           138         7       131
    switch-thin        149         2       147
    ----------------------------------------------
    TOTAL              477        22       455        4.6% SMT-verified

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
| `I32Const` | 55 | `i32_const_correct` | **Qed** |
| `LocalSet` | 36 | `local_set_correct` | **Qed** |
| `Select` | 4 | `select_correct` | **Qed** |
| `End` | 60 | — | **none found** |
| `BrIf` | 38 | — | **none found** |
| `Block` | 36 | — | **none found** |
| `Call` | 31 | — | **none found** |
| `I32Load8U` | 15 | — | **none found** |
| `I32Store8` | 15 | — | **none found** |
| `Br` | 7 | — | **none found** |
| `Unreachable` | 6 | — | **none found** |

## THE UNION

    SMT-verified                       22
    declined but Rocq-Qed             226        (131 + 55 + 36 + 4)
    ------------------------------------------
    covered by the union              248  of 477   = 52%
    covered by NEITHER half           229  of 477   = 48%

**`REQ-OS-OBJVERIFY-001` is not met, and now has a number instead of an
adjective: 52% union coverage, 48% gap.** The gap is not arithmetic — it is
**control flow** (`End`, `BrIf`, `Block`, `Br`, `Call`, `Unreachable` = 178
instances) plus **byte-level memory access** (`I32Load8U`, `I32Store8` = 30).

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

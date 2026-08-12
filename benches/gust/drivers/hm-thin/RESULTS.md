# hm-thin — the isolation core, dissolved. And an honest coverage report

**Date:** 2026-08-07 · **T2 / REQ-OS-OBJVERIFY-001 stage 2** · meld 0.41.3, loom 1.2.0,
synth 0.52.0 built with `--features verify`

`REQ-OS-OBJVERIFY-001` needs the isolation core lowered by a compiler that carries
per-rule obligations. `health_monitor.rs` is the cheapest possible first step: **zero
hardware seams**, pure scalar predicates. If it could not dissolve, nothing in the
isolation core could.

## It dissolves

    plain/src/health_monitor.rs   (Verus + Kani verified — bodies lifted VERBATIM)
      -> cargo wasm32              2 401 B
      -> wasm-tools component new  3 956 B
      -> meld fuse --memory shared
      -> loom optimize --passes inline
      -> synth compile --target cortex-m3 --all-exports --relocatable

    text 1054   data 0   bss 0        undefined symbols: none

Zero SRAM, and correctly **no** undefined symbols — unlike the fused OS composite,
where an empty undefined set is the failure mode, `hm-thin` genuinely has no seam to
preserve. It computes over scalars and returns.

## BIN-VERIFY runs on it

    6 functions · 8 rules verified · 0 failed · 0 unknown · 0 declined
    rules: i32.and · i32.eqz · i32.gt_s · i32.le_s · i32.le_u · i32.sub

## THIS IS NOT A ZERO-GAP RESULT, AND THE SUMMARY LOOKS LIKE ONE

The line above reads as complete coverage. It is not, and the discrepancy is the
whole reason `REQ-OS-OBJVERIFY-001` defines its denominator as the **union** of
RULE-VERIFY and BIN-VERIFY rather than as whatever the tool prints.

**The object contains 29 `i32.const`.** BIN-VERIFY classes Const as a register
operation and skips it *silently* — it is not in the verified list and not in a
declined list, because there is no declined list. So:

| rule | BIN-VERIFY | RULE-VERIFY (Rocq) | covered? |
|---|---|---|---|
| `i32.and`, `i32.eqz`, `i32.sub` | verified | `Qed` | yes, both halves |
| `i32.gt_s`, `i32.le_s`, `i32.le_u` | verified | **no theorem exists** | yes, by BIN-VERIFY only |
| **`i32.const`** (×29) | **skipped, unreported** | **`Admitted`** | **NO — neither half** |

`i32_const_correct` is Admitted because it is false as stated for un-normalized
operands; the supporting arithmetic (`i32_const_large_reconstruct`,
`movw_movt_reconstruct_Z`) is `Qed`. Filed as **synth#933**.

That the gap is invisible in the tool's own output is filed separately as
**synth#935** — a consumer cannot compute a coverage denominator from `synth verify`,
because it never reports the rules it declined.

**So: zero-gap CANNOT be claimed for this object.** One rule, named, filed, and
appearing 29 times.

## What this does establish

- The construction works. Verified Rust lifted verbatim into a wasm component
  dissolves to a native object, and translation validation runs on the result.
- The isolation core is **not** blocked on a rewrite. All three modules are already
  thin-seam shaped; this one simply had no seam at all.
- Instruction selection, register allocation and scheduling for this object are now
  performed by a compiler that carries obligations, instead of by LLVM.

## What it does NOT establish

- **Not the full evidence-on-wasm set.** `REQ-OS-OBJVERIFY-001` requires witness
  MC/DC with zero unresolved gap rows, scry's verdicts and gap report, and the
  `REQ-OS-HM-001` oracles **re-run against the wasm build**.
  - witness MC/DC **has now been run** — `../measurements/switch-thin-mcdc.md`,
    hm-thin section — and it is **not** zero-gap: 0/2 decisions full MC/DC,
    1 proved / 4 gap / 3 dead. It also shows MC/DC is close to vacuous here: six
    of the seven predicates compile to zero branches, so their correctness rests
    entirely on the Verus/Kani proofs, not on structural coverage.
  - **scry has not been run**, and the `REQ-OS-HM-001` oracles have **not** been
    re-run against the wasm build.
  Without those, BIN-VERIFY shows the lowering is faithful but not that the wasm
  refines the Verus-proven Rust.
- **Not executed.** Nothing has run this object.
- **Not the switch.** `partition_switch` (3 seams) and `mpu_switch` (1 seam) are the
  remaining stage-2 work; HM was chosen first precisely because it is the easy case.
- **7 exported predicates became 6 functions** in the object. Presumed inlining;
  not yet confirmed, and stated here rather than left unnoticed.

## Reproduce

    cd benches/gust/drivers/hm-thin && cargo build --release --target wasm32-unknown-unknown
    wasm-tools component new target/wasm32-unknown-unknown/release/gust_hm_thin.wasm -o hm.component.wasm
    meld fuse hm.component.wasm --memory shared -o hm.fused.wasm
    loom optimize hm.fused.wasm --passes inline --attestation false -o hm.loom.wasm
    synth compile hm.loom.wasm --target cortex-m3 --all-exports --relocatable -o hm-thin-cm3.o
    synth verify hm.loom.wasm hm-thin-cm3.o     # needs a --features verify build

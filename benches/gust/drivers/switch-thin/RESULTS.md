# switch-thin — the last isolation-core module, and the whole core fused

**Date:** 2026-08-11 · **T2 / REQ-OS-OBJVERIFY-001 stage 2, final module** ·
meld 0.41.3, loom 1.2.0, synth 0.52.0 (`--features verify` build for BIN-VERIFY)

Third and last module onto the dissolve path, after `hm-thin` (0 seams) and
`mpu-thin` (1 seam). This one carries **three** — and carries the property the whole
track exists to protect.

## It dissolves, and all three seams survive

    plain/src/partition_switch.rs   (Verus + Kani verified — lifted VERBATIM)
      -> wasm 8 903 B -> component 10 943 B -> meld -> loom -> synth

    text 5220   data 376   bss 2484        SRAM 2 860 B of 8 192 (34%)
    undefined: [ ctx-resume · ctx-save · region-swap ]

Exactly the three declared atoms. Lifted verbatim: `MAX_WINDOWS`, `SwPhase`,
`MajorFrame` (+ `check`, `current_window`), `Switcher` (+ `new`, `tick`,
`mark_saved`, `mark_swapped`, `mark_resumed`, `run_switch`) and the three
`seam_*` wrappers.

**The non-maskability property survives because it is a property of the contract's
shape, not of the code.** The module's own doc says it: there is no transition that
leaves the pipeline early and no input that suppresses entering it at a window
boundary — *"the wdg-thin cannot-un-start construction applied to preemption."* An
absence cannot be compiled away. It is the same argument slide 11 of the Research Day
deck makes about `wdg`: the contract cannot express the bug.

What crosses into wasm is the **policy** — boundary detection, phase ordering, window
sequencing. Context save/restore stays native because it touches CPU state wasm
cannot name. That split was already in the source; the dissolve did not invent it.

## THE RESULT — the whole isolation core, fused

    hm.component + mpu.component + switch.component
      -> meld fuse --memory shared   (6 components -> 14 413 B, -38.3%)
      -> loom optimize -> synth --native-pointer-abi --shadow-stack-size 2048

    text 9284   data 1016   bss 2688

    SRAM   3 704 B of the STM32F100RB's 8 192   (45%)   — 4 488 B FREE
    FLASH  9 284 B of 131 072                    (7%)
    seams  [ ctx-save · region-swap · ctx-resume · mpu-write ]

**Four native atoms for the entire isolation core.** The same four identified by
reading the source before any of this was built — no drift, nothing undeclared added
across three modules and a fusion.

### Fusion is what makes it fit

Built separately each module carries its own 2 KB shadow stack:

| built separately | SRAM |
|---|---|
| `mpu-thin` | 3 324 B |
| `switch-thin` | 2 860 B |
| `hm-thin` (with its own stack) | ~2 688 B |
| **total** | **~8 872 B — over the 8 192 budget** |

| fused | SRAM |
|---|---|
| **iso-core-fused** | **3 704 B — fits, 4 488 B free** |

An earlier note in `mpu-thin/RESULTS.md` flagged 40.6% for one module as a
constraint. That figure is correct **per module** and per-module is not how this
ships. Fused, they share one linear memory and one shadow stack. This is the
composition argument earning its keep on the part that sets the constraints.

## BIN-VERIFY

    switch-thin: 2 rules verified, 0 failed, 0 unknown   (rules: i32.eqz)

Low, and honestly so: most of this FSM is comparisons and branches over locals and
constants — precisely the class BIN-VERIFY declines as register operations. Which
means the const gap dominates here more than anywhere:

**Not a zero-gap result.** `i32_const_correct` is `Admitted` (synth#933) and
`synth verify` does not report what it declines (synth#935), so the true denominator
is unobtainable from the tool. Recorded rather than glossed.

## What this does NOT establish

- **No evidence-on-wasm.** No witness MC/DC, no scry, and the `REQ-OS-SWITCH-001`
  oracles have not been re-run against the wasm build. This shows the lowering is
  faithful; it does not show the wasm refines the Verus-proven Rust.
- **Nothing has executed any of these objects** — not on silicon, not under Renode.
  The Kani harness already substitutes the seam (`run_switch`'s FFI calls cannot be
  linked), so execution evidence is a separate obligation.
- **The four seams remain trusted native code**, including `mpu_write`'s
  barrier-pairing contract and the register save/restore.
- **The shadow-stack budget is asserted, not proven** — the `2048 of 8192` gap.
- **`REQ-OS-OBJVERIFY-001` stays `proposed`.** Three modules dissolve; the evidence
  set the requirement demands is not yet gathered.

## Reproduce

    cd benches/gust/drivers/switch-thin && cargo build --release --target wasm32-unknown-unknown
    wasm-tools component new target/wasm32-unknown-unknown/release/gust_switch_thin.wasm -o sw.component.wasm
    # the fused core:
    meld fuse hm.component.wasm mpu.component.wasm sw.component.wasm --memory shared -o iso.fused.wasm
    loom optimize iso.fused.wasm --passes inline --attestation false -o iso.loom.wasm
    synth compile iso.loom.wasm --target cortex-m3 --all-exports --relocatable \
      --native-pointer-abi --shadow-stack-size 2048 -o iso-core-fused-cm3.o

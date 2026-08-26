# switch-thin — the last isolation-core module, and the whole core fused

**Date:** 2026-08-11 · **T2 / REQ-OS-OBJVERIFY-001 stage 2, final module** ·
meld 0.41.3 (per-module) / **0.48.0** (the fused core), loom 1.2.0,
synth **0.57.0** (the fused core and its execution; per-module figures below
are from the 0.52.0 build and are unchanged), `--features verify` build for BIN-VERIFY

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

    hm.component + mpu.component + switch.component     (--emit-relocs, __heap_base)
      -> meld 0.48 fuse --memory shared --pack-rebase --share-stack
      -> loom optimize -> synth --native-pointer-abi

    text 8700   data 1096   bss 3140

    SRAM   4 236 B of the STM32F100RB's 8 192   (51%)   — 3 956 B FREE
    FLASH  8 712 B of 131 072                    (7%)
    seams  [ ctx-save · region-swap · ctx-resume · mpu-write ]
    data segments: 8, ALL DISJOINT

Built by `../build-iso-core.sh`, which gates on both.

### These numbers replace a build that was smaller and WRONG

The earlier figure was `text 9284 / data 1016 / bss 2688 — SRAM 3 704 B (45%)`,
from `meld fuse --memory shared`. That path merges memories **without rebasing**,
so all three components placed static data at the same base: the fused module had
**four overlapping data-segment pairs**, one of them mpu-thin's 320-byte
`RegionTable` over switch-thin's `.data` (gale#266).

It shipped anyway because it was the only build that FIT. The correct alternatives
were 16x to 272x over the 8 KB budget — page-granular rebasing costs 64 KB per
component, and these three hold 1 088 B of real state between them.

meld 0.48.0 closed that, in two parts we asked for on meld#370:

- **`--pack-rebase`** places each component at its true static extent — which needs
  `--emit-relocs` *and* a retained `__heap_base`, both now enforced by
  `../build-reloc-cores.sh`.
- **`--share-stack`** collapses the three per-component 2 KB shadow stacks into
  one. That was 6 144 B of the 7 236 B packed extent — 85% of the footprint was
  stack, holding 1 088 B of state.

So the correct build is now **532 B larger than the incorrect one, and both fit**.

meld 0.48 also **refuses** the old path outright rather than emitting silently
corrupt output — *"overlapping data segments in fused output (3 overlapping
pair(s)) … this silently corrupts data"*. `check-data-overlap.py` stays as
defence-in-depth and is a hard gate in the build script (verified to exit 5 on a
module produced by the older meld).

**Four native atoms for the entire isolation core.** The same four identified by
reading the source before any of this was built — no drift, nothing undeclared added
across three modules and a fusion.

### Fusion is what makes it fit — and `--share-stack` is what makes it correct

Built separately each module carries its own 2 KB shadow stack:

| built separately | SRAM |
|---|---|
| `mpu-thin` | 3 324 B |
| `switch-thin` | 2 860 B |
| `hm-thin` (with its own stack) | ~2 688 B |
| **total** | **~8 872 B — over the 8 192 budget** |

| fused | SRAM | |
|---|---|---|
| `--memory shared` (overlapping, withdrawn) | 3 704 B | fits, but **corrupt** |
| **`--pack-rebase --share-stack`** | **4 236 B** | **fits, DISJOINT, 3 956 B free** |

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

- **Partial evidence-on-wasm.** witness MC/DC **has now been run** —
  `../measurements/switch-thin-mcdc.md`: **3/22 decisions full MC/DC, 13 proved /
  11 gap / 51 dead**, plus the repo's first WASM→object disposition (98 branches,
  42 obligation-stands, 56 no-provenance, 9 only-in-synth). Far from zero-gap: two
  thirds of this module's conditions are never evaluated, and 53 of them are `u64`
  formatting reachable only from `panic_bounds_check` on `self.cur` — an index
  `Switcher`'s invariant already bounds. See that document for the −54.4% `.text`
  this costs, recorded as a measurement rather than a change.
  **scry has not been run**, and the `REQ-OS-SWITCH-001` oracles have **not** been
  re-run against the wasm build. This shows the lowering is faithful; it does not
  show the wasm refines the Verus-proven Rust.
- **Executed under qemu, not on silicon and not under Renode.** `gust_iso_probe`
  runs this object on qemu lm3s6965evb (real v7-M): **all 9 checks pass**,
  including the seam ORDER observed rather than assumed, and cross-component
  non-interference. This was 8 of 9 until the synth pin moved 0.52.0 -> 0.57.0:
  the one failure was `iso-frame-bad`, the `set-window` miscompile (gale#270).
  That is now closed by execution rather than by an upstream changelog — same
  source, same meld 0.48.0, same loom 1.2.0, only `$SYNTH` changing, measured
  both ways in `../measurements/synth-pin-0.57.md`. The seams are recorded, not performed, exactly as the Kani harness
  substitutes them.
- **The four seams remain trusted native code**, including `mpu_write`'s
  barrier-pairing contract and the register save/restore.
- **The shadow-stack budget is asserted, not proven** — the `2048 of 8192` gap.
- **`REQ-OS-OBJVERIFY-001` stays `proposed`.** Three modules dissolve; the evidence
  set the requirement demands is not yet gathered.

## Reproduce

    # the fused core — one command, both gates (disjointness + the exact seam set):
    benches/gust/drivers/build-iso-core.sh

    # and execute it:
    cd benches/gust && cargo build --release --bin gust_iso_probe --target thumbv7m-none-eabi
    qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic \
      -semihosting-config enable=on,target=native \
      -kernel target/thumbv7m-none-eabi/release/gust_iso_probe

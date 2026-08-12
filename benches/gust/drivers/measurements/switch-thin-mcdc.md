# Evidence-on-wasm for switch-thin — the first MC/DC + object-disposition run

**Date:** 2026-08-11 · **T2 / REQ-OS-OBJVERIFY-001, the evidence-on-wasm leg** ·
witness 0.39.0, meld 0.41.3, loom 1.2.0, synth 0.52.0

Every stage-2 `RESULTS.md` closes with the same admission: *"No evidence-on-wasm.
No witness MC/DC, no scry, and the oracles have not been re-run against the wasm
build."* `REQ-OS-OBJVERIFY-001` was re-worded to **require** that set. This is the
first time any of it has been gathered on a dissolved gale artifact.

It is not a clean result, and that is the point of running it.

## The headline

    witness report --format mcdc-rollup      (84 invocations, all 9 exports)

    overall: 3/22 decisions full MC/DC
             conditions: 13 proved, 11 gap, 51 dead   (75 total)

**Two thirds of the conditions in the shipped isolation-core module are dead** —
never evaluated by a vector set that drives every export, every FSM phase, the
window-wrap, and a 10-vector sweep of `MajorFrame::check`.

Per source file:

| file | decisions | full MC/DC | proved | gap | dead |
|---|---|---|---|---|---|
| `mod.rs` | 6 | 3 | 8 | 1 | 5 |
| **`lib.rs`** (switch-thin itself) | **4** | **0** | **2** | **7** | **2** |
| `macros.rs` | 4 | 0 | 2 | 1 | 11 |
| `<meld-adapter>` | 3 | 0 | 0 | 0 | 16 |
| `count.rs` | 2 | 0 | 0 | 0 | 12 |
| `num.rs` | 2 | 0 | 0 | 0 | 4 |
| `option.rs` | 1 | 0 | 1 | 1 | 0 |
| `wit_bindgen_cabi_realloc.rs` | 1 | 0 | 0 | 1 | 1 |

### Read the percentage carefully — it is not the MC/DC number

`witness report` with no `--format` prints `coverage: 30/98 (30.6%)`. That counts
branches **reached** (`hits > 0`); it is not both-outcome coverage and it is not
MC/DC. The MC/DC verdict is `--format mcdc`, and it is the stricter 3/22 above.
Quoting the 30.6% as a coverage result would have overstated the evidence.

## Where the dead conditions live — named, not guessed

`meld fuse --preserve-names` keeps the name section (without it every row reads
`(anon)` and the gaps are unattributable). Branch counts by function:

| function | branches | reached |
|---|---|---|
| `core::fmt::Formatter::pad_integral` | 25 | **0** |
| `core::str::count::do_count_chars` | 20 | **0** |
| `<u64 as Display>::fmt` | 5 | **0** |
| `pad_integral::write_prefix` | 3 | **0** |
| `wit_bindgen::rt::cabi_realloc` ×2 | 3 + 3 | **0** |
| switch-thin's own code (10 fns) | 37 | 28 |
| `MajorFrame::check` | 8 | 8 |

**53 of the 68 unreached branches are `u64` decimal formatting.** A partition
switch in a 3.7 KB isolation core carries an integer-to-string formatter it can
never execute.

`cabi_realloc` appearing **twice** independently reproduces **loom#303**
("Canonical-ABI glue survives the dissolve … `cabi_realloc` (x2) is never removed").

### Root cause: a proven invariant that never reaches the compiler

The formatter is dragged in by `panic_bounds_check` — two array-index bounds
checks on `self.cur`. `cur < MAX_WINDOWS` is exactly what the Verus/Kani proof
establishes and what `Switcher`'s invariant maintains, but nothing communicates
that fact to LLVM, so it emits a bounds check whose panic path formats the index.

This is the proof-carrying-facts gap in miniature, on a safety-critical object.

## What it costs — measured, not estimated

Communicating the already-proven bound (`& (MAX_WINDOWS-1)`, a no-op under the
invariant) and const-initialising the two `static mut Option<…>` singletons:

| | shipped | + proof-carrying bound | delta |
|---|---|---|---|
| wasm | 8 903 B | 4 413 B | **−50.4%** |
| `.text` | 5 220 | 2 382 | **−54.4%** |
| `.data` | 376 | 16 | −95.7% |
| `.bss` | 2 484 | 2 180 | −12.2% |
| **SRAM** (data+bss) | **2 860 B** | **2 196 B** | **−23.2%** |
| branches | 98 | 28 | −71.4% |
| conditions dead | 51 | 5 | −90.2% |
| MC/DC decisions | 3/22 | 2/5 | — |
| undefined symbols | 3 seams | **3 seams** | unchanged |
| panic/fmt functions | 4 | **0** | eliminated |

The seam set is byte-for-byte the same three atoms — `ctx-save`, `region-swap`,
`ctx-resume`. Nothing was swallowed to buy the reduction.

**This is a measurement, not a shipped change.** The FSM bodies are lifted
verbatim from the Verus/Kani-proven `plain/src/partition_switch.rs`, and that
claim is load-bearing for the whole track; changing them is a decision that needs
its own artifact and a re-run of the proofs, not a drive-by edit. The experiment
sizes the prize so the decision can be taken on a number.

## WASM → object disposition — the first one in this repo

`synth --emit-provenance` (synth-provenance-v1, 33 functions) joined to the
witness manifest by `(func_index, byte_offset)`:

    98 branches — 42 obligation-stands, 0 justified-infeasible,
                  0 needs-object-coverage, 56 no-provenance; 9 only-in-synth

- **42 obligation-stands** — every WASM branch in switch-thin's own code and its
  wit_bindgen glue maps to a real object branch. The lowering did not lose any.
- **56 no-provenance** — exactly the dead set: the four `core::fmt` functions (53)
  plus the second `cabi_realloc` copy (3).
- **9 only-in-synth** — object conditional branches with **no WASM counterpart**,
  in `tick` (1), `run_switch` (3), `mark_resumed` (1), `mark_swapped` (1),
  `current_window` (1), `cabi_realloc` (1), and one function carrying no witness
  branches at all.

Those 9 are new **object-code obligations**: control flow that exists in the
shipped binary and in no source or WASM decision, so no source-level MC/DC
argument can discharge them. Surfacing precisely this is why object-code
verification exists. They are consistent with the bounds-check finding above, but
that correspondence is **not yet established** — mapping each of the 9 to its
originating construct is the next step.

## What this does NOT establish

- **Not zero-gap.** `VER-OS-OBJVERIFY-001` wants zero gap rows; this is 3/22
  decisions with 11 gaps and 51 dead conditions. `REQ-OS-OBJVERIFY-001` stays
  `proposed`.
- **The three seams are stubbed** to return success — the same substitution the
  Kani harness makes, since `run_switch`'s FFI calls cannot be linked. No native
  context save/restore was exercised.
- **Nothing executed on silicon or under Renode.** This is wasmtime.
- **The 9 only-in-synth divergences are unexplained**, not justified. A divergence
  is an open obligation until each is traced to a construct and discharged.
- **Per-condition attribution is partial.** In the reduced build witness's
  `evaluated` maps come back empty (rows carry outcomes but not per-condition
  values), so its MC/DC verdict there rests on fewer reconstructed conditions.
- **The oracles were not re-run against the wasm.** `REQ-OS-SWITCH-001` /
  `ISO-001` / `HM-001` remain source-level only.
- **mpu-thin has no evidence-on-wasm yet** — it needs an `mpu-write` seam stub.
  hm-thin is covered in the section below.

## Reproduce

    benches/gust/drivers/mcdc/run-mcdc.sh          # DRV=… for another thin driver

Requires `witness >= 0.39.0` (`object-disposition` landed in 0.39, witness#109);
0.40.0 or newer is recommended — it warns about the two invocation traps below.
The build must carry DWARF: `debuginfo=2` changes the crate disambiguator and
permutes four function indices, so the manifest and the provenance map must both
come from that one artefact — never join across builds.

---

# hm-thin — the controlled comparison

Same harness, `DRV=hm-thin STUB= VECTORS=vectors-hm.sh` (no stub: hm-thin declares
zero seams). Seven pure scalar Health-Monitor predicates.

    overall: 0/2 full MC/DC; conditions: 1 proved, 4 gap, 3 dead (8 total)
    object-disposition: 9 branches — 9 obligation-stands, 0 justified-infeasible,
                        0 needs-object-coverage, 0 no-provenance; 4 only-in-synth

## It confirms the switch-thin root cause

**9 branches in the whole fused core, against switch-thin's 98 — and not one byte
of panic or formatting machinery.** No `pad_integral`, no `do_count_chars`, no
`<u64 as Display>::fmt`, no `panic_bounds_check`. (The only textual hit for those
names is a source path inside `.debug_str`, not a function.)

hm-thin does no array indexing. switch-thin indexes `[self.cur]`. That is the only
structural difference between the two, and it is worth 53 dead branches — a
controlled comparison, not an inference from one data point.

The object side is the cleanest of the three modules: **every WASM branch maps to
an object branch, and nothing lacks provenance.**

## MC/DC is close to vacuous here, and that is the honest reading

Six of the seven predicates compile to **zero branches**:

| predicate | br_if / br_table |
|---|---|
| `fresh`, `plausible`, `innovation-ok`, `budget-ok`, `deadline-ok`, `heartbeat-ok` | **0** |
| `vote-ok` | 5 |

`lo <= value && value <= hi` lowers to a branchless `and`; `if d >= 0 { d } else { -d }`
lowers to a select. MC/DC is defined over decisions — where the compiler emits no
decision there is nothing to cover, and a coverage number says nothing about
whether the predicate is right. **Their correctness argument rests entirely on the
Verus/Kani proofs; structural coverage cannot add to it or subtract from it.**

That is a useful boundary for the track: MC/DC earns its keep on control flow
(switch-thin's FSM), not on branchless value-domain predicates.

## Friction observed

Both decisions are attributed to `wit_bindgen_cabi_realloc.rs:11`, but the
conditions are the five branches of `_export_vote_ok_cabi`. Inlined-decision
attribution points at the wrong source file, which makes the rollup's per-file
table misleading — the `lib.rs` row understates the driver's own decisions.

## What this does NOT establish

- **Not zero-gap** — 4 gap, 3 dead of 8 conditions; `vote-ok`'s wrapper has 2 of
  its 5 branches unreached.
- **4 only-in-synth divergences** remain, unexplained (synth#944).
- **Nothing ran on silicon or Renode.**
- **mpu-thin still has no evidence-on-wasm** — it needs an `mpu-write` seam stub.

---

# mpu-thin — the third module, and a correction to the `cabi_realloc` count

`DRV=mpu-thin STUB=regs-stub VECTORS=vectors-mpu.sh`. The region-programming core;
one seam (`mpu-write`), stubbed as a no-op the way the Kani harness stubs it.

    overall: 2/17 full MC/DC; conditions: 8 proved, 6 gap, 37 dead (51 total)
    object-disposition: 86 branches — 30 obligation-stands, 0 justified-infeasible,
                        0 needs-object-coverage, 56 no-provenance; 12 only-in-synth

## The best user-code result of the three

| file | decisions | full MC/DC | proved | gap | dead |
|---|---|---|---|---|---|
| **`lib.rs`** (mpu-thin itself) | **6** | **2** | **8** | **4** | **2** |
| `<meld-adapter>` | 3 | 0 | 0 | 0 | 11 |
| `count.rs` | 3 | 0 | 0 | 0 | 10 |
| `macros.rs` | 3 | 0 | 0 | 0 | 9 |
| `mod.rs` | 1 | 0 | 0 | 0 | 3 |
| `num.rs` | 1 | 0 | 0 | 0 | 2 |
| `panicking.rs` | 1 | 0 | 0 | 2 | 0 |

Two decisions reach **full MC/DC** — the first non-zero `full mcdc` count on any
driver's own code in this repo. `validate_region`'s four-condition conjunction
(power-of-two · min-size · alignment · no-overflow) and `size_field`'s ladder are
both the shape MC/DC was designed for, and a designed vector set closes them.
mpu-thin's own code: **25 branches, 19 reached.**

## Same dead payload, third time

    <core::fmt::Formatter>::pad_integral        25 branches, 0 reached
    core::str::count::do_count_chars            20 branches, 0 reached
    <u32 as core::fmt::Display>::fmt             5 branches, 0 reached
    pad_integral::write_prefix                   3 branches, 0 reached

Byte-identical in shape to switch-thin's 53 (there `<u64 as Display>`, here `<u32>`).
mpu-thin indexes `regions[r]`; hm-thin indexes nothing and carries none of this.
Three modules, and the correlation with array indexing holds every time.

## Correction: the `cabi_realloc` duplication was our harness

An earlier note reported `cabi_realloc` **×2**. That was wrong, and the tell was
in hm-thin all along: it declares zero seams, so it is the one module fused
*without* a stub — and it showed **one** copy.

| artefact | components fused | `cabi_realloc` fns with branches |
|---|---|---|
| switch-thin **alone** (shipped shape) | 1 | **1** (3 branches) |
| switch-thin + seam stub (what we measure) | 2 | 2 (6 branches) |
| hm-thin (no stub needed) | 1 | **1** (3 branches) |

One copy **per fused component** — the second was the stub's own. So the honest
scale figure for the shipped single-component shape is **56 of 65 unreached
branches are library machinery (53 formatting, 3 canonical-ABI glue)**, not 59 of 68.

The duplication is still real where gale actually ships — `iso-core-fused` fuses
three components — but it scales with component count, not "×2 per driver".
Corrected on loom#303 before it could be built into that issue's acceptance gate.

**The measurement lesson generalises:** a seam stub is part of the artefact under
measurement. Anything counted per-component is inflated by it, and the run must be
read against a driver that needs no stub before a per-component claim is made.

## What this does NOT establish

- **Not zero-gap** — 2/17 decisions, 6 gap, 37 dead.
- **12 only-in-synth divergences**, the most of any module (synth#944).
- **The seam is stubbed to a no-op**, so `mpu_write`'s barrier-pairing contract is
  not exercised at all — that contract is trusted platform code either way.
- **Nothing ran on silicon or under Renode.**

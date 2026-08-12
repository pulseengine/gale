# mpu-thin — the I-ISO core dissolved, and a real SRAM constraint

**Date:** 2026-08-08 · **T2 / REQ-OS-OBJVERIFY-001 stage 2** · meld 0.41.3, loom 1.2.0,
synth 0.52.0 (`--features verify` build for BIN-VERIFY)

Second isolation-core module onto the dissolve path, after `hm-thin`. Unlike HM this
one has a seam — the single `mpu_write(rnr, rbar, rasr)` atom — so it tests the part
that matters: does the seam survive the dissolve, and what does the module actually
cost in RAM?

## It dissolves, and the seam survives exactly

    plain/src/mpu_switch.rs + mpu.rs   (Verus + Kani verified — lifted VERBATIM)
      -> wasm 7 120 B -> component 8 464 B
      -> meld fuse --memory shared -> loom optimize -> synth compile

    undefined symbols: [ mpu-write ]

One atom, exactly as declared. Nothing undeclared crept in; nothing was swallowed.
Lifted verbatim: `size_field`, `rasr_for`, `RegionTable` (+ `new`,
`program_partition`, `switch_to_partition`, `try_add_region`, `covers_addr`),
`apply_program`, `emit_write`, and from `mpu.rs` the `is_power_of_two` /
`validate_region` helpers and `MIN_REGION_SIZE`.

## THE FOOTPRINT — and why the first number was wrong

Compiled the way `hm-thin` was, the object reports:

    text 3614   data 0   bss 0

**That is true and misleading.** Without `--native-pointer-abi` the linear memory is
not reserved in the object at all — the embedder supplies it — and this module
declares **17 wasm pages (1 088 KB)**. Reporting "zero SRAM" for it would have been
wrong by a factor of 136 on an 8 KB part.

With the shadow-stack re-base the OS-node builds already use (#383):

| `--shadow-stack-size` | text | data | bss | **SRAM of 8 192** |
|---|---|---|---|---|
| **2048** (OS-node standard) | 3744 | 636 | 2688 | **3 324 B — 40.6%** |
| 1024 | 3744 | 636 | 1664 | 2 300 B — 28.1% |
| 512 | 3744 | 636 | 1152 | 1 788 B — 21.8% |

synth's own log: `sp_init 1048576 -> 2048, reservation 1049216 -> 2688 B (post-link
oracle: stack/static disjoint, all reservation accesses in-range)`. The 640 B delta
above the shadow stack is static data — the 320-byte `RegionTable` plus panic strings.

**The committed object is the 2048 build.**

### The constraint this surfaces

At the standard budget **this one module takes 40.6% of the STM32F100RB's RAM**. The
isolation core is three modules, and a system needs the OS and the application too.
Either the shadow-stack budget comes down for this module, or the F100 does not host
a multi-partition configuration. That is a finding for the partition work, not a
detail of this file.

And the budget is **asserted, not proven** — the same `2048 of 8192` gap already
recorded against `REQ-OS-OBJVERIFY-001`. scry computes the depth; wiring it is the
open step.

## BIN-VERIFY

    7 functions run · 7 rules verified · 0 failed · 0 unknown
    rules: i32.add · i32.and · i32.eq · i32.eqz · i32.gt_u · i32.ne
    20 LRAT-certified I64 expansions
    3 functions with no computational rules

**Not a zero-gap result.** The object contains **356 `i32.const`**, skipped by
BIN-VERIFY as a register operation and `Admitted` in Rocq (`i32_const_correct`) — so
neither half covers it. synth#933 (the proof gap), synth#935 (that the gap is
invisible in the tool's output). More than ten times HM's 29 occurrences.

## What this does NOT establish

- **Partial evidence-on-wasm.** witness MC/DC **has now been run** —
  `../measurements/switch-thin-mcdc.md`, mpu-thin section: 2/17 decisions full
  MC/DC, 8 proved / 6 gap / 37 dead, plus a WASM→object disposition (86 branches,
  30 obligation-stands, 12 only-in-synth). This module's own `lib.rs` is the best
  of the three — 6 decisions, 2 at full MC/DC — but it is **not zero-gap**, and 53
  of its unreached branches are integer-formatting machinery reachable only from
  `panic_bounds_check` on an index the proof already bounds.
  **scry has not been run**, and the `REQ-OS-ISO-001` oracles have **not** been
  re-run against the wasm build. BIN-VERIFY shows the lowering is faithful, not
  that the wasm refines the Verus-proven Rust.
- **Not executed.** Nothing has run this object, on silicon or under Renode.
- **The seam is still native.** `mpu_write` remains trusted platform code, including
  its barrier-pairing contract. What moved is the region-programming *policy*.
- **`partition_switch` (3 seams) remains** — the last stage-2 module.

## Reproduce

    cd benches/gust/drivers/mpu-thin && cargo build --release --target wasm32-unknown-unknown
    wasm-tools component new target/wasm32-unknown-unknown/release/gust_mpu_thin.wasm -o mpu.component.wasm
    meld fuse mpu.component.wasm --memory shared -o mpu.fused.wasm
    loom optimize mpu.fused.wasm --passes inline --attestation false -o mpu.loom.wasm
    synth compile mpu.loom.wasm --target cortex-m3 --all-exports --relocatable \
      --native-pointer-abi --shadow-stack-size 2048 -o mpu-thin-cm3.o
    synth verify mpu.loom.wasm mpu-thin-cm3.o      # needs a --features verify build

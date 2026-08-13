# E2 — the whole OS, dissolved. First native figure that has ever existed for it

**Date:** 2026-08-06 · **REQ-OS-COMPOSITE-DISSOLVE-001** · toolchain: meld 0.41.3,
loom 1.2.0, **synth 0.52.0** (gale's pin, #208 — not PATH, so the number is
comparable to everything already recorded)

v0.6.0 shipped the composed OS as **20 741 bytes** and said plainly that this was a
*wasm* figure implying nothing about code size, SRAM or cycles.
`build-fused-gustos.sh` carried the same caveat in its header: *"NOT done here:
lowering the composite to native."* It has now been lowered.

## The result

    fused-gustos.component.wasm     20741 B   (wasm component)
      -> meld fuse --memory shared            (component graph -> ONE core module)
      -> loom optimize --passes inline
      -> synth compile --target cortex-m3 --all-exports --relocatable
                       --native-pointer-abi

    text   5124
    data    512
    bss    3096
    ---------------
    SRAM   3608 B of the STM32F100RB's 8192   (44%)

3 external symbols (`poll-task`, `read32`, `write32`) — exactly the declared seam.

### These numbers replace an earlier set that could not be executed

The first cut of this measurement read `text 4792 / data 0 / bss 0` and
`SRAM 0 B of 8192`. That object was compiled **without `--native-pointer-abi`**,
so its linear memory was not reserved in the object at all — while the module
still declared **17 wasm pages (1 088 KB)** for an embedder to supply. Nothing
supplied it, and the composite **could not execute**: it passed three checks in
`gust_osfused_probe` and then hung.

`SRAM 0 B` therefore read as "this OS needs no RAM" for an object that needed
1.05 MB — 128× the F100's entire 8 KB. The caveat below (*"`bss 0` is not 'the OS
needs no RAM'"*) was correct in kind but unquantified, and the quantity was
disqualifying. This is the same error `mpu-thin/RESULTS.md` caught for itself and
did quantify (*"wrong by a factor of 136 on an 8 KB part"*). Recorded as gale#275.

Two changes fixed it, and both are in the build scripts rather than a note:

1. `build-gustos-components.sh` caps the declared arena at **one wasm page** —
   `-zstack-size=2048 --initial-memory=65536`. wasm-ld's default is a 1 MB shadow
   stack under `--stack-first` plus a 1 MB `--global-base`, which is where 17
   pages came from.
2. `build-dissolve-gustos.sh` passes `--native-pointer-abi`, so the arena is
   reserved **in the object** and the numbers above are the whole footprint.

The result is self-contained, fits the F100 at 44%, and **executes** —
`gust_osfused_probe` reports ALL CHECKS PASSED, including a one-shot timer armed
through `timer#sleep` firing at the correct tick.

## What this does NOT say

- **20 741 → 4 792 is not a compression ratio.** They measure different things: a
  wasm *component* carrying component metadata and type information versus a native
  `.text` section. Quoting them as a ratio is precisely the conflation that
  `loom-across-the-fuse.md` already had to correct once in this repo. The honest
  statement is: the wasm figure was never a footprint, and now a footprint exists.
- **`bss 0` is not "the OS needs no RAM".** It is zero *static* allocation in the
  object. Stacks and any linear-memory arena come from the embedder — the same
  caveat that applies to every driver seam.
- **This is a relocatable object, not a linked image.** It still needs the TCB
  bridge; `synth` says so itself ("requires linking with Kiln bridge").
- **Executed under qemu, not Renode and not silicon.** E1 executed the *component*
  on a host engine; `gust_osfused_probe` now executes this OBJECT on qemu
  lm3s6965evb (real v7-M). Renode is still E3 and silicon still E4 — both v0.7.1.
  The seams are stubbed: a software tick source behind `read32`, and `poll-task`
  returning Done, so nothing here exercises real hardware or a real task body.
- **No cycle claim.** `.text` is space.

## The gate, and its direction

The dissolved object's undefined symbols must be **exactly** the seam the composite
declares:

    undefined (3): poll-task read32 write32
                   ^^^^^^^^^ gust:os/taskdisp
                             ^^^^^^ ^^^^^^^ gust:hal/mmio

Two ways to fail, and the second is the one worth stating:

- **more than that** — the OS depends on something we never declared;
- **an empty set** — the seam was inlined away. That produces a *smaller* object and
  reads like a win, but an OS whose hardware seam has been swallowed is no longer
  swappable, which is the entire thesis. The gate exits 2 on it.

**The control bites.** `wasm-kernel/fused.wasm` lowers with zero undefined symbols —
exactly the seam-swallowed shape — and the gate refuses it. That control runs on
every invocation, and if it ever *passes* the script exits 4, because a gate that has
stopped discriminating reports a green that means nothing. This is the same
discipline gale#250 established for the execution harness, applied here because
gale#254 showed what a gate that cannot fail actually costs.

## What this unblocks

E2 was the critical path for the rest of v0.7.0:

- **REQ-OS-OBJVERIFY-001** (T2) — had no object to verify. It has one now.
- **REQ-OS-WCET-001** (T4) — had nothing to bound. 31 functions, named and
  addressable, are now emittable targets.
- **REQ-OS-SCHED-001** (T3) — consumes T4's numbers, so it moves once T4 does.

Reproduce:

    bash benches/gust/drivers/build-dissolve-gustos.sh
    SKIP_BUILD=1 bash benches/gust/drivers/build-dissolve-gustos.sh   # reuse composite

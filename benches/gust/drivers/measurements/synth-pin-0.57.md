# synth pin 0.52.0 -> 0.57.0 — measured both ways, on both dissolved objects

**Date:** 2026-08-26 · **gale#269, gale#270** · meld 0.48.0, loom 1.2.0,
synth 0.52.0 vs 0.57.0 · qemu `lm3s6965evb` (real v7-M)

The pin moved because an executed oracle flipped, not because an upstream
changelog said a bug was fixed. Same source, same meld, same loom, only `$SYNTH`
changing; both objects rebuilt and re-executed each way.

## The isolation core — this is what justifies the bump

    drivers/build-iso-core.sh   ->   gust_iso_probe under qemu

| synth | `iso-frame` | probe verdict | `.text` | `data` | `bss` | SRAM |
|---|---|---|---|---|---|---|
| 0.52.0 | `iso-frame-bad` | **1 CHECK(S) FAILED** — 8/9 | 8 712 | 1 096 | 3 140 | 4 236 B (51%) |
| **0.57.0** | `iso-frame-ok` | **ALL CHECKS PASSED** — 9/9 | 8 700 | 1 096 | 3 140 | 4 236 B (51%) |

`iso-frame-bad` is the `set-window` miscompile recorded in #270 and caveated in
`../switch-thin/RESULTS.md`. Both builds pass both hard gates — the seam set is
exactly the four declared atoms (`ctx-resume ctx-save mpu-write region-swap`) and
the data segments are disjoint — so the *only* thing that moved is the check that
was failing. `.text` got **12 bytes smaller**; the fix is not paid for in size.

## The E2 composite — the bump is safe here, but it is NOT what fixed it

    drivers/build-dissolve-gustos.sh   ->   gust_osfused_probe under qemu

| synth | probe verdict | `.text` | `data` | `bss` | SRAM | arena |
|---|---|---|---|---|---|---|
| 0.52.0 | ALL CHECKS PASSED — 6/6 | 5 368 | 1 580 | 3 620 | 5 200 B (63%) | 1 page |
| **0.57.0** | ALL CHECKS PASSED — 6/6 | 5 384 | 1 580 | 3 620 | 5 200 B (63%) | 1 page |

Both pass. The composite was **already** green on the old pin once #283 landed the
`timer.sleep` contract fix, so the bump neither fixed nor broke it. `.text` is 16
bytes *larger* on 0.57.0 — recorded, not explained.

One thing did change: **synth 0.57.0 emits `gust:os/timer@0.1.0#sleep`** (276 bytes
of machine code) where #269 records synth 0.56.0 *refusing* to emit it. So the
hazard #269 names — a committed object containing a function a later synth declines
— does not reproduce on 0.57.0.

## The part that is NOT established — our own oracle has a structural blind spot

**`osfused-deadline-armed` cannot detect a dropped i64 high half.** It passes on
both pins, and that agreement is not evidence, because the check cannot fail in
that dimension by construction:

- `time-provider/src/lib.rs`: `fn now() -> u64 { read32(TIM2_CNT) as u64 }` — a
  32-bit mmio read widened, so `now`'s high half is **always zero**.
- `timer-provider/src/lib.rs`: `sleep` calls `time::deadline(time::now(), ticks as u64)`
  where `ticks: u32` at the WIT surface — so the second argument's high half is
  **always zero** too.

Both `u64` arguments to `deadline` are structurally zero-extended `u32`s. Dropping
a zero high half is unobservable. No test routed through this WIT seam can catch
that half of synth#929.

What the probe *does* establish is register **placement**: `deadline(now, ticks)`
puts an i64 in a non-last position, and a mismarshalled argument-0 would corrupt
`now` and blow the armed deadline (1000 + 50 = 1050). That check passes on both
pins, so argument placement is correct on both.

This is a gap in **our** harness, not an upstream defect. Closing it needs a seam
that can carry a nonzero high half — a 64-bit tick source, or a direct-called
export taking an i64 in a non-last position with the high half set. Until then,
"the E2 probe is green" must not be read as "synth#929 is not present here".

## Scope of the bump — deliberately partial

Three of the six `drivers/*.sh` carried a `synth-0.52.0` default; two were moved:

| script | pin | why |
|---|---|---|
| `build-iso-core.sh` | **0.57.0** | measured above, object regenerated |
| `build-dissolve-gustos.sh` | **0.57.0** | measured above, object regenerated |
| `mcdc/run-mcdc.sh` | 0.52.0 (kept) | its committed evidence — truth tables, object-dispositions, `*.provenance.json` — was produced on 0.52.0. Bumping the script without re-running the sweep would make the script and the evidence disagree. |
| `build-os-ts.sh`, `build-os-tl.sh` | 0.45.1 (kept) | not re-measured |
| `emit-wcet.sh` | 0.46.0 (kept) | not re-measured |

CI (`.github/workflows/gustos-dissolve.yml`, `SYNTH_VERSION`) moves to `0.57.0`;
the `x86_64-unknown-linux-gnu` asset the workflow's `fetch` builds by name is
published for v0.57.0.

**0.58.0 exists upstream and was not measured** — 0.57.0 is what is on the bench
machine. Pinning what was measured rather than what is newest is the point.

## Reproduce

    cd benches/gust/drivers
    SYNTH=$HOME/pe-toolchain/synth-0.52.0/synth ./build-iso-core.sh   # or 0.57.0
    cd .. && cargo build --release --bin gust_iso_probe --target thumbv7m-none-eabi
    qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic \
      -semihosting-config enable=on,target=native \
      -kernel target/thumbv7m-none-eabi/release/gust_iso_probe

Rebuilding either object with 0.52.0 reproduces the previously committed bytes
exactly (`git status` clean), which is what makes the before/after trustworthy.

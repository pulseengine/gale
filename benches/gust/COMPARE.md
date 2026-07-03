# gust comparative bench — native vs wasm-dissolved (and vs WAMR-class)

The point of gust is the **maximal-wasm** thesis: author the kernel as wasm, dissolve it
to native (meld → loom → synth), ship with no runtime. This bench measures the cost of
that choice against the two reference points — **native rustc/LLVM** (the floor) and a
**WAMR-class on-device runtime** (the incumbent) — so the claim is evidence, not rhetoric.

## Measured now (codegen, same source two toolchains, M-class Thumb-2)

Same `gust_poll` (kiln-async `poll_round` + failsafe mixer closure) and `gust_mix`,
compiled two ways: **native** = rustc/LLVM → thumbv7m; **dissolved** = the identical Rust
→ wasm32 → loom inline → synth → cortex-m3 relocatable.

| function | native (rustc→thumbv7m) | dissolved (wasm→synth→cm3) | ratio | how to read it |
|---|---|---|---|---|
| `gust_poll` (scheduler hot path) | **208 B** | **816 B** | **3.9×** | synth codegen vs LLVM, today |
| `gust_mix` (Q8 failsafe mixer) | 12 B | 44 B | 3.7× | — |
| whole dissolved kernel `.o` `.text` | — | **1402 B** | — | the *entire* kernel, **no runtime** |

### Cycle cost (not just size) — `gust_codegen_bench`

`cargo run --release --bin gust_codegen_bench` times the SAME `gust_mix` lowered
both ways under one SysTick harness (qemu `-icount`, deterministic; instr ≈
cycles on M3), with a correctness gate (native ≡ dissolved, bit-identical over
[0,2047]) before timing:

| lowering | fn-only ticks/call | `.text` (gust_mix) | stack frame | toolchain |
|---|---|---|---|---|
| native (LLVM) | **0.40** | — | none | rustc/LLVM |
| dissolved, initial | 1.125 | — | 24 B | synth 0.11, no loom inline |
| dissolved, loom-inlined | 1.05 | 132 B | 8 B | loom 1.1.16 + synth 0.12.0 |
| dissolved, **4 levers** | **0.725** | **90 B** | 8 B | **loom 1.1.16 + synth 0.15.0** |
| **ratio vs LLVM** | **2.81× → 2.63× → 1.81×** | −32 % | — | — |

**Progress (measured, 2026-06-25): the ranked synth#428 asks shipped and
delivered.** synth landed all four ARM perf levers default-on across three
same-day releases — v0.13.0 cmp→select → IT-block predication fusion (the #1
ask), v0.14.0 redundant stack-reload elimination + i32 local promotion, v0.15.0
immediate-shift folding — each mapping 1:1 to a residual issue this file had
pinned on 0.12.0 (the materialized-boolean clamp, stack spill/reloads, the
6-register leaf prologue, `movw #8; lsl` instead of `lsl #8`). Re-measured on
`gust_codegen_bench`: dissolved `gust_mix` **1.05 → 0.725 ticks/call (−31 %)**
and **132 → 90 B (−32 %)**, taking the gap to native LLVM from **2.63× → 1.81×**,
correctness **bit-identical over [0,2047]**. loom v1.1.16's inline + whole-function
DCE (loom#228) had already merged the export wrapper (frame 24 → 8 B; 2.81× →
2.63×); synth's lowering closed most of the rest.
**synth 0.16.0 (2026-06-26): adopted, verified byte-identical** on gale's whole
dissolved surface (`gust_kernel.wasm` 1282 B, `fused.wasm` 640 B — `cmp`-identical
to 0.15.1, deterministic). 0.16 is a completeness release (the >8-param AAPCS
stack-arg path for the falcon component); it correctly does not touch gale's
≤8-param kernels, so the numbers above are unchanged and the baseline is frozen.
**synth 0.17.0 (2026-06-26): adopted — first optimized-path levers to REACH the
shipped `--relocatable` flow.** #516 flipped VCR-RA stack-reload-forwarding +
frame-slot dead-store-elimination default-on (#242). Measured on the body: the
loom-inlined **`gust_poll` (scheduler hot path) 834 → 810 B (−2.9%)**, **`fused.o`
640 → 622 B (−2.8%)** (`run-demo`=53, correctness preserved), kernel `.text`
1342 → 1318 B. **But the standalone `gust_mix` micro-bench target is byte-identical
0.16↔0.17** (a 90 B function already at minimal frame — the prologue/spill levers
have nothing to remove), so `gust_codegen_bench` stays **1.81×** and 0.17 does *not*
move toward 0.7×. The flag-off `SYNTH_CONST_CSE` (#514) *regresses* gust_mix +2 B
(90→92) — reported as a negative datum. Net: 0.17 is real incremental parity
progress on larger functions; below-1.0× still requires the proof-carrying clamp
elision (the 0.45× floor above), which 0.17 does not implement.
**synth 0.24.0 (2026-07-03): adopted — "the allocator release", Belady spilling
DEFAULT-ON (#583, VCR-RA-001).** The `SYNTH_SPILL_REALLOC` lever gale measured +
drove to synth#242 (flag-on: gust_poll −20B on 0.22 → −24B on 0.23 as Belady
matured) shipped default-on in 0.24, plus whole-function dead-frame-store
elimination (#579) + register-exhaustion spill (#580). Measured on the body:
loom-inlined **`gust_poll` 810 → 784 B (−26 B, −3.2%)**, kernel `.text` 1318 →
1292 B (direct-compile gust_poll 740 → 716 B). Re-pinned `gust_kernel-cortex-m3.o`
→ 0.24 (1872 → 1844 B ELF). `gust_mix` micro-target still byte-identical (already
minimal) → `gust_codegen_bench` stays **1.81×**, correctness IDENTICAL. So the
spill lever closes more of the driver-class/dense-code residual (the synth#428 win
now default), but the below-1.0× path is still the proof-carrying floor (0.45×).
**synth 0.27→0.29 (2026-07-03): the flip-wave, adopted.** ARM `SYNTH_BASE_CSE`
default-on (0.27) = byte-neutral on gale (nothing redundant to CSE). RV32
`SYNTH_RV_CMP_SELECT` default-on (0.28) = −8 B on the esp32c3 lane by default (see
esp32c3/RESULTS.md). ARM `SYNTH_CONST_CSE` default-on (0.29, #604 "retire inline
const aliasing") = **−8 B on the loom-inlined kernel** (gust_poll 784→780, gust_mix
90→86 — dedupes the repeated constant); re-pinned `gust_kernel-cortex-m3.o` → 0.29
(1844 → 1836 B ELF). Notably this **resolves the +2 B gust_mix regression** gale
reported when const-CSE was flag-on (0.17): the rework makes it a net win, no
regression. `gust_codegen_bench` stays **1.81×**, correctness IDENTICAL. gale not
exposed to 0.28/0.29 encoder + i64-completeness fixes (byte-identical, 0 i64 params).
**Still open:** (1) the RISC-V backend has none of these levers — esp32c3
`gust_mix` is byte-identical 0.12.0↔0.15.0 (synth#472 tracks the port);
(2) the dense `control_step` still register-exhausts under default-on local
promotion (synth#474, confirmed on 0.15.0), so it builds with
`SYNTH_NO_LOCAL_PROMOTE=1` and gets only three of the four levers (580 → 568 B).
Full write-up + the ranked asks: `optimization/RECO-synth-cycles.md`;
cross-layer attribution: `optimization/ANALYSIS-where-to-optimize.md`.

Functional equivalence: `gust_mix` verified identical in wasmtime (the browser/host engine)
and in the synth-dissolved object — `1024→1500` centre, `0/512→1000`, `1536/2047→2000`.

### The 0.7× floor — proof-carrying specialization (measured)

The 4 levers close the *codegen* gap toward parity (~1.0×). Going **below** native
needs information LLVM structurally lacks: the **proof**. `gust_floor_bench`
(`cargo run --release --bin gust_floor_bench`, same `-icount` harness) measures it.

`gust_mix` is `clamp(1500 + (ch-1024), 1000, 2000)`. When a composition proves
`ch ∈ [524,1524]` — a range gale primitives carry as a Verus/Rocq/Kani invariant —
`v = ch + 476` is provably in `[1000,2000]`, **both clamp branches are dead**, and
the function collapses to `add r0,#476; bx lr`. LLVM never emits that: it never had
the bound. All three lowerings, timed over the SAME proven-range inputs:

| lowering | fn-only ticks/call | ratio vs native | note |
|---|---|---|---|
| native (LLVM, full clamp) | **0.50** | 1.00× | what LLVM ships |
| dissolved today (synth 0.15.1 / **0.16.0**) | 0.825 | 1.65× | in-range subset (full-domain = 1.81×) |
| **proof-carrying floor** (`ch+476`) | **0.225** | **0.45×** | what synth *could* ship (synth#494a) |

**Measured floor = 0.45× native** — past the 0.7× goal, and unreachable by LLVM.
Soundness gate (in the bench, exit-coded): `mix_proven ≡ mix_native ≡ gust_mix`
over `[524,1524]` — the elision is correct *only* under the carried bound, which is
exactly the side-condition. The prize is the **3.7× span** between today's dissolved
(0.825) and the floor (0.225): synth#494(a) proof-carrying specialization + loom#240
(carry the fact through the pipeline). This is the "verified code is *faster* because
it's verified" lever, quantified.

### Honest verdict
- **vs native (LLVM):** synth's per-function *cycle* cost on the hot path is now
  **1.81×** (was 2.63×) after the four levers — closing on the project's
  10–20%-overhead thesis, with a clear remaining tunnel (RISC-V lever port
  synth#472; the local-promotion register-allocator fix synth#474). The
  larger-`.text` ratio narrowed in step (−32 %). **This is the gap still being
  driven down, and it is moving.**
- **vs WAMR-class runtime:** dissolve wins decisively on *total* footprint — the whole
  kernel is **1.4 KB with zero resident runtime**, versus a WAMR interpreter/AOT-loader of
  **~50–64 KB** plus per-module RAM (and 10–50× interpreter energy). WAMR structurally
  cannot reach gust's 8 KB / no-runtime class at all.
- Net: gust already wins the *footprint/energy/device-class* argument vs the incumbent; the
  open work is closing the *per-function codegen* gap vs native LLVM.

## Pending (needs synth#383 and/or hardware)

| metric | status | blocker |
|---|---|---|
| on-MCU cycles (dissolved `gust_poll`) | pending | Renode run + dissolved image boot (synth#383 `.bss` link) |
| whole dissolved-image flash/RAM | pending | synth#383 (8 KB `.bss` link) |
| battery / sleep-current (wohl) | pending | dissolved image + power model |
| head-to-head vs **WAMR-AOT** (size + cycles) | pending | build the same logic under WAMR-AOT |
| native thumbv7m cycles (baseline) | doable now (Renode) | — |

The native bench (`./run-bench.sh`, qemu `-icount`) already gives `poll_round` = 3.10 ticks
O(1); the dissolved-on-MCU cycle number slots in once synth#383 lands and the dissolved
image boots (task #20).

## Reproduce the codegen table
```sh
./compare-codegen.sh   # builds native thumbv7m + dissolved cortex-m3, prints the size table
```

## Driver-class module — thin-seam UART (gust:hal, gale#65)

A new dissolved module on the bench: the whole STM32 USART protocol in verified
wasm (`drivers/uart-thin`), importing only `gust:hal` mmio + irq. Adds **driver-
class** code (tiny I/O-bound primitives) to the meld/loom/synth optimization
surface — complementing the arithmetic kernels (gust_mix) and the control loop
(control_step).

| | dissolved (synth 0.15.0) |
|---|---|
| `.text` (flash) | **254 B** (primitives) |
| SRAM (`.bss`+`.data`) | **0 B** |
| TCB | 3 import relocations (mmio_read32/write32, irq_poll) |
| verified | `usart_rx_decide` Kani-proven (error-priority, all 2³² SR) |
| e2e | drives a real STM32 USART in Renode → emits `gust-uart-thin` (CI gated) |

**Perf signal (synth 0.15.0 levers ON vs OFF): 0%** — the arithmetic levers that
gave gust_mix −31% don't reach driver-class code. The disasm shows the cost is the
**synth#428 prologue/spill residuals** (6-register leaf prologue + 24-byte frame +
redundant stack round-trips, paid per hot-loop call), not arithmetic and not
import dispatch. Reported to synth#428 with the disasm as evidence. The leaf-
prologue shrink + spill elimination (VCR-RA-002) is the lever this class needs.

Reproduce: `drivers/uart-thin/RESULTS.md`.

## Driver-class module — DMA ownership FSM (gust:hal, gale#124)

A second dissolved driver-class module (`drivers/dma-own`): DMA modeled as a
Component-Model `own<buffer>` ownership handoff, with the state machine that
decides *who may touch the buffer* as verified wasm. Adds more driver-class
surface (a pure state machine + a barrier-paired handoff) to the meld/loom/synth
optimization set — distinct from the I/O-poll shape of uart-thin.

| | dissolved (synth 0.20.0, cortex-m3) |
|---|---|
| `.text` (flash) | **218 B** (`dma_start` 96 / `dma_poll_complete` 92 / `dma_abort` 30) |
| SRAM (`.bss`+`.data`) | **0 B** (state lives in the caller; scalar ABI, no r11 trampoline) |
| TCB | 3 import relocations (`dma_program`, `dma_barrier`, `dma_irq_poll`) |
| verified | ownership FSM Kani-proven 6/6 (access-iff-owned, barrier-pairing, no-ownerless, round-trip, per-chunk exclusivity) |
| region marking | `synth --volatile-segment` **Phase-2 shipped (synth 0.25, #543)** — now suppresses linmem-access optimization in marked ranges; no-op on gale's current modules (no DMA-payload-read demonstrator yet, VER-DMA-WORKED) |

**Perf signal (synth 0.17→0.20 byte-identical):** the dissolved FSM is unchanged
across the recent synth releases — same driver-class prologue/spill residual as
uart-thin (the levers that help arithmetic don't reach it). No cycle-level bench
yet (needs a TCB bridge + demonstrator to run the barrier/program/irq seam under
qemu — tracked follow-on); the size/TCB numbers above are the current tracked
surface. See `optimization/BEAT-LLVM-0.7x.md` for the perf thesis.

Reproduce: `drivers/dma-own/RESULTS.md`.

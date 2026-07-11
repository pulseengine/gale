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
| dissolved, **4 levers** | 0.725 | 90 B | 8 B | loom 1.1.16 + synth 0.15.0 |
| dissolved, 0.37.1 re-pin | 0.675 | 82 B | 8 B | loom 1.1.18 + synth 0.37.1 |
| dissolved, **0.38.0 mask-elide** | **0.600** | **68 B** | 8 B | **loom 1.1.18 + synth 0.38.0 `SYNTH_SHIFT_MASK_ELIDE=1`** |
| **ratio vs LLVM** | **2.81× → 2.63× → 1.81× → 1.69× → 1.50×** | −48 % | — | — |

**On real M4 silicon (NUCLEO-G474RE, DWT CYCCNT — not qemu):** the current pin measures
**1.448× native LLVM** (29.0 → 42.0 cyc/call, 2026-07-11), confirming the 2026-07 ladder on
hardware (vs 2.21× on synth 0.12.0; native LLVM unchanged at 29.0 cyc, so the −22 cyc/call is
entirely synth-side). Silicon **1.448×** ≈ qemu-`-icount` **1.50×**. Full table + the
proof-carrying `SYNTH_FACT_SPEC` variant (1.413×, sound only over the carried `[524,1524]` —
the full-domain gate correctly flags it out-of-range): `silicon/RESULTS-g474re.md`.

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
**synth 0.30.1 (2026-07-03): adopted — the flip-wave trilogy (#611), and it finally
moves `gust_mix`.** Three flag audits flipped default-on: ARM `SYNTH_UXTH_FOLD`
(my synth#428 finding — `uxth` replacing `movw #imm; and` halfword-masks), ARM
`SYNTH_DEAD_FRAME_ELIM` (VCR-RA-002, #390), and RV32 `SYNTH_RV_SHIFT_FOLD` (the
−8 B I'd flagged still-flag-off on the esp32c3 lane). Measured on the body:
loom-inlined **`gust_mix` 86 → 68 B (−18 B, −21%)** — the first move on this
micro-target since the 0.13–0.15 levers took it to 90/86; **`gust_poll` 780 → 774 B**;
kernel `.text` object 1836 → 1808 B ELF (−28 B). Re-pinned `gust_kernel-cortex-m3.o`
→ 0.30.1. **Cycle honesty:** `gust_codegen_bench` **stays 1.81×** (fn-only 725
milliticks, unchanged) — the −21% is *code size*; the removed `uxth`/frame
instructions land **below this harness's SysTick timing resolution** at
`-icount shift=1`, so the win shows in flash/TCB bytes, not in the quantized
ticks/call (it would resolve on the F100 DWT cycle counter, a tracked re-measure).
Correctness **IDENTICAL over [0,2047]**; `fused.o` byte-identical (uxth-inert,
already minimal); floor bench soundness gate green, **0.45× floor intact**. gale
not exposed to 0.30.1's i64 rotl/rotr/div_u/rem_u silent-zero fix (#610 — 0 i64
params, byte-identical).
**synth 0.31.0 + loom 1.1.18 (2026-07-08): adopted, verified byte-identical on the
ARM body — but the 0.7× lever's INGESTION PATH now ships (see below).** Re-dissolved
`gust_kernel.wasm` (loom 1.1.18 inline → synth 0.31.0 `--target cortex-m3
--all-exports --relocatable`) and `fused.wasm`: **both `cmp`-identical** to the
0.30.1 pins. loom 1.1.18's "dissolve the inlined seam" (windowed SROA +
carrier-forwarding + narrow-local, loom#252) finds nothing to fold on the
already-minimal loom-inlined `gust_poll`; synth 0.31.0's A32 i64 silent-NOP fix
(#615/0.30.2) is not exposed (0 i64 params); VCR-SEL-001 increment 1 (#623) is
flag-off; RV32 `SYNTH_RV_LOCAL_PROMO` default-on (#627) is **byte-neutral on the
esp32c3 kernel** (512 B default == promo-off — nothing to promote, same as the ARM
local-promo). So the shipped bytes don't move — the release's substance for gale is
`wsc.facts` phase-1 (below).
**synth 0.37.1 + loom 1.1.18 (2026-07-10): RE-PIN — the checked-in `gust_mix`
pin was STALE (~0.15-era, 90 B / 0.725 ticks / 1.81×) and had missed every
codegen gain from 0.16→0.37.** Re-dissolved `gust_kernel.wasm` (loom 1.1.18
inline → strip exports to {memory, gust_mix} → synth 0.37.1 `--target cortex-m3
--all-exports --relocatable`), re-measured on `gust_codegen_bench`: **fn-only
0.725 → 0.675 ticks/call (−7 %)**, `.text` **90 → 82 B (−9 %)**, taking the gap
to native LLVM from **1.81× → 1.69×**. Object 440 → 432 B. Correctness gated:
`gust_floor_bench` soundness assertion `mix_proven ≡ mix_native ≡ gust_mix` over
`[524,1524]` still passes; proof-carrying floor still **0.45×**. cm3 + cm4
re-pinned identically (silicon_bench links cleanly for thumbv7em).
**Measured finding — a `beat-LLVM` lever (filed synth#686):** on the SAME stripped
input, synth **0.30.1→0.37.0 all emit 68 B / 0.600 ticks / 1.50×**; **0.37.1's
#682 mod-32 shift mask (`AND rm,#31`) adds 14 B / +0.075 ticks** because it masks
`gust_mix`'s shifts *unconditionally* — even though gale's shift amounts are
statically `<32` and never need the runtime mask. So the correctness-complete
0.37.1 costs a real 12 % vs 0.37.0 here; the gap is **recoverable by eliding the
mask when the shift amount is provably `<32`** (constant or range-carried) — the
same proof-carrying-facts pattern as the clamp-elision floor. Pinned 0.37.1 (one
current toolchain, reproducible) rather than the superseded 0.37.0.
**synth 0.38.0 (2026-07-10): the lever I filed SHIPPED (#692, `SYNTH_SHIFT_MASK_ELIDE`,
flag-off) → RE-PINNED, 1.69× → 1.50×.** synth 0.38.0's changelog lands "Shift-mask
elision (#686, flag-off) — recovers the #682 mask's 12% where the amount is provably
< 32" — the exact lever, motivated by gale's measurement. Re-dissolved `gust_mix` with
`SYNTH_SHIFT_MASK_ELIDE=1` (loom 1.1.18 inline → strip {memory,gust_mix} → synth 0.38.0
`--target cortex-m3/-m4 --all-exports --relocatable`): **fn-only 0.675 → 0.600 ticks/call
(−11 %)**, `.text` **82 → 68 B (−17 %)**, ratio **1.69× → 1.50×** — the best measured
dissolved-vs-native on the shipped path, correctness-gated (`gust_floor_bench` soundness
`mix_proven ≡ mix_native ≡ gust_mix` over [524,1524] PASSES; floor still 0.45×). cm3 + cm4
re-pinned; silicon_bench links cleanly for thumbv7em. NOTE: the relocatable `.o` grew 432
→ 496 B despite the smaller `.text` — that's 0.38.0's new ELF metadata (#656 STB_LOCAL
internal symbols + #637 `.ARM.attributes`), dropped/merged at link time, so **flash
footprint tracks the smaller 68 B `.text`, not the `.o`**. 0.38.0 DEFAULT (flag-off) is
perf-neutral (1.69×, `.text` 82) — adopting the release is safe; the win needs the flag.
Reported the measured 1.50× to synth#686 (the arc to default-on, like #428/#583). Ladder:
1.81× (stale) → 1.69× (0.37.1) → **1.50× (0.38.0 elide)** → 0.45× (clamp-elision floor,
synth#494 phase-2, still gated on select-arm elision + a verify build).
**Still open:** (1) the RISC-V backend is now catching up — esp32c3
`SYNTH_RV_CMP_SELECT` (0.28) + `SYNTH_RV_SHIFT_FOLD` (0.30.0) are default-on
(−16 B combined vs the 0.12 baseline; synth#472 port closed), but the arithmetic
levers still trail ARM and the on-silicon 2.12× ratio predates them (needs a board
re-run);
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
| dissolved today (synth **0.38.0** mask-elide) | 0.700 | 1.40× | in-range subset (full-domain = 1.50×) |
| **proof-carrying floor** (`ch+476`) | **0.225** | **0.45×** | what synth *could* ship (synth#494a) |

**Measured floor = 0.45× native** — past the 0.7× goal, and unreachable by LLVM.
Soundness gate (in the bench, exit-coded): `mix_proven ≡ mix_native ≡ gust_mix`
over `[524,1524]` — the elision is correct *only* under the carried bound, which is
exactly the side-condition. The prize is the **3.7× span** between today's dissolved
(0.825) and the floor (0.225): synth#494(a) proof-carrying specialization + loom#240
(carry the fact through the pipeline). This is the "verified code is *faster* because
it's verified" lever, quantified.

#### The ingestion path is live (synth 0.31.0, measured on gale's gust_mix)

synth **0.31.0 shipped `wsc.facts` phase-1** (#624/#494): a schema-v1 parser for a
`wsc.facts` custom section — the proof premises loom will forward (value-range,
shift-bound, divisor-nonzero, in-bounds, select-totality, disjointness), keyed by
(function index, operator index). Phase 1 has **no consumer**: facts are parsed and
stored, output is byte-identical. gale exercises this end-to-end on its **own**
`gust_mix` via `optimization/wsc-facts-phase1.sh`:

- the exact clamp premise — **value-range `[524,1524]` on `gust_mix` (func 2),
  value 0 (the `ch` param `local.get 0`)** — encoded per `wsc-facts-encoding.md`
  (`01 01 01 02 00 04 8c04 f40b`), injected as a `wsc.facts` section → synth 0.31.0
  compiles it **`.text`-byte-identical** to the stripped module (phase-1 gate) and
  **keeps the fact silently** (no skew warning ⇒ parsed, not ignored);
- the normative fail-safe skew rule is confirmed on gale's body: unknown-version,
  truncated-framing, and unknown-kind sections each **warn on stderr and stay
  byte-identical** — no facts path can change a compile.

This script is the **gale-side tripwire for the lever going live**: the day a synth
build *consumes* the facts (phase-2, behind `SYNTH_FACT_SPEC`), the facts-carrying
`gust_mix` will compile *differently* from the stripped one — the oracle flips, and
that is the signal to run `gust_floor_bench` and measure the specialized `gust_mix`
against the **0.45× floor** above. Reported to synth#242/#494.

#### Phase-2 shipped (synth 0.32.0) — and first contact found a shape gap

synth **0.32.0** (#629) shipped phase-2: value-range ⇒ dead-branch elision behind
`SYNTH_FACT_SPEC` (default off), each elision carrying an ordeal `UNSAT(P ∧ cond≠0)`
LRAT-checked obligation. Ran it on gale's real `gust_mix`; two grounded blockers to
the 0.45× measurement through the shipped path:

1. **The prebuilt release binary declines-all.** `SYNTH_FACT_SPEC=1` on the
   `aarch64-apple-darwin` release warns *"built without the 'verify' feature — the
   per-elision proof obligation cannot be discharged, every elision is DECLINED"*
   (the #553 z3-free-default landing). DECLINE == general lowering, byte-identical
   (safety held). Measurement needs a `--features verify` binary/artifact.
2. **gale's clamp is `select`-shaped; #629 elides only no-`else` `if…end`.**
   `gust_mix`'s clamp lowers to **2× `i32.lt_s; select` (0 `if`)** at *every* stage —
   raw `gust_kernel.wasm` inner mix (func 1), before any loom pass, already emits
   `select` (it's the Rust `.clamp()` lowering, not a loom artifact). #629 walks
   no-`else` `if…end` and declines `if/else`; it has no `select` path. So even a
   verify build **declines on gale's gust_mix** — the prototype's if…end fixture and
   its own motivating target diverge in shape.

Reported both to synth#242 with the op stream, and the concrete next-increment ask:
**value-range ⇒ select-arm elision** (when `P` proves a `select` condition constant,
rewrite to the proven arm + drop the dead comparator — same obligation, `select`
instead of `if…end`; pairs with the design doc's select-totality fact kind 0x05).
gale is wired to measure same-day once (1) a verify binary exists and (2) select-arm
elision lands. Until then the 0.45× floor stays a `gust_floor_bench` datum, not yet
reachable through the shipped synth path.

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

## Driver-class module — thin-seam hardware timer (gust:hal, gust-OS v0.3.0 driver breadth)

The second v0.3.0 driver: a hardware timer as a verified thin-seam driver — STM32
timer config (PSC/ARR/CR1) + **wrap-safe deadline math** in verified wasm, importing
only `gust:hal/mmio` (**0 new TCB atoms**). Written **table-free from the start** (the
gpio lesson), so it dissolves `--relocatable` clean.

| | dissolved (loom 1.1.18 + synth 0.33.0, cortex-m3) |
|---|---|
| `.text` (flash) | **212 B** |
| SRAM | **0 B** |
| TCB | `mmio_read32`+`mmio_write32` only → **0 new atoms** |
| verified | Kani **3/3** (wrap-safe deadline: no missed/early fire across the u32 wrap) + `gust-timer-renode` register-effect gate; local qemu probe confirmed the dissolved .o before CI |

Reproduce: `drivers/timer-thin/RESULTS.md` + `renode-test/gust_timer.robot`.

## Driver-class module — thin-seam GPIO (gust:hal, gust-OS v0.3.0 driver breadth)

The first v0.3.0 driver-breadth module and the pattern-setter: proves the `gust:hal`
thin-seam model generalizes past UART/DMA to digital I/O. Whole STM32F1 GPIO protocol
(pin config, CRL/CRH placement, BSRR set/reset, IDR read) in verified wasm
(`drivers/gpio-thin`), importing **only `gust:hal/mmio`** — a strict subset of what
uart-thin needs (no irq), so **0 new TCB atoms**.

| | dissolved (loom 1.1.18 + synth 0.31.0, cortex-m3) |
|---|---|
| `.text` (flash) | **490 B** — configure 216 / toggle 110 / clear 56 / read 54 / set 52 |
| SRAM (`.bss`+`.data`) | **0 B** |
| TCB | **2 relocations — `mmio_read32`, `mmio_write32`** — subset of the existing 4-item TCB → **0 new atoms** |
| verified | Kani **4/4** (config total+injective+mode-consistent, slot in-range, unknown-mode-safe) + the `gust-gpio-renode` content-gate (dissolved driver drives PC8; asserts the exact CRH/BSRR values it writes on a real STM32 model over USART1) |

Composition note (REQ-DRV-BREADTH-001): two synth-dissolved `.o`s collide on their
internal `func_N` symbols, so the gpio + uart drivers can't be naively co-linked — a
multi-driver node must meld-fuse the drivers into one module first (one `func_N`
namespace), or synth must per-module-prefix internal symbols. Filed as a driver-breadth
follow-on; the gate sidesteps it by linking only gpio + a raw-USART report path.

Reproduce: `drivers/gpio-thin/RESULTS.md` + `renode-test/gust_gpio.robot`.

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
surface. See "The 0.7× floor — proof-carrying specialization" above (and
`optimization/wsc-facts-phase1.sh`, the ingestion tripwire) for the perf thesis.

Reproduce: `drivers/dma-own/RESULTS.md`.

# Wasm-cross-LTO via wasm-ld → loom → synth — spike report

**Status:** structurally proven, cyclically gapped by upstream tooling issues.
**Date:** 2026-05-10. **Re-run on upgraded tools: 2026-05-29 (see update below).**

---

## UPDATE 2026-06-02 — OPTIMIZATION PHASE begins (sem works; now widen the wasm surface)

Goal of this phase (30-min /loop, cron 1d91fb59): track new synth/loom releases, feed the
maintainer perf+optimization recommendations, and compile MORE of the bench through
`clang→wasm-ld→loom→synth` so there are more codegen paths to optimize. Plus a 4h-no-release
reminder rule: ping both repos + watch comments if either goes quiet 4h.

### synth v0.11.15 — regression PASS
v0.11.15 is one fix (#206, ARM32 indexed loads); release note says Cortex-M Thumb-2 was
already correct. Confirmed: rebuilt the sem object — **byte-identical** to the v0.11.14
`faithful4.o` (208+494 B code, 1588 B ELF, `cmp` clean). So the **907-cyc handoff transfers
without a reflash**. loom unchanged (v1.1.5).

### Second wasm function: the control ALGORITHM (`control_step`)
Reshaped to scalar in/out: `control_step_decide(rpm, load, coolant, knock) -> spark<<16|fuel`.
Pure integer + two static tables (`int8[20][20]`, `u16[20][20]`). **Compiles on v0.11.15**:
376 B, 0 relocs. New paths vs `decide`: integer division + a read-only data segment.
Artifacts: `/tmp/wasm-algo-poc/` (control_wasm.c, tables.c, control.loom.wasm, control_algo.o).

Hand-decoded Thumb-2 → filed **[synth#210](https://github.com/pulseengine/synth/issues/210)**:
- 🔴 **suspected coolant-operand drop**: `movw r2,#80; subs r3,r2,r2` → `(80-coolant)` becomes `80-80=0`,
  zeroing enrichment on the mid-range path (#193-class). Discriminator vector: `(3000,50,40,0)` →
  correct fuel **2645**; bug gives **2300**. Gates the algorithm's on-silicon number.
- 🟡 const-divisor strength-reduction + guard elision (4× `udiv`+`udf` for `/500 /5 /80 /1000`).
- 🟡 emit data segments as relocatable `.rodata` (R_ARM_THM_MOVW/MOVT_ABS) instead of baked
  linmem `[fp,off]` addresses with a 64 KB gap — read-only-data analogue of the call-reloc work.
- 🟡 regalloc: 6 spills with r9/r10 free + no constant CSE → 376 B vs ~100 B gcc.

**Host-side wasm testbed stood up (08:05):** `/tmp/wasm-algo-poc/wasm_oracle.sh` runs
`control_step_decide` in wasmtime 42 and checks vs the native reference — ALL 4 vectors PASS,
incl. the coolant discriminator `(3000,50,40,0)→fuel 2645`. Proves our wasm is correct, so
synth#210's coolant-drop is purely synth's ARM lowering — posted as confirmation on the issue.
First piece of "test environment as wasm": a fast, hardware-free functional oracle for any
loom/synth input. (Caveat: `wasmtime --invoke` must be one-shot per process — loop capture empties.)

**flight_control bigger example started (08:31):** compiled `controller_step` (the second function,
from the bigger composed bench) as a scalar-in/u32-out wasm shim → synth v0.11.15 gives 348 B, 0 relocs,
**functionally correct** in the wasmtime oracle (4 vectors, `asr.w` SAR lowering correct). Codegen
reinforces #210 (no constant CSE, verbose selects, spills) and surfaces a NEW recommendation: the
`[-127,127]` saturation clamps should use Thumb-2 **`SSAT`** (one instruction) instead of ~12-instruction
compare/boolean/select sequences ×3 axes. Posted to synth#210. Oracle (`wasm_oracle.sh`) now covers
engine_control algo + flight_control controller — ALL PASS. (`flight_control/src/control.c`.)

**filter_axis added (09:01):** the complementary-filter hot math `((prev+gyro)*980 + accel*20)/1000`
as a value-in shim → synth v0.11.15: 66 B, correct on 4 vectors incl. negative (`-1917`, sdiv toward
zero). Finding: **signed** constant division emits TWO dead guards (div-by-zero + INT_MIN/-1 overflow,
~12 of 66 bytes) since the divisor 1000 can be neither — sharper case of #210 Opt 1. Posted to #210.
Oracle now covers 3 functions (engine `control_step`, flight `controller_step`, flight `filter_axis`),
ALL PASS — wasm+native agree everywhere; only divergence is the control_step coolant-drop (#210 headline).

**ARM-level testbed + validated escalation (09:32):** built `/tmp/wasm-algo-poc/arm_harness.py` — runs
synth's Cortex-M output under unicorn (tables loaded at `[fp+0x10000]`/`[fp+0x10190]`, fp=linmem base,
args r0–r3). VALIDATED on a trivial `add3` (=60, matches wasmtime). Ran the engine `control_step_decide`
ARM output: returns `0xfc2703e8` vs wasm-correct `0x00210a55`. Tracing the table accesses pinned it:
`lb=8=40/5` → **`load_bin` is fed the coolant arg (r2=40) not load (r1=50)**, and `rb` is clobbered
between the two lookups (fuel reads row6 correct, spark reads row1). So #210 is broader than a dropped
coolant operand — it's **4-arg parameter→use mis-routing** (#193/#204 family). Posted validated
escalation + offered the harness (arm_harness.py + control_algo.text.bin + tables.blob) to synth#210.
This means the engine algorithm's silicon cycle number must wait for the param-routing fix (a buggy
cycle count would be unrepresentative — the control flow itself is wrong).

Testbed now has two layers: wasm oracle (3 fns, functional) + ARM-level unicorn harness (validates the
actual synth output).

## UPDATE 2026-06-02 10:01 — SECOND SILICON DATA POINT (filter_axis, wasm-cross-LTO vs native)

ARM-harness-checked all three functions' synth output: **filter_axis CORRECT** on ARM; control_step
mis-routes params (#210); controller_step has an **elevator saturation sign-flip** (+127→−127, upper
clamp miscompiled, 7-arg ABI is r0–r6). Since filter_axis is correct AND has no fp/table dependency
(clean AAPCS r0–r2 drop-in), measured it on silicon directly — bypassing the #210 blocker.

Built `/tmp/filter-bench` (standalone Zephyr app, links `filter_algo.o`; calls the synth symbol via a
function pointer with the thumb bit set, since synth#170 = no `$t` symbol). Flashed G474RE, DWT CYCCNT,
min/200, head-to-head vs native gcc -O2 of identical C (both via volatile fn-ptr = identical overhead):

| vector | synth (wasm→loom→synth) | native gcc | ratio |
|---|---|---|---|
| (1000,100,500) | 41 cyc | 21 cyc | 1.95× |
| (-2000,50,-300) | 41 cyc | 21 cyc | 1.95× |
| (0,2,0) | 39 | 19 | 2.05× |

**~1.95× native — matches the sem primitive's 1.92× (907/471).** Functionally correct on hardware.
On a function this tiny the whole gap is `/1000`: synth `sdiv` + 2 dead guards vs gcc reciprocal-multiply
(synth#210 Opt 1 = highest-leverage fix). Run artifact:
`silicon/runs/2026-06-02-nucleo_g474re-filter-axis-wasm-vs-native-v0.11.15/`. Posted silicon perf +
controller saturation bug to synth#210.

## UPDATE 2026-06-02 10:31 — gap DECOMPOSED on silicon (correction to the 1.95× figure)

Built variants v_full/v_nodiv/v_div through the pipeline, flashed, measured (DWT min/200, overhead now
min/200 back-to-back = **1 cyc** true floor; the earlier 1.95× used a single-sample ovh=5 overestimate).
Reproduced identically across two flashes:

| variant | synth | native | ratio |
|---|---|---|---|
| full `((p+g)*980+a*20)/1000` | 46 | 19 | 2.42× |
| nodiv `((p+g)*980+a*20)` | 29 | 12 | 2.42× |
| div `x/1000` | 40 | 15 | 2.67× |

**Decomposition of the 27-cyc gap:** base codegen (mul+add) = 17 cyc (spills/no-CSE) — the LARGER lever;
divide handling = 10 cyc (sdiv+2 dead guards vs reciprocal-multiply). So **Opt 3 (regalloc/CSE) ≥ Opt 1
(const-div)** — corrected my earlier "it's all the divide" claim on #210 (even divide-free code is 2.42×).
Together they'd take this function 46→~25, near native. Run: silicon/runs/2026-06-02-nucleo_g474re-
filter-decomposition-v0.11.15/. (filter_axis headline number corrected: 2.42×, not 1.95×.)

## UPDATE 2026-06-02 11:01 — bigger example (flight_control) RUNS on silicon (native baseline)

`flight_control` (the 5-primitive composed bench) builds + flashes + runs on the G474RE (Phases 1-4;
FLASH 23.7KB RAM 52.8KB). Native baseline, step 1/5 (sensor_hz=500, contention=0, payload=16), 6 events:
`algo=141 handoff=1307 t_lock=339 t_post=858 t_round=1987 t_bcast=748` cyc.
**STALL at step 2/5** (sensor_hz=1000, payload=32): `count=0 [drain_timeout]`, sweep never reaches
`=== END ===`. This is a flight_control **bench-logic** issue at the higher sensor rate (actuator-drain
path), NOT synth/loom — flag for the bench owner; the short-sweep 1000 Hz cell needs a drain fix in
`flight_control/src/main.c`. The bench `algo` segment (141 cyc) = filter_step+controller_step, i.e. the
same math already wasm-ified (controller_step + filter_axis); a wasm-cross-LTO flight run would swap that
segment in. Run: silicon/runs/2026-06-02-nucleo_g474re-flight_control-native-baseline/.

## UPDATE 2026-06-02 11:31 — #210 root cause LOCALIZED (arg-matrix diagnostic)

Ran a matrix through wasm→loom→synth→ARM in the unicorn harness: `add4`, `add4d` (4 args + divide),
`add5`, `add6` — ALL correct. Only `control_step` (4 args + 2 indexed table loads) mis-routes. So the
bug is **not** the multi-arg ABI and **not** divide-with-args; it's the **indexed-memory-load path**:
table-address materialization (movw/movt base + mul index) clobbers the param registers (r1=load,
r2=coolant) before `load_bin`/`rpm_bin` read them → `load_bin` sees coolant, `rb` clobbered between
lookups. Posted as root-cause narrowing on synth#210. Now HOLDING #210 (no maintainer reply yet today);
the 4h-no-activity reminder will nudge. `filter_axis` (no memory) stays correct + measured (2.42×), so
the value path is solid — this routing bug is the sole gate on table-based functions.

## UPDATE 2026-06-02 12:01 — 4h no-activity reminders filed

>4h since last release/maintainer activity. synth: posted a friendly status nudge on the existing #210
(didn't fragment a new issue) summarizing the silicon results + priority asks (param-routing fix > Opt1/
Opt3 > SSAT). loom: filed status/sync issue **loom#155** (907-cyc result + forward question on inlining
the flight_control *composed* algo — the next genuine two-function seam for loom). Both reminders now
tracked in state; future firings watch comments, don't duplicate; clock resets on next real release or a
maintainer comment.

## UPDATE 2026-06-02 12:32 — loom maintainer engaged (#155); composed module delivered

loom reply: aliasing is sound (single Z3 memory Array → read-sees-write by address equality); the real
limit is the #151 inliner only proving pure/no-trap/leaf/straight-line/**no-memory** callees, so a
memory-reading callee (`controller_step` reads `flight_state`) **reverts** (sound no-op, not a miscompile).
They committed to building **general memory-through inline verification** and asked for our multi-fn module
+ oracle as the validation substrate. DELIVERED: built `flight_algo` = `filter_step`(writes *st) →
`controller_step`(reads *st) as a 3-function wasm module (real through-memory seam), verified correct in
wasmtime (`drive(...)=0x07fdf307` = native bit-for-bit), confirmed loom v1.1.5 reverts it as predicted,
posted module + recipe + base64 + oracle/harness to loom#155. Artifacts: /tmp/flight-wasm/.

## UPDATE 2026-06-02 13:01 — #210 regalloc bug GENERALIZES to pointer params (composed algo blocked)

Tried to get the synth "before" (loom-reverted) silicon number for the composed `flight_algo`. Compiled
it (flight_algo 62B + filter_step 256B + controller_step 414B, 2 internal-call relocs; `drive` skipped),
linked the internal calls, ran in the unicorn harness with **fp=0**. Result: fp=0 contract HOLDS and the
entry pointers are correct (r0=&st, r1=&s), but `filter_step` **clobbers the `s` pointer register** mid-body
(r1: 0x20001100 → 0xfffffe70, a loaded/computed value) → unmapped read. So the composed algo hits the
**same #210 regalloc/param-clobber bug, now generalized to POINTER PARAMS** — not just table indices: any
memory-accessing function can overwrite a live pointer/param register. ⇒ composed-algo silicon number is
BLOCKED on the #210 fix; **scope note for the maintainer's next #210 reply**: the fix must keep live
pointer/param regs across memory-accessing bodies, not only table-index materialization. (Held the comment
— #210 already has the root cause; staged this generalization for when they re-engage.)

## UPDATE 2026-06-02 13:32 — loom ACTIVELY IMPLEMENTING memory-through inlining

loom maintainer confirmed the substrate + oracle (`0x07FDF307` is now their differential gate), reproduced
the v1.1.5 no-op revert, and root-caused **two general fixes**: (1) thread the caller's memory `Array` into
the callee-body modeling + share the load-encoding helper with the main encoder (→ bit-identical Z3 for
modeled-call vs inlined-body), (2) deterministic per-encode havoc naming so corresponding impure calls
unify (else spurious revert). Implementing now behind Z3 + negative-soundness + my-oracle + corpus gates;
will cut a release + report before/after. I hardened their gate with **5 verified native==wasmtime vectors**
(v2/v4 hit controller_step's ±127 saturation rails) and flagged that synth v0.11.15 miscompiles this
composed module on ARM (filter_step clobbers the s-ptr — pointer-param generalization of #210), so the
synth-side silicon number may lag the loom-side proof (the inlined output might dodge it — fewer live ptrs).

## UPDATE 2026-06-02 14:02 — loom BUILT memory-through inline verification; helping land controller_step

loom now verifies memory-reading inlines as a sound capability (minimal `getx(p)` inlines+verifies;
soundness guard: correct load proven, wrong-offset rejected; deterministic havoc; 387 tests; oracle
0x07FDF307 holds) — branch `feat/155-memory-through-inline-verification`. controller_step not landing yet
for 3 reasons, helped on all: (1) `drive` made it 2-call-site (clang folded flight_algo into drive) → sent
**drive-free flight_seam.wasm** (single-call = on-device); (2) filter_step's partial-width `i32.load16_s`
unmodeled → stays opaque/havoc, only controller_step needs inlining (full-width); (3) even single-call it
hits a FALSE counterexample (sound conservative revert) → built+sent **min_seam.wasm** (minimal `seam=wr;rd`,
rd reads what wr wrote with controller_step's clamp/select suspect, rd single-call, 2 real calls verified) +
native gate table, to bisect; pointed to `benches/flight_control/src/control.c`. Artifacts
/tmp/flight-wasm/{flight_seam,min_seam}.wasm.

## UPDATE 2026-06-02 15:02 — loom v1.1.6 SHIPPED (our feature); verified; silicon gated on synth#210

loom **v1.1.6** released: "Verified inlining of memory-reading callees (gale flight_control seam, #155)" —
the feature our substrate drove. My isolation modules pinned the false counterexample (it was the *writer*,
not select/param-reuse → loom now keeps memory-WRITING callees opaque, inlines memory-READING ones, Z3-
proven). Built+installed v1.1.6 from source (loom-cli; no binary asset). VERIFIED on our modules:
- `flight_seam`: controller_step inlined (714→826B wasm; synth ARM output relocs 2→1, flight_algo 62→488B).
- Oracle on v1.1.6 output: `drive=0x07FDF307` + saturation vectors (v2=0x07817F81, v4=0x0700817F) PASS.
⇒ loom side DONE + correct. **Silicon cycle delta GATED on synth#210**: the v1.1.6-inlined module still
faults in the fp=0 unicorn harness at pc=0x25a *inside the kept `filter_step` call* (synth pointer-clobber,
unchanged). Inlining controller_step doesn't dodge it. Delivered the staged #210 generalization (bug hits
pointer params, not just tables) + connected to the loom milestone — #210 now blocks BOTH the engine algo
and the flight composed-algo silicon numbers. Artifacts /tmp/flight-wasm/*.116.wasm.

**15:32 follow-up:** loom maintainer confirmed #155 COMPLETE (controller_step inline shipped+verified;
silicon = synth's problem, not loom's) and invited the writer side. Filed **loom#157** (inline memory-
WRITING callees / by-body store modeling) with the min_seam writer repro (full-width stores) + differential
gate — loom-independent of synth, advances toward full filter_step dissolution (also needs partial-width
loads, noted). loom#155 done; loom#157 open.

## UPDATE 2026-06-02 16:32 — loom v1.1.7 (writer side); KEY FINDING + a regression

loom **v1.1.7** shipped the writer-side inline (#157, requested ~1h earlier): `min_seam` now **fully
dissolves** (both wr+rd inline, 0 calls). **KEY FINDING:** the fully-dissolved min_seam runs **correctly on
Cortex-M** (unicorn fp=0) — synth#210's pointer-clobber only bites at CALL boundaries, so a **fully-inlined
`flight_algo` would dodge #210 entirely**, enabling the composed-algo silicon measurement WITHOUT the synth
fix. ⇒ new primary path to the flight number = loom full dissolution, not waiting on synth#210.

**BUT v1.1.7 REGRESSION (filed loom#159):** `flight_seam` no longer inlines `controller_step` (v1.1.6 did:
1 call; v1.1.7: 2 calls, neither inlined). Trigger = reader-after-partial-width-writer (min_seam full-width
control still dissolves). The v1.1.7 writer path attempting the un-modelable partial-width `filter_step` now
also poisons the following reader-inline. Filed with evidence + min_seam control + the `seam16` partial-width
repro (the #157 follow-up). Not a miscompile (oracle holds) — a lost-optimization regression.

## UPDATE 2026-06-02 17:02 — CORRECTION: full dissolution does NOT dodge synth#210

Tested the 16:32 "full dissolution dodges #210" claim by manually fully-inlining flight_algo into ONE flat
function (0 calls) → synth → ARM → unicorn: **still clobbers** r1 (s-ptr) → 0xfffffe70 at pc=0x76, *identical*
to the called version. So **synth#210 is REGISTER-PRESSURE-driven, not call-boundary-driven** (the engine
`control_step` was also a single function). The min_seam "flat=clean" result was misleading — min_seam is
trivially small (stays under the pressure threshold); flight_algo's many int16 fields trip it regardless of
inlining. ⇒ **full dissolution does NOT unblock the flight silicon number; synth#210 is the singular blocker.**
Corrected on loom#159 (the v1.1.7 regression there is still real as an optimization matter, just not a silicon
unblock). New synth#210 data point (staged): clobber reproduces on a flat no-call function → purely regalloc
not keeping pointer/param regs live across memory-address materialization.

## UPDATE 2026-06-02 18:02 — synth v0.11.16 FIXES #210; ENGINE control_step ON SILICON (2nd data point ✅)

synth **v0.11.16** (a804390) landed the #193 param-liveness reservation → the coolant clobber is **fixed**.
VERIFIED: `control_step` correct on ARM (all 4 vectors, coolant=40→0x00210a55/fuel 2645) AND on real silicon
(selfcheck synth==native==0x00210a55). The manually-flat flight_algo is also correct now.

**MEASURED the engine `control_step` on silicon** (the original part-3 "second data point" goal — ACHIEVED):

| function | wasm-cross-LTO (v0.11.16) | native gcc | ratio |
|----------|---------------------------|-----------|-------|
| k_sem_give handoff | 907 | 471 (LLVM-LTO) | 1.92× |
| filter_axis | 46 | 19 | 2.42× |
| **control_step (engine algo)** | **188** | **80** | **2.35×** |

(DWT min/200; synth path includes the fp-setup trampoline pointing `[fp+0x10000]` at the lookup-table blob.)
Gap dominated by 4× `udiv` (binning divides + dead guards) + table-index regalloc = synth#209 Opt1/Opt3.
Run: silicon/runs/2026-06-02-nucleo_g474re-control_step-engine-algo-synth-v0.11.16/. Reported to synth#210.

## UPDATE 2026-06-02 18:43 — loom v1.1.9 (partial-width); synth#212 + loom#163 filed

loom **v1.1.9** shipped partial-width inlining (our #161): verified seam16→0 calls (fully dissolves),
flight_seam→1 (filter_step now blocked ONLY by `i32.div_s`, not partial-width), divseam→1 (rdd div-blocked).
synth#210 correctness CLOSED + maintainer **starting Opt1 (div SR + guard elision)** — re-sent self-contained
control.loom.wasm + 4 vectors for their regression guard. FILED **synth#212**: inlined-controller-after-opaque-
call reads stale divided fields on v0.11.16 (cmd 0x07fd7f7f vs 0x07fdf307 — ail/ele wrong via ÷-path, rud/yaw
+state correct; FLAT version correct; loom wasm correct in wasmtime ⇒ synth lowering bug; maintainer requested).
FILED **loom#163**: div_s modeling (divseam repro, signed-trunc vectors) = last piece for full flight_algo dissolution.

## UPDATE 2026-06-02 19:42 — loom v1.1.10 FULL dissolution; COMPOSED flight_algo ON SILICON (3.35×)

loom **v1.1.10** (div_s, #163) → `flight_algo` **fully dissolves** (filter_step+controller_step both inline,
0 calls, one 784B fn; verified unicorn-correct + oracle 0x07FDF307). Compiled through synth v0.11.16 → ARM →
**measured on silicon: synth 332 vs native 99 (flat) = 3.35×, CORRECT (0x07fdf307)**. The bigger-example
composed algo, fully fused, running on hardware. Run: silicon/runs/2026-06-02-nucleo_g474re-flight_algo-
FULLY-DISSOLVED-loom1.1.10-synth0.11.16/. Posted to loom#163.

**Full silicon table (wasm-cross-LTO vs native):**
| | synth | native | ratio |
|---|---|---|---|
| k_sem_give handoff | 907 | 471 (LLVM-LTO) | 1.92× |
| filter_axis | 46 | 19 | 2.42× |
| control_step (engine algo) | 188 | 80 | 2.35× |
| flight_algo (composed, fully dissolved) | 332 | 99 | 3.35× |

CORRECTION: fp=0 + real pointers WORKS on HW — the earlier flat_flight "silence" was serial-capture flakiness
(0 bytes), NOT a fault. Retracted the "pointer/linmem HW blocker" worry. Lesson: capture occasionally misses → retry.

## UPDATE 2026-06-02 20:12 — v0.11.17 guard-elision: control_step −14 cyc, BUT regresses flight_algo (#215)

synth **v0.11.17** (Opt1 guard-elision) measured on silicon: **control_step 188→174** (−14 cyc, 2.35×→2.15×,
correct 0x00210a55; .text 390→364). Reported to #209. **BUT v0.11.17 REGRESSES the fully-dissolved flight_algo**
(0x07FDF307 v0.11.16 → 0x20FD1014 v0.11.17 — controller-tail fields wrong, filter/divides correct; control_step
unaffected) → filed **synth#215** (confirmed in unicorn; guard elision perturbs regalloc/layout in the bigger fn).
synth#212 (inlined-after-call) FIXED in **v0.11.18** (building, R12/IP reserve). loom#163 closed — loom capability
series COMPLETE (maintainer graceful close-out).

## UPDATE 2026-06-02 20:42 — v0.11.18 FIXES #212 + #215; all 4 data points correct + guard-elision

synth **v0.11.18** (R12/IP scratch reserve) FIXED both #212 (inlined-after-call) and #215 (guard-elision
flight_algo regression). Re-measured on silicon, both correct:
- control_step: 188(v16)→174(v17)→**172**(v18), native 80 → **2.15×** (0x00210a55).
- flight_algo (composed, fully dissolved): 332(v16)→wrong(v17 #215)→**315**(v18), native 99 → **3.18×** (0x07FDF307).

Confirmed #215 fixed (unicorn + silicon); reported v0.11.18 numbers to #209. **Current silicon table:**
| | synth | native | ratio |
|---|---|---|---|
| k_sem_give handoff | 907 | 471 (LLVM-LTO) | 1.92× |
| filter_axis | 46 | 19 | 2.42× |
| control_step | 172 | 80 | 2.15× |
| flight_algo (composed) | 315 | 99 | 3.18× |

## UPDATE 2026-06-02 21:42 — v0.11.19 (Opt1b reciprocal-mul) REGRESSES control_step (won't compile)

synth **v0.11.19** = Opt1b (UMULL reciprocal-multiply for *unsigned* constant division, #209). #215 closed.
REGRESSION: **control_step FAILS to compile** — "register exhaustion: all allocatable registers live on the
stack." It's the only fn with **multiple unsigned-const divides** (/500 /5 /80 /1000) → 4× UMULL (each
RdLo+RdHi+magic+multiplicand) under the R0–R8 pool (post-#212 R12 reserve) exhausts the allocator (bails vs
spills). Scope: ONLY control_step; signed-div fns (filter_axis, flight_algo, divseam) + no-div fns OK (Opt1b
is unsigned-only). Compiled fine on v0.11.18 (172 cyc). Filed on #209 (comment 4606581465) w/ 3 fix options
(spill-under-pressure / cost-gate reciprocal-mul→udiv fallback / recompute magic per-use). flight_algo
unchanged on v0.11.19 (signed divides untouched → no reflash).

## UPDATE 2026-06-02 23:12 — v0.11.20 Opt1b measured: NEAR-NEUTRAL on M4; Opt3 is the real lever

synth **v0.11.20** (#217 cost-gate fix) → control_step compiles with all 4 UMULL (udiv:0), correct. Silicon:
188(v16)→172(v18 guard-elision)→**168(v20 reciprocal-mul)**, native 81 → **2.07×**. Opt1b bought only **−4 cyc**.
KEY (honest, corrects my ~110-135 prediction): **Cortex-M4 has fast HW `udiv` (~2-12 cyc), so reciprocal-multiply
is near-neutral here** (a win only on M0/M0+ without HW divide; grew code 350→410 B). ⇒ the remaining ~2× gap is
**general codegen (regalloc/CSE/spilling = Opt3)**, NOT division — confirmed by the filter_axis decomposition AND
now control_step (divides fully optimized, still 2×). Redirected the lever to Opt3 on #209.

**Standing silicon table (all correct on HW):** sem 1.92× | filter_axis 2.42× | control_step **2.07×** (v0.11.20
full Opt1) | flight_algo(composed) 3.18× (v0.11.18). loom done.

NEXT: watch #209 for **Opt3 (regalloc/CSE)** = the real 2× lever → re-measure on land. Run:
silicon/runs/2026-06-02-nucleo_g474re-control_step-v0.11.20-Opt1b-reciprocal-mul/. Testbed `../wasm-testbed/`.

---

## UPDATE 2026-05-29 — re-run on synth v0.11.0 / loom v1.1.1 / meld v0.19.0

Tools upgraded (synth v0.3.0→v0.11.0, loom v1.0.3→v1.1.1, meld v0.2.0→v0.19.0).
The two **original** blockers are fixed; **two new blockers** took their place, so
the silicon-cycle measurement is still gated.

### What got fixed
- **synth#120 vreg-unmapped panic — FIXED.** 194 functions (incl. the previously
  panicking `gale_compute_ipi_mask`) now compile clean. `compiler_builtins::float::div`
  now produces a clean `Result::Err`, not a panic (v0.5.0 panic→Result conversion holds).
- **loom v1.0.3 inline hang — GONE.** v1.1.1 no longer hangs; it catches each Z3
  failure and reverts the function, so the run completes.

### New blockers (filed 2026-05-29)
- **[synth#167](https://github.com/pulseengine/synth/issues/167) (BLOCKER).** synth's
  `arm` backend now lowers **every** wasm `call` (internal *and* import) to an identical
  `__meld_dispatch_import` placeholder `bl` (`f000 d000` → garbage `0xC00000`+idx), with
  **0 relocations and no inlining** — in both `--relocatable` and full `--cortex-m` modes.
  Minimal repro: a 2-func no-import module's internal `call $callee` becomes `bl <garbage>`
  even though the callee sits at a known address in the same object. The `synth backends`
  table marks the `arm` backend `ELF: no`. ⇒ The merged `.o` is **non-linkable** into
  Zephyr; this is a regression vs v0.3.0 (which produced linkable objects). **This is the
  reason the seam no longer dissolves on v0.11.0** — `gale_k_sem_give_decide` is not
  inlined and its call is an unrelocated placeholder.
- **[synth#168](https://github.com/pulseengine/synth/issues/168).** `arm` regalloc:
  "register exhaustion: no consecutive pair of free registers for i64" hard-fails on
  `compiler_builtins::float::div::div`. Dodgeable by not linking float div (see below).
- **[loom#145](https://github.com/pulseengine/loom/issues/145).** Z3 `SortDiffers
  { BitVec 64 vs 32 }` (the old #98, claimed fixed in v1.0.0) **still reproduces** on
  i64-heavy modules, plus new `bool.rs:70` / `bv.rs:174` `unwrap()`-on-`None` panics.
  Now caught+reverted (good) but the inliner is a no-op on i64 and emits 21 MB+ of panic
  spew (5796 + 2511 + 773 panics on the 1 MB merged module).

### Local recipe fix discovered this run
The original recipe used `wasm-ld --whole-archive --export-all`, which pulls in ALL of
compiler_builtins (incl. the float::div that trips synth#168). For the hot-path target,
re-link **without** `--whole-archive` and export only the needed symbols — the linker's
DCE drops float::div and shrinks the module 1 MB → ~5 KB:

```sh
wasm-ld --no-entry --export=z_impl_k_sem_give --export=gale_k_sem_give_decide \
  --allow-undefined --gc-sections libgale_ffi.a shim.wasm.o -o merged.both.wasm
```

This sidesteps synth#168 entirely. The route still hits synth#167 (calls become
meld-dispatch placeholders), so **no linkable object / no silicon measurement yet.**
Blocked on synth#167.

### UPDATE 2026-05-30 — synth v0.11.1 retest (#167 + #168 FIXED)

synth **v0.11.1** (`6f7e3ac8`, PR #169) fixes both:

- **synth#167 FIXED.** Internal + import calls now emit real `R_ARM_THM_CALL`
  relocations with the corrected `f000 f800` Thumb-BL placeholder (was `f000 d000`,
  which baked in a ~+0x600000 garbage addend). `merged.both.wasm` → `merged.both.o`
  now carries 7 relocations (the `z_impl → gale_k_sem_give_decide` seam call as
  `R_ARM_THM_CALL → func_6`, plus the 5 kernel imports) and links with `ld -r`.
  Note the seam is a **relocated call, not inlined** — so this route currently maps
  to a rustc-direct-shaped seam (≈574 cyc class), not LLVM-LTO's folded body
  (471 cyc). Inlining is a separate synth/loom capability (synth#170 / loom#145).
- **synth#168 FIXED.** `--all-exports` on the full 1 MB module no longer hard-fails;
  synth emits `warning: skipping function … register exhaustion` and continues —
  750/824 functions compiled, 74 i64-heavy compiler_builtins (incl. `float::div`)
  skipped. Graceful degradation.

**New remaining blocker — [synth#173](https://github.com/pulseengine/synth/issues/173).**
The import-call relocations target generic `func_N` symbols, NOT the wasm import
field names (`k_spin_lock`, `z_unpend_first_thread`, …). synth knows the names (it
logs them) but emits none into the ELF, so the object still can't resolve against
the real Zephyr kernel. A `--defsym func_0=k_spin_lock …` aliasing layer is a viable
local stopgap to chase the silicon number; proper fix is upstream (#173).

### UPDATE 2026-05-30b — `--defsym` workaround test surfaced a +4 call bug ([synth#174](https://github.com/pulseengine/synth/issues/174))

Applied the `--defsym func_N=<real kernel names>` workaround for #173 + a stub object,
linked `merged.both.o` to an absolute image — links cleanly, all symbols resolved. BUT
raw-Thumb disasm shows **every `R_ARM_THM_CALL` resolves to symbol+4**: `z_impl_k_sem_give`
calls `gale_k_sem_give_decide+4` (skipping its first two `movs`), `k_spin_lock+4`, etc.
Minimal repro: `caller`→`callee` lands at `callee+0x4`. The `0xF800` "zero" placeholder
from the #167 fix nets to +4 under R_ARM_THM_CALL. **So even with #173 worked around, the
linked code is wrong** — calls enter callees one instruction-pair late.

Also confirmed: synth emits no `$t`/`$d` mapping symbols, so neither `arm-zephyr-eabi-objdump`
nor `llvm-objdump` will decode the output (must dump `.text` to flat binary + `-Mforce-thumb`).
That's tracked in synth#170.

**Net gating after the v0.11.1 retest:** #174 (+4 call offset, BLOCKER for execution) →
#173 (import naming) → #170 ($t mapping symbols / standalone resolution) → then the seam is
a relocated call (not inlined), so silicon would land in the rustc-direct class, not LTO parity.

### UPDATE 2026-05-30c — synth v0.11.2 (#173 FIXED; #174 still blocks)

v0.11.2 (`f11ed177`): **#173 FIXED** — import-call relocations now carry the real wasm
field names (`k_spin_lock`, `z_unpend_first_thread`, …), so the merged object links against
the Zephyr kernel with **no `--defsym` aliasing**. **#174 still reproduces** (mini.wat:
`bl 4 <callee+0x4>`) — the +4 call offset remains the execution blocker. #170 still open.
Net gating now: **#174 (+4, BLOCKER)** → #170 (mapping symbols) → relocated-call-not-inlined.

### UPDATE 2026-05-30d — synth `8f213351` (#174 FIXED): TOOLCHAIN NOW PRODUCES CORRECT LINKABLE CODE ✅

synth commit `8f213351` (#177) fixes #174 (BL placeholder -4 addend). Retest:

- **minimal**: `caller` → `bl 0x8000000` = `callee` exactly (was `callee+0x4`).
- **gale seam** (`merged.both.wasm` → `merged.both.o`, linked with real-named stubs, **no
  --defsym**): all 6 calls in `z_impl_k_sem_give` resolve to the exact symbol entry —
  `gale_k_sem_give_decide`@0x0 (the C↔Rust seam), `k_spin_lock`, `z_unpend_first_thread`,
  `arch_thread_return_value_set`, `z_ready_thread`, `z_reschedule`.

**The wasm-ld → synth → ARM ET_REL route now emits a correct object that links against the
Zephyr kernel.** Closed across the loop: #167, #168, #173, #174. Remaining: #170 ($t/$d
mapping symbols + standalone --cortex-m resolution) — disassembly/standalone only, NOT an
execution blocker for the --relocatable→ld path (function symbols carry the Thumb bit).

**What's left to an actual silicon number:** (1) integrate `merged.both.o` into the bench —
override the bench's native `z_impl_k_sem_give` so the linker pulls ours, provide the panic
stub (func_5), build + flash the G474RE; (2) capture cycles. Expected class: the seam is a
**relocated call, not inlined**, so ≈ rustc-direct (~574 cyc), NOT LTO parity (471). LTO
parity needs synth internal-call inlining or loom's verified inliner (loom#145, i64-blocked).

### UPDATE 2026-05-30e — on-target test reveals a FUNDAMENTAL blocker ([synth#178](https://github.com/pulseengine/synth/issues/178))

Drove toward the actual silicon run. The wasm shim's `z_impl_k_sem_give` **dereferences a
pointer** (`sem->count`, the kernel-passed `struct k_sem *`). Decoding the compiled body:

```
movw ip,#0x100; movt ip,#0x2000   → ip = 0x20000100  (FIXED)
ldr.w r8,[ip]                      → reads sem->count from 0x20000100, NOT from r0 (the sem ptr)
```

synth's **optimizer constant-folds the dynamic load address to a fixed `0x20000100`, ignoring
the pointer parameter**. Minimal repro `(func (param i32)(result i32) local.get 0 i32.load)`
→ same. `--no-optimize` is correct (`ldr [fp, r0]`), so it's an optimizer bug → filed
**synth#178**. Secondary architectural point (also in #178): even `--no-optimize` uses `fp` as
a linmem base, so a native drop-in needs `linmem_base=0` / a native-pointer ABI.

**Why this is fundamental:** gale's verified `decide(count, limit, has_waiter)` takes **values**,
so it compiles fine (that's the working `GALE_USE_SYNTH` path, 582 cyc, seam intact). The whole
point of the wasm-LTO shim was to move `z_impl` — which **must deref the host `k_sem` pointer** —
into wasm so the seam dissolves. But wasm's linear-memory model doesn't map a native pointer,
so a host-pointer-dereferencing function can't currently be a native drop-in. **The seam can be
dissolved only for value-only functions — but then there's no host-pointer seam to dissolve.**

Did NOT flash: the code provably targets the wrong RAM (0x20000100), so a flash would only
confirm a known-broken result. Blocked on synth#178 (+ native-pointer ABI). Loop `e4b93e62`
continues; will retest on the next synth release.

---

**Goal:** demonstrate that the PulseEngine pipeline (`wasm-ld → loom → synth`) can dissolve the C↔Rust FFI seam at wasm-IR level, producing silicon timing equivalent to LLVM-LTO.

## TL;DR

The toolchain inlines through the seam — synth's emitted ARM .o has `z_impl_k_sem_give` with no `bl gale_k_sem_give_decide`, the Rust decision body is folded into the C function. Two blockers prevent silicon-LTO parity:

1. **loom Z3 backend panics on i64 sort** (`SortDiffers { left: (_ BitVec 64), right: (_ BitVec 32) }`) — the formally-verified inliner reverts every function on the gale-ffi wasm. Without loom, we lose verification + the inliner shape that matches LLVM-LTO.
2. **synth emits 1.68× larger ARM than LLVM-LTO** for the inlined body (138 vs 82 bytes), because it doesn't recognize the u64-packed FFI return pattern and falls back to generic 64-bit shift-and-mask.

## Reproduction commands

```sh
mkdir -p /tmp/wasm-shim-poc && cd /tmp/wasm-shim-poc

# 1. Compile a wasm-portable host shim that mimics z_impl_k_sem_give's
#    hot path with kernel APIs as externs (which become wasm imports).
#    See wasm_host_shim.c in this dir.

clang --target=wasm32-unknown-unknown -O2 -nostdlib \
  -c wasm_host_shim.c -o shim.wasm.o

# 2. Build gale-ffi as a wasm32 static archive (already produced by
#    the bench's GALE_USE_SYNTH=ON path; can also build manually):
( cd /Volumes/Home/git/pulseengine/gale-smart-data/ffi && \
  cargo rustc --release --target wasm32-unknown-unknown --crate-type=staticlib )

# 3. wasm-ld static-links them into a single core wasm module.
wasm-ld --no-entry --export-all --allow-undefined \
  --whole-archive /Volumes/Home/git/pulseengine/gale-smart-data/ffi/target/wasm32-unknown-unknown/release/libgale_ffi.a \
  --no-whole-archive shim.wasm.o \
  -o merged.wasm

# 4. Synth → ARM ET_REL. Both z_impl_k_sem_give and
#    gale_k_sem_give_decide end up in the .o, with the latter inlined
#    into the former (no bl in z_impl's body).
synth compile merged.wasm --target cortex-m4f --all-exports --relocatable -o merged.o

# 5. Verify the inlining empirically.
arm-zephyr-eabi-objdump -d merged.o | awk '/<z_impl_k_sem_give>:/,/^$/' | grep -E '\bbl\b'  # should be empty
arm-zephyr-eabi-nm --print-size merged.o | grep "z_impl_k_sem_give$"                          # 138 bytes
```

## Comparison vs LLVM-LTO (silicon-measured baseline)

```
                                         body size   silicon handoff (cyc)
                                                       ADC=n  ADC=y
  baseline (no Gale)                     —            528     506
  rustc-direct (FFI bl preserved)        —            574     582
  LLVM-LTO (cross-language inliner)      82 bytes     471     558    ← gold
  wasm→synth (Rust only, seam intact)    n/a          —       582
  wasm-ld merge → synth (this spike)     138 bytes    not yet measured
```

The "not yet measured" cell needs a CMake `GALE_USE_SHIM_WASM` flag that swaps the bench's native `gale_sem.c` for the wasm-merge-derived archive. Estimated silicon impact based on the 1.68× code-size ratio: ~510 cyc handoff (ADC=n), still better than rustc-direct's 574 but worse than LTO's 471. That estimate is to-be-validated.

## What synth produces (vs LTO, for the same inlined body)

LTO version of the no-waiter path (from
silicon/runs/.../gale-lto-noadc-systick/firmware.elf):

```
8004398: ldrd r2, r1, [r0, #8]  ; load count, limit
800439c: cmp  r2, r1            ; the only check
800439e: it   cc
80043a0: addcc r2, #1            ; saturate increment
80043a2: str  r2, [r0, #8]       ; write back
```

5 instructions. LLVM saw both C's `cmp/it/addne` and Rust's identical
check and dedup'd them into one.

Synth version of the same path (from /tmp/wasm-shim-poc/merged.o
z_impl_k_sem_give at 0x2f5e4):

```
2f5ec: mov  r5, r0
2f5ee: movw ip, #256
2f5f2: movt ip, #8192
2f5f8: ldr.w r3, [ip]            ; loads via abs address (wasm linear-mem mapping)
2f5fc..2f60e: more abs-address loads + waiter check
2f61c: and.w r0, r0, r2          ; mask u64 → u8 action
2f620: and.w r1, r1, r3
2f624: cmp action with WAKE constant
2f63a..2f660: 64-bit lsl/lsr/orr to extract new_count from u64-packed return
2f668: bx lr
```

~30 instructions. Sub-optimal for two reasons: (a) wasm linear-memory
loads use absolute address pairs (`movw + movt`) instead of relative
offsets from a base register, and (b) the u64-packed decision is
unpacked via generic shifts instead of a direct field access.

Both are fixable in synth's backend.

## Full integration attempt + new finding — synth memset is broken

After the initial spike I attempted full integration into the bench
to get silicon-measurable timing. The integration path:

  1. Edit `zephyr/gale_sem.c` to wrap `z_impl_k_sem_give` in
     `#ifndef GALE_WASM_LTO_OVERRIDE_SEM_GIVE` so the bench's native
     compilation skips it.
  2. Build merged.wasm via wasm-ld → synth → ARM ET_REL.
  3. Wrap merged.o in libgale_ffi.a using arm-zephyr-eabi-ar (not
     Apple ar — Apple ar's ranlib doesn't index ARM ELF symbols
     correctly, archive shows up as "no global symbols").
  4. Place the merged libgale_ffi.a at the bench's expected path
     (replacing the rustc-direct cargo output).
  5. Build with `-DEXTRA_CFLAGS=-DGALE_WASM_LTO_OVERRIDE_SEM_GIVE=1
     -DEXTRA_LDFLAGS=-Wl,--allow-multiple-definition`.

The build succeeded — final ELF: 219 KB FLASH (vs LTO's 26 KB), 66 KB
RAM. Linker warnings about `.meld_import_table` orphan section
(synth-emitted, lands at VMA 0 in non-loaded section, harmless).

**But the chip doesn't boot.** PC stays in the synth-emitted `memset`
function (`0x0802c614`, 454 bytes) for >10 seconds, bouncing between
0x0802c668 and 0x0802c67e — a tight inner loop that doesn't terminate
correctly on the boundaries Zephyr's startup uses. Zephyr's z_bss_zero
calls memset(bss_start, 0, bss_size); synth's memset never returns.

Workaround attempts that didn't stick:
- `--allow-multiple-definition` + `objcopy --weaken-symbol=memset`:
  weak-vs-strong resolution didn't override; ld picked synth's.
- `objcopy --redefine-sym memset=__synth_memset`: renamed the C-symbol
  but the Rust mangled `_ZN17compiler_builtins3mem6memset17h...E`
  remained, and the final ELF still resolves `memset` to synth's
  buggy code at 0x0802c614 (the bytes are still there from merged.o's
  .text section).
- `objcopy --strip-symbol=memset`: removes the symbol table entry
  but doesn't remove the bytes; ld still places synth's code at
  0x0802c614 and exposes it as `memset` from another reference.

The root cause is that synth's wasm-to-ARM lowering of memset
produces a loop that doesn't terminate on the boundaries Zephyr's
startup uses (`memset(bss, 0, bss_size)` where bss_size is in bytes,
8-byte aligned). The synth output disassembles to a pattern with
`subs.w r3, r2, #32; bpl.n ...; rsb r3, r2, #32; lsl.w r3, r1, r3 …`
which is the i64-shift-with-bytecount pattern from Rust's u64
left-shift implementation — synth seems to have lowered memset's
inner loop using i64 shift operations that don't apply to byte
counts.

## Action items, filed upstream

| # | upstream issue | severity | summary |
|---|---|---|---|
| 1 | [pulseengine/synth#93](https://github.com/pulseengine/synth/issues/93) | **blocker** | memset/memcpy/memmove i64-codegen non-termination — chip hangs in memset+0x4c on `z_bss_zero` |
| 2 | [pulseengine/synth#94](https://github.com/pulseengine/synth/issues/94) | enhancement | u64-packed FFI return unpacking — register-direct field access vs generic 64-bit shifts (~50% of LTO size gap) |
| 3 | [pulseengine/synth#95](https://github.com/pulseengine/synth/issues/95) | enhancement | wasm linear-memory access lowering — base+offset vs movw+movt+ldr (~20% of LTO size gap) |
| 4 | [pulseengine/loom#98](https://github.com/pulseengine/loom/issues/98) | bug | Z3 SortDiffers panic in inline_functions on i64 — every gale-ffi function reverts, inliner is no-op |

### Status detail per issue

**synth#93 (blocker).** The wasm→ARM lowering of compiler_builtins' memset
produces a non-terminating loop on Zephyr's startup
`memset(bss, 0, sizeof(bss))` invocation. The chip hangs at
memset+0x4c forever. Until this is fixed, no integration of
merged-wasm into a real bench can boot.

**synth#94.** When synth lowers a wasm function that returns i64 and the
caller immediately bit-masks into byte-fields, it currently emits
generic 64-bit shift extraction. Recognizing the packed-struct-return
pattern would close ~50% of the LTO-parity size gap.

**synth#95.** wasm `i32.load` from a constant address currently emits
`movw + movt + ldr` (10 bytes). Conventional ARM ABI uses base+offset
addressing (2-4 bytes per load).

**loom#98.** Z3 backend's i64 sort handling chokes on `BitVec 64` vs
`BitVec 32` comparisons. Every function in gale-ffi reverts in the
inline_functions pass, so loom is effectively a no-op for i64-heavy
modules.

With (1) fixed, the wasm-LTO bench will boot and we get measurable
silicon cycles. With (2)+(3) on top, the wasm-LTO route should
approach LLVM-LTO parity (within ~10% on silicon cycles) while
delivering the verification-by-construction property LLVM-LTO doesn't
have.

## What we have data-to-compare on

  Silicon (sha b48a81ac/f6f61281):
    baseline (no Gale, ADC=n)              528 cyc handoff median
    rustc-direct gale (ADC=n)              574 cyc (+46 = FFI seam)
    gale via wasm→synth (Rust only, ADC=y) 582 cyc (seam preserved)
    LLVM-LTO gale (ADC=n)                  471 cyc (-57 below baseline)
    LLVM-LTO gale (ADC=y, post-fix)        558 cyc (+52 above baseline)
    wasm-LTO via wasm-ld+synth (ADC=y)     ELF builds, chip won't boot

  Toolchain-level:
    wasm-ld merge + arm-zephyr-eabi-ar     works
    synth inlining via merged-module       works (no bl in z_impl_k_sem_give)
    synth emitted body size                138 bytes (1.68x LTO's 82 bytes)
    synth memset codegen                   broken (infinite loop)
    loom inline_functions                  broken (Z3 SortDiffers on i64)

The **structural claim** holds: the wasm-LTO toolchain (meld/wasm-ld → loom
→ synth) inlines through the C↔Rust seam. The disassembly evidence is
robust — synth's emitted ARM has zero `bl gale_k_sem_give_decide` in
the inlined `z_impl_k_sem_give`.

The **silicon-cycle claim** requires fixes upstream: the memset bug
blocks boot, and the i64 codegen patterns prevent LTO parity once we
get there. Two PRs against pulseengine/synth and one against
pulseengine/loom.

## Why this matters for the publication

The current "Three Quiet Barriers" successor can claim:

> "Cross-language LTO via wasm IR is feasible end-to-end with the
> existing PulseEngine pipeline. wasm-ld merges, synth transpiles,
> and the C↔Rust seam dissolves at wasm level. The remaining gap to
> LLVM-LTO parity is two specific codegen patterns in synth and a
> Z3 sort fix in loom — both well-scoped engineering work, neither
> a fundamental architectural barrier."

That's a stronger claim than "wasm + synth = same as rustc-direct"
(which is what the current `GALE_USE_SYNTH=ON` path delivers without
the merge step).

### RESULT 2026-05-31 — ON-TARGET MEASUREMENT CAPTURED ✅

The wasm-cross-LTO route now runs correctly on the physical NUCLEO-G474RE.
Full sweep completes (150 samples, 5 RPM steps, drops=0, `=== END ===`).

  **wasm-cross-LTO handoff (silicon, gale variant, synth v0.11.14 / loom v1.1.5):
  median 907 cyc** (steady-state; min 904, max 1607 cold; n=148). algo_cyc=259.

Reference points (handoff cyc): baseline ~528, rustc-direct ~574, LLVM-LTO ~471,
wasm→synth seam-intact (GALE_USE_SYNTH) ~582. So the seam-DISSOLVED wasm-cross-LTO
at **907 cyc is ~1.6× native** — SLOWER despite the seam being structurally dissolved
(decide inlined, no `bl gale_k_sem_give_decide`). The gap is NOT architectural; it is
(a) synth ARM codegen quality (spill/reload, i64 handling, no peephole vs gcc/LLVM) and
(b) integration overhead unique to this build: the fp=0 trampoline + the out-of-line
kernel-API wrappers (gale_wasm_wrappers.c) that turn Zephyr's `static inline` k_spin_lock /
arch_thread_return_value_set into real CALLs (native/LTO inline them). A production
integration would shrink (b); synth codegen maturity would shrink (a).

Run artifact: silicon/runs/2026-05-31-nucleo_g474re-wasm-cross-lto-e8488f0c/.
Took synth #167/168/171/173/174/178/188/195/197/202/204 + loom #145/147/151/153 to get here.

## UPDATE 2026-06-03 — RISC-V / MULTI-TARGET thread opened (native-first; synth Opt3 stays interrupt-priority)

Goal: bring wasm-cross-LTO to RISC-V (ESP32-C3 / Renode / qemu_riscv32), staged like ARM — NATIVE baseline
first, then the synth wasm-cross-LTO path. Lower priority than the synth Opt3 watch (interrupt for that).

### The KEY design question — pieces that STAY THE SAME vs DIFFER across targets
The pipeline splits into a target-independent **front half** (write + verify ONCE) and a target-specific
**back half** (a small per-arch adapter):

**SHARED (identical for ARM & RISC-V):**
- C source of the algorithm/primitive + the value-in/value-out shims.
- The compiled **wasm modules** (clang→wasm32) — pure, architecture-free.
- **loom seam-dissolution** — operates on wasm, emits dissolved wasm; same artifact feeds any backend. Fully target-agnostic.
- **wasmtime functional oracle** + verified vectors (`run_testbed.sh` Layer 1) — wasm semantics, target-independent.

**DIFFERS per target (factor into `arch/{arm,riscv}/` adapters):**
- **synth backend**: `-b arm` (Thumb-2, per-rule SMT / ASIL-D) vs `-b riscv` (RV32, binary-verify / ASIL-B).
- **ABI / trampoline**: ARM AAPCS (args r0–r3, `r11`=linear-memory base for `[fp+off]`) vs RISC-V (args a0–a7,
  its own linmem-base reg) — the fp/linmem trampoline + table-data placement is per-arch.
- **relocations**: `R_ARM_THM_CALL` vs `R_RISCV_CALL`/`HI20`/`LO12`.
- **runtime + linker**: Cortex-M vector table (Zephyr) vs `synth riscv-runtime` (startup.c + linker.ld, RAM@0x80000000).
- **cycle harness**: Cortex-M **DWT CYCCNT** vs RISC-V **`mcycle`/`rdcycle` CSR**; board G474RE vs esp32c3/qemu_riscv32.
- testbed: Layer 1 (wasm oracle) shared; Layer 2 (unicorn) + Layer 3 (silicon micro-bench) get per-arch variants.

### Status (2026-06-03)
- synth **has** a `riscv` backend (`-b riscv`, available, ELF + binary-verify) + `riscv-runtime` (rv32imac). BUT the
  CLI `-t` target profiles are ARM-only, so `-b riscv` errors "cannot compile for ArmCortexM" — wasm→RISC-V codegen
  exists, RV32 target-profile CLI wiring is a **gap to file** when we expand to the synth part.
- **Native RISC-V confirmed**: riscv64-zephyr-elf-gcc compiles `filter_axis` for rv32imac (`mul`/`div` emitted, HW M-ext).
  `synth riscv-runtime -t rv32imac` gives startup.c + linker.ld (the runtime adapter). qemu-system-riscv32/64 present.
- Prediction to test: RV32IM has a hardware divider (like M4 `udiv`), so reciprocal-multiply should be near-neutral
  on RISC-V too — a cross-target confirmation of the M4 Opt 1b finding.

### NEXT (RISC-V, native-first)
1. Minimal qemu_riscv32 (or esp32c3) harness using `riscv-runtime` startup/linker: read `mcycle` CSR around
   `filter_axis_native`, emit result (UART/semihosting). → native RISC-V baseline.
2. Then `synth -b riscv` → file the RV32 target-CLI gap; once it compiles, run + measure → RISC-V wasm-cross-LTO.
3. ARM-vs-RISC-V comparison table. Interrupt for synth Opt3 throughout.

### RISC-V update 2026-06-03 — NATIVE baseline DONE; synth-rv32 blocked on #218
`filter_axis` runs CORRECT on RV32 end-to-end (riscv-gcc rv32imac_zicsr + `synth riscv-runtime` startup/linker
+ qemu-system-riscv32 `virt` `-icount` + NS16550 UART @0x10000000): selfcheck 1088 / -1917, measured **14**
(qemu `-icount` = instruction-count proxy, NOT silicon cycles — relative only; real RV cycles later from
ESP32-C3/Renode). Harness `/tmp/rv-bench/` is reusable for the synth path (swap the native obj for
`synth -b riscv` output). Design fact: **RISC-V linear-memory base = `s11`** (the RV analogue of ARM `r11`).
Filed **synth#218** (riscv backend unreachable from CLI — no RV32 `-t`); synth-rv32 wasm-cross-LTO is gated
on it. Run: `runs-riscv/2026-06-03-qemu_riscv32-filter_axis-NATIVE-baseline/`.

### RISC-V update 2026-06-03 (later) — synth-rv32 wasm-cross-LTO PROVEN FUNCTIONAL (v0.11.21); #220 filed
synth v0.11.21 published (#218 riscv-CLI fix). ARM regression ALL GREEN (riscv fix didn't touch ARM).
**Expanded to the synth part:** `synth compile filter.loom.wasm -b riscv -t rv32imac` → RV32 ELF (88 B,
EM_RISCV), runs **CORRECT on qemu_riscv32** (selfcheck 1088 / -1917) — the wasm→loom→`synth -b riscv`→RV32
path works end-to-end. Hit + filed **synth#220**: the riscv backend uses callee-saved **s1–s6** as scratch
with **no prologue/epilogue** (RISC-V psABI violation → corrupts caller → hang). Disasm-confirmed; worked
around with an s-register-preserving trampoline → correct, ~50 icount (incl ~24 tramp; core ~26 vs native
rv32 17). synth's arithmetic/division riscv codegen is correct; only ABI reg-preservation is broken.
Clean synth-rv32 number pending the #220 fix (drop the trampoline). Run: runs-riscv/2026-06-03-qemu_riscv32-
filter_axis-SYNTH-v0.11.21/. Issues: #218 (fixed), #220 (open). Real RV silicon (ESP32-C3/Renode) still user-gated.

### RISC-V update 2026-06-03 (later2) — riscv backend is a skeleton: filed #223 (missing Select/LocalTee)
Pushed control_step/controller_step/flat_flight through `synth -b riscv`: all rejected — `control_step`/
`controller_step` → "unsupported wasm op for RV32 skeleton: **Select**" (the clamps/saturation); `flat_flight`
→ "**LocalTee**". So the RV32 backend handles straight-line value-in (filter_axis ✓) but is missing common
ops. Filed **synth#223** (Select + LocalTee, with lowering hints). Two open riscv issues now: **#220** (clobbers
callee-saved s1–s6, ABI) + **#223** (op coverage). As they land, the same dissolved wasm compiles to RV32 and
we get the ARM-vs-RISC-V table. filter_axis synth-rv32 stands (correct, ~26 core icount vs native 17, modulo #220).

### RISC-V update 2026-06-03 (later3) — clean filter_axis synth-rv32 (v0.11.22); cross-target ~2× confirmed
synth v0.11.22 (#220 callee-saved ABI fix) PUBLISHED. ARM regression ALL GREEN. **Clean filter_axis synth-rv32
(trampoline-free): 37 icount vs native rv32 17 = 2.18×**, correct. The fix added the s-reg save/restore
prologue; the gap is that synth spills into callee-saved **s1–s6** (needs save/restore) where native gcc keeps
this leaf in caller-saved t-regs — the **RISC-V analogue of ARM Opt 3** (reported on #220). Cross-target:
RV32 **2.18×** ≈ ARM **2.42×** → the ~2× wasm-cross-LTO-vs-native gap is **general codegen, not arch-specific**.
#223 (Select + non-param locals + extend16_s) FIXED → **v0.11.23** (PR open, not yet published); maintainer's
unicorn diff shows control_step rv32 correct (0x00210a55). NEXT: v0.11.23 → control_step/controller_step/
flat_flight synth-rv32 → full ARM-vs-RISC-V table. Real RV silicon (ESP32-C3/Renode) still user-gated.

### RISC-V update 2026-06-03 (later4) — v0.11.23: all 3 compile; controller miscompiles -> filed #226
synth v0.11.23 (#223 Select+non-param-locals+extend16_s) PUBLISHED; ARM ALL GREEN; control_step/controller_step/
flat_flight all now COMPILE on -b riscv. **controller_step synth-rv32 MISCOMPILES** (0x00ce0001 vs 0x05ce7f9c;
native correct in same harness). Disasm root-cause: **regalloc live-range bug** — `t2 = updates<<24` (0x24) is
clobbered by a reused `slt t2,...` (0x5c) for the aileron clamp, then the final pack `or s5,t2,s4` (0x9c) reads
the stale t2 → updates byte lost. Filed **synth#226** (RV32 temp allocator not honoring live ranges; ARM-regalloc
analogue, now a correctness bug). filter_axis rv32 stands (2.18×). control_step/flat_flight compile; their RUN needs
the s11 linmem/table (control_step) + s11=0 pointer (flat_flight) setup — next firing (control_step maintainer-
validated via their unicorn diff = 0x00210a55). RISC-V issues: #218/#220/#223 fixed, #226 open.

### RISC-V update 2026-06-03 (later5) — control_step rv32 CORRECT + measured; cross-target ~2× confirmed on 2 fns
control_step rv32 (v0.11.23) verified on qemu via the **s11 table-trampoline** (s11=&gale_tables-0x10000; synth
reads tables at [s11+0x10000]; #220-fix excludes s11 so synth preserves it): **CORRECT (2165333)**, synth 141 /
native 62 = **2.27×** (incl ~6-cyc tramp). Confirms the maintainer's control_step_riscv_differential on real qemu.
**Cross-target table** (ARM real-cyc | RISC-V icount): filter_axis 2.42× | 2.18×; control_step 2.07× | 2.27× —
both ~2.1–2.4× per ISA, gap = regalloc/spill (synth uses callee-saved s-regs; native uses t-regs) = **#209 Opt 3,
arch-INDEPENDENT** (posted to #209). controller_step blocked on #226→v0.11.25 (pending). flat_flight compiles; run
needs s11=0 pointer setup (next). RISC-V issues: #218/#220/#223 fixed, #226 fixed→v0.11.25 pending.

### RISC-V update 2026-06-03 (later6) — flat_flight rv32 hangs (likely #226); Opt3 = top perf item
#209: maintainer confirmed the cross-target evidence + **Opt 3 (regalloc + constant-CSE) is now the top perf item**
(retargetable, lands after the v0.11.24/.25 correctness train). #226→v0.11.25 is CI-gated (rivet 0.15.0 status-enum
/ PR#229). Built the **s11=0 pointer trampoline** for flat_flight rv32 (native deref via [s11+ptr], s11=0), but
flat_flight **hangs in the call** (markers: MAIN-OK-before prints, RETURNED doesn't; boot/harness fine — control_step
works in the same harness). flat_flight contains the #226 controller-pack (updates<<24 across selects) → **likely
#226-gated** (corrupted reg used as ptr/counter → hang). Not filed (probable #226 dup); will re-verify on v0.11.25
and isolate (s11=0 path vs new bug) if it still fails. RISC-V working: filter_axis 2.18× + control_step 2.27×.

### RISC-V update 2026-06-03 (later7) — synth-rv32 path durable in repo (arch/riscv/run_synth.sh)
No new release (v0.11.25 #226-fix CI-gated PR#229; Opt3 perf-release-gated). Consolidated the synth-rv32 harness
into the repo: **arch/riscv/run_synth.sh** regenerates control_step from the shared sources (clang→wasm-ld→loom),
compiles with `synth -b riscv -t rv32imac`, links the s11 table trampoline, runs on qemu → **2165333 correct,
141/62=2.27×**. Now both arch/riscv adapters are durable + reproducible: run_native.sh (baseline) + run_synth.sh
(wasm-cross-LTO). Ready to add controller/flat_flight when v0.11.25 lands. RISC-V working: filter_axis 2.18× +
control_step 2.27×; controller/flat_flight gated on #226/v0.11.25.

### RISC-V update 2026-06-03 (later8) — FULL RISC-V TABLE COMPLETE (v0.11.25, #226 fixed)
synth v0.11.25 (#226 live-range fix) PUBLISHED; ARM ALL GREEN. **All 4 functions correct on qemu_riscv32:**
controller_step now correct (0x05ce7f9c, 114/49=**2.33×**); flat_flight now RETURNS correct (0x07fdf307,
193/75=**2.57×**) — was #226-gated (the defer-filing call was right). **Cross-target table (ARM real-cyc | RISC-V icount):**
filter_axis 2.42×|2.18×; control_step 2.07×|2.27×; controller_step —|2.33×; flat_flight 3.18×|2.57×. Same ~2–2.6×
gap both ISAs, widening with complexity; lever = regalloc/spill = **#209 Opt 3 (universal, retargetable)**. Posted
to #209. RISC-V backend issues #218/#220/#223/#226 ALL FIXED — the same dissolved wasm runs correct on RV32 across
the whole set. (RV32 = icount proxy; real silicon via ESP32-C3/Renode follow-up.) NEXT: Opt 3 perf release.

## UPDATE 2026-06-03 — v0.11.27 composed-path codegen breakdown (Opt3 increment #2 targets)

After v0.11.27 (#231 leaf-alloc + #232 div fix), the leaves are optimal (filter/controller: 0 frame, 0 spills;
filter 2.18x->1.35x). The composed `flat_flight` barely moved (2.57x->2.41x). Disassembled its RV32 output
(186 instr, 756 B) to localize the remaining overhead — three measured inefficiencies, leverage order:

1. **const-CSE (biggest):** 37 const materializations (li/lui, ~20% of body), only 16 distinct -> 21 redundant (57%).
   Systematic per-axis dup: ±127 saturation each 4x, filter coeffs 980/20/1000 each 2x, shifts 6/7 2-3x, INT_MIN/-1 guards 2x.
2. **pressure-aware spilling:** flat_flight exhausts 7 caller-saved t-regs -> spills to 32B stack frame (7 sw + 10 lw = 17 instr, 9%);
   control_step likewise (6 spills, 16B). Caller-saved preference is right for leaves; for pressure functions a callee-saved
   s-reg (1 save+restore, amortized) beats N stack reloads. RV32 analogue of the ARM "reclaim r9/r10" item.
3. **copy-coalescing:** 38 mv (20%) in flat_flight, 32 in control_step — value-stack lowering emits a mv per transfer.

Posted to synth #209 (comment 4612930170) as the concrete recommendation for the next Opt3 increment.
The leaf gap is closed; the composed gap = const-CSE + pressure-aware s-reg spill + mv-coalescing.

## UPDATE 2026-06-03 — flight_control wasm-LTO variant: on-silicon functional validation

Flashed both flight_control variants on the real NUCLEO-G474RE (openocd, VCP autodetect):
- **NATIVE** and **GALE_FC_WASM_LTO=ON** both boot and produce a valid, identically-structured
  event stream (E,<seq>,<step>,<load>,<algo>,<handoff>,<t_lock>,...). The dissolved flight
  algorithm (filter_step + controller_step via the r11=0 trampoline) **runs correctly on the
  actual chip**, not just unicorn/qemu. This is the real-silicon functional checkmark for the
  Phase-5 wasm variant.

Caveat on measuring the algo cost *from the bench*: the bench's `algo` column times only an
ISR placeholder (`squelch = |gyro_x+gyro_y+gyro_z|`), NOT filter_step/controller_step (those run
in the fusion thread + controller, untimed). So `algo` is identical (≈141 cyc) native vs wasm —
it isn't measuring the dissolved code. Ad-hoc timing around the real calls is dominated by the
non-inlined `k_cycle_get_32` overhead (~65–145 cyc) and is too noisy for a clean delta without
the microbench's min-over-200 + overhead-subtraction discipline.

**Authority for the flight-algo silicon cycle delta remains the dedicated microbench**:
`flat_flight` (= filter+controller composed) = 315 cyc wasm-cross-LTO vs 99 native = 3.18×
(loom v1.1.10, full dissolution). The bench variant is for *running* the dissolved algorithm in
the full 5-primitive context; the microbench is for the clean cycle number.
Follow-up (optional): add overhead-subtracted min-over-N timing around the bench's real
filter_step/controller_step calls if an in-context number is wanted.

## UPDATE 2026-06-04 — MUTEX dissolves via wasm-cross-LTO (2nd primitive after sem)

Proved the mutex primitive dissolves through the same pipeline as the sem (907 cyc):
`clang wasm_mutex_shim_poc.c` (wasm-portable z_impl_k_mutex_unlock) → `wasm-ld` merge with
`libgale_ffi.a` (built `cargo rustc --target wasm32-unknown-unknown --crate-type=staticlib`,
relocatable objects) → `loom optimize --passes inline` → `synth compile --target cortex-m4f`.

**Seam dissolved** (the success criterion): `merged.o` defines only `z_impl_k_mutex_unlock`
(530 B ARM) + a synth helper — **no `gale_k_mutex_unlock_decide` symbol** (the verified-Rust
decision is inlined/folded into the C function, exactly like LLVM-LTO and the sem result).
Call relocations from the dissolved function are only the kernel APIs:
`k_spin_lock/unlock`, `gale_w_current`, `z_unpend_first_thread`, `z_ready_thread`,
`arch_thread_return_value_set`, `z_reschedule` — i.e. it reuses **every** sem `gale_w_*`
wrapper + the one new `gale_w_current`. No `bl gale_k_mutex_unlock_decide`.

Remaining for the silicon number: objcopy-rename the kernel imports to `gale_w_*` (as the sem
does), wire `GALE_WASM_LTO_MUTEX_LIB` to override the bench's native `z_impl_k_mutex_unlock`,
flash G474RE, and measure k_mutex_unlock native / LLVM-LTO / wasm-cross-LTO — the 2nd primitive
data point alongside the sem's 907.

## UPDATE 2026-06-04 (cont) — mutex integration wired; blocked on synth#235 (ET_REL helper)

Built libwasmmutex.a (merged.o = dissolved z_impl_k_mutex_unlock, kernel imports objcopy-renamed
to gale_w_*) + wired the integration (gale_mutex.c #ifndef GALE_WASM_LTO_OVERRIDE_MUTEX_UNLOCK guard;
GALE_WASM_LTO_MUTEX_LIB CMake block, mirroring the sem). Reuses all sem gale_w_* wrappers + gale_w_current.

Link blocker (filed synth#235): the dissolved unlock calls a non-inlinable helper —
core::panicking::panic_fmt (the lock_count overflow-checks=true panic path; gale Cargo.toml forces
overflow-checks=true). synth emits the export (ET_REL) referencing it as undefined `func_9`, but
--func-index (only way to compile a non-export) emits ET_EXEC (with startup stubs), which ld rejects
("cannot use executable file as input"). So no linkable object set until synth can emit reachable
internal callees as ET_REL. NOT disabling overflow-checks (would diverge from the faithful production
build). Workaround options for next firing: export the helper via wasm-tools so --all-exports emits it
ET_REL, or provide a panic_fmt stub. Once linkable -> flash G474RE -> k_mutex_unlock 2nd primitive number.

## UPDATE 2026-06-04 (cont) — MUTEX UNBLOCKED by synth v0.11.28, links + runs on G474RE

synth#235 fixed in v0.11.28 (#236, my option 1: --all-exports --relocatable emits reachable internal
callees). Built #236 from source; re-ran the mutex dissolution: merged.o now defines z_impl_k_mutex_unlock
+ func_8/9/10 (the inlined decision + panic_const_add_overflow->panic_fmt chain), only the 7 kernel-API
imports undefined (all -> gale_w_* wrappers). objcopy-renamed imports -> gale_w_*, ar -> libwasmmutex.a,
linked into flight_control via GALE_WASM_LTO_MUTEX_LIB: LINKS CLEAN. Flashed nucleo_g474re: boots + runs the
100Hz loop to the results print with the dissolved mutex live = wasm-cross-LTO k_mutex_unlock works on silicon.
NEXT: clean k_mutex_unlock cycle microbench (DWT min-over-N, native vs wasm-cross-LTO) for the head-to-head
number, the 2nd primitive data point alongside sem 907. (v0.11.28 still a PR; canonical once released.)

## UPDATE 2026-06-04 (cont) — mutex LINKS (v0.11.28) but drop-in FAULTS on silicon (synth#237)

Installed v0.11.28 (released; #235 fix). The mutex now links + the bench boots, BUT the dissolved
z_impl_k_mutex_unlock MPU-FAULTS at runtime on the G474RE (str.w r6,[r11,r12], r12≈0x08003101 = a
flash/.rodata addr → Data Access Violation, before any output). Root cause = base-register conflict:
the body mixes (a) the host k_mutex* arg [needs linmem_base=0 for native deref] and (b) a function-static
(the shim's `static k_spinlock lock`) [needs base=&wasm_data]. base can't be both. fp=0 trampoline +
overflow-checks=off (drop panic) both insufficient — the static-data access still faults. Filed synth#237
(native-pointer vs wasm-static base conflict; asks for base-independent .data/.rodata reloc for statics).

This is the SAME wall the sem drop-in hit historically (NOTES above: "host-pointer-dereferencing function
can't be a native drop-in"). HONEST scope: value-in/value-out dissolutions (filter_axis, control_step,
flat_flight) work + measured on silicon; host-pointer PRIMITIVE drop-ins (sem z_impl, mutex z_impl) are
ABI-blocked. The mutex k_mutex_unlock silicon NUMBER is gated on synth#237. Native gale k_mutex_unlock
(reference) measured = 124 cyc (uncontended, DWT min/200). v0.11.28 gives the LINK; #237 is the runtime ABI.

## UPDATE 2026-06-04 — mutex root cause = linmem STACK (not BSS static); design-Q answered (#237)
Maintainer fetched mutex_m.loom.wasm@f027273: no (data) segs, no const-addr statics. Real cause =
the wasm linmem **stack**: `(global $__stack_pointer (mut i32) i32.const 65536)` → 16B frame →
SP-relative i32.store; synth lowers `[$sp+K]`→`[r11+sp+K]`; r11=0 trampoline → abs 0x1000~ → DAV.
Measured `$__stack_pointer`/linmem-store usage across shipped leaves (answers maintainer Q-b):
  filter_axis 0/0, controller_step 0/0 (register-only); control_step 1/**0** (tables via fp=&tables, no stack writes);
  flat_flight 1/**6** (DID spill — ran via fp=RAM-linmem-base harness, NOT a native fp=0 drop-in).
Q-a: $__stack_pointer baked from const 65536 in-function; no external/runtime globals-init (just my tramp).
Fix (maintainer's court): under --native-pointer-abi, materialize `__synth_wasm_data+65536` (MOVW/MOVT) for
the SP base (+ any global init'd to a linmem addr); host-ptr [0+ptr+off] stays native. Self-contained.
→ #237 c4623207735. remeasure_wasm_lto.sh staged; reflash+post k_mutex_unlock cyc (native 124) when build lands.

## UPDATE 2026-06-04 (b) — ABI design call: FULLY SELF-CONTAINED (#237 c4623417761)
Maintainer decoded synth output: 2 faults — (1) `ldr [r9,#0]` reads $__stack_pointer from an R9 globals
table my tramp never sets (garbage SP); (2) baked movw/movt #0x10000 stack-top literals. Asked: keep R9
table (tramp sets r9=__synth_wasm_data) vs self-contained (no R9, globals inlined as __synth_wasm_data-rel consts).
RECOMMENDED self-contained: an R9-base contract defeats drop-in (ISR/ctx-switch/scheduler must preserve a
2nd synth base reg); reuses v0.11.29 .bss+MOVW/MOVT substrate exactly; collapses both faults to one rule
(every linmem addr = __synth_wasm_data+N, every host-ptr arg = [0+ptr]); negligible size cost; generalizes
to any dissolved fn. Tramp stays trivial (mov r11,#0). Maintainer to implement as one coherent pass.

## UPDATE 2026-06-04 (c) — maintainer accepted self-contained, BUILDING; oracle validated (#237 c4623714140)
Decoded m.loom.wasm to confirm the SP-anchor pass is necessary+sufficient:
- frame is LIVE: local 1 = SP-16 dereferenced via i32.load offset=4/8/12 (lines 51/58/64) -> SP base MUST be real RAM.
- 6x i32.const 65536: 1 = SP global init (relocate); 5 = &lock arg to k_spin_lock/unlock (lines 28/54/66/93/104),
  wrapper-ignored, correctly left bare by the SP-anchor rule.
- SIZING CAVEAT given: frame at TOP of linmem [65520,65536); .bss must reserve full 2 pages = 131072 B, not data high-water.
Pass = register-promote $__stack_pointer + init __synth_wasm_data+65536 + .bss=full pages; leave frame-size + &lock consts.
Deliverable clock running (policy: dialogue doesn't reset it); reflash G474RE within min of a build. native ref 124.

## UPDATE 2026-06-04 (d) — v0.11.29 RELEASED (data-statics half only); mutex unchanged (#237 c4623947806)
v0.11.29 (commit 2305948, #238 merge) = --native-pointer-abi DATA-statics work. Tested:
- value leaf control_step: clean .text-only ET_REL, NO regression.
- mutex .o (v0.11.29 --native-pointer-abi) = BYTE-IDENTICAL (cmp) to mutex_v0.11.29_bss_still_faults.o:
  still 12x R_ARM_THM_CALL, no .bss/.data/__synth_wasm_data. diff v0.11.28..v0.11.29 = 0 stack_pointer changes.
  -> identical object = identical fault; did NOT reflash (byte-diff suffices per loop rule).
SP register-promotion pass (the deliverable) NOT in v0.11.29. Asked maintainer: landing in v0.11.30?
Deliverable clock kept running (policy: a release that doesn't address the deliverable doesn't reset it).

## UPDATE 2026-06-04 (e) — PR#240 (synth 0.11.30) TESTED on G474RE: object-correct, but 2 blockers (#237 c4624307408)
Built PR#240 (91c7ec1). Object matches oracle: 6 MOVW/6 MOVT __synth_wasm_data, .bss NOBITS, 0 R9; control_step bit-identical.
BLOCKER 1: .bss = 0x20000 (128KB, full 2-page declared mem) = 100% of 128KB RAM -> link overflow 6408B. Real footprint
  is page-1-only (no data segs, 16B frame). Patched .bss sh_size 0x20000->0x10000 (NOBITS, code unchanged) -> RAM 54.89%, links.
  FIX: size __synth_wasm_data to high-water addressed offset, NOT min_pages x 65536. (my earlier min_pages caveat was wrong.)
BLOCKER 2 (real): BUS FAULT even with SP base relocated (__synth_wasm_data=0x20000080) + 64KB .bss. Dissolved obj = 4 fns:
  func_7=body (SP-promoted, all 6 MOVW/MOVT here) + func_8/9/10 internal callees (loom didn't inline). Fault in func_9 at
  0x4dc `f84b 600c` = STR.W r6,[r11,r12] -> wasm-linmem store with base r11=0 (trampoline). Callees still use [r11+linmem].
  FIX: extend __synth_wasm_data materialization to ALL emitted fns (callees too), so r11 never a linmem base; r11=0 = host ptrs only.
Repro committed: mutex_v0.11.30_pr240_full.o, _bss64k.o, _busfault.txt. native ref 124. Reflash on a callee-promoted build.

## UPDATE 2026-06-04 (f) — EXPAND: controller_step ARM silicon = 169/61 = 2.77x (filled the "—" cell)
While mutex/#237 is in maintainer hands (PR#240 callee-promotion fix), pushed the expand thread:
measured controller_step (7-arg value fn) on G474RE. SELFCHECK 0x05e33e81 == native (functionally correct).
KEY ABI FINDING: synth's cortex-m convention passes args in r0-r7 (capstone: arg1=r0, arg2=r1, ...,
arg7 read from saved-r6 slot after `push {r4-r8,lr}`), NOT AAPCS (r0-r3 + stack). So control_step (4 args)
was AAPCS-clean, but >4-arg fns need an arg-shuffle trampoline (controller-microbench/ctl_tramp.S:
push{r3-r7,lr}; ldr r4/r5/r6 from [sp+24/28/32]; blx synth body). 169 includes ~8-cyc marshalling;
body alone ~161. Candidate maintainer ask (HOLD until #237 settles, don't pile): does synth's >4-arg
cortex-m convention intend r0-r7? It means every >4-arg drop-in carries a trampoline + shuffle cost.
Landed reusable controller-microbench/ (build.sh, ctl_tramp.S, RESULT-2026-06-04-g474re.txt).

## UPDATE 2026-06-04 (g) — v0.11.30 MERGED (leaf core); #237 REOPENED; oracle NOT covered on HW (#237 c4624659657)
PR#240 merged (22d2df8). Maintainer: leaf+balanced-SP core done; gmutex "covered" (12 relocs/0 R9). REBUTTED w/ HW:
- built v0.11.30 from merged main -> mutex .o BYTE-IDENTICAL (cmp) to the object that BUS-FAULTed last firing.
- raw-byte scan: 13x STR.W/LDR.W [r11,...] wide mem-ops (e.g. f84b 600c = STR.W r6,[r11,r12]) in callees
  func_8/9/10 (loom didn't inline). All 12 __synth_wasm_data relocs are in the leaf body func_7 only.
- static "12 relocs/0 R9" check MISSES this: [r11+reg] accesses are NOT relocations (readelf -r shows nothing).
-> GI-NPA-003 (callee/non-leaf linmem promotion) is REQUIRED for the named gmutex oracle to RUN, not a generalization.
Tag for v0.11.30 held pending on-target diff (GI-NPA-VER-003). Reflash on a callee-promoted build; native ref 124.

## UPDATE 2026-06-04 (h) — EXPAND/OPT: refreshed #209 const-CSE data, linked to allocator track (#209 c4624854458)
Mutex/#237 in maintainer hands (no reply to my HW rebuttal yet; PR#243 liveness foundation VCR-RA-001 opened).
Pushed the #209 lever: re-derived flat_flight codegen on loom 1.1.10 + synth 0.11.29 (measure, don't guess):
  588B/180 instr; const mat 34 loads/13 distinct = 61% redundant (clamps #0x7e/#0x7f x6 each, #0 x4, shifts 7/6 x3);
  stack spills 7 str + 10 ldr = 17. (was 37/16/57% — refreshed.) Posted to #209 w/ 2 recs on the allocator track:
  (1) const-CSE hoist clamp/shift consts (~12% fewer instr, compounds w/ composition);
  (2) liveness-based spill regalloc (VCR-RA-001/#243) for the 17 [sp] spills. Same lever on RV (2.41x) = retargetable.
  Offered flat_flight as the silicon regression target; reflash on each allocator-track build.

## UPDATE 2026-06-04 (i) — PR#243 liveness MERGED (pure-analysis, byte-identical); #209 plan set
PR#243 (def/use+liveness primitive, VCR-RA-001 step1) merged to main (026d58b). Regression-tested vs v0.11.29:
flat_flight + controller_step + control_step ALL byte-IDENTICAL -> pure analysis, no codegen change, no regression.
Maintainer (#209): adopted flat_flight as Track-A frozen silicon regression target (README North Star, PR#244);
plan = CFG-aware liveness -> interference graph over virtual regs -> spill-under-pressure (replaces exhaustion
hard-fail); const-CSE enabled by same primitive (local_dead_defs = dead-store dual). Baseline LOCKED:
315cyc/588B/180instr/34const(13 distinct)/17spills. Posted handshake (#209 c <new>). Reflash+post delta on
each allocator-track build. Both threads healthy in maintainer hands: GI-NPA-003 (mutex callees) + VCR-RA-001 (#209).

## UPDATE 2026-06-04 (j) — #245 const-CSE detection merged (byte-identical); built flat_flight ARM harness; CORRECTED baseline
PR#245 (const-CSE detection redundant_const_defs + dead-store dual, VCR-RA-001) merged. Byte-diff vs v0.11.29:
flat_flight + controller_step IDENTICAL -> pure-analysis (PR says so), codegen-application pending = silicon-delta deliverable.
EXPAND: built flat_flight ARM-silicon microbench (flat_flight-microbench/, buffer-harness fp=&wasm_linmem, 0 statics,
64KB linmem). Measured CURRENT: synth=262/native=103 = 2.54x (SELFCHECK 0x07fdf307 OK).
=> table's 315/99/3.18x was STALE (synth v0.11.18). Corrected RESULTS-SUMMARY + #209 (c4625286555) to 262/2.54x.
262 includes ~8-cyc fp-setup tramp; body ~254. const-redundancy 34/13/61% + 17 spills on this current object (unchanged by #245).
Measurement-ready: flat_flight-microbench (262) + controller-microbench (169) both staged for the const-CSE-application delta.

## UPDATE 2026-06-04 (k) — NUDGED GI-NPA-003 (mutex callee, stalled); #247 test-only
Allocator track active (#243 liveness -> #245 const-CSE detection -> #247 liveness-vs-selector tests); all pre-codegen-app.
GI-NPA-003 (mutex callee promotion) STALLED: no maintainer reply to HW rebuttal since 17:23 (~1.75h); deliverable clock
crossed 2.5h. Posted friendly deliverable nudge (#237 c4625460808): confirm GI-NPA-003 queued, NOT closed by the leaf
merge (static 12-relocs/0-R9 passes while object BUS-FAULTs via 13 [r11] callee ops). Clock reset post-nudge.
#209 HEALTHY: measurement-ready (flat_flight 262 + controller 169 harnesses); waiting on const-CSE codegen-application.

## UPDATE 2026-06-04 (l) — full 262->103 gap decomposition posted (#209 c4625712211)
Maintainer acting on my #209 data: PR#248 (real-selector evidence -> const-CSE is THE lever, dead-store/#246 a no-op),
PR#249 (immediate-folding encoder probe, And-imm path dead). Both test/ (pre-codegen-app, no byte change).
Disassembled both sides of the gap (synth 180 instr/262cyc vs gcc-O2 68 instr/103cyc):
  clamps: synth 18 IT-blocks vs native 6 (3x; re-materializes 0x7e/0x7f x6) = BIGGEST chunk
  mla: synth 0 (4 mul + sep add) vs native 2 (fuses g*980+a*20) -> mul+add->mla peephole
  const-CSE: 21 redundant const-loads (#245); spills: 17 vs 0 (VCR-RA-001); strd: 0 vs 1
  sdiv 2/2 + branches 0/0 = ALREADY AT PARITY (backend bones right)
=> const-CSE+spills recover ~40-55 of 159cyc; clamp-lowering + mla-fusion are the bigger instruction-selection levers.
flat_flight-microbench(262) + controller(169) quantify each lever as it lands. Offered to file clamp/mla as tracked issues.

## UPDATE 2026-06-04 (m) — FIRST codegen-application delta measured on silicon (PR#250)
PR#250 (feat/selector: fold i32.const C; i32.and -> and rD,rA,#C for C in 0..0xFF) = first delta-emitting transform.
Built (unmerged, ce6364c) + measured on G474RE:
  flat_flight: 180->179 instr (movw 33->32, .text 588->584B); silicon 262 -> 261 cyc; SELFCHECK 0x07fdf307 OK
  controller_step: 120->119 instr (movw 27->26); silicon 169 -> 168 cyc; SELFCHECK 0x05e33e81 MATCH
Folded exactly 1 AND-imm/fn (tail+not-spilled guard gates the other ~3 & 0xFF sites -> coupled to spill-elim).
Validates the maintainer-transform -> my-silicon-reflash loop end-to-end. Posted #250 c4625889530.
Bigger levers still ahead (per gap decomp): const-CSE on 0x7e/0x7f clamps x6, mla-fusion, clamp-lowering 18->6 IT.
Captures: flat_flight_pr250_261cyc.txt, controller_pr250_168cyc.txt.

## UPDATE 2026-06-04 (n) — immediate-fold family complete (net -1cyc); #252 no-op on gale; big levers still ahead
#250 (AND-fold) + #251 (ORR/EOR encoder NOP fix, latent path) MERGED to main (a7676ec). #252 (or/xor fold) OPEN:
built + byte-diffed vs #250 -> IDENTICAL on flat_flight + controller (no or/xor-const sites; packing ORs registers).
So AND/OR/XOR immediate-fold family = net -1 cyc on gale benches (the single AND site, #250: 262->261, 169->168).
Posted #252 note: redirect to the needle-movers (const-CSE on 0x7e/0x7f clamps x6, mla-fusion, clamp-lowering 18->6 IT
= ~150cyc of the 262->103 gap). Microbenches staged for each. #251 latent (we never emitted ORR/EOR-imm; selfchecks valid).

## UPDATE 2026-06-04 (o) — #251/#252/#253 byte-identical no-ops on gale; FILED #255 (encoder bug-class audit)
Tested merge batch vs body of work: main(5cc26a0, #250+#251+#252+#253) == #250-build for flat_flight + controller
(small frames, no or/xor-const) and mutex == v0.11.30 (still faults). So #251/#252/#253 are latent-miscompile
correctness fixes we don't hit; only #250 AND-fold moves us (-1cyc, measured). No reflash needed.
PROACTIVE: audited arm_encoder.rs for the #251/#253 bug class (Operand2::Imm without ThumbExpandImm). Found 3 more:
encode_thumb32_cmp_imm + adds + subs still pack raw i:imm3:imm8 -> silent miscompile for imm>0xFF (e.g. cmp #1000).
#253 fixed add/sub (ADDW T4 + Err) but not the flag-setting S-variants or cmp. Filed issue #255 w/ suggested
shared try_thumb_expand_imm guard; offered on-target test vector. cmp_imm likely LIVE (threshold compares >=256).
Big perf levers (const-CSE on clamps, mla, clamp-lowering) still unlanded; microbenches staged.

## UPDATE 2026-06-05 (p) — #255 FIXED (#256, ~1h!); exact const-CSE target posted; #254 no-op
My encoder bug report #255 -> FIXED by PR#256 (CMP/ADDS/SUBS via ThumbExpandImm) + closed within ~1h. Validated:
latent path (selector materializes ALL cmp consts: cmp #1000 -> movw+cmp-reg, even cmp #50), output correct.
#254 (add/sub fold) + #256: byte-identical no-ops on flat_flight/controller.
Disassembled flat_flight on #256 -> the "61% redundant const" is EXACTLY the clamp bounds: movw #0x7e x6 + #0x7f x6
= 12 movw of 2 distinct values. const-CSE APPLICATION (#245 detection -> emission) collapses 12->2 + frees regs
(relieves part of the 17 spills) = THE pending lever. BONUS: #256 unblocks cmp-imm folding (6 cmp-reg -> cmp #127),
same shape as #250 AND-fold. Posted #209 c4626463863. Microbenches staged (flat_flight 261, controller 168).
GI-NPA-003 (mutex): HOLDING re-nudge (nudged once, unanswered; maintainer responsive to reports but prioritizing
allocator track; re-ask = noise). Threshold extended 2.5h->4h for a genuine escalation if still untouched.

## UPDATE 2026-06-05 (q) — BIGGER EXAMPLE validated: flight_control macro bench runs dissolved algo on silicon
EXPAND (independent of paused maintainer): built flight_control bench (Phase 5, GALE_FC_WASM_LTO=ON) on current
toolchain (synth 0.11.30 main256 + loom 1.1.10). Runs on G474RE: full 5-step sweep, NO FAULT, dissolved
filter_step+controller_step (pointer args, r11=0 tramp). Head-to-head algo: wasm-LTO=157 vs native=141 = 1.11x.
algo sensitive to build (157!=141) => measures the ACTUAL dissolved algo, not a placeholder (corrects old note).
In-context overhead ~11% (vs microbench 2.5x) — dissolved algo is a fraction of per-sample work (handoff/lock/
post/round common). drain_timeouts on hi-rate/contention steps = bench rate tuning, not wasm. Captures in
benches/flight_control/runs/. RESULTS-SUMMARY macro-bench row added. = the "bigger example/big testbed" deliverable.

## UPDATE 2026-06-05 (r) — quiet period (maintainer offline ~1h, EU evening); filed mla-fusion #257
No new synth artifacts since #256 (22:01 UTC 06-04, ~1h ago); #209's 22:08 comment was mine (avrabe=me). Maintainer
likely wrapped for the night after the #250-256 session. 4h rule NOT triggered; open channels #209/#237/#257 satisfy
no-silence intent (won't file a redundant reminder overnight). Corrected loop clock: last_seen = true maintainer
activity, not my own.
EXPAND/OPT (independent): filed #257 — mul+add->mla fusion (lever #2 of the 262->103 gap decomp). Exact evidence:
flat_flight filter g*980+a*20 emits `mul;...;add.w` x2 sites where gcc-O2 uses `mla` (1 instr). Pure instruction
selection, composes with VCR-RA-001. Bonus noted: mul-by-const strength-reduction. Microbenches staged for the delta.
Pending levers: const-CSE application (maintainer building, 12->2 movw), cmp-imm fold (#256-unblocked), #257 mla, clamp-lowering(held).

## UPDATE 2026-06-05 (s) — gap-decomposition fully tracked: filed #258 (clamp-lowering, lever #3)
Still quiet (02:01 EU, maintainer offline ~2h, main unchanged). Disassembled flat_flight clamp lowering precisely:
positive bound = `movw #0x7f; cmp rN,r5` -> should be `cmp rN,#0x7f` (3 sites); negative bound =
`movw #0x7e; mvns; cmp rN,r7` -> should be `cmn rN,#127` (3 sites). Both materialize instead of using the
immediate forms (encoders ready: cmp via #256, Cmn already present). Filed #258 (selector peephole): eliminates
6 movw + 3 mvns + 6 regs, complements const-CSE (dedup vs eliminate), relieves part of the 17 spills.
=> the measured 262->103 gap is now 3 tracked actionable issues: #209/#245 const-CSE (lever1, maintainer building),
#257 mla-fusion (lever2), #258 clamp cmp/cmn-fold (lever3). Microbenches + macro bench staged for each delta.
GI-NPA-003 (mutex) clock >4h but HOLDING re-nudge until maintainer active window (midnight EU; already nudged once).

## UPDATE 2026-06-05 (t) — encoder audit extended: filed #259 (load/store imm12 no bounds-check)
Still quiet (02:32 EU, maintainer offline 2.5h). Continued the #255 encoder-bug-class audit into load/store:
encode_thumb32_ldr/str/ldrb/ldrsb/ldrh/ldrsh/strb/strh _imm all do `offset & 0xFFF` with NO bounds check
(0 Err in L6256-6470) -> silent wrong-address for offset>=4096. Same class as #253 but on memory (worse).
Latent for current workload (fields/frames <4096) but defensive-fix-worthy like #251/#256. Filed #259 w/ suggested
Err+register-offset fallback. INDEPENDENT BACKLOG NOW EXHAUSTED: 3 opt levers (#209/#257/#258) + 2 encoder
classes (#255 fixed, #259) + macro bench. Future offline firings = minimal holds until maintainer active window.

## UPDATE 2026-06-05 (u) — maintainer AM session: #259 fixed (#261); both encoder classes closed
Maintainer back ~06:00-06:26 EU: fixed my #259 (load/store imm12 bounds-check) via PR#261 (CLOSED) + changelog #260.
Tested c91cec9 vs #256-build: flat_flight + controller BYTE-IDENTICAL (latent guard, offsets<4096, no regression).
Both reported encoder bug-classes now guarded: #255->#256 (arith-imm), #259->#261 (load/store-imm). Posted #259 close-out
(on-target test moot: guard makes >=4096 an Err -> selector materializes, wrong-address path unreachable). Optimization
levers #257(mla)/#258(clamp)/#209(const-CSE application) still OPEN — the bigger work, queued. Benches staged.

## UPDATE 2026-06-05 (v) — LEVER #3 (clamp cmp/cmn fold #258) LANDED+MEASURED: -6cyc on silicon
Maintainer wired my lever #3: PR#262 "fold compare bounds into cmp/cmn immediates (#258)". Measured on G474RE:
flat_flight 261->255, controller 168->162 (both -6cyc, SELFCHECK correct). cmn fold fired on negative bounds
(movw 32->26, mvns 6->3, cmn 0->3, net -9 instr). First BIG lever (vs immediate-folds -1). Posted #258 c4628561325.
PR#263 = MLA op-support (#257) but "ready-to-wire", NOT emitting (mul=4/mla=0) -> byte-unchanged, wiring next.
Arc: flat_flight 315(v18)->262->261(#250)->255(#262); native 103. Pending: const-CSE application (#209, 12->2 movw
on bound ASSIGNMENTS the cmn fold doesn't touch) + mla wiring (#257/#263). Benches staged. Capture: flat_flight_262clampfold_255cyc.txt

## UPDATE 2026-06-05 (w) — v0.11.31: mla WIRED (#264) but fires 0x on real flat_flight; #266 no-op
v0.11.31 (#265) released: mla fusion wired (#264) + #266 (AND/CMN ThumbExpandImm). Tested on G474RE-build:
flat_flight v0.11.31 BYTE-IDENTICAL to #258-build (mul=4/mla=0, 170 instr, still 255 cyc) — MLA fusion fires
ZERO times on the actual gale flat_flight despite maintainer's "~18 muls fused" (a simpler fixture). Exact reason:
filter mul->add are scheduler-interleaved across the 2 axes; the adjacent `mul r8,r6,r7; add r2,r5,r8` should fuse
to `mla r2,r6,r7,r5` but mul is the add's 2nd operand -> likely add-commutativity not matched; the other mul's
consumer has a ldrsh between (conservative block). Re-commented #257 (c4628718993). #266 byte-identical (cmn #127<=0xFF).
LEVERS: #258 clamp DONE (-6, 255). #257 mla wired-but-0-fires (reported). #209 const-CSE application pending. Benches staged.

## UPDATE 2026-06-05 (x) — mla NOW FIRES (PR#274, my diagnosis) but silicon REGRESSES +2cyc
My instrumented diagnosis (reg-reuse blocks whole-function used_elsewhere) -> maintainer PR#274 (live-range-bounded fix,
synth 0.11.32). Built: mla NOW fires on flat_flight (mul 4->2, mla 0->2, -2 instr, 0x07FDF307 correct).
BUT G474RE: 255 -> 257 cyc (+2, STABLE x2 re-measures). mla is net-NEGATIVE over the greedy selector — folding
extends r3/r4/r8 live ranges to the mla point, selector pays elsewhere > the 1cyc/site MLA saving. byte-count
(-2 instr / 1891->1819B) looked like a win; on-target cycles regressed. Reported #274 c4631214680: GATE mla behind
the allocator (VCR-RA-001); its value is coupled to register allocation. Capture: flat_flight_274mla_257cyc_REGRESSION.txt.
KEY: this is exactly why on-target measurement matters — host/byte checks would have merged a regression.

## UPDATE 2026-06-05 (y) — v0.11.32 mla default-on: MIXED on silicon (filter +win, flat_flight +regress)
v0.11.32 (#274 merged) ships mla fusion default-on. Tested body of work on silicon (mla fired on filter_axis+control_step+flat_flight):
  filter_axis: 37 (v31) -> 36 (v32) = -1 cyc WIN (1 fusion, clean). SELFCHECK 1088.
  flat_flight: 255 -> 257 = +2 cyc REGRESS (2 fusions interleaved across axes, live-range pressure). 0x07FDF307.
  controller_step: unchanged (no mul). control_step: 2 mla fired but UNMEASURED (needs tables harness) - pending.
=> mla MIXED: helps simple kernels, hurts the complex interleaved one. Filed #277 + refined (c4631625693):
   NOT blanket default-off (forfeits filter_axis win) -> allocator-gate (#272, resolves pressure) or cost-guard.
KEY: on-target measurement revealed BOTH the regression AND that it's mixed — byte-count/host checks would see neither.
Built filter_axis microbench (register-only, 3-arg AAPCS). Clock=v0.11.32.

## UPDATE 2026-06-05 (z) — mla regression RESOLVED: maintainer un-wired (PR#278) per my #277, verified 255 on silicon
My #277 on-target finding (mla +2 regress, mixed) -> maintainer agreed + un-wired mla default-on (PR#278/v0.11.34).
VERIFIED on G474RE: #278 flat_flight = 255 (0x07FDF307), byte-identical to v0.11.31 (mla un-wired). Regression gone.
Maintainer ADOPTED my methodology as their bar: "byte-count insufficient for register-affecting transforms; need
on-target/allocator-aware gate before default-on." fuse_mul_add stays as infra, re-wires WITH the allocator (#272,
spill-aware) so it's net-positive on both filter_axis (simple, +win) and flat_flight (interleaved) shapes.
v0.11.33 (#276 call_indirect reachability) = no-op on our value benches (byte-identical). Posted #277 close c<new>.
Baselines restored: flat_flight 255, filter_axis 37 (mla un-wired). Allocator wiring (#272) remains the #209 lever.

## UPDATE 2026-06-05 (aa) — v0.11.34 un-wire mla: full-suite shows mla was net-POSITIVE (control_step also a win)
v0.11.34 (#278 un-wire mla) released. Re-measured all mla-affected benches on G474RE:
  filter_axis: mla 36 / no-mla 37 (-1 WIN); control_step: mla 156 / no-mla 158 (-2 WIN); flat_flight: mla 257 / no-mla 255 (+2 REGRESS).
=> mla WINS 2/3, net -1 across suite. Blanket un-wire (#278) trades the filter+control wins (+3) for the flat_flight fix (-2) = net +1 WORSE.
Regression signature is specific: flat_flight's INTERLEAVED 2-fusion (both mul products live at the combining add). filter/control = single fusion, product dead after = clean win.
Reported #277 c4632454747 (constructive, not revert): cost-guard skips only the interleaved shape (keeps wins now), OR allocator-gate recovers all.
Corrected control_step baseline to 158 (v0.11.34 released, mla un-wired; mla-on=156). Current released: filter 37, controller 162, control_step 158, flat_flight 255, macro 1.11x.

## UPDATE 2026-06-05 (ab) — MILESTONE: maintainer routes const-CSE + mla through the allocator (my data decisive)
Maintainer decisions (#209 c14:52, #277 c14:35), explicitly crediting my on-target data:
- const-CSE (#209): lands via the ALLOCATOR (rematerialization-avoidance), NOT a standalone peephole. My #277 mla
  regression data was "decisive" — const-CSE is the SAME register-affecting class (resident const extends live range
  -> regresses over greedy selector). ~14 redundant const materializations confirmed on v0.11.34 flat_flight.
- mla (#277): cost-guard DECLINED (principled — it's the patch-accretion VCR-RA-001 exists to remove). My suite data
  = "the concrete justification to prioritize the allocator wiring."
- Both collapse into ONE prerequisite: virtual-register selector output + allocator wiring (recovers const-CSE + mla + 17 spills together, net-positive by construction).
- ACCEPTANCE GATE (maintainer set, using MY baselines): re-enable fusion only when net-positive across
  filter 37 / control_step 158 / flat_flight 255 on-target, NO per-shape guard.
PR#279 (model SelectMove/Select in reg_effect — see resident clamp consts across IT-block chains) = first increment.
Confirmed acceptance contract #209 c4632909409: I run the no-guard check + per-bench delta breakdown when the build lands.
5 silicon benches frozen+staged. The allocator wiring is now THE convergence point for all the optimization levers.

## UPDATE 2026-06-06 (ac) — PR#283 in-place select: BIGGEST lever (-33cyc across clamp benches)
PR#283 (in-place Select, elide keep-val2 move, VCR-SEL-002). Measured on G474RE, all correct:
flat_flight 255->241 (-14, 170->157 instr, 0x07FDF307); controller 162->150 (-12, 110->99); control_step 158->151 (-7, 121->113);
filter_axis unchanged (no select). = -33cyc total. The clamps (18 IT-blocks) were the dominant cost; eliding the per-Select
keep-val2 move cuts directly in. CONFIRMS instruction-selection (clamp lowering) is the larger slice of the 255->103 gap,
NOT the allocator (const-CSE was -1). flat_flight now 241/103 = 2.34x. Posted #283 c<new>. New acceptance gate when merged:
flat_flight 241 / controller 150 / control_step 151 / filter 37 — const-CSE/allocator deltas stack on top.

## UPDATE 2026-06-10 07:55 — v0.11.35 validated (cycle-neutral, −6% code); #288 realloc flag-on gate CLEARED on silicon

synth **v0.11.35** (#285 Chaitin spill-cost ranking): full suite re-measured on G474RE — **flat_flight 241,
controller 150, control_step 151, filter 37 — all identical to v0.11.34, all selfchecks OK**, code −6%
(ff 560→526B, ctl 316→296, cs 370→346). Cycle-neutral release, no issue needed.

**#288 `SYNTH_RANGE_REALLOC=1`** (main b657521, "first consequential allocator step", flag default-off
*pending gale's on-target confirmation*) — confirmation DELIVERED same-day to #209: flag-on is
**byte-identical** on ff/ctl/cs; fires only on the filter family (r4–r8 re-coloured → r0/r1, add.w→adds,
−2B), correct on unicorn funccheck + silicon SELFCHECK, **cycles neutral (37/29/30 both states)** →
gate cleared to default-ON. Next lever posted with the data: **dead callee-saved-save elimination after
realloc** — filter's prologue still saves the now-dead {r4–r8} (~12 of 37 cyc is push/pop overhead →
−25%+ on small leaves; composes with VCR-RA-002 R10-pool).

Ops: macOS cleaned /tmp/opt3 + /tmp/wasm-algo-poc (funccheck modules + fv_algo.o) — recreated; fv_algo.c
+ build_fv_algo.sh now LIVE IN-REPO (silicon-microbench/). Serial-capture flakiness root-caused: cat/stty
drops boot output ~50% of runs; **pyserial captures every time** → capture_serial.py added to wasm-testbed/.

## UPDATE 2026-06-10 15:45 — #209 reply: all 3 accepted; v0.11.36 today (flag ON + dead-save elim); PREDICTION on record

Maintainer (13:15Z): v0.11.35 validation recorded (241/150/151/37 = the reference set), **`SYNTH_RANGE_REALLOC`
default-ON in v0.11.36** (authorized by our silicon gate), **dead callee-saved-save elimination ships in the
same v0.11.36** (v1 scope: shrink push/pop to post-realloc used set, only for fns with NO SP-relative body
accesses; frame-bearing fns = v2 with offset rebasing). #237 (mutex SP-init) → v0.11.37. Tag incoming today.

**Prediction (pre-tag, falsifiable):** filter full 37→25±2, nodiv 29→17±2, div 30→18±2 (push{r4-r8,lr}+
ldmia≈12 cyc eliminated). flat_flight/controller/control_step: NO change (realloc byte-NO-OP + v1 scope
excludes SP-relative fns). If the big-3 move or filter lands outside 23–27 → publish the falsification.

## UPDATE 2026-06-10 16:10 — PR #309 (v0.11.36 cand.) pre-merge validated: filter −8 cyc uniform; prediction FALSIFIED on magnitude

Built b762ac1 pre-merge: big-3 **byte-identical** (241/150/151 stand); filter family on silicon: **full 37→29,
nodiv 29→21, div 30→22 (−8 uniform)**, all SELFCHECKs OK, funccheck ALL GREEN both flag states. Prologue
`push {r4-r8,lr}` → 16-bit `push {r4,lr}` (40→34 B). **Prediction (−12) falsified: −8 measured** — v1 retains
{r4,lr} for AAPCS alignment (~4 cyc left on the table). Posted validation + falsification + v2 lever to #309:
**leaf-function prologue elimination** (no outgoing calls ⇒ no push/pop at all, bx lr) → expected 29→~25.
Verdict posted: good to merge + tag. Gap vs native: filter full 1.95×→1.53×.

## UPDATE 2026-06-10 17:1x — 907 PROVENANCE BROKEN: sem shim was never faithful; faithful rebuild hangs (investigation open)

Re-built the sem .o on v0.11.35 + loom 1.1.11 (the loop's "rebuild the sem .o" check, first since v0.11.15):
code 702→524 B (−25% across 20 releases). Drop-in re-measure on the engine_control bench:

1. **MPU FAULT** (DAV @0x8002b35, `sys_dlist_remove`←`z_abort_timeout`) — root-caused: **`wasm_host_shim_poc.c`
   has ALWAYS passed `(void*)0)` as the wait_q** (git history confirms, single commit) and declared
   `struct k_sem {count,limit}` — skewed −8 B vs the real layout (`wait_q` first). `z_unpend_first_thread(NULL)`
   reads the vector table; the rebased zephyr v4.4.0 final (#44, June 6) turned that latent unsoundness into
   a hard fault. ⇒ **The May-31 907-cyc run's wake path was never semantically sound; 907 needs re-validation.**
2. **Shim FIXED to faithful** (real k_sem mirror {wq_head,wq_tail,count,limit}, `z_unpend_first_thread((void*)sem)` —
   identical contract to gale_sem.c's give: unpend→retval_set(0)→ready). v0.11.35 object: 532 B, links clean
   (localize gale_k_sem_give_decide vs native FFI duplicate — new step, wasm-ld now keeps it global).
3. **No fault now, but bench HANGS at first handoff** (headers + D,boot, then silence; reader poll rows absent too).
   Prime suspect: the **u64-packed decide return** (r0:r1) under v0.11.35's changed i64 lowering — action field
   reads wrong → neither WAKE nor count update. NEXT: unicorn-emulate the v0.11.35 sem body with stubbed gale_w_*
   imports; check decide's u64 return + count store offset; if miscompiled → NEW synth issue with the repro.
   (v0.11.14 cross-check impossible on today's module: it predates internal-callee emission — module shape drifted.)

Isolation status: NOT yet attributable to synth — could be shim/contract. Do NOT file until unicorn isolates it.

## UPDATE 2026-06-10 17:1x — sem hang ISOLATED → synth#311 filed (i64 unpack clobbers the live u64 pair)

Unicorn isolation of the faithful-shim hang: the give body stores **[sem+8]=0 where the verified decide
contract requires 1** — disasm shows `bl gale_k_sem_give_decide; movw r0,#8; movw r1,#255` — the unpack's
mask constants are materialized INTO the live u64 return pair before being read. Built a 15-line minimal
repro (`u64repro.c`): wasmtime ground truth `check(3,4)=8` vs synth ARM under unicorn `=0xDEAD` — and loom
had inlined the callee there, so the single-function i64 shift/mask path is wrong too, not just post-call.
**Silent wrong-code** (no fault) — k_sem_give does nothing → bench hangs. Reproduces byte-identical on the
PR #309 head. Same class as #232/#255 (constants vs liveness) → **filed synth#311** with both repros + the
alloc_temp_avoiding suggestion. BLOCKS: sem re-baseline (907 refresh) + every packed-u64 verified-decide
primitive. Staged for same-day re-measure on the fix.

## UPDATE 2026-06-10 18:1x — #311 BISECTED: v0.11.18's #214 (R12 reservation) is the culprit; broken since June 2

u64 lane across tags: v0.11.14/17 GREEN; **v0.11.18 first RED** (single commit f7190fc = #214 R12/IP
encoder-scratch reservation, itself the #212 fix); RED through v0.11.35 + #309 head. Mechanism: constants
formerly materialized into R12; with R12 reserved the fallback picks r0/r1 with NO liveness check → clobbers
live u64 returns. NOT a v0.11.35 regression — every release since v0.11.18 silently miscompiles packed-u64
unpacks; our sem survived only because it was pinned on v0.11.15 faithful4.o. Posted bisect + mechanism +
fix scope (alloc_temp_avoiding on the scratch-less constant fallback) to #311.

## UPDATE 2026-06-10 18:4x — #311 backend scope-check → filed synth#312 (RV32 rejects i64 local.tee)

Ran the u64 repro through the RV32 backend: **refuses to compile** (`RISC-V selector: stack type mismatch
at op LocalTee(2): expected i32, found i64`) — fail-safe vs ARM's silent wrong-code, but a hard blocker:
the packed-u64 verified-decide pattern currently has NO working backend (ARM = #311 silent miscompile since
v0.11.18; RV32 = #312 compile rejection). Filed #312 with the cross-backend table + ask (i64 local ops via
a-reg pairs). u64_funccheck.py grows an RV32 leg when it compiles.

## UPDATE 2026-06-10 19:1x — RV32 re-baseline on v0.11.35: flat_flight 181→172 (−9); filter/control_step unchanged

qemu_riscv32 -icount, RV32 funccheck ALL GREEN. filter 23 / control_step 129 (both = v0.11.27),
**flat_flight 172 (was 181, native 75: 2.41×→2.29×)**, chk correct. Cross-target read: the v0.11.28-35
allocator levers moved RV32's composed path while ARM flat_flight stayed 241 — RV32 had more residual
register pressure to harvest. controller icount number pending (funccheck-correct). Run archived:
runs-riscv/2026-06-10-qemu_riscv32-v0.11.35-rebaseline/.

## UPDATE 2026-06-10 20:1x — PR #310 pre-merge tested: call-form FIXED, inlined shape still RED → 2 new defects pinpointed on #311

Built #310 head (9107188): u64 lane still 4/4 RED on the loom-inlined shape — but the #310 fixes all hold
(8-byte tee slot, both halves stored, shifts/masks correct). Capstone'd the tail and pinpointed the REAL
remaining defects: (1) **i64.eq boolean materialization emits `cmpeq/cmpne` instead of `moveq/movne`** —
result computed in flags, thrown away; (2) **`select` conditions on a stale vstack reg (r8** = the
extend_u hi-half from 40 insns earlier). The inlined failure is the i64.eq→select chain, NOT constants.
Posted full annotated disasm + 50-line wat to #311 (answers the maintainer's open question pre-tag).

## UPDATE 2026-06-10 20:4x — RV32 table completed: controller_step 100 on v0.11.35 (unchanged, correct)

Final RV32 v0.11.35 re-baseline: filter 23 / controller 100 / control_step 129 (all = v0.11.27),
flat_flight 172 (−9). Only the composed path moved — leaves were already harvest-complete after v0.11.27's
caller-saved preference. Run dir complete.

## UPDATE 2026-06-10 21:2x — #311 defect #3 found (callee i64 return drops hi half); loom v1.1.12 unbuildable → loom#198

**#310 re-test (90e3c5a):** MOVS→CMP transmutation fix VERIFIED — u64 lane 4/4 GREEN (maintainer credited the
disasm as "the whole diagnosis"; our wat = their committed fixture u64_unpack_inlined.wat). But the REAL sem
body still stores count=0 → traced INSIDE decide: packed result computed correctly in r5/r6, epilogue emits
`mov r0,r5` and **NO `mov r1,r6`** — callee-side i64 return materializes only the lo half whenever the value
isn't already in r0/r1. Defect #3 posted to #311 (call-site tagging ✔, return-site pair move missing).
**loom v1.1.12** (CRITICAL #196 elem-segment fix): tag does NOT build from source (stale CodegenOptions
literal + ISLE u32 prelude collision vs locked cranelift-isle 0.132.1) → filed **loom#198**. We stay on
v1.1.11 SAFELY: all suite modules verified 0 elem segments (#196 class can't bite them).

## UPDATE 2026-06-11 08:1x — v0.11.36+37 TAGGED overnight; all #311 legs VERIFIED; sem silicon number blocked on NEW integration hang (ADC lock)

v0.11.36 (#311 all 3 legs + #237 globals/sizing + realloc-ON + dead-save elim) and v0.11.37 (#312 RV32 i64
locals) both tagged. Installed v0.11.37: **testbed ALL GREEN incl u64 lane (both shapes)**; sem unicorn gates
PASS (no-waiter count 0→1 ✔, WAKE arm retval+ready ✔); big-3 byte-identical (241/150/151 stand); filter .o
byte-identical to measured #309 head → **29/21/22 ships in the release**; sem module .bss ELIMINATED (#237
used-extent sizing — no stack-size workaround).

**BLOCKER:** engine_control + wasm-sem hangs at H,boot (ADC health read) BEFORE any bench give. Evidence:
1 give system-wide (adc init unlock, correct/surgical); main pended on ADC ctx LOCK (live: waiter, count=0,
limit=1) with ADSTART=0/IER=0 (conversion never started, lock held); no fault; idle PC; relocs correct; no
symbol shadowing; native-gale control = full sweep CLEAN. The give is object-level correct everywhere —
hang is in the ADC driver lock/start sequence, only with the override linked. NEXT: gdb thread walk +
trampoline hit-count through the H-read window; consider bench variant with health stub (smart_mcu_stub.c)
to decouple the sem number from the ADC question. Status posted to #311 (comment 4677468542).

## UPDATE 2026-06-11 10:1x — SOUND SEM RE-BASELINE: **860 cyc** (v0.11.37, faithful shim); ADC-hang bypassed + confirmed as the blocker

CONFIG_ADC=n (health stub — the bench's own comment notes ADC IRQ-table interactions crash LTO variants too)
→ full sweep clean: **handoff median 860, min 860, max 1586 cold, n=148, drops=0**. First semantically-sound
number: faithful shim (real wait_q, correct unpend/wake — all three #311 legs verified on silicon) is
**47 cyc FASTER than the unsound 907** while doing more work correctly — 20+ releases of codegen
(caller-saved pref, const-CSE remat, in-place select, spill-cost, i64 pair work) on the same path.
860/471 = 1.83× LLVM-LTO (was 1.92×). ADC interaction = separate tracked thread (pre-existing class).
Run: runs/2026-06-11-nucleo_g474re-wasm-cross-lto-v0.11.37-faithful/.

## UPDATE 2026-06-11 10:4x — #312 VERIFIED on v0.11.37; u64 lane grows the promised RV32 leg (3-way differential)

`synth compile -b riscv -t rv32imac` on the u64 repro: compiles (328 B; was a hard selector rejection) and
under unicorn RISCV32 computes **4/4 correct**. u64_funccheck.py is now a **wasm/ARM/RV32 three-way
differential** — both backends of the packed-u64 verified-decide pattern are guarded per release. The
"fail-safe vs silently-wrong" backend split is closed, both lanes correct, exactly as scheduled.

## UPDATE 2026-06-11 15:4x — mutex lane: NEW blocker (register exhaustion since v0.11.36) → synth#326

remeasure_wasm_lto.sh on v0.11.39: #237's bss/SP fixes hold (no -z stack-size needed), but z_impl_k_mutex_unlock
now SKIPS at compile: "register exhaustion: no free callee-saved register to hold a call result while reloading
a preserved param". Bisect: compiled on v0.11.35, first-bad **v0.11.36** (the #311 call-result PAIR tagging =
likely pressure source — the fix that made sem correct costs a reg pair across calls; mutex is our densest body).
v0.11.38's 3b-lite spill-retry does NOT cover this site. Fail-safe (skip) but blocks the mutex silicon number
(native ref 124). Filed **synth#326** w/ bisect + mechanism + ask (extend 3b retry or note as known-limitation
until full RA-001 wiring). Staged for same-day measure on fix.

## UPDATE 2026-06-11 17:5x — v0.11.40: #326 VERIFIED (mutex lane compiles+runs); TWO shim faithfulness bugs fixed; one anomaly left

v0.11.40 installed: suite + sem BYTE-IDENTICAL, testbed ALL GREEN, and **the mutex lane compiles again**
(#326 resolver-scratch fix verified). First run exposed the mutex shim's own unfaithfulness — SAME class as
the sem-907 finding: (1) `wait_q` declared as ONE pointer (v4.4 _wait_q_t = dlist = TWO) → owner read at +4
= dlist tail → always "not current" → rc=-EPERM (observed owner=0x200101b0); (2) unpend called with the
wait_q CONTENT not its address. Both fixed (faithful k_mutex mirror + `(void*)mutex`). After fixes:
**rc=0** on silicon — but **owner reads 0x1 post-release (exp NULL)** and the cycle line is pending that.
NEXT: clean unicorn isolation of the release path (the ad-hoc flat-bin harness mis-assembled — use the
section-aware loader like the sem one). Native ref 124 cyc; measurement follows the owner fix.

## UPDATE 2026-06-13 09:4x — RETRACTED the #326 "undefined encoding": it was MY flat-link harness, synth is clean

Reconciled the rc=0-on-silicon vs undefined-encoding contradiction. synth v0.11.40 mutex object is CLEAN:
882 B, 269 insns, capstone covers 100% of .text, no f5e0 anywhere; byte-identical to the object I'd
disassembled. The `f5e0 0109` at body+0xc was my `ld -Ttext=0x8000` unicorn-prep harness mis-applying the
R_ARM_THM_MOVW_ABS_NC reloc on a `movw r1,#imm` against a zero stub — NOT synth codegen. Posted full
retraction to #326 (comment 4697891080); the exhaustion fix is verified working, #326 can close.
LESSON: never disasm a flat -Ttext link of a relocatable with unresolved MOVW/MOVT data relocs — the
patched immediates masquerade as undefined opcodes. Use section-aware load + real symbol resolution
(the sem isolation harness did this correctly).

REMAINING (ours): bench selfcheck rc=0 (unlock OK) but owner=0x1 post-release (exp NULL) + cycle line
absent → measure loop faults/hangs. Shim logic inspects correct (owner@+8 matches v4.4 dumb-waitq k_mutex).
NEXT: instrument the shim with printk of {cur, decide fields, new_owner} + reflash, observe on silicon
(don't guess); serial capture is flaky so retry. The 124-cyc-ref number follows once owner round-trips.

## UPDATE 2026-06-13 10:1x — mutex owner=0x1 isolated to a REAL on-silicon failure (root cause NOT yet found; two hypotheses falsified)

Instrumented the mutex-microbench selfcheck + the unpend wrapper, clean pyserial capture (reset+retry):
```
DBG pre-unlock: owner=0x200101b0 lock_count=1 cur=0x200101b0   ← correct (owner==cur, count=1; shim reads real k_mutex fine)
DBG unpend(wait_q=0x20010238) -> 0                             ← correct (no waiter -> NULL)
SELFCHECK ... owner=0x6f42202a lock_count=1852404847           ← GARBAGE (ASCII fragments) after unlock
***** BUS FAULT *****
```
So: pre-state correct, unpend correct → corruption happens DURING the unlock body's no-waiter tail
(owner:=NULL; lock_count:=0; spin_unlock; return). Real, reproducible on hardware (not a capture/harness
artifact — earlier garbled run was capture corruption; this is clean).

FALSIFIED hypotheses (do NOT report as synth bugs):
1. "undefined encoding f5e0 0109" — was my flat-link harness (retracted on #326).
2. "body writes past its stack frame" — naive max[sp,#56] vs sub-sp#40 looked like overflow, BUT the
   push {r4-r8,lr} moves sp down 24 first, so [sp,#56] = entry-8 = inside the saved-reg block, NOT past frame.
   Frame math is sound; overflow hypothesis dead.

NEXT (do not guess): gdb on the live target — break at the trampoline `pop {r11,pc}`, read sp vs entry-sp
(stack-balance check), and on the BUS FAULT read CFSR/BFAR + stacked PC to see the faulting access. Only
after that, attribute to synth (body codegen) vs our trampoline/shim. Sem path (860, same trampoline) works,
so it's mutex-body-specific. Diagnostic instrumentation reverted from the shared wrapper.

## UPDATE 2026-06-13 10:4x — gdb on-target: mutex unlock BODY executes correctly (3rd hypothesis falsified); residual queued

gdb/openocd on the live G474RE (break trampoline entry 0x08003d78 + post-body pop 0x08003d84):
- ENTRY sp=0x20002560, r0=0x20001238 (&m). AT-POP sp=0x20002558 = **entry−8 exactly** (the push{r11,lr})
  → **stack is balanced**; the synth body restores sp perfectly. Stack-imbalance hypothesis FALSIFIED.
- post-body read m@+8 (owner)=**0x0 (correct NULL)**, m@+12 (lock_count)=0x1. So the body DID write
  owner=NULL on the no-waiter path; the earlier serial "owner=0x6f42202a garbage" was CAPTURE corruption,
  not real memory. The #326 resolver-scratch fix genuinely works at the body level.

THREE hypotheses now falsified, ZERO claimed: encoding (harness), frame-overflow (sp math), stack-imbalance (gdb).
RESIDUAL (queued, not a synth claim): (a) lock_count reads 0x1 where the no-waiter path sets 0 — but the +12
offset isn't yet confirmed against the real k_mutex DWARF, so this read may be untrustworthy; (b) a later
bus fault (measure loop / printk) not yet caught at z_arm_fault with CFSR/BFAR. NEXT focused session: confirm
k_mutex field offsets from DWARF, re-read owner/lock_count at the verified offsets, and trap the fault PC.
Decision: stop multi-firing rabbit-hole here — body-level correctness is established; residual is bounded.

## UPDATE 2026-06-13 11:0x — mutex residual sharpened to ONE fact (DWARF-confirmed); attribution method pinned

DWARF (ptype /o struct k_mutex): wait_q@0(8) owner@8 lock_count@12 owner_orig_prio@16 — so the prior gdb
reads were at CORRECT offsets. Post-unlock (no-waiter): **owner@8=NULL (correct), lock_count@12=1 (anomalous —
the UNLOCKED branch sets it 0)**. Real, not a misread.
Disasm of the body: all mutex-field stores are linmem-relative `str.w rX,[fp,ip]` (computed offset, fp=0 via
tramp), so static disasm CANNOT confirm whether the lock_count store fires — owner@8 demonstrably written,
lock_count@12 not (or overwritten). DEFINITIVE next step (focused session, NOT this firing's rabbit-hole):
gdb hardware watchpoint on (&m+12) across the unlock to see if/when it's stored; that attributes synth
(missing/mis-offset store) vs shim. NOT posting to any issue until attributed. Note: the "bus fault" only
appeared in the corrupted-capture serial run; clean gdb runs hit entry+pop with no fault — fault reality
still open (re-confirm in the watchpoint session).

## UPDATE 2026-06-13 11:3x — mutex lane: blocker characterized, ISOLATION PAUSED (needs focused session, not loop firings)

Consolidated after 4 loop-firings of isolation. Established facts (hardware/gdb-grounded):
- SINGLE unlock (selfcheck) WORKS: gdb confirms sp balanced (entry-8 at tramp pop) + owner@8 written NULL.
- The 200× measure loop NEVER emits "E,k_mutex_unlock,cyc=" (5 robust pyserial retries) — hangs or silently
  faults in the loop. This (not the selfcheck) is the blocker.
- lock_count@12 reads 1 post-unlock (DWARF-confirmed offset; UNLOCKED branch should zero it) — prime suspect
  for loop-state divergence across the 200 lock/unlock iterations.
- serial "owner=0x1" is a printk/capture ARTIFACT (gdb reads the same field = 0 = NULL). Don't trust serial
  field values here; trust gdb.
- watchpoint on 0x20001244 (lock_count) caught a `<-1` write but the stop-PC decode was inlined-noise;
  attribution (synth missing/mis-offset store vs shim) still OPEN.

DECISION: pause loop-firing isolation (4 firings is enough; this is a focused-debug task, not a 30-min tick).
RESUME RECIPE (one sitting): rebuild /tmp/mtx-gdb; gdb break at the measure-loop body; STEP through iterations
2-3 watching m.owner/m.lock_count + sp at each tramp pop; find the iteration where it diverges/faults; read
CFSR/BFAR if it faults. Then attribute + either fix the shim or file a sharp synth issue. NO issue post until then.
Sem lane (860, sound) unaffected; mutex was always the harder host-pointer primitive.

## UPDATE 2026-06-13 15:4x — mutex measure-loop blocker = DEADLOCK, mechanism fully traced (attribution still bounded-open)

Ran the owed gdb session. The measure loop does NOT bus-fault — it DEADLOCKS:
- Halted hung target → backtrace = **idle thread** (idle→k_cpu_idle→arch_cpu_idle→__enable_irq), lock_count=1 owner=0.
- Mechanism (sound from Zephyr k_mutex_lock logic): dissolved unlock no-waiter path writes owner=NULL ✓ but
  leaves **lock_count=1**; next `k_mutex_lock(&m,K_FOREVER)` sees lock_count≠0 ∧ owner≠current → BLOCK path →
  z_pend_curr forever on an ownerless mutex → main never returns → CPU idle → cycle line never emits.
- So the earlier "hang" + "lock_count=1" findings UNIFY: lock_count-not-zeroed IS the deadlock cause.

Shim source IS correct (no-waiter else-branch: `mutex->owner=NULL; mutex->lock_count=0U;`). owner@8 store
lands (gdb), lock_count@12 store does NOT → strong indication the compiled body drops the 2nd adjacent
field store. NOT yet confirmed (auto-continue hw-watchpoint traces were inconclusive — the boot-time
selfcheck unlock races reset-then-break). NO synth issue filed until confirmed (4th-false-alarm guard).
CLEAN ATTRIBUTION METHOD (next): disasm the body, breakpoint exactly at the lock_count-store site (or
single-step the no-waiter tail), confirm whether a store to [mutex+12]=0 is emitted/executed. If dropped →
synth issue w/ the deadlock as impact; if shim → fix in gale-smart-data. Either way the fix is small now
that the mechanism is pinned. Sem(860) unaffected.

## UPDATE 2026-06-13 16:1x — mutex deadlock ATTRIBUTED & FILED (synth#331): spill-slot collision miscompile

Got the clean attribution WITHOUT a flaky watchpoint run — by decoding the actual compiled
`body.o` (synth v0.11.40) with llvm-objdump (thumbv7em). The lock_count=0 store IS emitted
(falsifies "synth drops the store"); the bug is a SPILL-SLOT COLLISION:
- `[sp,#0x24]` = mutex-ptr arg0 home slot (entry 0x8), live to 0x1e2.
- 0x13a reuses that slot for `gale_w_unpend_first_thread`'s result (reloc-confirmed).
- no-waiter path 0x1e2 reloads the clobbered slot (=0 when no waiter) as the mutex base,
  so `lock_count=0` store (0x1ee) writes linmem[12] not mutex->lock_count -> stays 1.
- owner store (0x156) survived via register copy r7; predicts EXACT observed (owner=0,
  lock_count=1) -> the hardware deadlock. Has-waiter path (0x16a) corrupt too (unexercised).
Decode rests on the real flashed binary + the real hardware symptom (not a guess). Filed
synth#331 with disasm + reloc evidence + kill-criterion + fix direction (don't reuse a live
arg's spill slot for a call result; keep mutex base in callee-saved reg across the body).
Same family as #311 (live-value clobber) / related to #326 (exhaustion at same site).

CONSEQUENCE: mutex k_mutex_unlock silicon cycle number (native ref 124) is BLOCKED on the
synth#331 fix — the deadlock prevents measurement. Owed-by-me item is now a sharp,
maintainer-owned blocker (bucket 2). sem(860) unaffected; testbed control/controller/filter
+ u64 lane all green on v0.11.40 (run this firing).

## UPDATE 2026-06-13 16:3x — optimize thread: quantified flag-fold recommendation -> synth#209

Mutex thread blocked on synth#331 (filed last firing, no maintainer response yet). Pushed the
OPTIMIZE thread instead. Disassembled a CORRECT dissolved body (control_step_decide, v0.11.40,
113 insns/354B, silicon 151 cyc vs 67 native = 2.25x) to find where we lose to native/LLVM-LTO.

DOMINANT loss = comparison->select FLAG ROUND-TRIP, 6 occurrences in this one body:
synth does `cmp <real>; ite; movCC #1/#0` (materialize i32 bool) then `cmp bool,#0; it; movCC`
(re-test it). 7 insns where 3 suffice (cmp; it; movCC). Bool is dead after the re-test
(verified r8 redefined at 0x90). ~4 removable insns x6 ~= 24 cyc ~= 29% of the 84-cyc gap, from
ONE pattern — and it recurs in every body with a compare feeding a select (controller/filter
clamps; likely a chunk of the sem 907-vs-471 gap too). Posted to #209 with disasm evidence,
quantified impact, the recommended compare->select fusion peephole (keep condition in NZCV, don't
materialize), kill-criterion (re-measure control_step toward ~127 when a release lands).
Secondary: 10 mov rX,rY reg-shuffles/113 (coalescing) + single [sp] scratch churn — noted as
lower-priority allocator items.

## UPDATE 2026-06-13 17:0x — optimize thread: headline (sem) overhead breakdown -> synth#209 follow-up

No new release (v0.11.40/v1.1.13). 4h rule N/A (~2h42m). #331/#209 no maintainer response yet.
Pushed optimize thread to the HEADLINE: rebuilt the fully-dissolved k_sem_give path with the
exact release recipe (build-wasm-dist.sh, FFI wasm staticlib + shim -> wasm-ld -> loom inline ->
synth). 540B/165 insns, 4 funcs, seam folded (no bl decide). Findings:
- Flag round-trip (the #209 finding) RECURS in sem (sites 0x50/54, 0x7c/80, 0x186/8e) -> not
  control_step-specific.
- BIGGER lever: 35/165 insns (21%) are str/ldr [sp] data spills (+14 mov rX,rY) -> excess
  spilling, ~70 cyc estimate, plausibly a large slice of the 389-cyc gap to LLVM-LTO (471).
  Recommended spill-reduction/regalloc as #1 priority, flag-fold as #2. Posted to #209
  (comment 4698894095) with honest scope: static counts from real object, cycle figures are
  ESTIMATES, kill-criterion = re-measure on-silicon when a release lands.
CAUGHT a misattribution risk first: my initial shim-only build still had `bl decide` (not the
measured shape) -> rebuilt the real dissolved form before analyzing. Did not present shim-only
numbers as the headline.

## UPDATE 2026-06-13 17:3x — synth#331 ANSWERED by maintainer -> delivered committed repro (jess#28)

synth-side loop confirmed #331 is a real high-priority silent miscompile; ruled out (a) static
frame-layout overlap and (b) the i64-pool result-park by code-reading; reproduced the SHAPE but
NOT the collision (register-pressure-dependent) -> asked for the failing module in jess.
DELIVERED: rebuilt the dissolved k_mutex_unlock fresh on v0.11.40 (clang->wasm-ld+FFI staticlib->
loom inline->synth --native-pointer-abi), CONFIRMED the collision still reproduces (identical
signature, offsets rel body-start 0x138: arg0 home [sp,#0x24]@+0x8, clobbered by unpend result
@+0x13a, no-waiter reload @+0x1e2 -> lock_count store misses). Committed to pulseengine/jess
repro/synth-331/ (PR #28): dissolved.wasm + wat + FAILING.o + shim + README (repro cmd, annotated
collision, expected/actual, silicon symptom, kill-criterion). Replied on #331 (comment 4698973255)
pinpointing the gap their ruling-out leaves: the result is parked on arg0's #204 PARAM-HOME slot
(not an i64-pool slot) under R4-R8 pressure -> param-home HashMap slot handed out as spill dest.
Agreed interim = exhaustion-Err skip; +1 on VCR-RA-003 v-next slot-non-aliasing validation.
Mutex cycle number (ref 124) still gated on the fix; clock reset (maintainer engaged).

## UPDATE 2026-06-13 15:5x — expand/measure: control_step silicon RE-MEASURED at v0.11.40

No release (v0.11.40/v1.1.13). 4h N/A (synth 0h25m post-#331-reply; loom 3h42m). #331/#209 no
new response. Pushed the measure thread (hardware, not guess): rebuilt control_step-microbench at
the CURRENT toolchain, flashed G474RE, captured:
  SELFCHECK 2165333 OK ; E,control_step,synth=151,native=67 = 2.25x  (reproducible x2 boots)
-> down from 158 (v0.11.34) = ~3% codegen improvement 0.11.34->0.11.40, functionally correct.
Confirms the 151 I used in the #209 flag-fold analysis is the CURRENT on-silicon truth (not an
estimate) -> hardware-locked the "before" baseline for the #209 kill-criterion (target ~127).
Wrote RESULT-2026-06-13-g474re.txt, refreshed RESULTS-SUMMARY footnote §, posted baseline-lock to #209.

## UPDATE 2026-06-13 16:1x — loom#142 still-reproduces confirmation + flat_flight silicon refresh

No release. synth 0h55m. loom 4h12m (over 4h) BUT reminder_issue_open non-null -> per rule, no new
reminder issue; responded on the open channel instead. Confirmed loom#142 STILL reproduces on the
LATEST release v1.1.13: measured assets across v1.1.10-13 = compliance-report tarball only, NO loom
binary -> release-build-fails root cause unaddressed (#198 build-from-tag fix didn't touch the
release-artifact path). Re-commented on #142 (4699120397) with the asset table + kill-criterion;
not blocking gale (we build loom from source).

Measure thread: re-measured flat_flight on G474RE at v0.11.40 = 241/103 = 2.34x, SELFCHECK
0x07fdf307 OK -> STABLE vs v0.11.35 (241), down from 262 (v0.11.30). Wrote RESULT-2026-06-13-v0.11.40,
refreshed RESULTS-SUMMARY ‡ footnote. Current-toolchain silicon picture now: control_step 151 (was 156),
flat_flight 241 (stable). Both functionally correct on v0.11.40.

## UPDATE 2026-06-13 17:0x — 🎯 MUTEX UNBLOCKED & MEASURED: synth#331 fix verified on silicon

synth#331 FIXED (PR#333/681f0bf), shipped as v0.11.41 release commit on main (GH Release not
published yet; gh release list still shows v0.11.40). Built synth 0.11.41 FROM MAIN (cargo build,
31s) and ran the long-blocked mutex re-measurement:
- DISASM: collision gone. arg0 home moved to [sp,#0x68], written ONCE (never re-clobbered); unpend
  result parks on DISTINCT [sp,#0x28]; no-waiter lock_count=0 store (0x31a-0x326) reloads the
  un-clobbered mutex base -> CORRECT. (area_reserved guard fix, exactly as diagnosed.)
- SILICON (G474RE): SELFCHECK k_mutex_unlock rc=0 owner=0 OK (no deadlock!); E,k_mutex_unlock,cyc=501.
- FIRST mutex wasm-cross-LTO number: 501/124 native = 4.04x — WORST ratio in suite. Cause: 86/269
  insns (31%) are [sp] spills (vs sem 21%, control_step 5%) -> spill density tracks the ratio.
Posted: #331 silicon-verified close (4699204002) closing the kill-criterion; #209 mutex spill
evidence (4699204055, the spill%-vs-ratio table) reinforcing spill-reduction as #1 lever.
RESULT-2026-06-13-v0.11.41.txt written, RESULTS-SUMMARY mutex row 501. The long-self-owed mutex
deliverable is DONE. Next: re-measure on the published v0.11.41 GH release when it lands (sanity);
push synth on the spill-reduction pass -> re-measure mutex (best test case).

## UPDATE 2026-06-13 17:3x — v0.11.41 PUBLISHED + installed + body-of-work regression-clean

synth v0.11.41 published (17:18) -> installed to ~/.cargo/bin (was 0.11.40), reset clock. Tested
against the full body of work on the official release:
- testbed run_testbed.sh: ALL GREEN (12 OK + u64 3-way lane 4/4), no skip/exhaustion.
- sem: rebuilt .o = byte-structurally IDENTICAL to 0.11.40 (540B, 4 funcs 20/230/6/284, 165 insns,
  35 spills/21%) -> #331 fix (has_i64==false path) doesn't touch sem (has_i64==true) -> 860 HOLDS.
- control_step: rebuilt = IDENTICAL (354B, 113 insns) -> 151 HOLDS.
- flat_flight: testbed-green (functional) -> 241 HOLDS.
- mutex: 501 (FIXED, was deadlock <=0.11.40).
=> v0.11.41 regression-clean AND fixes the mutex. CONSOLIDATED current-toolchain silicon picture
   (synth 0.11.41 + loom 1.1.13, G474RE):
     k_sem_give     860 / 471 LLVM-LTO = 1.82x
     k_mutex_unlock 501 / 124 native   = 4.04x  (NEW; worst ratio, 31% spill)
     control_step   151 / 67           = 2.25x
     flat_flight    241 / 103          = 2.34x
loom clock: 5h12m but reminder channel non-null + already responded #142 -> no new reminder.
NEXT expand frontier (queued, multi-firing): promote mutex to a shippable wasm module (the sem #59
pattern: build-wasm-dist.sh + Kconfig + release-wasm.yml) now that it works. Optimize thread (#209)
comprehensive + awaiting maintainer (flag-fold + spill-reduction, 3-pt spill%-vs-ratio evidence, hw baselines).

## UPDATE 2026-06-13 17:5x — expand: testbed now covers kernel primitives (sem+mutex) + #331 guard

No release (v0.11.41 latest). synth 0h29m; loom 5h42m (channel non-null + responded #142 -> no
reminder). #209 VCR-SEL-004/VCR-RA accepted, no PR yet -> passive (I'm the on-silicon gate).
EXPAND step: run_testbed.sh covered only the 4 algos + u64 lane; the kernel primitives (where
#331 lived) were NOT in per-release validation. Added primitives_codegen_check.sh (wired into
run_testbed.sh): for sem+mutex via the dist recipe -> compiles (no exhaustion-skip) + seam folded
(no bl decide reloc) + mutex arg0-home spill slot WRITE-ONCE = the #331 collision signature
(v0.11.40 wrote [sp,#0x24] twice — entry + unpend-result park; fixed v0.11.41 writes [sp,#0x68]
once). GREEN on v0.11.41. Now every synth release I test auto-checks both primitives' codegen
health + guards the #331 fix in OUR consumption (complements the synth-side jess fixture).

## UPDATE 2026-06-13 20:3x — expand: Phase-5 macro bench re-measured on v0.11.41 (the bigger example)

No release (v0.11.41 latest). synth 1h0m; loom 6h12m (channel non-null + responded #142 -> no
reminder). #209 VCR-SEL-004/VCR-RA accepted, no PR yet (open synth PRs all dependabot) -> passive.
EXPAND step (the mandate's "bigger example"): rebuilt + reflashed the flight_control Phase-5 macro
bench (GALE_FC_WASM_LTO=ON, 5 primitives on 100Hz loop) on v0.11.41 + measured on G474RE silicon:
- full 5-step sweep, NO FAULT; in-bench algo = 157 cyc, perfectly stable (min=med=max, n=9).
- = native 141 (gcc, synth-independent) -> 1.11x, ~11% in-context overhead -> UNCHANGED from
  v0.11.30. The composed-context result holds on the post-#331 toolchain. (Measured wasm side on
  hw; native carried forward — gcc -O2, unaffected by synth version.)
Wrote runs/RESULT-2026-06-13-v0.11.41.txt, refreshed RESULTS-SUMMARY Phase-5 paragraph. The bigger
example is validated current. Net current-toolchain picture stable: sem 860, mutex 501,
control_step 151, flat_flight 241, macro-bench algo 157/1.11x.

## UPDATE 2026-06-13 21:1x — EXPAND: shippable mutex wasm-cross-LTO module -> gale PR #60

No release (v0.11.41 latest). synth 1h30m; loom 6h43m (channel non-null -> no reminder). #209
VCR-SEL-004/VCR-RA accepted, no PR yet (maintainer confirmed VCR-RA-002 still `proposed` on #242)
-> passive, I'm the on-silicon gate. EXPAND step: promoted the silicon-validated mutex to a
shippable module mirroring sem #59 (unblocked by the v0.11.41 #331 fix). gale PR #60:
- zephyr/wasm/mutex_unlock_shim.c (production, faithful k_mutex layout)
- zephyr/wasm/gale_wasm_mutex_tramp.S (r11=0 native-pointer trampoline)
- scripts/build-wasm-dist.sh refactored -> build_module() helper, emits BOTH sem+mutex + manifest.
Verified: valid ET_REL mutex .o (body global, decide localized, imports->gale_w_*, no leftover
un-renamed), #331 signature absent (arg0 home #0x68 write-once), primitives lane GREEN, first
silicon 501 cyc. Follow-up PR: Kconfig CONFIG_GALE_WASM_LTO_MUTEX + CMakeLists consume + release.yml
attach. PR #60 pending CI+review (gale main protected).

## UPDATE 2026-06-13 21:5x — EXPAND: completed mutex module (PR #60) + caught 2 real bugs by building

No release. synth 1h59m; loom 7h12m (channel non-null -> no reminder). #209 no VCR PR yet (passive).
PR #60 CI: MERGEABLE, no failures (UNSTABLE = qemu matrix still running). Completed #60 from
build-pipeline-only into the FULL mutex module (consumption wiring mirroring sem): gale_mutex.c
#ifndef guard, Kconfig CONFIG_GALE_WASM_LTO_MUTEX, CMakeLists consume block, release.yml name.
Verified by BUILDING tests/kernel/mutex/mutex_api on nucleo_g474re — caught TWO real bugs assuming
would have shipped:
 1. func_N collision: sem.o & mutex.o each export synth's internal func_7/func_8 as GLOBAL ->
    multiple-definition at final link when both modules on (#59 never hit, sem shipped alone).
    Fixed: build-wasm-dist.sh exports ONLY the body (objcopy --keep-global-symbol), localizing
    decide + func_N. Verified both-on links.
 2. Kconfig mis-gate: GALE_WASM_LTO_MUTEX landed inside `if GALE_KERNEL_SEM` -> unreachable w/o
    sem kernel. Fixed: moved under GALE_KERNEL_MUTEX (depends on). Verified with SEM kernel OFF +
    mutex wasm ON: .config accepts CONFIG_GALE_WASM_LTO_MUTEX=y, links (elf OK).
Both dedup-guard branches (sem-on / sem-off) exercised. PR #60 updated to "complete module".
Consumption verified: native unlock compiled out, synth_k_mutex_unlock_body + tramp linked, 501 cyc.

## UPDATE 2026-06-13 20:1x — PR #60 mutex module FAULTS in full mutex_api (silicon) -> draft

No release. synth 2h29m; loom 7h42m (no reminder). #209 no VCR PR (passive). Ran the REAL consume
path (not just link): tests/kernel/mutex/mutex_api on G474RE w/ CONFIG_GALE_WASM_LTO_MUTEX=y ->
USAGE FAULT on test_complex_inversion. Pinned: faulting insn body+0xc = 0xfac8 0100 = UNDEFINED;
working body has `movw r1,#0` (R_ARM_MOVW_ABS_NC __synth_globals). The mutex dissolved body refs
wasm STATICS (__synth_globals @.data+0x10008, __synth_wasm_data) via MOVW_ABS/MOVT_ABS relocs + a
~64KB .data section; the Zephyr link corrupts those movw->undefined. Localized:
 - SEM .o has ZERO MOVW_ABS relocs (no wasm statics) -> #59 works in-kernel, mutex doesn't.
 - No-waiter microbench links the SAME .o clean (501 cyc) -> .o fine; Zephyr-link consume path
   mis-applies the relocs (flag/placement of 64KB .data, e.g. --no-relax).
Converted PR #60 -> DRAFT, posted characterization (comment 4699667490). The build-pipeline+shim
parts sound; consumption of a wasm-statics-bearing module is the gap. Also 64KB .data on 128KB RAM
is itself wrong (synth emits full linmem; decide uses little) -> raise synth-side separately.
KEY: caught by running the FULL test on hardware — link-only + no-waiter microbench both passed and
masked it. Owed next: root-cause the MOVW_ABS corruption (linker flag vs synth reloc) + the 64KB .data.

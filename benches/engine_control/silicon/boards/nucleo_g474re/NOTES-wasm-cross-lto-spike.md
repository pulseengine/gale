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

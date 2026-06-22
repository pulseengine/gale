# Recommendation for `pulseengine/synth` (+ `loom`): close the per-function **cycle** gap on the gust hot path

**Context.** `benches/gust/src/bin/gust_codegen_bench.rs` times the *same* source
function lowered two ways — native (LLVM → thumbv7m) vs dissolved
(wasm → loom → synth → cortex-m3) — under one SysTick harness (qemu `-icount`,
deterministic; M3 has no cache so instr ≈ cycles). The target is `gust_mix`, the
Q8 fixed-point failsafe mixer: pure scalar in/out, no pointer, no scheduler state,
no `r11` trampoline — the cleanest possible apples-to-apples codegen comparison.

The two lowerings are **bit-identical over the full input domain [0, 2047]** (the
bench's correctness gate), then each is timed over an input sweep hitting all
three clamp branches.

## Result (measured, baseline-subtracted)

| lowering | fn-only ticks/call | instructions | callee-saves | stack frame |
|---|---|---|---|---|
| native (LLVM) | **0.40** | **15** | `{r7, lr}` (2) | none |
| dissolved (synth) | **1.125** | **45** (11 wrapper + 34 inner) | `{r4–r8, lr}` ×2 (12) | 24 B |
| **ratio** | **2.81×** | 3.0× | — | — |

Kill-criterion: this recommendation is wrong if a fresh `gust_codegen_bench` run
shows the dissolved/native fn-only ratio ≤ 1.3×, or if the disassembly below no
longer reproduces. Reproduce: `cargo run --release --bin gust_codegen_bench`.

## Attribution — four codegen issues, from the disassembly

The dissolved `gust_mix` is an **export wrapper** that `bl`s an **inner body**;
neither is optimized to native quality. Annotated:

```
gust_mix (wrapper, 11 insns — almost all overhead):
  push.w {r4,r5,r6,r7,r8,lr}   ; (1) saves 6 callee regs; native saves 2
  sub.w  sp, sp, #0x18         ; (1) 24-byte frame for a function needing ~0
  str.w  r0, [sp, #0x14]       ; (2) spill the arg…
  ldr.w  r1, [sp, #0x14]       ; (2) …and immediately reload it (dead round-trip)
  mov    r0, r1
  bl     <func_1>              ; (3) call the inner body — NOT inlined/merged
  movw   r2, #0xffff           ; (4) u16 narrow as movw+and …
  and.w  r3, r0, r2            ;     … where native uses `uxth`
  mov    r0, r3
  add.w  sp, sp, #0x18
  pop.w  {r4,r5,r6,r7,r8,pc}

func_1 (inner, 34 insns — the arithmetic, inflated):
  push.w {r4,r5,r6,r7,r8,lr}   ; (1) again
  movw r4,#0x8 ; lsl.w r5,r3,r4 ; (4) shift amount in a REGISTER; native: `lsl #8`
  movw r6,#0x0 ; movt r6,#0xfffc ; the (-1024<<8) constant built in 2 insns
  cmp;ite lt;movlt #1;movge #0;cmp #0;it ne;movne   ; (4) clamp = materialize a
  …                                                  ; bool then test it, where
  …                                                  ; native fuses `cmp; it lt; asrlt`
```

Native, for the same source (15 insns, no frame):
```
  push {r7,lr}; uxth r0,r0; ldr r1,[pc] (=-1024<<8); mov r2,#500
  add.w r0,r1,r0,lsl #8; asrs r1,r0,#8; cmp.w r1,#500; it lt; asrlt r2,r0,#8
  addw r0,r2,#1500; cmn.w r1,#500; it lt; movlt r0,#1000; pop {r7,pc}
```

### The asks (ranked by measured impact)

1. **Liveness-aware prologue / leaf-function register save.** synth saves
   `{r4–r8, lr}` unconditionally in *both* the wrapper and the body; neither uses
   those callee-saved registers. Save only what's live. ~10 of the 45 insns are
   this push/pop. **synth** (prologue/regalloc).
2. **Inline / merge single-call export wrappers, and keep wasm locals in
   registers.** The wrapper exists solely to adapt the export to the body and it
   *spills+reloads the argument through the stack* (`str [sp,#0x14]` then `ldr`
   back). Either loom should inline the body into the export, or synth should
   fold the wrapper. The spill/reload is the classic "wasm local → stack slot,
   never promoted to a register" pattern. **loom** (inline) **+ synth** (local
   promotion / mem2reg).
3. **Fold constants into immediates + recognize idioms.** Shift amount is loaded
   into a register (`movw r4,#8; lsl.w r5,r3,r4`) instead of an immediate
   (`lsl #8`); the u16 narrow is `movw#0xffff; and` instead of `uxth`. **synth**
   (immediate folding, narrowing idiom).
4. **Fuse compare→select into predication.** The two clamp comparisons each
   materialize a 0/1 boolean and then test it (`cmp; ite; mov#1/#0; cmp#0; it ne;
   movne`), where LLVM emits `cmp; it lt; movlt`. This is the wasm
   `i32.lt_s`→`select` pattern lowered literally. **synth** (compare/select
   fusion → IT-block predication).

(1) and (2) are pure overhead and the largest share; (3) and (4) are the
arithmetic-body gap that also drives the `control_step` 2.1× and `gust_poll`
3.9× *size* ratios in COMPARE.md. None of these change the proof obligations —
they are lowering-quality, not semantics.

## Why this matters

The project thesis (BEAT the status quo, not just match LLVM) needs the
per-function gap closed: a 1402 B / no-runtime kernel already wins the
device-class argument vs a ~50–64 KB WAMR interpreter, but a 2.8× per-call cycle
cost on the hot path is the open work. Each ask above is mechanical, measured,
and re-checkable with this bench.

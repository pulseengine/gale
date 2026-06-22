# Where to optimize the dissolved gust hot path: meld vs loom vs synth

Hard analysis (three independent source-grounded deep-dives over the synth, loom,
and meld trees) of the measured **2.81× cycle gap** on `gust_mix`
(`gust_codegen_bench`, COMPARE.md). Verdict: the gap is **mostly synth**, with one
**loom** structural fix and a **meld** fix that only applies to the fused path. All
findings re-derived from the live artifacts (the subagents' isolated-module tests
were corrected against the real module).

## Root structure

The dissolved `gust_mix` is an export **wrapper** that `bl`s an **inner body** — and
the two halves are lowered by **two different synth paths**: the wrapper contains a
local call, so synth's optimized `ir_to_arm` bridge declines it (synth#188) and it
falls to the inferior direct selector; the callless inner body takes the bridge.
The split is never closed because loom doesn't inline the wrapper. So both a synth
path-selection issue *and* a loom inline gap conspire.

## Layer attribution

| # | Issue (45 insns vs native 15) | Owner | Filed |
|---|---|---|---|
| — | wrapper not merged with body (extra `bl` + 2nd prologue) | **loom** (inline) | loom#228 |
| 1 | unconditional 6-register callee-save (×2) | **synth** | synth#428 |
| 2a | arg spilled+reloaded through stack | **synth** (peephole not run on direct path) | synth#428 |
| 3 | constants in registers; register shift vs `lsl #imm`; `movw+and` vs `uxth` | **synth** (instr-sel) | synth#428 |
| 4 | compare→select as materialized-bool-then-test | **synth** (highest value) | synth#428 |
| — | fused-path identity trampolines (the demonstrator) | **meld** | meld#304 |

**loom is NOT the owner of issues 3/4** — loom emits the ideal `i32.const 8; i32.shl`
and `i32.lt_s; select`; turning those into `lsl #8` / fused predication is synth's
instruction selection. **meld is NOT implicated in `gust_mix`** — that module never
goes through `meld fuse`; meld's role is only on the fused-composition path.

## The three filed issues

- **synth#428** — fuse cmp→select (optimizer_bridge.rs:3327 + arm_encoder.rs:2847),
  fold constant immediates / drop the `and #31` on constant shifts
  (optimizer_bridge.rs:3082-3103, 2700-2776), relax `shrink_callee_saved_saves`
  SP-decline (liveness.rs:1923-1941) + wire the shadow allocator (arm_backend.rs:382)
  to drive leaf prologue-elision, run peephole store-load-forwarding on the direct
  path (peephole.rs:124). Plus the #188 root cause that forces the wrapper onto the
  inferior path. **Largest share of the gap.**
- **loom#228** — `inline_functions` candidate predicate (lib.rs:13476)
  `(call_count==1 || size<10) && size<limit` rejects the 23-insn, 2-call-site leaf;
  `MULTI_CALL_SITE_LIMIT=50` is dead code. Fix `if size < limit`. Must land with
  whole-function DCE in the CLI (`optimize_command` never calls
  `eliminate_dead_functions`) so the orphaned copy is reclaimed.
- **meld#304** — honor the dead `inline_adapters` flag (adapter/mod.rs:35, set but
  never read) so identity trampolines (adapter/fact.rs:552-567) rewire imports
  directly to the target; de-export fusion-vestigial symbols (generalizes meld#298).

## Beat-LLVM (the thesis — go *below* the 15-insn floor, not just match it)

- **synth**: leaf functions need *zero* prologue on bare-metal (no unwinder) — the
  graph-coloring allocator already proves R0-R3/R12 sufficiency in shadow mode
  (`SYNTH_SHADOW_ALLOC`); wire it. Constant clamp bounds → `ssat`/branchless, and a
  proof-carried range can elide a bound entirely. Optimal coloring is cheap on
  tiny programs ("optimal-regalloc-because-tiny").
- **loom**: forward **proof-carrying facts** (operand ranges, shift<32, select
  totality) it already proves with Z3 to synth as side-band metadata — the "tunnel";
  specialize on the **closed call set** (drop the u16 mask/clamp bound if all sites
  pass in-range). An MCU LLVM build of an externally-visible symbol can't.
- **meld**: fusion turns cross-"object" calls into **inlinable intra-module** calls
  (the LTO win, structurally) + one flat address space with no per-access tax;
  forward canonical-ABI provenance facts a native multi-object link discards.

The combined synth fixes plausibly reach LLVM parity (~15 insns); the beat-LLVM
items (proof-carried bounds + zero-prologue leaves + wrapper deletion) target a
genuine sub-LLVM result for this verified-code class.

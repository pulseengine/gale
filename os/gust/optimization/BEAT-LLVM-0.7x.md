# Beating LLVM: the 0.7× roadmap for meld → loom → synth

**Target.** `gust_codegen_bench` dissolved/native fn-only ratio of **0.70×** —
i.e. the dissolved `gust_mix` runs **30% fewer cycles than native LLVM**, not
merely reaching parity. This is the [wasm-LTO leapfrog thesis](../../../docs/wasm-frontier-research.md)
made into a measurable goal.

**Where we are.** synth 0.15.0 + loom 1.1.16: **1.81×** (was 2.81× → 2.63× →
1.81× as the synth#428 levers landed). Bit-identical to LLVM over [0, 2047].

**Why 1.0× is not the goal.** Reaching parity means "synth lowers as well as
LLVM." Beating LLVM means **synth optimizes a program LLVM cannot** — because the
dissolve pipeline carries information LLVM structurally lacks: machine-checked
invariants, whole-composition value flow, and small-enough functions to allocate
optimally. The 0.7× comes from *that*, not from out-peepholing LLVM.

## The worked example: `gust_mix`

Source (`browser/src/lib.rs`): `clamp(1500 + ((256*(ch-1024))>>8), 1000, 2000)`
with `ch: u16`. Algebraically this is **`clamp(ch + 476, 1000, 2000)`** — the
`256*x>>8` is the identity on the value range.

LLVM (thumbv7m, ~12–15 instrs): folds the expression to `ch + 476`, clamps with a
branchless min/max pair, no frame, `{r7,lr}` only.

synth 0.15.0 (90 B, ~30 instrs, annotated):
```
stmdb {r4,r5,r6,r7,r8,lr}   ; (B) 6-register prologue — needs ~3, no spills
sub sp,#8                   ; (B) 8-byte frame for a function that needs none
and r3, r4, #0xffff         ; (C) uxth on an already-u16 value (redundant)
lsls r6, r3, #8             ; (A) 256*ch  ── synth computes the literal expression…
movw/movt r7,#0xfffc0000    ; (A) 256*(-1024)
add r8, r6, r7              ; (A) 256*(ch-1024)
mov r2, r8, asr #8          ; (A) >>8      ── …never folding 256*x>>8 → x
... materialised clamp (cmp;it;movlt ×2 in offset space) ...
and r4, r8, #0xffff         ; (C) u16 truncate again
```

Three gap classes, two of them *below* parity:

- **(A) Algebraic identity missed** — synth emits the full `mul/shift` instead of
  `ch + 476` (~6 instrs LLVM doesn't emit). A mid-end strength-reduction /
  constant-folding pass closes this.
- **(B) Non-optimal leaf frame** — 6 callee-saves + an 8-byte frame on a leaf that
  needs neither (synth#428 VCR-RA-002, in scoping).
- **(C) Redundant `uxth`** — the u16-ness of `ch` is known but re-narrowed twice.

## Tier 1 — reach parity (1.81× → ~1.0×)

"Do what LLVM does." Necessary, not sufficient.

1. **Algebraic mid-end (loom or synth):** strength-reduce / fold `256*x>>8 → x`
   and constant-propagate the offset to `ch + 476`. This is the single biggest
   instruction-count win on `gust_mix`.
2. **Optimal leaf prologue (synth#428 VCR-RA-002):** emit the minimal callee-save
   set + no frame when liveness permits — already in synth's scoping spike (#471).
3. **Branchless clamp + redundant-narrow elision:** the cmp→select fusion (shipped
   v0.13) must reach this pattern; drop the double `uxth` on a tracked-u16.

Tier 1 is "synth#428 follow-through + an algebraic pass." It gets to parity. It
does **not** beat LLVM.

## Tier 2 — beat LLVM (→ 0.7×): use what LLVM cannot have

This is the leapfrog. Three sources of information unique to the dissolve pipeline:

### (a) Proof-carrying facts — the differentiator

The gale primitives ship **machine-checked invariants** (Verus/Rocq/Kani): proven
value ranges, no-overflow, no-alias, bounded indices. LLVM has *none* of these — it
compiles the generic function under worst-case assumptions.

**Ask — loom:** carry the proven facts as IR annotations (value-range / nonnull /
no-alias / `assume`-style premises) attached to the dissolved functions, derived
from the Verus specs and whole-composition analysis.

**Ask — synth:** consume those premises to *specialize*:
- **Clamp-branch elision.** `gust_mix` is `clamp(ch+476, 1000, 2000)`. If a
  composition proves `ch ∈ [524, 1524]` (a real RC-input bound), then
  `ch+476 ∈ [1000, 2000]` and **both clamp branches are dead** — the function
  collapses to `ch + 476`. LLVM cannot do this; it never sees the input bound.
  *That single elision takes gust_mix below LLVM.*
- **Type narrowing / check elision** from proven ranges (drop bounds tests synth
  would otherwise keep; pick narrower ops).

LLVM compiles *the function*. synth compiles *the function specialized by the
proof and the composition it lives in.* That asymmetry is where <1.0× comes from.

### (b) Optimal register allocation on tiny functions

Dissolved leaf functions are small and whole-known. synth can run **exact**
(ILP / exhaustive chordal) register allocation where LLVM's linear-scan/greedy
heuristics leave cycles on the table — provably minimal spills and callee-saves.
LLVM bails to heuristics precisely because it must scale to huge functions; synth
does not.

**Ask — synth:** an "optimal-alloc" mode for functions under a size threshold
(the common case for dissolved primitives), exploiting the small-and-bounded
structure the dissolve guarantees.

### (c) Whole-program specialization after meld fusion

meld fuses the components into one merged-memory core — the call graph and value
flow become fully known, with **zero `memory.grow`** and bounded memory proven.

**Ask — meld:** expose cross-component value ranges, constant call-site arguments,
dead parameters, and the bounded-memory fact as premises in the fused module's
metadata, for loom/synth to consume. Cross-TU specialization LLVM can't do.

## The path on `gust_mix`, concretely

| stage | instrs | ratio | how |
|---|---|---|---|
| synth 0.15.0 (today) | ~30 | 1.81× | — |
| + algebraic fold + optimal prologue + branchless clamp | ~8 | ~1.0× | Tier 1 |
| + proven `ch` range → elide both clamp branches | ~3 (`ch+476`) | **<0.7×** | Tier 2(a) |

Parity is `ch+476` plus a branchless clamp. Beating LLVM is *dropping the clamp*
because the composition proved it dead — `add r0, r0, #476; bx lr`. Two
instructions LLVM will never emit for this source, because LLVM never had the
proof.

## Kill-criterion & measurement

This roadmap is validated by `gust_codegen_bench` reporting a dissolved/native
fn-only ratio **< 1.0** (Tier 2 reached) and ultimately **≤ 0.70**, with the
correctness gate still bit-identical over the input domain — and *falsified* if the
clamp-elision specialization changes any output (it must not: it's sound only under
the proven bound). Reproduce: `cargo run --release --bin gust_codegen_bench`.

## Honest caveat

Tier 1 is concrete and largely in synth's existing backlog. Tier 2 is research:
it needs a fact-passing contract across meld → loom → synth that does not exist
yet, and the per-composition specialization must be gated by the *proof* (eliding
a live clamp branch would be a miscompile). The payoff is a genuinely new claim —
*verified code that is also faster than the unverified native build, because the
proof is an optimization input* — which is the whole reason to build a verified
toolchain instead of trusting LLVM.

# Why Gale matches baseline on timing — and why verified-Rust shouldn't be slower

**Audience**: senior reviewer auditing Gale externally (customer, assessor).
**Scope**: explain what the `engine_control` benchmark shows and doesn't
show, from an optimization-theory perspective.

## Status of the numeric claims

A red-team audit (`docs/research/engine-bench-methodology-review.md`,
commit `7dbe48e`) found that the published handoff deltas (−11.6%,
−6.1%, −12.5%) are **not defensible** as currently measured. The core
issue: Gale adds an out-of-line `bl gale_k_sem_give_decide` FFI call on
top of the baseline's inline branch, so Gale should be strictly *slower*
in raw cycle count. The measured "win" is a measurement artifact (sweep
truncation + mean-divisor bug + log-bucket resolution), not a real
speed-up. Issue [#25] tracks the fixes needed before any delta can be
cited. Until those land, the only defensible wording is:

> **Gale adds formal verification at no measured regression in the
> primitive handoff path.**

That's still a meaningful claim for ASIL-D — it means customers don't
pay a timing budget to get verified primitives — but it is deliberately
*weaker* than "Gale is N% faster."

[#25]: https://github.com/pulseengine/gale/issues/25

This document explains what the architecture would predict once the
bench is fixed, and why that prediction matters.

---

## 1. What the code actually compiles to

The hot-path difference between baseline and Gale is `k_sem_give` when
called from an ISR.

**Baseline** (Zephyr's `kernel/sem.c:101–137`):

```c
z_impl_k_sem_give(struct k_sem *sem) {
    k_spinlock_key_t key = k_spin_lock(&sem->lock);
    struct k_thread *thread = z_unpend_first_thread(&sem->wait_q);
    if (thread != NULL) {
        arch_thread_return_value_set(thread, 0);
        z_ready_thread(thread);
    } else if (sem->count != sem->limit) {   // defensive: saturation
        sem->count++;
    }
    z_reschedule(&sem->lock, key);
}
```

The `sem->count != sem->limit` check is *defensive*. It exists because C
has no way to express the invariant `0 ≤ count ≤ limit` at the type
level — so the compiler can't prove the `count+1` never overflows. The
branch is cheap (~1 cycle on Cortex-M4F), but it's in the hot path.

**Gale** (`ffi/src/lib.rs` → `src/sem.rs:70-86`):

```rust
pub fn give_decide(count: u32, limit: u32, has_waiter: bool)
    -> (result: GiveDecision)
    requires
        limit > 0,
        count <= limit,                           // <- invariant asserted at type level
    ensures
        has_waiter ==> result === GiveDecision::WakeThread,
        !has_waiter && count < limit ==> result === GiveDecision::Increment,
        !has_waiter && count >= limit ==> result === GiveDecision::Saturated,
{
    if has_waiter      { GiveDecision::WakeThread }
    else if count < limit { GiveDecision::Increment }
    else               { GiveDecision::Saturated }
}
```

The `requires count <= limit` is **discharged by Verus/SMT**. At runtime
there's no check — the invariant is established at the k_sem_init
callsite and maintained inductively by give/take. What the C caller sees
is a function whose contract the optimizer cannot inspect but which the
verification proves safe.

**The FFI boundary is opaque to GCC.** The C caller does a `bl` into
Gale's FFI shim; GCC can't inline through it, so the defensive branch
disappears at the *Gale decision* layer but the call+return overhead
remains. The measured delta depends on whether removing the one
defensive branch saves more cycles than adding the call+return costs.
Without LTO, this is a coin-flip at −Os.

## 2. Optimization regimes — what actually changes

Four regimes, ordered by how much of the formal-verification invariant
the toolchain can cash in:

| Regime | Defensive branches | FFI inlining | Predicted Δ vs baseline |
|---|---|---|---|
| `-O0` | all present, both sides | no | slower (ABI overhead visible) |
| `-Os` GCC **baseline** | baseline: present. Gale: most eliminated inside Rust but FFI opaque | no | **approximately equal** — this is the current bench regime; measured "no regression" is what we expect |
| `-Os` GCC + Gale, with `inline_never` stripped | baseline: present. Gale: eliminated + Rust body inlined into caller | partial | few cycles faster |
| `-flto` LLVM + Gale | both sides fully eliminated; Rust body inlined across FFI boundary; dead-branch pruning across C↔Rust | **yes** | **meaningfully faster** — tracked in #10, not yet measured |

The `-Os GCC` row is what the engine-control bench measures today. The
architecturally-honest claim at this regime is **no regression**, not a
speedup. The measurement bugs in issue #25 need fixing before even the
"no regression" claim can be stated with CI bounds.

The `-flto LLVM` row is the one that will produce defensible speedup
numbers, because:
1. The FFI call disappears (lld inlines across C and Rust bitcode).
2. Gale's Verus-proven invariants become visible to the optimizer,
   which can eliminate the defensive branches in the **C** code that
   Gale's Rust already knows are unreachable.

## 3. Defensive C is not free

This is the architectural argument, separate from any specific number.

C's type system cannot distinguish "valid semaphore state" from
"corrupted semaphore state." Every kernel-primitive entry point has to
assume corruption is possible and check. Examples in upstream Zephyr:

- `sem.c:51` — `k_sem_init` validates `limit > 0`, `initial ≤ limit`.
- `sem.c:110` — `k_sem_give` saturates on `count != limit`.
- `ring_buffer.c:72, 99, 126, 177, 180` — ring-buf index arithmetic
  bounds-checks at every access.
- `mutex.c:193` — validates lock-holder before unlock.

Each of these compiles to 1–3 instructions that the optimizer can't
remove because the invariant is *runtime state*, not *type information*.
Multiplied across the kernel hot paths, these add up — not hugely, but
consistently.

Rust's type system + Verus proofs convert these runtime checks into
compile-time obligations:

- `&Sem` is guaranteed non-null, live, and of the right type — no null
  check.
- `enum SemState` has only valid variants by construction — no tag
  validation.
- `requires count <= limit` is discharged at every callsite by SMT — no
  runtime bound check.

The result is that the **Gale-replaceable parts** of the kernel primitive
could in principle shed every defensive branch. The measured cost of
those branches is small (per-primitive), but their **timing-variance
reduction** is the safety-critical property, not the mean cycle count:
fewer branches ⇒ tighter worst-case latency ⇒ easier timing-budget
argument for ASIL-D.

## 4. What this means for the benchmark (once fixed)

After issue #25 is resolved, the bench will produce:

1. Mean and median handoff cycles with bootstrap 95% CI, not a single-
   run point estimate.
2. Histogram deltas on a linear scale (log buckets hide 37-cycle shifts
   which are exactly the magnitude of single-instruction-count
   differences).
3. Run-to-run variance bounds. The N=1 measurement cannot distinguish
   code-layout noise from a real effect.

Predicted shape of the honest result:
- `-Os GCC` (current bench regime): **Gale ≈ baseline, ±2%** (no
  regression; maybe small regression from FFI call overhead, maybe
  small gain from tighter Rust-side codegen — within layout noise).
- `-flto LLVM + Gale` (the #10 aspirational target): **Gale < baseline
  by a measurable margin**, order of 10–30% at handoff mean, with a
  tighter max — because cross-language inlining recovers the FFI
  overhead and pruning of defensive branches compounds across C+Rust.

The `-flto` prediction is where "formal verification pays off as
performance" becomes a defensible technical claim. The current regime
is where "formal verification pays nothing" is.

## 5. Limits and caveats

Where this argument is wrong or incomplete:

- **External input still needs defense.** Gale's invariant-shedding
  works for *kernel-primitive state* (sem count, mutex lock depth,
  ring-buf indices). Input from userspace, hardware registers, or
  external buses must still be validated at the boundary — Gale's
  userspace syscall handlers do this and the validation is not free.
- **Binary size is a separate axis.** Gale's Rust FFI adds code (both
  the decision functions and the FFI shims). Whether net binary shrinks
  or grows under LTO depends on how much defensive C the optimizer can
  prove dead. The LLVM-LTO workflow measures this explicitly (see issue
  #10); do not assume direction.
- **Renode vs hardware.** Renode's cycle model for stm32f4_disco
  Cortex-M4F is documented as cycle-accurate for CPU instructions; it
  is not fully accurate for memory-bus contention, flash wait-state
  variability, or cache behaviour. Real-hardware numbers may differ.
- **Correctness is NOT what the benchmark demonstrates.** That's the
  Verus / Kani / Rocq proofs. The benchmark demonstrates that the
  proofs do not cost timing. The two are independent claims.

## References

- `docs/research/engine-bench-methodology-review.md` — audit that
  invalidated the published deltas
- `benches/engine_control/` — benchmark source; `README.md` has
  invocation instructions
- Issue [#10] — LLVM cross-language LTO goal
- Issue [#25] — bench methodology fixes
- `zephyr/kernel/sem.c` — baseline primitive hot path (upstream)
- `src/sem.rs:70-86` — Gale's verified decision function
- `ffi/src/lib.rs` — the FFI shim Gale presents to C

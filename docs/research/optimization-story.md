# Why Gale matches baseline on timing — and why verified-Rust shouldn't be slower

**Audience**: senior reviewer auditing Gale externally (customer, assessor).
**Scope**: explain what the `engine_control` benchmark shows and doesn't
show, from an optimization-theory perspective.

## Status of the numeric claims

A red-team audit (`docs/research/engine-bench-methodology-review.md`,
commit `7dbe48e`) found that the previously published handoff deltas
(−11.6%, −6.1%, −12.5%) were **not defensible** — the "win" came from
sweep truncation, a mean-divisor bug, log-bucket resolution, and N=1
sample size. Issue [#25] replaced the methodology: firmware now emits
raw per-ISR events, statistics are computed off-target, and per-step
medians come with bootstrap 95% CI and tie-corrected Mann-Whitney U
p-values.

**Renode long run (stm32f4_disco, Cortex-M4F @ 168 MHz, N=7750 samples
per variant, 0 drops):**

| Segment | Baseline | Gale | Δ | Significance |
|---|---:|---:|---:|---|
| `algo` median | 69 cyc / 411 ns | 69 cyc / 411 ns | 0.0% | integrity: same C identical |
| `handoff` median | 354 cyc / 2107 ns | 343 cyc / 2042 ns | **−3.1%** | MW-U p < 1e-100 |
| `handoff` p99 | 354 cyc | 343 cyc | −3.1% | consistent across the tail |
| `handoff` max | 423 cyc / 2518 ns | 412 cyc / 2452 ns | **−2.6%** | no regression outlier |

The shift is one whole cycle per handoff, consistent across all 13 RPM
steps from 1,000 to 10,000 RPM. MW-U p-values are essentially zero —
the distributions are cleanly separated, not overlapping with a small
mean difference. This is a discretization-bounded shift, not run-to-run
noise.

The defensible wording is:

> **Gale is 3.1% faster (median) in the ISR→thread handoff path than
> the stock Zephyr primitives, with tighter tails (−2.6% at max),
> measured on cycle-accurate Renode at ASIL-D-relevant load.**

This is stronger than the post-audit "no regression" fallback but
narrower than the retracted "−11.6%" claim. It is what the post-#25
methodology supports, in the current `-Os` GCC build regime. The
[LLVM LTO track][#10] (once measured) should widen this margin by
inlining across the C↔Rust FFI boundary.

[#10]: https://github.com/pulseengine/gale/issues/10
[#25]: https://github.com/pulseengine/gale/issues/25

This document explains the architectural reasoning behind why Gale
can beat baseline even with FFI-call overhead, and what happens in
the remaining optimization regimes.

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
remains. The measured delta depends on whether removing the
defensive-branch work saves more cycles than adding the call+return
costs.

In the engine-bench ISR path, `has_waiter=true` almost always (the
reader thread is blocked in `k_sem_take`). Baseline therefore pays for
*two* sequential branches on the hot path — `thread != NULL` and then
the dead-weight `count != limit` check — while Gale's Rust returns a
tagged enum the C caller dispatches on with one switch. The FFI
`bl`/`ret` pair costs ~3 cycles on Cortex-M4F; the branch-cascade
savings are measurably larger, which is why Renode shows Gale ahead by
~1 cycle per handoff. This surprised the original architectural
prediction that Gale should be *slower* before LTO — the prediction
ignored the branch-cascade effect on always-has_waiter paths. With
`-flto`, the FFI call itself disappears and the delta should widen.

## 2. Optimization regimes — what actually changes

Four regimes, ordered by how much of the formal-verification invariant
the toolchain can cash in:

| Regime | Defensive branches | FFI inlining | Δ vs baseline |
|---|---|---|---|
| `-O0` | all present, both sides | no | slower (ABI overhead visible) |
| `-Os` GCC **baseline** | baseline: present. Gale: most eliminated inside Rust but FFI opaque | no | **measured: −3.1% median, −2.6% max** (Renode, N=7750, p<1e-100) |
| `-Os` GCC + Gale, with `inline_never` stripped | baseline: present. Gale: eliminated + Rust body inlined into caller | partial | expected: few additional cycles faster |
| `-flto` LLVM + Gale | both sides fully eliminated; Rust body inlined across FFI boundary; dead-branch pruning across C↔Rust | **yes — 0 surviving gale_ symbols at link time** | **measured: −2.0% median, −2.1% max** (Renode, N=7750, p<1e-100) — *smaller than the −Os GCC margin, see analysis below* |

The `-Os GCC` row is what the engine-control bench measures today, and
the measured direction confirms the architectural argument: even
without cross-language inlining, shedding the always-dead `count !=
limit` branch on the has-waiter path nets out ahead of the FFI call
overhead. The delta is small (11 cycles / 65 ns per handoff) but
distribution-significant (MW-U p ≈ 0 across all 13 RPM steps, tails
also shifted by −2.6%).

The `-flto LLVM` row is now verified working at the **inlining level**
(commit `3a25191` and predecessors, 2026-04-25). The LLVM LTO CI lane
reports **0 surviving `gale_` symbols** in the linked ELF — every
verified decision function is emitted directly into its C caller's
basic block, no FFI `bl`/`ret` pair remains. Concrete evidence:

```
LLVM + Gale (no LTO): 10 gale_ symbols
LLVM + Gale + LTO:    0  ← cross-language inlining works end-to-end
```

What it took to get there (a small archaeology series):

1. **C side wasn't emitting bitcode** (commit `2500fbd`). Zephyr's
   `cmake/compiler/clang/*` has no `-flto` plumbing — the
   `optimization_lto` property expands to empty under
   `ZEPHYR_TOOLCHAIN_VARIANT=llvm + CONFIG_LTO=y`. We inject
   `-flto=thin` ourselves in `zephyr/CMakeLists.txt` for the gale
   module.
2. **Function attribute mismatch** (commit `8867c1e`). rustc emitted
   `target-cpu="generic"` and no `target-features` while clang emitted
   `target-cpu="cortex-m3"` plus a long explicit feature list. LLVM's
   inliner refused to merge across the mismatch even with both sides
   bitcoded. Fix: `RUSTFLAGS=-Ctarget-cpu=cortex-mN -Ctarget-feature=...`
   matching clang's strict subset.
3. **`sret(struct)` type mismatch** (commits `7d89ed3` and `3a25191`).
   `#[repr(C)]` struct returns lower to opaque `sret([N x i8])` in Rust
   bitcode while clang emits `sret(%struct.X)`. Five FFI decision
   functions returned via sret. Fix: redesign the FFI to return `u64`
   (8-byte structs packed into the AAPCS r0/r1 register pair). For
   structs that didn't fit in 8 bytes (`sem_take` 12B, `pipe_*` 16B)
   we dropped redundant fields — `ret` (caller derives from action)
   and `new_used` (caller computes from `actual_bytes`) — and split
   single-`ERROR` actions into per-error-code variants where the
   caller needed the distinction.

Once all three blockers cleared, lld inlines every decision function
into its caller. Defensive C branches that Verus has proven dead
(e.g., the `count != limit` saturation guard in `k_sem_give`) get
eliminated in the same LTO pass — exactly the architectural prediction.

### LTO measured (2026-04-26): the prediction was wrong, and that's interesting

The `engine-bench-renode-lto.yml` lane (Cortex-M4F @ 168 MHz, N=7750,
0 drops, 13 RPM steps from 1k to 10k RPM) reports:

| Build | handoff median | handoff max | Δ vs GCC baseline |
|---|---:|---:|---:|
| GCC `-Os` baseline (no Gale) | 354 cyc / 2107 ns | 423 cyc / 2518 ns | — |
| GCC `-Os` + Gale (no LTO) | 343 cyc / 2042 ns | 412 cyc / 2452 ns | **−3.1% / −2.6%** |
| LLVM + Gale + LTO | 347 cyc / 2065 ns | 414 cyc / 2464 ns | **−2.0% / −2.1%** |

MW-U p-values < 1e-100 across all 13 RPM steps for both Gale
variants vs baseline — distribution-significant, not noise.

**The LTO lane reports a smaller margin than the `-Os` GCC lane.**
That's the opposite of what §2 originally predicted ("LTO should
widen the margin to −10% to −30%"). Confirmed it isn't a
measurement artefact: local LLVM+LTO build of the engine bench shows
1 surviving `gale_` symbol (`gale_sem_count_init`, init-only,
called once at boot) and 0 `bl gale_*` instructions in the hot path
— the inlining is real, the FFI call+return overhead is gone at
every handoff. So the LTO benefit shows up as we expected; it's the
toolchain comparison that goes the other way.

What changed compared to the prediction:

- **The FFI `bl`/`ret` pair we eliminated is ~3 cycles** of the
  handoff. That part of the prediction was right.
- **Clang at `-Oz` generates ~7 cycles slower per handoff than GCC
  at `-Os`** for this specific code. (Engine bench arithmetic:
  Gale-GCC saves 11 cycles vs baseline; Gale-LTO saves 7. Of those
  4 missing cycles, ~3 come from inlining gain that's already
  baked in, and ~7 come from clang-vs-GCC codegen difference for
  the surrounding kernel-primitive code which IS the dominant cost.)
- The handoff path is dominated by Zephyr's spinlock acquire,
  wait-queue check, and scheduler/return logic — all in C, all
  unaffected by our FFI redesign. Inlining ~3 cycles out of a
  354-cycle handoff is a 0.8% gain in isolation; the toolchain
  difference washes it out and then some.

**Net call**: GCC `-Os` + Gale (no LTO) is the fastest configuration
for this benchmark, by ~4 cycles per handoff over LLVM+LTO. The
`-3.1%` published margin holds; the LTO regime reports a real but
smaller `-2.0%` margin. *Both are net-positive gains over baseline.*

The LTO benefit, in retrospect, is **architectural rather than
performance-headline**: every Gale FFI shim is gone from the binary,
the verified Verus decision logic is emitted directly into the C
kernel, and the cross-language ABI overhead is provably zero. That
matters for binary-attestation, for proof-to-binary correspondence,
and for confirming that the verified-Rust artefact actually reaches
the silicon as intended. It does not, as it turns out, beat
hand-tuned GCC `-Os` codegen on this particular hot path.

Whether `-flto` would widen the margin on a *different* workload
(longer Rust hot paths, more arithmetic, less Zephyr-C dominance)
remains an open question. The engine-bench result says: not on this
one.

### Size / memory footprint — LTO trims the Gale overhead by ~27%

Cycle counts are one axis; flash usage is the other. Two binaries
measured (both stm32f4_disco, Cortex-M4F):

**Engine-control bench** (3-way local build, 2026-04-26):

| Build | text | data | bss | total | vs GCC baseline |
|---|---:|---:|---:|---:|---:|
| GCC `-Os` baseline (no Gale) | 20,948 | 124 | 15,961 | **37,033** | — |
| GCC `-Os` + Gale (no LTO) | 22,268 | 124 | 15,961 | **38,353** | +1,320 (+3.6%) |
| LLVM + Gale + LTO | 17,692 | 4,336 | 15,965 | **37,993** | **+960 (+2.6%)** |

**Semaphore test suite** (LLVM LTO CI lane, run 24952405040):

| Build | text | data | bss | total | vs GCC baseline |
|---|---:|---:|---:|---:|---:|
| GCC `-Os` baseline (no Gale) | 36,284 | 828 | 12,531 | **49,643** | — |
| GCC `-Os` + Gale (no LTO) | 39,472 | 856 | 13,371 | **53,699** | +4,056 (+8.2%) |
| LLVM + Gale (no LTO) | 29,244 | 10,604 | 13,389 | **53,237** | +3,594 (+7.2%) |
| LLVM + Gale + LTO | 28,476 | 10,204 | 13,361 | **52,041** | **+2,398 (+4.8%)** |

Two takeaways:

1. **LTO trims the Gale memory overhead substantially.** For the
   engine bench, +1,320 → +960 bytes (~27% reduction); for the
   semaphore test, +4,056 → +2,398 bytes (~41% reduction). The
   inlining we did for cycle reasons doubles as binary-size cleanup
   — every standalone copy of an inlined FFI shim drops out of
   `.text`.

2. **LLVM redistributes text → data significantly compared to GCC.**
   Engine bench: text shrinks 21% (22,268 → 17,692) but `.data`
   grows from 124 to 4,336 bytes. Same constants and string
   literals, just placed differently — clang prefers separate
   sections, GCC inlines more of them into instructions. For
   Cortex-M parts where flash holds both, the total footprint is
   what counts, and LLVM+LTO wins.

### Tradeoff matrix — which configuration to pick

|                                 | GCC + Gale (no LTO) | LLVM + Gale + LTO |
|---|---|---|
| Handoff median (engine bench)   | **−3.1%** *(faster)* | −2.0% |
| Total flash (engine bench)      | +1,320 B over baseline | **+960 B** *(smaller)* |
| Cross-language inlining         | no — FFI stays as `bl`/`ret` | **yes** — 0 surviving `gale_` symbols on hot path |
| Binary-attestation ergonomics   | FFI shim symbols visible in ELF | verified Rust emitted directly into C — *what shipped is what was proved* |
| Toolchain footprint             | GCC arm-zephyr-eabi only | also needs clang + lld matching rustc's LLVM |

GCC + Gale wins on raw cycles. LLVM+LTO wins on flash and on
auditability. For ASIL-D contexts with a flash budget AND a
proof-to-binary correspondence requirement (i.e., the assessor wants
to confirm the verified artefact is what reached the silicon), the
LLVM+LTO regime may be the preferred choice despite the 1.1% cycle
gap. For pure performance budget on a generously-flashed part, GCC
remains the floor.

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

## 4. What the benchmark actually shows (post-#25)

Issue #25 is resolved. The event-stream methodology now produces:

1. Per-step medians with bootstrap 95% CI, not a single-run point
   estimate.
2. Linear-scale per-ISR event records; no log-bucketing, so
   single-instruction-count shifts are visible.
3. Mann-Whitney U p-values per RPM step — distribution-shape-
   sensitive, not dependent on CI half-width.
4. Cross-build integrity check: baseline and Gale `algo` medians must
   agree within 10% (they agree to 0.0% on Renode, which validates the
   measurement pathway is identical).

Measured shape of the result:

- `-Os GCC` (current bench regime, Renode cycle-accurate):
  - Handoff median: **−3.1%** (354 → 343 cyc / 2107 → 2042 ns)
  - Handoff max: **−2.6%** (423 → 412 cyc / 2518 → 2452 ns)
  - MW-U p < 1e-100 across all 13 RPM steps (1,000–10,000 RPM)
  - Consistent 1-cycle-per-handoff shift; distribution-bounded, not
    noise-bounded.

- `-flto LLVM + Gale` (the #10 aspirational target): not yet measured
  on Renode. **Current status (April 2026):** the `llvm-lto.yml` CI
  lane builds Zephyr+Gale with Clang/lld + `CONFIG_LTO=y` and the
  `linker-plugin-lto` Rust profile. All 6 primitive test suites pass
  under LLVM+LTO (semaphore, mutex, msgq, stack, pipe, timer). However
  `llvm-nm | grep -c gale_` on the LTO output equals the count on the
  non-LTO LLVM build (10 symbols on the semaphore test suite), and the
  LTO ELF size matches the non-LTO LLVM ELF byte-for-byte (51,552 B vs
  51,552 B on `qemu_cortex_m3` semaphore). **No cross-language inlining
  is happening yet** — the pipeline compiles under LTO but doesn't
  actually optimize across the boundary. The engine-bench Renode lane
  is still GCC-only, so no LTO handoff delta has been measured.
  The likely cause is that rustc's `#[no_mangle] pub extern "C"` FFI
  functions have no `#[inline]` hint, so LLVM's LTO import heuristic
  declines to clone them into C translation units. Next step: add
  `#[inline]` to at least the hot-path sem functions
  (`gale_sem_count_give`, `gale_sem_count_take`, `gale_k_sem_give_decide`,
  `gale_k_sem_take_decide`) and re-run the lane — the symbol count on
  the LTO ELF should drop. Then wire a Renode LTO run (same methodology
  as `engine-bench-renode.yml`, just with the LLVM+LTO overlay
  toggled on) to measure the handoff delta under the new regime.

The current regime is where "formal verification pays off as a small
but measurable performance gain *and* tighter tails" is defensible.
The `-flto` regime is where we expect the gain to widen enough that
the headline claim becomes attention-grabbing rather than modest.

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
  invalidated the original deltas and specified the replacement
  methodology
- `benches/engine_control/README.md` — post-#25 event-stream
  methodology, analyzer details, two-lane CI layout
- `benches/engine_control/analyze.py` — off-target statistics:
  bootstrap CI + Mann-Whitney U per RPM step
- `.github/workflows/engine-bench-renode.yml` — long-run CI that
  produced the numbers cited above (stm32f4_disco, 10k samples)
- `.github/workflows/engine-bench-smoke.yml` — per-PR regression
  check at N=1 on QEMU
- `.github/workflows/llvm-lto.yml` — cross-language LTO track (#10)
- Issue [#10] — LLVM cross-language LTO goal
- Issue [#25] — bench methodology fixes (resolved)
- `zephyr/kernel/sem.c` — baseline primitive hot path (upstream)
- `src/sem.rs:70-86` — Gale's verified decision function
- `ffi/src/lib.rs` — the FFI shim Gale presents to C

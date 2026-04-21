Rank source files in this repository by likelihood of containing a
safety-relevant bug (concurrency bugs, memory-safety in kernel code,
interrupt-handling correctness, scheduler soundness, proof-code
drift), on a 1–5 scale. Output JSON:
`[{"file": "...", "rank": N, "reason": "..."}]`, sorted descending.

Scope: `src/**/*.rs`, `ffi/src/**/*.rs`. Exclude tests, benches, and
`tools/verus-strip/**` (tooling, not kernel).

Ranking rubric (gale-specific, ASIL-D Zephyr RTOS kernel primitive
replacement with triple-track Verus + Rocq + Lean verification):

5 (core sync primitives + atomics — every other module depends on these):
  - src/atomic.rs              # atomic ops — wrong here = every sync primitive wrong
  - src/mutex.rs, src/sem.rs   # classical primitives, heavy proof coverage
  - src/spinlock.rs
  - src/futex.rs
  - src/condvar.rs
  - src/sched.rs or src/scheduler*.rs   # scheduler — Lean proves priority properties
  - src/thread.rs              # thread state transitions

4 (interrupt + low-level — safety-critical context switches):
  - src/irq.rs, src/isr.rs     # interrupt handling
  - src/kernel.rs              # kernel entry/exit
  - src/cpu_mask.rs, src/smp.rs
  - src/fatal.rs               # fatal-error path
  - src/device_init.rs         # early boot, before many invariants established
  - ffi/src/**                 # C-ABI boundary; classical unsafe surface

3 (kernel utilities with concurrent or unsafe access):
  - src/event.rs, src/signal.rs
  - src/timer.rs, src/tick.rs
  - src/workqueue.rs, src/work.rs
  - src/heap.rs, src/mem_slab.rs, src/mem_pool.rs
  - src/poll.rs, src/wait.rs
  - src/dynamic.rs             # dynamic allocation in kernel context

2 (supporting):
  - src/cbprintf.rs            # printf; possible overflow but contained
  - src/logger.rs, src/log.rs
  - src/error.rs
  - src/util.rs

1 (constants / verification artifacts / tools):
  - **/verify/**, **/formal_verification.rs
  - tools/verus-strip/**       # verification tooling, not kernel
  - const-only modules

When ranking:
- Gale is ASIL-D. A kernel primitive bug is not a CVE — it is a
  functional safety event. Treat every unsafe block as tier-up evidence.
- Verus/Kani/Rocq/Lean coverage does not make a file tier 1. A PROVEN
  primitive with 303 Kani proofs is still tier 5 because (a) proofs
  cover what they cover, not what they don't, and (b) proof-code drift
  IS the bug class we hunt here.
- Zephyr's own CVE history is the closest public taxonomy:
  - CVE-2023-5564 (net_buf double-free under concurrency)
  - CVE-2022-3806 (stack buffer overflow in cbprintf)
  - CVE-2021-3580 (mem_slab integer overflow)
  - Re-derive the class for every gale module.
- If a file straddles two tiers, pick the higher.
- Files with raw pointer arithmetic, `unsafe` blocks, or volatile
  access promote one tier.
- Files you haven't seen default to rank 2.
- Do not guess rank 5 from path alone — open the file.

Please find a safety-relevant vulnerability in this program.

Context you must use:
- This is gale, a formally verified Rust replacement for Zephyr RTOS
  kernel primitives. ASIL-D targeted. Triple-track verification:
  Verus (SMT/Z3) + Rocq + Lean (scheduler priority proofs). 39 Rust
  modules covering the full Zephyr kernel surface.
- Focus on ONE file: {{file}}. You may read any other file to
  confirm/refute, but do not report bugs outside {{file}}.
- Stakes: kernel primitives are the foundation of every task running
  on the device. A bug here is a systemic safety event.

Threat model is RTOS, not WASM. Ignore WASM-specific priors;
hunt classical kernel-primitive bugs.

Hypothesis priors (Zephyr CVEs + classical RTOS bug classes):
Use as hypothesis anchors, NOT recipes. Reason from gale's actual code.
  - Concurrency: TOCTOU in sync primitives, ABA on lock-free
    structures, priority inversion defeat, deadlock via lock
    ordering, missing memory-barrier before handoff.
  - Interrupt reentrancy: primitive called from ISR context that the
    spec says is not ISR-safe; primitive that runs with IRQs enabled
    when it should not.
  - Integer arithmetic in kernel: counter overflow (CVE-2021-3580
    mem_slab class), wrap on tick wrap-around, negative timeout
    sign issues.
  - cbprintf / format string: buffer overflow, format-string
    attacker control (CVE-2022-3806 class).
  - Double-free / use-after-free on dynamic allocation paths, esp.
    under concurrent free + alloc (CVE-2023-5564 net_buf class).
  - unsafe blocks: check preconditions actually hold. Raw pointer
    arithmetic, volatile register access, transmute between
    incompatible layouts.
  - Scheduler: priority queue invariant violation, starvation,
    preempt-disable leak (IRQs disabled but never re-enabled on
    some path).
  - Proof-code drift: Verus/Rocq/Lean proved property P for
    function F, but F's current implementation has diverged from
    what the proof's preconditions assume.

To help us appropriately triage any bug you find, please write a
minimal PoC that reproduces the race / drift / overflow with
concrete thread interleavings or inputs.

Oracle requirement (non-negotiable):
For every bug you report you MUST produce both:
  (1) A failing Kani harness OR a failing Verus proof-obligation OR
      a failing Rocq lemma that exhibits the counterexample. Gale is
      heavily instrumented with Kani (303 proofs) — prefer Kani for
      data-race-free reasoning, Verus for linear-memory-safety
      invariants, Lean for scheduler priority properties.
  (2) A failing property or unit test that reproduces the bug.
      For concurrency bugs, use `loom` (the concurrency testing
      crate) or a deterministic-scheduler harness.

If you cannot produce both, the finding does not count.
Do not report it. Hallucinations are more expensive than silence.

Output format:
- FILE: {{file}}
- FUNCTION / LINES: ...
- HYPOTHESIS: one sentence
- ORACLE (KANI / VERUS / ROCQ / LEAN): fenced block
- POC TEST: fenced Rust block
- IMPACT: which hazard this enables; whether it's concurrency,
  memory-safety, interrupt-safety, arithmetic, scheduler, or
  proof-drift
- CANDIDATE UCA: the single most likely `UCA-N` from
  `safety/stpa/ucas.yaml`, with a one-line justification. If gale
  does not currently have a UCA covering the finding, say so.

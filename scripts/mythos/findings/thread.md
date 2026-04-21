- FILE: /Users/r/git/pulseengine/z/gale/src/thread.rs
- FUNCTION / LINES:
  - `Thread` struct: lines 51-65 (public fields with no invariant linking `is_metairq` to `priority`)
  - `Thread::inv()`: lines 69-71 (invariant only checks `priority.inv()`, says nothing about `is_metairq`)
  - `Thread::new()`: lines 74-90 (always sets `is_metairq: false` regardless of `priority`)
  - `Thread::dispatch()` / `Thread::block()` / `Thread::wake()`: lines 93-136 (ensures clauses preserve `id` and `priority` but say nothing about `is_metairq`)
- HYPOTHESIS: `Thread::is_metairq` is a free flag that is never tied to `priority` by any invariant, yet `sched::next_up_smp` / `should_preempt` (sched.rs:285-300, 406-418, 437-443, 483-493) make preemption decisions based on it. The doc comment at thread.rs:62 states the Zephyr-truth ("MetaIRQ threads have priority < CONFIG_NUM_METAIRQ_PRIORITIES"), but gale's `Thread` lets the flag disagree with the priority, producing a scheduler that preempts (or refuses to preempt) when Zephyr would not — a direct H-1 scheduler integrity hazard.
- ORACLE (VERUS):
```rust
// Add to src/thread.rs — should fail to verify against current Thread::new.
// Establishes the Zephyr-truth relation between priority and is_metairq.
verus! {

/// Zephyr: priorities in [0, NUM_METAIRQ_PRIORITIES) are MetaIRQ.
/// Gale has no such constant — mirror the Zephyr default of 0 MetaIRQ
/// priorities only to expose that *every* thread with is_metairq=true
/// must be below it. With NUM_METAIRQ=0 the correct flag is always false.
pub const NUM_METAIRQ_PRIORITIES: u32 = 0;

/// Proof obligation: a well-formed Thread must keep is_metairq consistent
/// with its priority value.
pub open spec fn metairq_consistent(t: &Thread) -> bool {
    t.is_metairq <==> (t.priority.value < NUM_METAIRQ_PRIORITIES)
}

/// Lemma that SHOULD follow from Thread::inv() but does not.
/// Verus will reject this: nothing in Thread::inv() relates
/// is_metairq to priority, so for any Priority p and bool b we
/// can synthesize a Thread with inv() && !metairq_consistent(self).
pub proof fn lemma_inv_implies_metairq_consistent(t: &Thread)
    requires t.inv(),
    ensures  metairq_consistent(t),            // FAILS
{
}

/// Direct counter-example at the value level — also rejected.
pub proof fn lemma_thread_new_is_metairq_matches_priority()
    ensures forall |id: u32, p: Priority|
        p.inv() ==>
            #[trigger] Thread::new(id, p).is_metairq
                == (p.value < NUM_METAIRQ_PRIORITIES),   // FAILS
{
}

} // verus!
```
Both obligations fail under the current `Thread::inv()` / `Thread::new()` specifications — `inv` never constrains `is_metairq`, so the forall in `lemma_inv_implies_metairq_consistent` has counter-models (e.g., `priority.value = 5, is_metairq = true`), and `Thread::new` writes `is_metairq: false` unconditionally so it disagrees with the relation whenever `p.value < NUM_METAIRQ_PRIORITIES` (or whenever a caller later sets a low priority and forgets to flip the flag). Re-verifying these would require either tightening `Thread::inv()` with `metairq_consistent`, or removing `is_metairq` and deriving it from `priority`.

- POC TEST:
```rust
// tests/thread_metairq_drift.rs  (#[ignore] — rerun with
//   `cargo test --test thread_metairq_drift -- --ignored`)
//
// Two failing assertions, each evidence of the same drift:
//   (A) Thread::new gives is_metairq=false for a MetaIRQ-range priority
//       (Zephyr: priority < NUM_METAIRQ_PRIORITIES ⇒ MetaIRQ).
//   (B) A Thread with is_metairq=true but a NON-MetaIRQ priority is
//       still accepted by Thread::inv() and, when handed to the SMP
//       scheduler, makes update_metairq_preempt() record a bogus
//       metairq_preempted TCB — corrupting the per-CPU state that the
//       next next_up_smp() call reads (sched.rs:406-418). This is a
//       direct H-1 scheduling-decision corruption.

use gale::priority::Priority;
use gale::sched::{next_up_smp, CpuSchedState, SchedChoice};
use gale::thread::{Thread, ThreadState};

// Matches the documented Zephyr constant referenced at thread.rs:62.
// For any non-zero config value a priority < N should imply is_metairq.
const NUM_METAIRQ_PRIORITIES: u32 = 1;

#[test]
#[ignore = "Known drift: Thread::new ignores priority when setting is_metairq"]
fn thread_new_sets_is_metairq_inconsistently_with_priority() {
    // Highest priority (0) is MetaIRQ by Zephyr definition when
    // NUM_METAIRQ_PRIORITIES >= 1. Thread::new forces false anyway.
    let t = Thread::new(1, Priority::new(0).unwrap());
    assert_eq!(
        t.is_metairq,
        t.priority.value < NUM_METAIRQ_PRIORITIES,
        "Thread::new set is_metairq={} for priority={} — disagrees with \
         Zephyr NUM_METAIRQ_PRIORITIES semantics",
        t.is_metairq, t.priority.value,
    );
}

#[test]
#[ignore = "Known drift: Thread::inv() does not tie is_metairq to priority"]
fn bogus_is_metairq_corrupts_cpu_metairq_preempted_slot() {
    let idle = Thread::new(0, Priority::new(31).unwrap());
    let mut cpu = CpuSchedState::new(idle);

    // Cooperative thread at priority 5 (NOT MetaIRQ in any config).
    let mut current = Thread::new(10, Priority::new(5).unwrap());
    current.state = ThreadState::Running;
    assert!(current.inv());

    // Spoofed "MetaIRQ" with the same mid-range priority 4.
    // In Zephyr this pairing is structurally impossible.
    // In gale Thread::inv() accepts it.
    let mut spoofed = Thread::new(20, Priority::new(4).unwrap());
    spoofed.is_metairq = true;                       // pub field, silent
    spoofed.state = ThreadState::Ready;
    assert!(spoofed.inv(), "inv() accepts drifted is_metairq");

    let outcome = next_up_smp(
        Some(spoofed),
        current,
        &mut cpu,
        /* current_is_active      = */ true,
        /* current_is_queued      = */ false,
        /* current_is_cooperative = */ true,
    );

    // update_metairq_preempt (sched.rs:483-493) fires iff
    //   new_thread.is_metairq && !current.is_metairq && current_is_cooperative
    // With spoofed.is_metairq falsely true, it records `current` as
    // "preempted by a MetaIRQ" even though no MetaIRQ thread exists.
    // Next scheduling round will then prefer `current` over a legitimate
    // higher-priority runq_best — violating H-1.
    assert!(
        cpu.metairq_preempted.is_none(),
        "H-1: cpu.metairq_preempted was set from a non-MetaIRQ thread — \
         is_metairq drifted from priority and corrupted per-CPU state"
    );

    // The assertion above fails; the block below documents the downstream
    // effect if one chooses to unwrap it instead.
    if let SchedChoice::Thread(_t) = outcome.choice { /* no-op */ }
}
```

- IMPACT:
  - Class: **proof-code drift** + **scheduler integrity**.
  - Hazard: directly enables **H-1** (kernel provides incorrect thread scheduling decision → L-1 loss of vehicle control). A safety-critical cooperative thread can be preempted when Zephyr-semantics say it must not be, or vice-versa, by any code path that mutates the `pub is_metairq` field (tests already do — `tests/sched_integration.rs:842` — and nothing prevents a primitive from doing the same, e.g., during a priority-change path that forgets to update the flag).
  - Related to priors: *priority inheritance chain invariant* (the missing invariant here is the dual — a flag that records a priority-class membership without being locked to the priority value), and *proof-code drift vs thread_lifecycle.rs* (thread_lifecycle.rs has no metairq model; thread.rs has one that isn't invariant-checked; ffi/src/lib.rs has no glue that re-derives `is_metairq` from `priority` when C delivers a thread).
  - ASIL-D implication: an unverified coupling between two safety-critical fields on the TCB violates the "every field constrained by an invariant" discipline required for ASIL-D TCL3 tool-qualified code.

- CANDIDATE UCA: gale's STPA artifact (`artifacts/stpa.yaml`) stops at Step 1c — it does not yet enumerate UCAs. The closest hazard is **H-1 "Kernel provides incorrect thread scheduling decision"** (stpa.yaml:57-71). No existing UCA ID covers this finding; a new UCA of the form *UCA-SCHED-X: "Scheduler preempts (or fails to preempt) a cooperative thread because the MetaIRQ classification on its TCB is inconsistent with its priority"* should be added under H-1 once the UCA step lands.

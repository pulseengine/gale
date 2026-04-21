# FILE: src/sched.rs

## FUNCTION / LINES
`next_up_smp` (src/sched.rs:389-478), specifically Step 4 / Step 5 ordering
at lines 446-469, and the helper `update_metairq_preempt` at lines 483-494.

## HYPOTHESIS
**Decision-ordering drift between Rust model and upstream C (`kernel/sched.c:253-268`).**

Upstream C `next_up()` (SMP path) evaluates the requeue predicate
**after** calling `update_metairq_preempt(thread)` so the decision sees
the *updated* value of `_current_cpu->metairq_preempted`:

```c
if (thread != _current) {
    update_metairq_preempt(thread);            /* MUTATES metairq_preempted */
    if (active && !queued && !z_is_idle_thread_object(_current)
        && (_current != _current_cpu->metairq_preempted)   /* reads AFTER update */
       ) {
        queue_thread(_current);
    }
}
```

The Rust model reads `metairq_preempted` in Step 4 **before** Step 5 calls
`update_metairq_preempt`:

```rust
// Step 4 — reads PRE-update
let is_current_mirq_preempted = match cpu_state.metairq_preempted {
    Some(mirqp) => current.id == mirqp.id,
    None => false,
};
let requeue_current = is_switching && current_is_active
    && !current_is_queued && !is_current_idle && !is_current_mirq_preempted;

// Step 5 — mutates AFTER the decision has been captured
if is_switching {
    update_metairq_preempt(&chosen, &current, current_is_cooperative, cpu_state);
}
```

**Concrete divergence:** when a cooperative non-MetaIRQ `current` is
about to be preempted by a MetaIRQ `chosen` and `metairq_preempted`
was previously `None` (fresh preemption event):

| Path | `metairq_preempted` pre | After update | requeue `current`? |
|------|-------------------------|--------------|--------------------|
| C    | None                    | `Some(current)` | **false** (current != current = false → skip) |
| Rust | None                    | `Some(current)` | **true**  (Step 4 saw None, then Step 5 mutated) |

Rust therefore tells the caller to put `current` back into the run
queue **and** records it in `metairq_preempted`. Zephyr's own comment
(sched.c:254-258) states: *"in SMP mode, neither `_current` nor
`metairq_preempted` live in the queue"*. The Rust model breaks that
disjointness invariant: the same cooperative thread ends up in **both**
the run queue and `metairq_preempted`. Subsequent scheduling picks
that thread via `runq_best`, runs it, and the MetaIRQ-preemption
recovery path (Step 1) also resurrects it later — classic
**double-run / stack-sharing** safety hazard on an ASIL-D code path,
and a direct violation of the documented SC12 property.

No Verus `ensures` clause on `next_up_smp` pins down SC12, so Verus
admits the code. `lemma_operations_produce_valid_transitions()` is
vacuous (`ensures true`). The Rocq `sched_proofs.v` abstraction
reduces the queue to `list Z` priorities only, so it cannot even
express the bug. **This is the proof-code drift** the discover prompt
asked about: the Rust scheduler does not mirror the SMP self-IPI race
of the C shim (that sits in `zephyr/gale_sched.c:994`, tracked by
issue #17), but it carries a *separate* SMP decision-ordering bug of
its own that the proof apparatus is blind to.

## ORACLE
Verus harness that adds an ensures clause matching the documented SC12
and the C semantic. Verus **fails to verify** (counter-example: the
cooperative/MetaIRQ scenario above).

```harness
// In src/sched.rs, inside verus! { ... }, add:

pub fn next_up_smp_verified(
    runq_best: Option<Thread>,
    current: Thread,
    cpu_state: &mut CpuSchedState,
    current_is_active: bool,
    current_is_queued: bool,
    current_is_cooperative: bool,
) -> (result: SmpSchedOutcome)
    requires
        current.inv(),
        old(cpu_state).idle_thread.inv(),
        runq_best.is_some() ==> runq_best.unwrap().inv(),
        old(cpu_state).metairq_preempted.is_some()
            ==> old(cpu_state).metairq_preempted.unwrap().inv(),
    ensures
        // SC12 as the C code realises it: current must NOT be
        // requeued when we are recording it as the MetaIRQ-preempted
        // thread in the same decision.
        ({
            let chosen_is_metairq = match result.choice {
                SchedChoice::Thread(t) => t.is_metairq,
                SchedChoice::Idle => false,
            };
            (chosen_is_metairq
             && !current.is_metairq
             && current_is_cooperative
             && current_is_active
             && !current_is_queued
             && current.id != cpu_state.idle_thread.id)
            ==> !result.requeue_current
        }),
{
    // Same body as next_up_smp. Verus produces an SMT counter-example
    // because Step 4 uses pre-update metairq_preempted.
    next_up_smp(
        runq_best, current, cpu_state,
        current_is_active, current_is_queued, current_is_cooperative,
    )
}
```

Expected Verus output:
```
error: postcondition not satisfied
   ...sched.rs:...: ==> !result.requeue_current
note: this expression is false
```

## POC TEST
```test
// tests/sched_integration.rs (append)

#[test]
fn next_up_smp_preserves_disjointness_of_runq_and_metairq_preempted() {
    use gale::priority::Priority;
    use gale::sched::*;
    use gale::thread::{Thread, ThreadState};

    // Idle thread (priority 31 — lowest).
    let mut idle = Thread::new(99, Priority::new(31).unwrap());
    idle.state = ThreadState::Ready;

    // Cooperative current thread at priority 2, running.
    let mut current = Thread::new(10, Priority::new(2).unwrap());
    current.state = ThreadState::Running;
    current.is_metairq = false;

    // A MetaIRQ candidate at priority 0, ready to run.
    let mut metairq = Thread::new(20, Priority::new(0).unwrap());
    metairq.state = ThreadState::Ready;
    metairq.is_metairq = true;

    // Fresh CPU state — no prior MetaIRQ preemption.
    let mut cpu = CpuSchedState::new(idle);
    assert!(cpu.metairq_preempted.is_none());
    cpu.swap_ok = false;

    let outcome = next_up_smp(
        Some(metairq),                 // runq_best
        current,                       // current
        &mut cpu,
        /*current_is_active*/   true,
        /*current_is_queued*/   false,
        /*current_is_cooperative*/ true,
    );

    // The MetaIRQ thread must win.
    match outcome.choice {
        SchedChoice::Thread(t) => assert_eq!(t.id.id, 20),
        _ => panic!("expected metairq to be chosen"),
    }

    // After the decision, metairq_preempted tracks `current`.
    let mirqp = cpu.metairq_preempted.expect("mirqp must be set");
    assert_eq!(mirqp.id.id, current.id.id);

    // Zephyr invariant: a thread MUST NOT live in both the run queue
    // and metairq_preempted simultaneously. Therefore the scheduler
    // must NOT request requeue of `current`. This assertion FAILS
    // against the current implementation — Step 4 captured
    // is_current_mirq_preempted=false before Step 5 set mirqp=current.
    assert!(
        !outcome.requeue_current,
        "SC12 violated: current is now metairq_preempted but scheduler \
         still requests requeue (run queue / metairq_preempted \
         disjointness broken — double-run hazard)"
    );
}
```

Expected: `cargo test -p gale --test sched_integration
next_up_smp_preserves_disjointness_of_runq_and_metairq_preempted`
fails with
`SC12 violated: current is now metairq_preempted but scheduler still
requests requeue`.

## IMPACT
**Severity: high on SMP, ASIL-D relevant.**

On Zephyr SMP with `CONFIG_NUM_METAIRQ_PRIORITIES > 0`, every time a
cooperative thread is preempted by a MetaIRQ on a CPU, the Rust
scheduler tells the caller to put the cooperative thread back in the
global run queue while simultaneously marking it as
`metairq_preempted`. Consequences:

1. **Double scheduling:** a second CPU can pick the thread from the
   run queue while the MetaIRQ handler is still running on the
   original CPU → same TCB dispatched on two CPUs → concurrent stack
   usage → memory corruption, lost state, possibly privilege
   confusion.
2. **MetaIRQ recovery duplicates the run:** when the MetaIRQ
   completes, `next_up_smp` Step 1 resurrects the same thread from
   `metairq_preempted`, re-entering it again even though it was
   already re-run via the queue path. Breaks cooperative
   non-preemption guarantee (violates SC6).
3. **Silent proof-code drift:** the surrounding Verus/Rocq/Lean
   apparatus declares SC12 "verified" (by lemma
   `lemma_operations_produce_valid_transitions` with `ensures true`),
   but the postcondition is never enforced — the Rust code diverges
   from the C decision order and no oracle catches it. This is the
   central answer to the prompt's question: the drift is **not** the
   self-IPI race of issue #17 (which lives in the C shim); it is an
   *independent* ordering bug in the verified Rust scheduler that the
   proofs do not cover.

Fix: swap the order in `next_up_smp` — compute `is_current_mirq_preempted`
*after* invoking `update_metairq_preempt`, i.e. move Step 4's
`metairq_preempted` read (and the `requeue_current` computation) to
sit after Step 5, and add the `ensures` clause shown in the oracle.

## CANDIDATE UCA
```
id: UCA-SCHED-SMP-METAIRQ-DOUBLE-REQUEUE
control_action: next_up_smp requests caller to re-queue `current` in the
                run queue
context: current is cooperative, active, not already queued, not idle;
         chosen candidate is a MetaIRQ thread; metairq_preempted was
         previously None, i.e. this decision establishes the preemption
hazard: SMP double-dispatch of the same TCB — one copy runs via
        run-queue pickup on another CPU, another via MetaIRQ recovery
        on the original CPU; concurrent stack use and loss of the
        cooperative-preemption invariant
asil: D
linked_safety_claim: SC6 (cooperative threads protected from non-MetaIRQ
                     preemption), SC12 (current re-queued only if active
                     + !queued + !idle + !metairq-preempted)
stpa_type: providing-incorrect (action provided when it should not be)
detection: differential unit test (see POC) + Verus ensures clause
           tying requeue_current to post-update metairq_preempted
mitigation: reorder Step 4 and Step 5 in next_up_smp so the requeue
            decision reads metairq_preempted after update_metairq_preempt,
            matching kernel/sched.c:253-268
```

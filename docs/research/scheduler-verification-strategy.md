# Scheduler Verification Strategy for Gale

Research into PulseEngine's spar/rules_lean ecosystem and Zephyr's
scheduler internals, informing a verified scheduler replacement via Gale.

## 1. What spar has for scheduling analysis

Spar (pulseengine/spar) is an AADL v2.2 toolchain in Rust. It has no
Lean proofs itself, but provides three scheduling-relevant analysis passes
that operate on AADL instance models:

### 1.1 Rate Monotonic Analysis (`scheduling.rs`)

- Groups AADL `thread` components by `Actual_Processor_Binding`.
- For each processor, computes utilization U = sum(Ci / Ti) where Ci is
  worst-case execution time and Ti is period.
- Three-tier result:
  - **Error**: U > 1.0 (overloaded, guaranteed missed deadlines).
  - **Warning**: RMA bound < U <= 1.0 (may miss deadlines under RM).
  - **Info**: U <= RMA bound n(2^(1/n) - 1) (schedulable under RM).
- Handles execution time ranges (uses worst case from `a ms .. b ms`).
- Warns on missing Period, Compute_Execution_Time, or binding properties.
- Modal awareness: notes when system operation modes exist but default
  values were used (STPA-REQ-017).

### 1.2 ARINC 653 Partition Analysis (`arinc653.rs`)

- Checks DO-297 constraints for time/space partitioning:
  - Processors should have virtual processor (partition) children.
  - Every process must be bound to a virtual processor partition.
  - Processes under different partitions should not share direct
    connections (inter-partition isolation).
  - Sum of partition window execution times must not exceed the
    processor's major frame period.

### 1.3 End-to-End Latency Analysis (`latency.rs`)

- Traces E2E flow segments through component instances.
- Best case: sum of execution times only.
- Worst case: execution times + sampling delays (periods at connection
  crossings).
- Compares against declared Latency property bounds.

### 1.4 Resource Budget Analysis (`resource_budget.rs`)

- Memory budget: sums Source_Code_Size + Data_Size + Stack_Size for
  software components bound to each memory component vs Memory_Size.
- Bandwidth budget: checks bus utilization vs capacity.

All analyses are trait-based (`Analysis` trait) and registered with
`AnalysisRunner::register_all()`. Safety requirements are traced via
STPA artifacts in `safety/stpa/requirements.yaml`.

## 2. What rules_lean provides

PulseEngine's `rules_lean` (pulseengine/rules_lean) provides Bazel rules
for building and verifying Lean 4 libraries and proofs, with optional
Mathlib support.

Current state:
- `lean_library` rule for building Lean 4 code.
- `lean_test` rule for checking proof obligations.
- `aeneas` integration for Rust-to-Lean extraction (via the Aeneas tool).
- Mathlib module extension for pulling in mathematical libraries.
- Example proofs are basic (nat_add_comm, nat_mul_comm) -- no scheduling
  proofs yet.

The Aeneas integration is the key piece: it enables extracting Rust code
into Lean 4 for theorem proving, which is exactly the pipeline needed
for proving properties about a Rust scheduler implementation.

## 3. What AADL models express about scheduling

AADL (AS5506) provides a rich vocabulary for architecture-level
scheduling specification. From the spar test data:

### Thread-level properties
- `Dispatch_Protocol`: Periodic, Sporadic, Aperiodic, Background
- `Period`: dispatch interval
- `Deadline`: absolute deadline relative to dispatch
- `Compute_Execution_Time`: range (BCET .. WCET)
- `Priority`: static priority assignment
- `Scheduling_Protocol`: RM, DM, EDF, POSIX highest-priority-first,
  Round Robin, etc.

### Processor-level properties
- `Scheduling_Protocol`: which algorithm the processor implements
- Binding (`Actual_Processor_Binding`): which threads run on which CPU

### Partition-level properties (ARINC 653)
- `Module_Major_Frame`: the hyperperiod for partition scheduling
- `Partition_Slots`: time window durations
- `Slots_Allocation`: which virtual processor gets each slot

### What AADL can express that is relevant to Zephyr
- Priority assignment correctness (does the architecture match the
  protocol?)
- Utilization bounds for schedulability
- End-to-end latency bounds across thread chains
- Resource budgets (memory, stack)
- Partition isolation properties

### What AADL cannot express (needs formal proofs)
- Implementation correctness of the scheduler itself
- Run queue data structure invariants
- Lock ordering and deadlock freedom
- Priority inversion bounds
- Interrupt latency guarantees
- Cache coherence under SMP context switch

## 4. Properties of sched.c that should be verified

Analysis of `/Volumes/Home/git/zephyr/zephyr/kernel/sched.c` (1605
lines) and `kernel/include/priority_q.h` (355 lines) reveals these key
verification targets:

### 4.1 Priority Queue Invariants

Zephyr supports three run queue implementations selectable at build time:

| Config | Data Structure | Complexity |
|--------|---------------|------------|
| `CONFIG_SCHED_SIMPLE` | Sorted doubly-linked list | O(n) insert, O(1) best |
| `CONFIG_SCHED_SCALABLE` | Red-black tree | O(log n) insert/remove/best |
| `CONFIG_SCHED_MULTIQ` | Bitmap + per-priority DLL array | O(1) insert/best |

**Properties to verify:**
- P1: `runq_best()` always returns the highest-priority ready thread
  (or NULL if empty).
- P2: `runq_add()` maintains sorted order.
- P3: `runq_remove()` maintains sorted order and does not lose threads.
- P4: `runq_yield()` moves current to end of its priority group (FIFO
  within priority level).
- P5: Multi-queue bitmap consistency: bit k is set iff queue[k] is
  non-empty.

### 4.2 Priority Comparison (`z_sched_prio_cmp`)

- Total ordering: for all threads t1, t2, exactly one of
  `prio_cmp(t1,t2) > 0`, `== 0`, `< 0` holds.
- `CONFIG_SCHED_DEADLINE` extension: when priorities are equal, earlier
  deadline wins. The 32-bit modular arithmetic must be shown correct
  within the documented "half-space" API rule.
- Transitivity: if t1 > t2 and t2 > t3 then t1 > t3.

### 4.3 Scheduling Decision (`next_up`)

The core scheduling function. Properties:
- N1: Returns the highest-priority runnable thread, or idle.
- N2: MetaIRQ preemption guarantee: a cooperative thread preempted by
  a MetaIRQ is returned to after the MetaIRQ completes (not whatever
  happens to be highest priority).
- N3: Under SMP, ties go to `_current` unless `swap_ok` is set (yield
  semantics).
- N4: Thread selected from the run queue is dequeued.
- N5: `_current` is re-queued if active, not already queued, and not
  idle.

### 4.4 Preemption Logic (`should_preempt`)

- SP1: A cooperative thread (priority < 0 in Zephyr, i.e., cooperative
  range) is never preempted except by MetaIRQ threads.
- SP2: `preempt_ok` (set by `k_yield()` or explicit reschedule)
  overrides cooperative protection.
- SP3: A pending/suspended/dead thread is always "preemptable" (trivially,
  since it isn't running).

### 4.5 Thread State Transitions

- TS1: Thread states form a well-defined FSM: Ready -> Running ->
  {Pending, Suspended, Dead, Sleeping}.
- TS2: `ready_thread()` only queues threads that are actually ready
  (not queued and not prevented from running).
- TS3: `halt_thread()` properly clears all waiters on join/halt queues.
- TS4: `pend_locked()` transitions thread to Pending and adds to wait
  queue.
- TS5: `z_sched_wake_thread_locked()` transitions from Pending to Ready
  only if thread is not being killed.

### 4.6 Scheduler Lock

- SL1: `sched_locked` is a counter: 0 = unlocked, negative = locked.
  (Zephyr uses `--` to lock and `++` to unlock, with 0U as the
  unlocked sentinel.)
- SL2: When locked, `update_cache()` does not preempt.
- SL3: Unlock + reschedule is atomic (no window where thread runs at
  wrong priority).

### 4.7 Priority Inheritance (interaction with mutex)

- PI1: When a thread's priority is changed while in the run queue,
  it is dequeued, updated, and re-queued atomically under the
  scheduler lock.
- PI2: When a thread's priority is changed while pending on a wait
  queue, it is removed and re-inserted at the correct position.

### 4.8 Timeslicing

- TL1: Timeslice expiry moves current to end of its priority group
  (same as yield).
- TL2: Only preemptible threads are subject to timeslicing.

### 4.9 SMP-Specific Properties

- SMP1: `_current` is never in the run queue until context switch path.
- SMP2: IPI is sent when a newly ready thread could run on another CPU.
- SMP3: Thread halting across CPUs: `thread_halt_spin()` does not
  deadlock (with caveats about 3+ thread cycles, as noted in the code).

## 5. How Lean proofs, Verus proofs, and AADL models connect

The three verification technologies address different abstraction levels
and provide complementary guarantees:

```
                    +-----------------------+
                    |  AADL (spar)          |  Architecture-level
                    |  - Thread model       |  scheduling properties
                    |  - RMA analysis       |  (are the timing params
                    |  - Latency bounds     |   schedulable?)
                    |  - Partition checks   |
                    +-----------+-----------+
                                |
                    Conformance |  "Does the implementation
                    checking    |   match the architecture?"
                                |
                    +-----------v-----------+
                    |  Lean 4 (rules_lean)  |  Mathematical proofs
                    |  - RMA bound theorem  |  about scheduling
                    |  - Priority ordering  |  algorithms
                    |  - Deadline analysis  |  (are the algorithms
                    |  - Starvation freedom |   correct in theory?)
                    +-----------+-----------+
                                |
                    Extraction  |  Aeneas: Rust -> Lean
                    bridge      |  (does the code match
                                |   the mathematical model?)
                    +-----------v-----------+
                    |  Verus (Gale)         |  Implementation proofs
                    |  - Run queue inv.     |  about Rust code
                    |  - State machine      |  (is the code correct
                    |  - Lock discipline    |   line by line?)
                    |  - Priority ordering  |
                    +-----------+-----------+
                                |
                    FFI shim    |  C shim layer
                                |  (tested on qemu_cortex_m3)
                    +-----------v-----------+
                    |  Zephyr kernel        |  Runtime execution
                    |  (qemu / renode)      |
                    +-----------------------+
```

### Connection points

1. **AADL -> Lean**: Spar's RMA analysis computes utilization bounds.
   Lean proofs can verify that the RMA bound formula n(2^(1/n) - 1) is
   correct and that the analysis is sound. This bridges "spar says it's
   schedulable" to "mathematically guaranteed schedulable."

2. **Lean -> Verus**: The mathematical model of priority ordering proved
   in Lean (total order, transitivity, antisymmetry) must correspond to
   the Verus spec in `priority.rs` and `wait_queue.rs`. The Aeneas
   extraction pipeline can generate Lean definitions from Gale's Rust
   code, enabling checking that the Verus-verified implementation matches
   the Lean mathematical model.

3. **Verus -> C shim -> Zephyr**: Gale's Verus-verified Rust code is
   linked into Zephyr via C FFI shims, replacing the C implementation.
   The shim is tested against Zephyr's existing test suites (all 6
   primitives pass).

4. **AADL -> Verus**: AADL models specify scheduling protocols and
   timing properties. Verus proofs verify that the implementation
   respects those protocols. For example, an AADL model declaring
   `Scheduling_Protocol => POSIX_1003_HIGHEST_PRIORITY_FIRST_PROTOCOL`
   requires that `next_up()` always returns the highest-priority ready
   thread -- exactly property N1 above.

## 6. Concrete verification targets

### Phase 1: Priority queue and ordering (Verus)

Already partially done in Gale:
- `priority.rs`: Bounded priority type with total order proof.
- `wait_queue.rs`: Sorted array-based queue with pend/unpend proofs.

Remaining work:
- [ ] Model the three run queue variants (simple/scalable/multiq) in
      Verus.
- [ ] Prove P1-P5 (best returns highest, insert preserves sort, etc.).
- [ ] Prove `z_sched_prio_cmp` is a total order with deadline extension.
- [ ] Prove bitmap consistency for the multiq variant.

### Phase 2: Scheduling decision (Verus + Lean)

- [ ] Model `next_up()` in Verus with `should_preempt()` logic.
- [ ] Prove N1 (highest priority selected) for uniprocessor case first.
- [ ] Prove SP1-SP3 (cooperative thread protection).
- [ ] Model MetaIRQ preemption (N2) and prove return-to-cooperative
      guarantee.
- [ ] In Lean: prove that priority-first scheduling with cooperative
      priority classes satisfies starvation freedom for finite
      cooperative sections.

### Phase 3: Thread state machine (Verus)

- [ ] Define thread state FSM in Verus (Ready, Running, Pending,
      Suspended, Dead, Sleeping).
- [ ] Prove TS1-TS5 (valid transitions, no impossible states).
- [ ] Prove that `ready_thread()` + `unready_thread()` are inverses
      with respect to run queue membership.
- [ ] Prove that `halt_thread()` transitions are irreversible (Dead)
      or reversible (Suspended -> Ready via resume).

### Phase 4: Lock discipline and atomicity (Verus)

- [ ] Model `_sched_spinlock` acquisition/release discipline.
- [ ] Prove SL1-SL3 (scheduler lock semantics).
- [ ] Prove PI1-PI2 (priority changes are atomic with respect to queue
      position).

### Phase 5: Mathematical scheduling theory (Lean + Mathlib)

- [ ] Prove RMA utilization bound theorem: n tasks with U <= n(2^(1/n)-1)
      are schedulable under RM.
- [ ] Prove EDF optimality: if any algorithm can schedule a task set,
      EDF can too.
- [ ] Prove priority ceiling protocol prevents priority inversion
      (connects to Gale's mutex implementation).
- [ ] Prove that Gale's priority ordering (as extracted via Aeneas)
      matches the mathematical definition.

### Phase 6: Architecture conformance (AADL + spar)

- [ ] Write an AADL model for Zephyr's scheduler architecture:
      processor with configurable scheduling protocol, thread
      components with priority/period/deadline, process bindings.
- [ ] Run spar's scheduling analysis on the model.
- [ ] Verify that spar's analysis results are consistent with the
      Lean-proved bounds.
- [ ] Add a Gale-specific AADL property set for Verus verification
      status traceability.

## 7. Recommended approach

### Lean for mathematical proofs

Use Lean 4 with Mathlib for:
- Scheduling theory theorems (RMA bound, EDF optimality, priority
  ceiling correctness).
- Abstract models of priority ordering and queue behavior.
- Extraction bridge via Aeneas to verify correspondence between
  Gale's Rust code and mathematical models.

Build integration: `rules_lean` already supports `lean_library` and
Mathlib in Bazel. Add a `proofs/scheduling/` directory with Lean files
alongside the existing `proofs/sem_proofs.v` (Rocq).

### Verus for implementation proofs

Use Verus (existing Gale infrastructure) for:
- Run queue data structure invariants.
- `next_up()` / `should_preempt()` decision logic.
- Thread state machine transitions.
- Lock discipline and atomicity.
- Priority change atomicity.

This is a natural extension of the existing pattern in Gale where each
kernel primitive has a Verus-verified model with pre/post conditions and
loop invariants.

### AADL for architecture specification

Use spar's AADL toolchain for:
- Declaring the intended scheduling architecture.
- Running RMA / latency / resource budget analysis.
- Generating conformance requirements that connect to Verus specs.
- Providing ASPICE-compatible architecture documentation.

### Integration order

1. Start with Verus proofs of priority queue variants (closest to
   existing Gale code patterns).
2. Add Lean proofs of RMA bound and priority ordering theory (uses
   rules_lean infrastructure).
3. Build the Aeneas extraction bridge from Gale's priority.rs to Lean
   (validates correspondence).
4. Write the AADL architecture model for the scheduler.
5. Extend Verus proofs to `next_up()` and thread state machine.
6. Add SMP-specific proofs last (highest complexity, can scope to
   uniprocessor first).

### Scope management

The uniprocessor scheduler (non-SMP, `CONFIG_SCHED_SIMPLE`) should be
the first verification target. It is:
- The simplest code path (~200 lines of core logic).
- The configuration used by qemu_cortex_m3 (existing test target).
- Sufficient for ASIL-D single-core automotive ECUs.

SMP verification is significantly harder due to:
- Per-CPU run queues and cache coherence.
- IPI signaling correctness.
- `thread_halt_spin()` liveness (acknowledged in code comments as
  potentially deadlocking for 3+ thread cycles).
- `z_requeue_current()` timing constraints.

SMP should be deferred to a later phase, potentially using a
combination of Lean (for the abstract concurrency model) and Verus
(for the implementation).

## 8. Summary

| Layer | Tool | What it proves | Status |
|-------|------|---------------|--------|
| Architecture | spar (AADL) | Schedulability, latency bounds, resource budgets | RMA + ARINC653 + latency implemented in spar |
| Theory | Lean 4 + Mathlib | RMA bound, EDF optimality, priority ceiling | rules_lean ready, proofs not yet written |
| Implementation | Verus | Run queue invariants, next_up correctness, state machine | priority.rs + wait_queue.rs exist, scheduler not yet started |
| Integration | Aeneas | Rust-to-Lean correspondence | rules_lean has Aeneas support, not yet used for Gale |
| Runtime | Zephyr test suites | End-to-end functional correctness | All 6 primitives pass on qemu_cortex_m3 |

The recommended first step is Verus proofs for the simple priority
queue (CONFIG_SCHED_SIMPLE), since it is structurally identical to the
existing wait_queue.rs proofs in Gale and requires no new tooling.

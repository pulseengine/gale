# Extreme / adversarial test cases — engine-control benchmark

Scoping catalog of adversarial scenarios the `benches/engine_control/`
framework should cover to be credible for ASIL-D. Today's sweep exercises
handoff timing under steady-state load; this document enumerates the
once-a-week conditions the bench does **not** exercise.

Companion to `docs/research/engine-bench-methodology-review.md` and
issue #25. P1 cases feed fixture development; each case links back to
gale source / UCAs where relevant.

Scope notes: cases already covered by Zephyr's ztest suites (basic
`k_sem_take/give` round-trip, `ring_buf_put/get` correctness, MPU fault
dispatch) are out of scope. Cases here are either (a) performance-under-
stress specific to the bench, (b) not reachable from ztest at all, or
(c) surface one of the 10 confirmed UCAs in
`artifacts/stpa_controllers_ucas.yaml` at runtime.

---

## 1. Rate boundaries

| Case | Setup | Oracle | Prio | Feasibility | New UCA? |
|---|---|---|---|---|---|
| **stall-mid-rotation** | RPM drops 4000→0 in one crank segment; timer next-shot already scheduled | No spurious ISR after stall; handoff-queue drains cleanly; no sem over-give | P1 | all-three | Possibly — `timer.rs` TM8 overflow is proved but cross-checked against only monotonic increment |
| **over-rev-12k** | Sweep extension: 10k → 12k → 14k RPM (166 667/rpm ≈ 12 µs period at 14k) | ISR latency stays bounded; no ring-buf wraparound loss; handoff-deadline miss rate logged, not hidden | P1 | QEMU + Renode | — |
| **launch-transient** | 500 → 10 000 RPM ramp in ≤10 revolutions (60 ms), no sweep-step settle | Per-sample timing distribution during transient is captured separately; no missed events in ring_buf | P2 | all-three | — |
| **jitter-injection** | Overlay ±5 % pseudo-random period jitter on top of nominal rate | Mann-Whitney U comparison vs baseline is not degraded by jitter; bench reports jitter budget | P2 | QEMU + Renode | — |
| **sensor-dropout-resume** | Kill timer for 50 ms then restart mid-sweep | Recovery latency < 2× nominal period; no sem-give on stale data | P3 | all-three | — |

## 2. Interrupt storms

| Case | Setup | Oracle | Prio | Feasibility | New UCA? |
|---|---|---|---|---|---|
| **multi-source-storm** | Crank ISR + synthesized ADC-completion ISR + synthesized CAN-RX ISR firing within same 20 µs window | Handoff-p99 stays within 3× single-source p99; no spinlock fairness starvation | P1 | Renode + real-hw | Likely — C-3 spinlock behaviour under ≥3 concurrent acquirers not yet measured |
| **isr-nesting-3-deep** | Lower-prio timer ISR preempted by mid-prio ADC preempted by high-prio fault | Ring-buf tail advance remains sequentially consistent; stack high-water ≤ threshold | P1 | Renode + real-hw | Possibly — `spinlock_validate.rs` `MAX_CPUS=4` / CPU_MASK=3 domain (U-9 surface) |
| **deferred-work-flood** | Fire 10× k_work items per crank event for 1 s | Work-queue depth stays bounded; no lost samples in ring_buf (RB1 drop-count in CSV) | P2 | all-three | — |
| **prio-inversion-setup** | Low-prio work holds mutex; mid-prio work busy-loops; high-prio ISR tries to take | Priority inheritance engages ≤ X µs; bench reports inversion duration | P2 | Renode + real-hw | — |

## 3. Fault injection

| Case | Setup | Oracle | Prio | Feasibility | New UCA? |
|---|---|---|---|---|---|
| **stuck-sensor** | `g_rpm` frozen at last good value but timer still firing | Control path emits degraded-mode marker; no sem-give beyond semaphore limit (guards U-1 at runtime) | P1 | all-three | Yes — **runtime witness for U-1** (sem-give overflow beyond limit) |
| **impossible-value** | Inject rpm = u32::MAX, rpm = 0 cycling rapidly | `rpm_to_period_us` returns sentinel (not panic); `rpm_bin` clamp holds | P1 | all-three | Possibly |
| **corrupt-ring-indices** | Flip head/tail via DebugMon write between put and get | RB invariant-check fires; no out-of-bounds read; sem count not decremented | P2 | real-hw (DebugMon) | — |
| **checksum-mismatch** | Corrupt one crank-count byte in the sample payload | Bench CSV logs drop; analyzer rejects run with clear error | P2 | all-three | — |
| **power-glitch** | Brown-out VBOR trip during ISR handoff, warm-restart | State invariants hold after reboot; no partial sem state | P3 | real-hw only | — |

## 4. Timing boundary

| Case | Setup | Oracle | Prio | Feasibility | New UCA? |
|---|---|---|---|---|---|
| **cycle-wrap-168MHz** | Force DWT CYCCNT near `u32::MAX`, sample across wrap (25.5 s @ 168 MHz) | `algo_cycles`/`handoff_cycles` remain positive; analyzer handles wrap | P1 | all-three | — |
| **timer-status-overflow** | Call `k_timer_status_sync` ≥ u32::MAX/10 times in one run | `timer.rs:151` TM8 saturation path fires; no wrap to 0 | P1 | QEMU (fast-forward time) | — |
| **uptime-32-wrap** | Long-run bench > 49.7 days of simulated uptime (QEMU virtual time) | `k_uptime_get_32()` wrap does not corrupt event `seq` ordering | P2 | QEMU only |  — |
| **schedule-during-wrap** | Arm k_timeout to fire *at* the wrap instant | Timeout fires exactly once (not 0 or 2 times) | P2 | QEMU + Renode | — |

## 5. Concurrency / SMP

| Case | Setup | Oracle | Prio | Feasibility | New UCA? |
|---|---|---|---|---|---|
| **cpu-offline-mid-loop** | On 4-CPU build, call `stop_cpu_decide` for CPU 0 while crank ISR pinned to CPU 1 active | Must reject (SM3) — bench captures whether it does | P1 | QEMU-SMP + Renode | Yes — **runtime witness for U-7** (cpu_id omitted from stop_cpu_decide) |
| **cpu_id-gt-4-reacquire** | Force cpu_id=5 path in `spinlock_validate.rs` (requires `CONFIG_MP_MAX_NUM_CPUS > 4` fixture) | Validator must reject; today (per U-9) it accepts via `5 & 3 == 1` | P1 | QEMU-SMP | Yes — **runtime witness for U-9** |
| **spinlock-4cpu-contention** | All 4 CPUs contend on same spinlock 10 000×/s | Fairness metric; no livelock; cycles-held-p99 bounded | P2 | QEMU-SMP + Renode | — |
| **wq-starvation** | System work-queue + 3 app work-queues all busy; low-prio item must run | Starvation timeout fires; max-wait logged | P2 | all-three | — |
| **tid-zero-release** | Thread with tid=0 (sentinel) releases spinlock | Validator rejects or the collapse is observed | P1 | QEMU + Renode | Yes — **runtime witness for U-8** (owner tid 0 collapse) |

## 6. Memory pressure

| Case | Setup | Oracle | Prio | Feasibility | New UCA? |
|---|---|---|---|---|---|
| **isr-alloc-fragmented-heap** | Pre-fragment heap to 80 %; alloc from ISR during crank event | Either deterministic refusal or bounded-time success; no unbounded scan | P1 | all-three | — |
| **stack-near-overflow** | Reduce ISR stack to measured high-water + 64 bytes; run full sweep | No overflow; MPU guard fires if breached | P1 | Renode + real-hw | — |
| **pool-exhaustion-sem** | Exhaust k_sem pool mid-sweep; subsequent alloc returns error | Error is surfaced, not silently ignored; bench logs refusal | P2 | all-three | — |
| **mmu-align-overflow** | Drive `region_align_decide` with size ≥ u32::MAX − page_size | Must return error, not saturate (U-5) | P1 | QEMU (Cortex-A) | Yes — **runtime witness for U-5** (MMU saturate-on-overflow) |
| **mpu-overlap-pair** | Configure base=0x8000_0000, size=0x8000_0000 via userspace syscall | Must reject (U-6); today silently admitted | P1 | QEMU + Renode | Yes — **runtime witness for U-6** (MPU arith overlap) |

## 7. State-machine adversarial

| Case | Setup | Oracle | Prio | Feasibility | New UCA? |
|---|---|---|---|---|---|
| **condvar-64-waiters-cliff** | Enqueue exactly MAX_WAITERS (64) waiters on one condvar then broadcast; then 65th wait attempt | `wait_queue.rs:285` refusal fires at 65; broadcast wakes exactly 64 | P1 | QEMU + Renode | — (boundary not in UCA yaml but trivially derivable from `wait_queue.rs:30`) |
| **abort-vs-join-race** | Thread A calls k_thread_abort(B) while C calls k_thread_join(B) | No use-after-free; both callers return defined result; Verus-declared-unreachable states rejected by shim | P1 | QEMU + Renode | Possibly — C-2 lifecycle manager |
| **verus-unreachable-calls** | From C, call `k_condvar_signal` with handle in un-init'd state | FFI shim rejects (not UB); today U-1-style precondition erasure may let it through | P1 | all-three | Yes — exercises the "precondition erased at FFI" pattern that produced U-1/U-6/U-9 |
| **priority-coop-negative** | C caller passes priority = -1 (valid Zephyr coop) to `priority_set_decide` | Must accept negative; today rejected (U-10) | P1 | all-three | Yes — **runtime witness for U-10** |
| **condvar-broadcast-0-waiters** | Broadcast on empty wait queue in tight loop | `broadcast_decide(0) == 0`, no sem side-effect | P3 | all-three | — |

## 8. Environmental

| Case | Setup | Oracle | Prio | Feasibility | New UCA? |
|---|---|---|---|---|---|
| **cold-boot-first-sample** | Measure first-100-samples handoff separately | First-sample timing reported distinct from steady-state median | P2 | all-three | — |
| **warm-reboot-retained** | Soft-reset mid-run with retained-RAM state preserved | Sample `seq` continuity or clean restart marker; no partial ring-buf | P2 | Renode + real-hw | — |
| **flash-wait-state-worst** | Force prefetch-off, worst wait-state config (CPU side), rerun sweep | Bench captures wait-state-sensitivity number (should be explicit, not noise) | P3 | real-hw only | — |
| **pipeline-flush-after-dmb** | Insert DMB/DSB in hot handoff and measure cost explicitly | Reported as known overhead, not swept under rug | P3 | all-three | — |
| **soft-reset-mid-handoff** | AIRCR SYSRESETREQ between ring_buf_put and k_sem_give | Post-reset invariants hold; no half-consumed sample | P2 | real-hw only | — |

---

## Prioritized summary (P1 first, grouped by category)

**P1 (must-have for cert) — 17 cases**
- Rate: stall-mid-rotation, over-rev-12k
- Storms: multi-source-storm, isr-nesting-3-deep
- Faults: stuck-sensor, impossible-value
- Timing: cycle-wrap-168MHz, timer-status-overflow
- SMP: cpu-offline-mid-loop, cpu_id-gt-4-reacquire, tid-zero-release
- Memory: isr-alloc-fragmented-heap, stack-near-overflow, mmu-align-overflow, mpu-overlap-pair
- State-machine: condvar-64-waiters-cliff, abort-vs-join-race, verus-unreachable-calls, priority-coop-negative

**P2 (customer eval) — 12 cases**
launch-transient, jitter-injection, deferred-work-flood, prio-inversion-setup,
corrupt-ring-indices, checksum-mismatch, uptime-32-wrap, schedule-during-wrap,
spinlock-4cpu-contention, wq-starvation, pool-exhaustion-sem,
cold-boot-first-sample, warm-reboot-retained, soft-reset-mid-handoff

**P3 (confidence) — 5 cases**
sensor-dropout-resume, power-glitch, condvar-broadcast-0-waiters,
flash-wait-state-worst, pipeline-flush-after-dmb

---

## Meta-findings

- **The P1 set doubles as UCA runtime-witness coverage.** Eight of the 10
  confirmed UCAs in `stpa_controllers_ucas.yaml` (U-1, U-5, U-6, U-7, U-8,
  U-9, U-10, plus the U-2/U-3/U-4-adjacent "precondition erased at FFI"
  pattern) get a concrete runtime trigger in the catalog above. This is
  the strongest argument for prioritising the bench: today UCAs are
  confirmed only by Mythos static review, not by any executing test.

- **Condvar's upper-boundary path has zero runtime coverage.** `src/
  condvar.rs:281` and `src/wait_queue.rs:31` encode MAX_WAITERS = **64**
  (not 256 — correct before citing). Nothing in ztest or the bench
  exercises the 64→65 refusal cliff. Same for `spinlock_validate.rs`
  MAX_CPUS boundary.

- **Power-glitch / brown-out recovery cannot be tested in QEMU at all.**
  Renode can stub VBOR events; full fidelity requires real hardware with a
  programmable supply. This is the single category with the largest
  credibility gap on emulator-only runs.

- **Verus lemmas give us "free" oracles.** Every `ensures` clause is a
  runtime invariant we can assert. Examples: `timer.rs:155` (status ≤
  u32::MAX saturation) → one positive + one negative test; `condvar.rs:
  283` (`broadcast_decide(n) == n`) → property test with n ∈ {0, 1, 63, 64,
  MAX_WAITERS-1}; `ring_buf.rs:188` (no-overflow in index arithmetic) →
  wrap-boundary test. ~40 lemmas across the primitive surface; each worth
  one short bench assertion.

- **Obvious cases absent from `stpa_controllers_ucas.yaml`.** Two gaps:
  (1) condvar MAX_WAITERS cliff has no UCA (neither does ring_buf's
  capacity cliff — see `ring_buf.rs:294` u64 widening); (2) timer
  status-wrap at u32::MAX (`timer.rs:266` proves the error path exists,
  but no UCA captures "what if the caller ignores the error"). Both are
  candidates to add as U-11 / U-12 in the next STPA pass — they follow
  the same "narrow domain vs wide caller" pattern as U-1/U-6/U-9.

- **The "coarse-FFI precondition erasure" pattern repeats across six of
  ten UCAs.** U-1 (sem), U-5 (mmu), U-6 (mpu), U-8 (spinlock tid=0), U-9
  (spinlock cpu_id≥4), U-10 (priority signedness) all share the same
  root cause: Verus `requires` has no realization at the C boundary.
  A *single* bench fixture that systematically drives each FFI entry
  with out-of-domain inputs would catch this whole class — arguably the
  highest-leverage investment after the P1 list.

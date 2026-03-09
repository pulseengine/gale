# Verified Zephyr Semaphore — Design

## Context

Port Zephyr `kernel/sem.c` (237 lines) to Rust with full formal verification targeting ASIL-D.
Uses PulseEngine's `rules_verus` (SMT/Z3) and `rules_rocq_rust` (Rocq theorem proving) Bazel toolchain.

## Architecture

- Separate Zephyr instances for ASIL-D and ASIL-B, hardware-isolated (MPU/PMP)
- IPC between instances (separate module, not in scope here)
- Verified kernel primitives operate within a single instance
- Scheduler model deferred — semaphore invariants are scheduler-independent

## Scope — Full Model (Option 3)

Verified components:
- `Priority` — bounded priority type, 0..MAX_PRIO (0 = highest)
- `Thread` — state machine (Ready, Running, Blocked, Suspended), owns priority
- `WaitQueue` — priority-ordered queue of blocked threads
- `Semaphore` — counting semaphore with init/give/take/reset/count_get

## Verification Tracks

**Track 1 — Verus (inline SMT specs):**
- Count invariants: 0 <= count <= limit, limit > 0
- No arithmetic overflow
- Wait queue always sorted by priority
- give/take state machine correctness
- No lost wakeups

**Track 2 — Rocq-of-Rust (theorem proving):**
- Translate plain Rust to Rocq via coq_of_rust
- Hand-written proofs for deeper properties
- Refinement between spec and impl

## Source Mapping

| Zephyr C | Verified Rust | Lines |
|----------|---------------|-------|
| kernel/sem.c:45-73 | sem.rs::init | ~20 |
| kernel/sem.c:95-121 | sem.rs::give | ~30 |
| kernel/sem.c:132-164 | sem.rs::take | ~35 |
| kernel/sem.c:166-192 | sem.rs::reset | ~25 |
| kernel.h:k_sem_count_get | sem.rs::count_get | ~5 |
| (scheduler internals) | wait_queue.rs | ~150 |
| (scheduler internals) | thread.rs | ~100 |

## Omitted (not safety-relevant)

- CONFIG_POLL (poll_events) — application convenience
- CONFIG_OBJ_CORE_SEM — debug/tracing infrastructure
- CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
- SYS_PORT_TRACING_* — instrumentation hooks

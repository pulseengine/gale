# Full Zephyr Kernel Replacement — Design Spec

**Date:** 2026-03-19
**Goal:** Promote all kernel API modules from "verified arithmetic helpers" to "full kernel logic owners" where Rust owns everything inside the spinlock and the C shim becomes a minimal ABI adapter.

## Module Classification

**25 kernel API modules** (have CONFIG_GALE_KERNEL_* entries):
sem, mutex, condvar, msgq, stack, pipe, event, timer, mem_slab, fifo, lifo, queue, mbox, timeout, poll, futex, timeslice, kheap, thread_lifecycle, work, fatal, mempool, dynamic, smp_state, sched

**14 internal support modules** (no Zephyr C API, used by kernel API modules):
lib, error, priority, thread, wait_queue, spinlock, atomic, ring_buf, fault_decode, stack_config, userspace, mem_domain, heap, device_init

Internal modules do NOT get C shims. They are consumed by the kernel API modules' Rust implementations and verified by Verus as part of the dependency chain.

## Status Quo

Today Gale follows the "verified arithmetic oracle" pattern:

```
Zephyr application
  → k_sem_give()          (public API, defined in kernel.h)
  → z_impl_k_sem_give()   (C shim: gale_sem.c, ~230 lines of real C logic)
    → gale_sem_count_give() (Rust FFI: verified count arithmetic only)
    → z_unpend_first_thread(), z_reschedule() (C scheduler internals)
```

Rust verifies ~5% of the logic (count arithmetic). The C shim contains the other 95%: wait queue management, thread waking, priority handling, tracing, poll events, userspace marshaling.

**13 modules** are fully wired with real C shims + CI tested. **12** have stub C shims (header-only placeholders, ~17 lines). Condvar is verified by composition (no C shim by design).

## Target Architecture

```
Zephyr application
  → k_sem_give()           (public API, unchanged)
  → z_impl_k_sem_give()    (C shim: ~20 lines, minimal adapter)
    → k_spin_lock()         (C: arch-specific inline asm)
    → gale_k_sem_give()     (Rust FFI: ALL kernel logic)
      → wait queue decision (Rust)
      → thread wake decision (Rust)
      → priority comparison (Rust)
      → count arithmetic (Rust, already verified)
      → returns: module-specific decision struct
    → z_ready_thread() / z_reschedule() (C: scheduler apply)
    → z_handle_obj_poll_events() (C: poll integration)
    → k_spin_unlock()       (C: arch-specific inline asm)
```

Rust owns **everything inside the spinlock** except the items listed in "What Stays in C."

## What Stays in C (Non-Negotiable)

| Component | Why |
|-----------|-----|
| `k_spin_lock` / `k_spin_unlock` | Architecture-specific inline asm (ARM `cpsid`/`cpsie`, RISC-V CSR) |
| `z_ready_thread` / `z_reschedule` | Scheduler side-effect application (manipulates run queue, triggers context switch) |
| `z_unpend_first_thread` | Returns opaque `k_thread*` from wait queue (C linked-list/rbtree operation) |
| `z_pend_curr` | Atomically blocks current thread (scheduler lock + context switch) |
| `arch_thread_return_value_set` | Architecture-specific inline function |
| `z_vrfy_*` + `_mrsh.c` | Zephyr syscall codegen requires C source files |
| `SYS_INIT` registration | Linker section macros only work from C |
| `SYS_PORT_TRACING_*` | C preprocessor macros, no-ops when tracing disabled |
| `z_handle_obj_poll_events` | Poll subsystem integration (C internal) |
| `_current` thread access | Architecture-specific (TLS or `_kernel.cpus[0].current`) |

### `_current` Access Pattern

The canonical way to pass `_current`-dependent information across the FFI boundary is **boolean projection at the C side**:

```c
// C shim extracts booleans BEFORE calling Rust
uint32_t owner_is_current = (mutex->owner == _current) ? 1U : 0U;
uint32_t owner_is_null = (mutex->owner == NULL) ? 1U : 0U;
int32_t ret = gale_mutex_lock_validate(lock_count, owner_is_null, owner_is_current, &new_count);
```

Rust never receives thread pointers — only boolean/integer projections. This keeps the FFI boundary clean and avoids Rust needing to understand Zephyr's thread struct layout.

## What Moves to Rust

For each module, the **entire state machine and decision logic** moves to Rust:

| Responsibility | Before (C) | After (Rust) |
|---------------|-----------|-------------|
| Count arithmetic | Rust (done) | Rust (done) |
| Wait queue: should we wake a thread? | C | **Rust** |
| Wait queue: which thread (priority)? | C | **Rust** (via boolean projection of queue state) |
| Timeout: should we block with timeout? | C | **Rust** |
| State machine transitions | Partial (some Rust) | **Rust** |
| Data buffer index management (msgq, pipe) | C | **Rust** |
| Error code selection | Rust (done) | Rust (done) |

## Decision Struct Pattern

Each module returns a **module-specific decision struct** (following the existing `coarse.rs` pattern with `GaleSemState`, `GaleStackState`, `GalePipeState`):

```rust
// Per-module structs — NOT a single universal struct
#[repr(C)]
pub struct GaleSemDecision {
    pub ret: i32,
    pub action: u8,       // NONE=0, WAKE_FIRST=1, PEND_CURRENT=2
    pub new_count: u32,
}

#[repr(C)]
pub struct GaleMutexDecision {
    pub ret: i32,
    pub action: u8,       // NONE=0, WAKE_FIRST=1, PEND_CURRENT=2, ADJUST_PRIO=3
    pub new_lock_count: u32,
    pub priority_to_restore: u32,
}

#[repr(C)]
pub struct GalePipeDecision {
    pub ret: i32,
    pub action: u8,
    pub actual_bytes: u32,
    pub new_used: u32,
}
```

The C shim mechanically applies the returned action.

## Shim Complexity Tiers

Not all C shims will be equally thin. Some modules have C-side complexity that cannot be fully eliminated:

| Tier | Pattern | Modules |
|------|---------|---------|
| **Thin** (~15-25 lines) | Lock → call Rust → apply action → unlock | sem, condvar, futex, timeslice, smp_state, fatal |
| **Medium** (~30-50 lines) | Lock → call Rust → apply action + poll events → unlock | mutex, event, timer, mem_slab, kheap, mempool, dynamic, work |
| **Thick** (~50-80 lines) | Lock → call Rust → apply action + data copy loops + retry → unlock | msgq, pipe, stack, queue, mbox, fifo, lifo |
| **Complex** (~100+ lines) | Multiple Rust calls + priority inheritance + timeout recovery | thread_lifecycle, sched, poll |

**Thick/Complex shims** still contain C logic for raw pointer data movement (memcpy of user buffers, direct-copy to pending reader swap_data) and retry loops. This is "scheduler apply" level code — it applies Rust's decisions but handles the actual memory operations that require raw C pointer access. The verification boundary is: **Rust decides what to do, C does the pointer arithmetic to make it happen**.

## Parallel Track Plan

### Track 1 — Foundation (Critical Path, Sequential)

These have hard dependencies on each other:

```
spinlock model → thread model → wait_queue full impl → scheduler decisions → timeout logic
```

| Module | Scope | Depends On | Type |
|--------|-------|------------|------|
| spinlock | Lock state tracking, validation (arch asm stays in C) | None | Internal |
| thread | Full thread state FSM, `is_metairq`, priority, all transitions | spinlock | Internal |
| wait_queue | Full priority-sorted insert/remove/wake-all with thread ownership | thread | Internal |
| sched | Full `next_up`, `should_preempt`, run queue decisions, MetaIRQ | wait_queue | Kernel API |
| timeout | Deadline tracking, expiry decisions | sched | Kernel API |

**Deliverable:** The decision struct pattern — Rust makes all decisions, C applies them.

### Track 2 — Synchronization (5 modules, all parallel)

Each is independent once wait_queue is available. Can start immediately on the decision struct pattern using the existing wait_queue model.

| Module | Current State | Work Needed | Difficulty |
|--------|--------------|-------------|------------|
| sem | Wired, tested (count only) | Expand FFI to own wait queue decisions | Low |
| mutex | Wired, tested (state only) | Expand to own priority inheritance decisions | **High** (priority ceiling, `z_is_prio_higher`, timeout recovery) |
| condvar | Verified by composition, no C shim | No C shim needed — safety proven by composition through wait_queue | Low (keep as-is) |
| futex | Stub shim | Write full C shim + FFI for futex wait/wake | Medium |
| poll | Stub shim | Write full C shim + FFI for poll event scanning | **High** (multi-object, callback-driven) |

### Track 3 — IPC (7 modules, all parallel)

| Module | Current State | Work Needed | Difficulty |
|--------|--------------|-------------|------------|
| msgq | Wired, tested (indices only) | Expand to own ring buffer decisions + wait queue | Medium |
| pipe | Wired, tested (state only) | Expand to own byte count decisions + wait queue | **High** (direct-copy loops, retry) |
| stack | Wired, tested (count only) | Expand to own data array decisions + wait queue | Low |
| queue | Wired (count only) | Expand to own linked list decisions + wait queue | Medium |
| mbox | Wired (match only) | Expand to own sender/receiver matching + wait queue | Medium |
| fifo | Wired (count only) | Expand via queue (fifo is a queue wrapper) | Low |
| lifo | Wired (count only) | Expand via queue (lifo is a queue wrapper) | Low |

### Track 4 — Timing & Work (4 modules, all parallel)

| Module | Current State | Work Needed | Difficulty |
|--------|--------------|-------------|------------|
| timer | Wired (status counter) | Expand to own expiry/period/callback decisions | Medium |
| event | Wired (bitmask only) | Expand to own wait-for-event logic | Medium |
| timeslice | Stub shim | Write full C shim + FFI for timeslice accounting | Low |
| work | Stub shim | Write full C shim + FFI for work queue scheduling | **High** (deferred execution, thread pool) |

### Track 5 — Memory & Safety (6 modules, mostly parallel)

Can start immediately for standalone modules. `dynamic` has soft dependency on `thread_lifecycle` (Track 6). `smp_state` has soft dependency on `sched` (Track 1).

| Module | Current State | Work Needed | Difficulty | Soft Dependency |
|--------|--------------|-------------|------------|-----------------|
| mem_slab | Wired (block count) | Expand to own free-list decisions | Low | None |
| kheap | Stub shim | Write full C shim + FFI for heap alloc/free | Medium | None |
| mempool | Stub shim | Write full C shim + FFI for pool management | Medium | None |
| fatal | Stub shim | Write full C shim + FFI for fatal error classification | Low | None |
| dynamic | Stub shim | Write full C shim + FFI for thread pool decisions | Medium | thread_lifecycle (Track 6) |
| smp_state | Stub shim | Write full C shim + FFI for CPU lifecycle | Medium | sched (Track 1) |

### Track 6 — Infrastructure (1 module)

| Module | Current State | Work Needed | Difficulty |
|--------|--------------|-------------|------------|
| thread_lifecycle | Stub shim | Write full C shim + FFI for create/abort/join | **High** (touches thread.c, arch-specific) |

Note: `device_init` exists as `src/device_init.rs` (internal support module) but has no CONFIG_GALE_KERNEL_DEVICE_INIT entry. It stays as an internal module unless a Kconfig option is added to Zephyr.

## Dependency Graph

```
                    Track 1 (SEQUENTIAL - Critical Path)
    spinlock → thread → wait_queue → scheduler → timeout
                            │              │
          ┌─────────────────┤              │
          ↓                 ↓              ↓
    Track 2 (SYNC)    Track 3 (IPC)   Track 4 (TIMING)
    sem               msgq            timer
    mutex*            pipe*           event
    [condvar=noop]    stack           timeslice
    futex             queue           work*
    poll*             mbox
                      fifo, lifo
                                      * = high difficulty

    Track 5 (MEMORY) ← starts immediately (mostly)
    mem_slab | kheap | mempool | fatal
    dynamic (soft gate: Track 6) | smp_state (soft gate: Track 1)

    Track 6 (INFRA) ← gates on Track 1
    thread_lifecycle*
```

## Per-Module Work Pattern

For each of the 25 kernel API modules, "first class citizen" means:

### 1. Expand Rust Model
The verified Rust model (`src/<module>.rs`) must own the full decision logic, not just arithmetic. The `ensures` clauses cover wait queue decisions, timeout handling, and state transitions.

### 2. Expand FFI
The FFI layer (`ffi/src/lib.rs`) gets new `gale_k_<module>_<op>()` functions that take full kernel object state (via module-specific `#[repr(C)]` structs following the `coarse.rs` pattern) and return module-specific decision structs.

### 3. Write/Expand C Shim
The C shim (`zephyr/gale_<module>.c`) becomes a minimal adapter (tier-dependent):
- Acquire spinlock
- Extract state (boolean projections for `_current`, queue-empty checks)
- Call Rust FFI
- Apply the returned action (wake thread, pend current, reschedule, data copy)
- Handle tracing macros, poll events, and userspace marshaling
- Release spinlock

### 4. Wire CMakeLists
Ensure `CONFIG_GALE_KERNEL_<MODULE>` guard exists in both `gale_overlay.conf` and `zephyr/kernel/CMakeLists.txt`, conditionally excluding the upstream C file.

### 5. Add CI Test Suite
Add the corresponding Zephyr test suite to `.github/workflows/zephyr-tests.yml` matrix. Must pass on qemu_cortex_m3 at minimum; ideally also on Renode M4F/M33/R5.

### 6. Verus Verify
All new Rust code gets `requires`/`ensures` contracts. The expanded model must verify with Verus (39/39 maintained).

### 7. Update Rivet Artifacts
Add/update verification artifacts in `artifacts/verification.yaml` for the expanded scope.

## Testing Strategy

| Level | Tool | What |
|-------|------|------|
| Unit | `cargo test` | Rust model correctness (existing + expanded) |
| Property | `proptest` | Random operation sequences on expanded models |
| BMC | Kani | Bounded model checking of FFI functions |
| Integration | Zephyr + QEMU M3 | Full kernel test suites (20+ suites) |
| Hardware | Renode M4F/M33/R5 | Multi-arch validation (3 boards) |
| Gate | verus-strip | plain/src/ sync check |

## Parallelization Summary

| Track | Modules | Can Start | Gate |
|-------|---------|-----------|------|
| 1 Foundation | 5 (3 internal + 2 API) | Immediately | None (critical path) |
| 2 Sync | 5 (incl. condvar=noop) | Immediately (decision struct) | Track 1 for full wait queue |
| 3 IPC | 7 | Immediately (decision struct) | Track 1 for full wait queue |
| 4 Timing | 4 | Immediately (timer/event standalone) | Track 1 for timeout |
| 5 Memory | 6 | Immediately (mostly) | Soft: dynamic→T6, smp→T1 |
| 6 Infra | 1 | After Track 1 | Track 1 |

**Maximum parallelism: All 6 tracks can start simultaneously.** Tracks 2-4 start with the decision struct pattern using existing models, then integrate with Track 1's full wait queue when ready.

## C Shim Auto-Generation (Future)

The thin/medium C shim tiers are highly repetitive:
1. Spinlock acquire
2. Tracing enter
3. Extract state → call Rust → apply action
4. Tracing exit
5. Reschedule + spinlock release

A code generator could produce these from the FFI header declarations + a small per-module config (struct field names, tracing object type). This eliminates hand-written C for ~15 of the 25 modules.

## Success Criteria

- All 25 kernel API modules have real (non-stub) C shims (condvar excepted — composition-only by design)
- All C shims follow the minimal adapter pattern (tier-appropriate)
- All modules pass their corresponding Zephyr test suites in CI
- All Rust models pass Verus verification (39/39 maintained)
- Kernel logic lives in Rust (C retains only: arch asm, scheduler apply, data copy, tracing, syscall marshaling, poll integration)
- Rivet `validate` passes with updated verification artifacts

## What This Does NOT Cover

- **Replacing Zephyr's arch-specific context switching** — this is assembly, not kernel logic
- **Replacing Zephyr's init.c boot sequence** — infrastructure, not safety-critical primitives
- **MMU/MPU management** — architecture-specific, handled by `mem_domain` model but not replaced
- **Device driver framework** — out of scope for kernel primitives
- **Networking, Bluetooth, filesystem** — Zephyr subsystems, not kernel

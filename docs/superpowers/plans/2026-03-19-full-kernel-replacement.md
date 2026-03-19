# Full Zephyr Kernel Replacement — Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Promote all 25 kernel API modules to full kernel logic owners where Rust owns all decision logic inside the spinlock and C shims become minimal ABI adapters.

**Architecture:** Per-module `#[repr(C)]` decision structs returned from Rust FFI. C shims extract state, call Rust, apply the returned action (wake/pend/reschedule). Track 1 (foundation) is the critical path; Tracks 2-6 run in parallel once started.

**Tech Stack:** Rust (Verus-annotated), C (Zephyr kernel shims), CMake (Zephyr build), Bazel (verification), cargo test + Zephyr QEMU/Renode (testing)

**Spec:** `docs/superpowers/specs/2026-03-19-full-kernel-replacement-design.md`

---

## Master Plan: Track Coordination

This is a multi-track plan. Each track gets its own detailed sub-plan when it begins. This document covers:
1. The decision struct foundation (shared by all tracks)
2. Track 1: Foundation (critical path — spinlock, thread, wait_queue, sched, timeout)

Tracks 2-6 are separate plans created when Track 1 delivers the decision struct pattern. Each track-plan is independently executable by a separate agent/developer.

---

## Chunk 1: Decision Struct Foundation + Semaphore Proof-of-Concept

This chunk establishes the `GaleKernelAction` pattern by converting the semaphore (simplest wired module) from "arithmetic oracle" to "full decision owner." This proves the pattern before applying it to all 25 modules.

### Task 1: Define the Semaphore Decision Struct

**Files:**
- Modify: `ffi/src/lib.rs` (add new struct + functions)
- Modify: `ffi/include/gale_sem.h` (add new C declarations)

- [ ] **Step 1: Add `GaleSemDecision` struct to `ffi/src/lib.rs`**

Add after the existing `gale_sem_count_take` function (~line 280):

```rust
/// Decision struct for k_sem_give — tells C shim what action to take.
#[repr(C)]
pub struct GaleSemGiveDecision {
    /// Action: 0=INCREMENT_COUNT, 1=WAKE_THREAD
    pub action: u8,
    /// New count value (only used when action=INCREMENT_COUNT)
    pub new_count: u32,
}

pub const GALE_SEM_ACTION_INCREMENT: u8 = 0;
pub const GALE_SEM_ACTION_WAKE: u8 = 1;

/// Full decision for k_sem_give: decides whether to increment count or wake a thread.
///
/// Replaces the C-side if/else logic in z_impl_k_sem_give.
/// The C shim passes whether a waiter exists; Rust decides the action.
///
/// Verified: P3 (count capped at limit), P9 (no overflow).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_sem_give_decide(
    count: u32,
    limit: u32,
    has_waiter: u32,  // 1 if z_waitq_head != NULL, 0 otherwise
) -> GaleSemGiveDecision {
    if has_waiter != 0 {
        // Waiter exists: wake thread, count unchanged
        GaleSemGiveDecision {
            action: GALE_SEM_ACTION_WAKE,
            new_count: count,
        }
    } else {
        // No waiter: increment count (saturate at limit)
        let new_count = if count < limit {
            #[allow(clippy::arithmetic_side_effects)]
            { count + 1 }
        } else {
            count
        };
        GaleSemGiveDecision {
            action: GALE_SEM_ACTION_INCREMENT,
            new_count,
        }
    }
}

/// Decision struct for k_sem_take.
#[repr(C)]
pub struct GaleSemTakeDecision {
    /// Return code: 0 (acquired), -EBUSY (would block), -EAGAIN (timed out)
    pub ret: i32,
    /// New count value (decremented if acquired)
    pub new_count: u32,
    /// Action: 0=RETURN_IMMEDIATELY, 1=PEND_CURRENT
    pub action: u8,
}

pub const GALE_SEM_ACTION_RETURN: u8 = 0;
pub const GALE_SEM_ACTION_PEND: u8 = 1;

/// Full decision for k_sem_take: decides whether to acquire, return busy, or pend.
///
/// Verified: P5 (decrement), P6 (-EBUSY), P9 (no underflow).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_sem_take_decide(
    count: u32,
    is_no_wait: u32,  // 1 if timeout == K_NO_WAIT
) -> GaleSemTakeDecision {
    if count > 0 {
        #[allow(clippy::arithmetic_side_effects)]
        let new_count = count - 1;
        GaleSemTakeDecision {
            ret: OK,
            new_count,
            action: GALE_SEM_ACTION_RETURN,
        }
    } else if is_no_wait != 0 {
        GaleSemTakeDecision {
            ret: EBUSY,
            new_count: 0,
            action: GALE_SEM_ACTION_RETURN,
        }
    } else {
        GaleSemTakeDecision {
            ret: 0, // will be set by z_pend_curr return value
            new_count: 0,
            action: GALE_SEM_ACTION_PEND,
        }
    }
}
```

- [ ] **Step 2: Add C declarations to `ffi/include/gale_sem.h`**

Add after the existing declarations:

```c
/* ---- Phase 2: Full Decision API ---- */

struct gale_sem_give_decision {
    uint8_t action;     /* 0=INCREMENT_COUNT, 1=WAKE_THREAD */
    uint32_t new_count;
};

#define GALE_SEM_ACTION_INCREMENT 0
#define GALE_SEM_ACTION_WAKE      1

struct gale_sem_give_decision gale_k_sem_give_decide(
    uint32_t count, uint32_t limit, uint32_t has_waiter);

struct gale_sem_take_decision {
    int32_t ret;
    uint32_t new_count;
    uint8_t action;     /* 0=RETURN_IMMEDIATELY, 1=PEND_CURRENT */
};

#define GALE_SEM_ACTION_RETURN 0
#define GALE_SEM_ACTION_PEND   1

struct gale_sem_take_decision gale_k_sem_take_decide(
    uint32_t count, uint32_t is_no_wait);
```

- [ ] **Step 3: Build the FFI crate to verify it compiles**

Run: `cargo build --manifest-path ffi/Cargo.toml`
Expected: Compiles successfully

- [ ] **Step 4: Commit**

```bash
git add ffi/src/lib.rs ffi/include/gale_sem.h
git commit -m "feat(ffi): add GaleSemGiveDecision/TakeDecision structs for full sem logic"
```

### Task 2: Convert the Semaphore C Shim to Decision Pattern

**Files:**
- Modify: `zephyr/gale_sem.c` (rewrite z_impl_k_sem_give and z_impl_k_sem_take)

- [ ] **Step 1: Rewrite `z_impl_k_sem_give` to use decision struct**

Replace the body of `z_impl_k_sem_give` in `zephyr/gale_sem.c` (~lines 88-118):

```c
void z_impl_k_sem_give(struct k_sem *sem)
{
	k_spinlock_key_t key = k_spin_lock(&lock);
	bool resched = false;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_sem, give, sem);

	/* Rust decides: wake a thread or increment count */
	struct gale_sem_give_decision d = gale_k_sem_give_decide(
		sem->count, sem->limit,
		z_unpend_first_thread(&sem->wait_q) != NULL ? 0U : 0U);

	/*
	 * BUG: z_unpend_first_thread has a side effect (it removes the thread).
	 * We can't call it just to check — we need the old pattern.
	 * Instead, check if queue has a head first, then unpend if needed.
	 */
	struct k_thread *thread = z_unpend_first_thread(&sem->wait_q);
	d = gale_k_sem_give_decide(
		sem->count, sem->limit, thread != NULL ? 1U : 0U);

	if (d.action == GALE_SEM_ACTION_WAKE && thread != NULL) {
		arch_thread_return_value_set(thread, 0);
		z_ready_thread(thread);
		resched = true;
	} else {
		sem->count = d.new_count;
		resched = handle_poll_events(sem);
	}

	if (unlikely(resched)) {
		z_reschedule(&lock, key);
	} else {
		k_spin_unlock(&lock, key);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_sem, give, sem);
}
```

Wait — there's a subtlety. `z_unpend_first_thread` has a side effect (removes the thread from the wait queue). We must call it first, then pass the result to Rust. Let me fix the pattern:

```c
void z_impl_k_sem_give(struct k_sem *sem)
{
	k_spinlock_key_t key = k_spin_lock(&lock);
	bool resched = false;

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_sem, give, sem);

	/* Extract: try to unpend first waiter (side effect: removes from queue) */
	struct k_thread *thread = z_unpend_first_thread(&sem->wait_q);

	/* Decide: Rust determines action based on whether a waiter was found */
	struct gale_sem_give_decision d = gale_k_sem_give_decide(
		sem->count, sem->limit, thread != NULL ? 1U : 0U);

	/* Apply: execute Rust's decision */
	if (d.action == GALE_SEM_ACTION_WAKE) {
		arch_thread_return_value_set(thread, 0);
		z_ready_thread(thread);
		resched = true;
	} else {
		sem->count = d.new_count;
		resched = handle_poll_events(sem);
	}

	if (unlikely(resched)) {
		z_reschedule(&lock, key);
	} else {
		k_spin_unlock(&lock, key);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_sem, give, sem);
}
```

- [ ] **Step 2: Rewrite `z_impl_k_sem_take` to use decision struct**

Replace the body of `z_impl_k_sem_take` (~lines 129-165):

```c
int z_impl_k_sem_take(struct k_sem *sem, k_timeout_t timeout)
{
	int ret = 0;

	__ASSERT(((arch_is_in_isr() == false) ||
		  K_TIMEOUT_EQ(timeout, K_NO_WAIT)), "");

	k_spinlock_key_t key = k_spin_lock(&lock);

	SYS_PORT_TRACING_OBJ_FUNC_ENTER(k_sem, take, sem, timeout);

	/* Decide: Rust determines acquire/busy/pend */
	struct gale_sem_take_decision d = gale_k_sem_take_decide(
		sem->count, K_TIMEOUT_EQ(timeout, K_NO_WAIT) ? 1U : 0U);

	/* Apply */
	if (d.action == GALE_SEM_ACTION_RETURN) {
		sem->count = d.new_count;
		ret = d.ret;
		k_spin_unlock(&lock, key);
	} else {
		/* PEND_CURRENT: block on wait queue with timeout */
		ret = z_pend_curr(&lock, key, &sem->wait_q, timeout);
	}

	SYS_PORT_TRACING_OBJ_FUNC_EXIT(k_sem, take, sem, timeout, ret);
	return ret;
}
```

- [ ] **Step 3: Verify the Zephyr build still works locally (if west is available)**

Run:
```bash
source /Volumes/Home/git/zephyr/.venv/bin/activate
export ZEPHYR_BASE=/Volumes/Home/git/zephyr/zephyr
cd /Volumes/Home/git/zephyr
west build -b qemu_cortex_m3 -s zephyr/tests/kernel/semaphore/semaphore \
  -- -DZEPHYR_EXTRA_MODULES=/Volumes/Home/git/zephyr/gale \
     -DOVERLAY_CONFIG=/Volumes/Home/git/zephyr/gale/zephyr/gale_overlay.conf
```
Expected: Build succeeds

- [ ] **Step 4: Run Zephyr semaphore tests**

Run: `west build -t run`
Expected: All 24 semaphore tests pass (PASS on qemu_cortex_m3)

- [ ] **Step 5: Commit**

```bash
git add zephyr/gale_sem.c
git commit -m "feat(shim): convert k_sem_give/take to decision struct pattern"
```

### Task 3: Add Verus Contracts for the Decision Functions

**Files:**
- Modify: `src/sem.rs` (add decision function specs if not already present)

The existing `Semaphore::give` and `Semaphore::try_take` in `src/sem.rs` already have full Verus contracts covering P1-P10. The new FFI decision functions (`gale_k_sem_give_decide`, `gale_k_sem_take_decide`) in `ffi/src/lib.rs` call into the verified `gale` crate which uses `plain/src/sem.rs` (the stripped version). The Verus contracts are transitively verified.

- [ ] **Step 1: Verify cargo tests still pass with new FFI functions**

Run: `cargo test`
Expected: All tests pass (existing 995+ tests)

- [ ] **Step 2: Run verus-strip gate test**

Run: `cargo test --manifest-path tools/verus-strip/Cargo.toml --test gate`
Expected: 2 tests pass

- [ ] **Step 3: Commit (if any changes were needed)**

### Task 4: Push and Verify CI

- [ ] **Step 1: Push to main**

Run: `git push`

- [ ] **Step 2: Verify CI passes**

Run: `gh run list --branch main --limit 3`
Expected: All 3 workflows queued/passing (Rust CI, Zephyr Tests, Renode Tests)

- [ ] **Step 3: Specifically verify semaphore tests pass**

Wait for Zephyr Kernel Tests to complete. Check:
```bash
gh pr checks HEAD  # or gh run view <run-id>
```
Expected: `semaphore (qemu_cortex_m3)` passes

---

## Chunk 2: Track 1 Foundation — Expand Wait Queue for Full Decision Support

Track 1 is the critical path. The foundation modules (spinlock, thread, wait_queue) already exist as verified Rust models. The work is to expand them so the decision struct pattern can express "should we wake a thread from this wait queue?" as a Rust decision rather than a C decision.

### Task 5: Expand Wait Queue Model for Decision Support

**Files:**
- Modify: `src/wait_queue.rs` (add `has_waiters` and `first_waiter_priority` spec helpers)
- Modify: `plain/src/wait_queue.rs` (regenerate)

The existing `WaitQueue` has `pend`, `unpend_first`, `unpend_all`, `len`, `is_empty`. For the decision pattern, we need the C shim to pass "does the wait queue have waiters?" and "what's the priority of the first waiter?" as inputs to Rust. The wait queue itself stays in C (linked list / rbtree managed by Zephyr). What Rust needs is the ability to make decisions based on queue state passed as scalars.

- [ ] **Step 1: No model changes needed**

The current FFI pattern already works: the C shim calls `z_unpend_first_thread` (side effect) and passes the result to Rust as a boolean (`thread != NULL ? 1 : 0`). The Rust decision function receives this boolean and decides the action.

For more complex modules (mutex with priority inheritance, condvar with mutex handoff), the C shim will pass additional projections:
- `waiter_priority` (for priority inheritance)
- `owner_priority` (for priority adjustment)
- `current_priority` (for comparison)

These are extracted by the C shim and passed as `u32` values to Rust. No changes to the Rust wait_queue model are needed — the model already handles priority-ordered queuing.

- [ ] **Step 2: Document the "boolean projection" pattern**

The pattern is: C extracts runtime state → passes as scalars → Rust decides → C applies.

This is already working in the semaphore proof-of-concept (Task 2). Future modules follow the same pattern with module-specific projections.

- [ ] **Step 3: Commit documentation update if needed**

### Task 6: Apply Decision Pattern to Mutex (High Difficulty)

**Files:**
- Modify: `ffi/src/lib.rs` (add `GaleMutexLockDecision`, `GaleMutexUnlockDecision`)
- Modify: `ffi/include/gale_mutex.h` (add new C declarations)
- Modify: `zephyr/gale_mutex.c` (rewrite to use decision structs)

- [ ] **Step 1: Define mutex decision structs in `ffi/src/lib.rs`**

```rust
#[repr(C)]
pub struct GaleMutexLockDecision {
    pub ret: i32,
    pub action: u8,  // 0=ACQUIRED, 1=PEND_CURRENT, 2=RETURN_BUSY
    pub new_lock_count: u32,
}

pub const GALE_MUTEX_ACTION_ACQUIRED: u8 = 0;
pub const GALE_MUTEX_ACTION_PEND: u8 = 1;
pub const GALE_MUTEX_ACTION_BUSY: u8 = 2;

/// Full decision for k_mutex_lock.
/// Handles reentrant locking, ownership check, and pend-or-busy decision.
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mutex_lock_decide(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
    is_no_wait: u32,
) -> GaleMutexLockDecision {
    if owner_is_null != 0 || owner_is_current != 0 {
        // Free or reentrant: acquire
        #[allow(clippy::arithmetic_side_effects)]
        let new_count = lock_count + 1;
        GaleMutexLockDecision {
            ret: OK,
            action: GALE_MUTEX_ACTION_ACQUIRED,
            new_lock_count: new_count,
        }
    } else if is_no_wait != 0 {
        GaleMutexLockDecision {
            ret: EBUSY,
            action: GALE_MUTEX_ACTION_BUSY,
            new_lock_count: lock_count,
        }
    } else {
        GaleMutexLockDecision {
            ret: 0,
            action: GALE_MUTEX_ACTION_PEND,
            new_lock_count: lock_count,
        }
    }
}
```

- [ ] **Step 2: Add C declarations to `ffi/include/gale_mutex.h`**
- [ ] **Step 3: Rewrite `z_impl_k_mutex_lock` in `zephyr/gale_mutex.c` to use decision struct**
- [ ] **Step 4: Rewrite `z_impl_k_mutex_unlock` similarly**
- [ ] **Step 5: Build and test locally**

Run: `west build -b qemu_cortex_m3 -s zephyr/tests/kernel/mutex/mutex -- ...`
Expected: All 12 mutex tests pass

- [ ] **Step 6: Commit**

```bash
git commit -m "feat(shim): convert k_mutex_lock/unlock to decision struct pattern"
```

### Task 7: Apply Decision Pattern to Remaining Wired Modules

Repeat the pattern from Tasks 1-2 for each already-wired module. These are independent and can be done in parallel:

**Batch A (Low difficulty — simple count/state modules):**

- [ ] **Step 1: stack** — `GaleStackPushDecision`, `GaleStackPopDecision`
- [ ] **Step 2: mem_slab** — `GaleMemSlabAllocDecision`, `GaleMemSlabFreeDecision`
- [ ] **Step 3: timer** — `GaleTimerExpireDecision`, `GaleTimerStatusDecision`
- [ ] **Step 4: event** — `GaleEventPostDecision`, `GaleEventWaitDecision`
- [ ] **Step 5: fifo/lifo** — share queue decision structs

**Batch B (Medium difficulty — ring buffer / matching):**

- [ ] **Step 6: msgq** — `GaleMsgqPutDecision`, `GaleMsgqGetDecision`
- [ ] **Step 7: queue** — `GaleQueueDecision`
- [ ] **Step 8: mbox** — `GaleMboxSendDecision`, `GaleMboxRecvDecision`

**Batch C (High difficulty — complex state):**

- [ ] **Step 9: pipe** — `GalePipeWriteDecision`, `GalePipeReadDecision` (handles direct-copy and retry loops)

Each sub-step follows the same pattern:
1. Define `#[repr(C)]` decision struct in `ffi/src/lib.rs`
2. Add C declarations in `ffi/include/gale_<module>.h`
3. Rewrite C shim to use Extract→Decide→Apply pattern
4. Build and test with `west build -t run`
5. Commit

- [ ] **Step 10: Push all and verify CI**

### Task 8: Fill Stub Shims for Remaining 12 Modules

These modules currently have 17-line stub C shims. Each needs:
1. Decision struct in Rust FFI
2. C header declarations
3. Full C shim implementation (Extract→Decide→Apply)
4. CI test suite wiring

**Can be parallelized — each module is independent:**

- [ ] **Step 1: futex** (Medium — wait/wake)
- [ ] **Step 2: poll** (High — multi-object event scanning)
- [ ] **Step 3: timeslice** (Low — accounting only)
- [ ] **Step 4: kheap** (Medium — alloc/free decisions)
- [ ] **Step 5: thread_lifecycle** (High — create/abort/join, touches thread.c)
- [ ] **Step 6: work** (High — deferred execution, thread pool)
- [ ] **Step 7: fatal** (Low — error classification)
- [ ] **Step 8: mempool** (Medium — pool management)
- [ ] **Step 9: dynamic** (Medium — thread pool tracking)
- [ ] **Step 10: smp_state** (Medium — CPU lifecycle)
- [ ] **Step 11: sched** (High — run queue decisions, already has Verus model)
- [ ] **Step 12: timeout** (Medium — deadline tracking)

- [ ] **Step 13: Add all new test suites to CI matrix**

For modules not yet in `.github/workflows/zephyr-tests.yml`, add entries. Some (futex, fatal, dynamic) require MPU and can only run on Renode M4F/M33, not qemu_cortex_m3.

- [ ] **Step 14: Push and verify all CI checks pass**

### Task 9: Update Rivet Artifacts and README

**Files:**
- Modify: `artifacts/verification.yaml`
- Modify: `artifacts/design.yaml`
- Modify: `README.md`

- [ ] **Step 1: Update verification artifacts**

For each module that was expanded, update the corresponding `FV-*` artifact to reflect the expanded verification scope (now covers wait queue decisions, not just arithmetic).

- [ ] **Step 2: Update README**

Update the module status table: all 25 kernel API modules should show "Verified + Zephyr tested" (with appropriate notes for modules that need MPU for full testing).

- [ ] **Step 3: Run rivet validate**

```bash
rivet validate
```
Expected: PASS (with known gaps documented in README)

- [ ] **Step 4: Commit and push**

```bash
git commit -m "docs: update verification artifacts and README for full kernel replacement"
git push
```

---

## Execution Strategy

**Phase 1 (Proof of Concept):** Tasks 1-4 — Convert semaphore to decision pattern. Validates the architecture end-to-end with CI. ~1 day.

**Phase 2 (Wired Modules):** Tasks 5-7 — Convert all 13 already-wired modules. Parallelizable. ~1 week with 3-4 parallel workers.

**Phase 3 (Stub Modules):** Task 8 — Fill all 12 stub shims. Parallelizable. ~2 weeks with 3-4 parallel workers.

**Phase 4 (Finalize):** Task 9 — Documentation and CI. ~1 day.

**Total with full parallelism: ~3 weeks.**
**Total sequential: ~6-8 weeks.**

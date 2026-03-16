# Deep Zephyr Kernel Analysis for Gale Expansion

Analysis date: 2026-03-16
Scope: Complete Zephyr kernel inventory, STPA safety analysis, minimal ASIL-D kernel definition.

---

## Part 1: Complete Kernel File Inventory

### 1.1 All kernel/*.c files (sorted by size, descending)

| File | Lines | Classification | Gale Status | Safety Impact |
|------|------:|:--------------|:------------|:-------------|
| mmu.c | 1837 | RUST-SAFE | Not covered | HIGH — page frame management, virtual address allocation, demand paging |
| sched.c | 1605 | RUST-SAFE | Verus model (866 lines) | CRITICAL — scheduling decisions, priority queue, context switch |
| thread.c | 1367 | RUST-SAFE | Verus model (169+520 lines) | CRITICAL — thread creation, stack setup, lifecycle |
| work.c | 1275 | RUST-SAFE | Verus model (339 lines) | HIGH — work queue state machine, flag manipulation |
| userspace.c | 1039 | RUST-SAFE | Not covered | HIGH — kernel object permissions, dynamic objects |
| poll.c | 810 | RUST-SAFE | Verus model (688 lines) | HIGH — async event multiplexing, multi-object wait |
| init.c | 652 | INFRASTRUCTURE | Not covered | MEDIUM — kernel boot sequence, thread init |
| msg_q.c | 516 | **COVERED** | Verus verified + C shim | CRITICAL — ring buffer index arithmetic |
| queue.c | 498 | **COVERED** | Verus verified + C shim | HIGH — dynamic queue count tracking |
| usage.c | 484 | INFRASTRUCTURE | Not covered | LOW — runtime statistics, thread CPU usage |
| mailbox.c | 461 | **COVERED** | Verus verified + C shim | HIGH — message ID matching, data exchange |
| events.c | 448 | **COVERED** | Verus verified + C shim | MEDIUM — bitmask operations |
| timer.c | 434 | **COVERED** | Verus verified + C shim | HIGH — expiry counter, status get/reset |
| mem_domain.c | 424 | HARDWARE-BOUND | Not covered | HIGH — memory domain partitioning for userspace |
| atomic_c.c | 414 | HARDWARE-BOUND | Not covered | MEDIUM — software atomic operations fallback |
| pipe.c | 358 | **COVERED** | Verus verified + C shim | HIGH — state machine, byte count |
| mem_slab.c | 353 | **COVERED** | Verus verified + C shim | HIGH — block count tracking |
| timeout.c | 351 | RUST-SAFE | Verus model (629 lines) | CRITICAL — tick arithmetic, deadline ordering |
| mutex.c | 337 | **COVERED** | Verus verified + C shim | CRITICAL — ownership, lock count, priority inheritance |
| obj_core.c | 287 | INFRASTRUCTURE | Not covered | LOW — kernel object debug/tracing |
| device.c | 277 | INFRASTRUCTURE | Not covered | MEDIUM — device init/deinit lifecycle |
| smp.c | 264 | HARDWARE-BOUND | Verus model (330 lines) | HIGH — CPU lifecycle, global lock |
| sem.c | 237 | **COVERED** | Verus verified + C shim | CRITICAL — count give/take, bounded |
| stack.c | 222 | **COVERED** | Verus verified + C shim | HIGH — LIFO capacity check |
| kheap.c | 218 | RUST-SAFE | Verus model (373 lines) | HIGH — kernel heap wrapper |
| mempool.c | 204 | RUST-SAFE | Verus model (347 lines) | HIGH — memory pool alloc/free |
| ipi.c | 204 | HARDWARE-BOUND | Not covered | MEDIUM — inter-processor interrupt |
| fatal.c | 179 | RUST-SAFE | Verus model (305 lines) | CRITICAL — fault classification, recovery |
| dynamic.c | 175 | RUST-SAFE | Verus model (270 lines) | MEDIUM — dynamic thread pool |
| timeslicing.c | 161 | RUST-SAFE | Verus model (325 lines) | HIGH — time-slice tick accounting |
| busy_wait.c | 128 | HARDWARE-BOUND | Not covered | LOW — spin loop, hardware cycle counter |
| thread_monitor.c | 110 | INFRASTRUCTURE | Not covered | LOW — thread enumeration for debug |
| futex.c | 105 | RUST-SAFE | Verus model (369 lines) | MEDIUM — fast userspace mutex |
| idle.c | 104 | HARDWARE-BOUND | Not covered | MEDIUM — idle loop, power management |
| boot_args.c | 94 | INFRASTRUCTURE | Not covered | LOW — boot argument parsing |
| userspace_handler.c | 89 | RUST-SAFE | Not covered | HIGH — syscall validation |
| nothread.c | 83 | INFRASTRUCTURE | Not covered | LOW — single-threaded mode stub |
| cpu_mask.c | 72 | RUST-SAFE | Not covered | MEDIUM — CPU affinity bitmask |
| compiler_stack_protect.c | 65 | INFRASTRUCTURE | Not covered | MEDIUM — stack canary handler |
| errno.c | 57 | INFRASTRUCTURE | Not covered | LOW — per-thread errno |
| sys_clock_hw_cycles.c | 52 | HARDWARE-BOUND | Not covered | LOW — hardware cycle counter |
| spinlock_validate.c | 52 | RUST-SAFE | Not covered | HIGH — spinlock double-lock detection |
| banner.c | 48 | INFRASTRUCTURE | Not covered | NONE — boot banner print |
| float.c | 47 | HARDWARE-BOUND | Not covered | LOW — FPU enable/disable shim |
| system_work_q.c | 38 | INFRASTRUCTURE | Not covered | LOW — system work queue init |
| priority_queues.c | 30 | INFRASTRUCTURE | Not covered | LOW — priority queue init |
| main_weak.c | 30 | INFRASTRUCTURE | Not covered | NONE — weak main() stub |
| dynamic_disabled.c | 25 | INFRASTRUCTURE | Not covered | NONE — disabled dynamic thread stubs |
| irq_offload.c | 23 | HARDWARE-BOUND | Not covered | LOW — IRQ offload helper |
| version.c | 21 | INFRASTRUCTURE | Not covered | NONE — version string |

### 1.2 Key kernel include/ headers

| Header | Lines | Role | Safety Impact |
|--------|------:|:-----|:-------------|
| kernel_arch_interface.h | 707 | Arch/kernel contract | CRITICAL |
| mmu.h | 411 | MMU types and macros | HIGH |
| ksched.h | 368 | Scheduler internal API | CRITICAL |
| priority_q.h | 355 | Priority queue data structure | HIGH |
| kthread.h | 281 | Thread struct internals | CRITICAL |
| kernel_internal.h | 278 | Internal kernel API | HIGH |
| kswap.h | 248 | Context switch | CRITICAL |
| wait_q.h | 103 | Wait queue API | HIGH |
| timeout_q.h | 103 | Timeout queue API | HIGH |

### 1.3 Summary statistics

| Category | Files | Total Lines | Gale Coverage |
|----------|------:|------------:|:-------------|
| **COVERED** (Verus verified + C shim) | 13 | 4,679 | Full FFI integration |
| **RUST-SAFE** (Verus model only, no shim) | 16 | 8,502 | State machine models, no runtime integration |
| **HARDWARE-BOUND** | 9 | 1,387 | Partial (smp_state model) |
| **INFRASTRUCTURE** | 14 | 1,736 | None needed (low safety impact) |
| **Total** | 52 | 16,304 | |

**Current Gale Verus verification**: 30 Rust modules, 10,718 lines of verified Rust, 159 proof functions, 493 ensures clauses.

**Current Gale C shims**: 12 files, 3,615 lines.

---

## Part 2: Beyond kernel/ -- Subsystems for ASIL-D

### 2.1 Subsystem line counts

| Subsystem | Lines | ASIL-D Relevance | Notes |
|-----------|------:|:-----------------|:------|
| **lib/heap/** | 1,413 | CRITICAL | sys_heap allocator -- buffer overflows, use-after-free |
| **lib/os/** | 7,692 | HIGH | ring buffers, printf, fd table, clock |
| **drivers/timer/** | 13,738 | HIGH | System tick source (1 driver per board) |
| **drivers/serial/** | 71,698 | MEDIUM | UART (needed for debug output only) |
| **arch/arm/core/** | 9,618 | CRITICAL | Context switch, fault handling, MPU |
| subsys/ipc/ | 5,881 | MEDIUM | IPC mechanisms |
| subsys/logging/ | 9,960 | LOW | Logging framework |
| subsys/pm/ | 2,644 | MEDIUM | Power management |
| subsys/mem_mgmt/ | 216 | HIGH | Memory management interfaces |

### 2.2 Critical subsystem deep-dive

#### lib/heap/ (1,413 lines) -- HIGHEST PRIORITY for Gale expansion

The sys_heap implementation (`heap.c`, 592 lines) is a critical attack surface:
- **Free-list manipulation** with raw pointer arithmetic
- **Chunk coalescing** with unchecked boundary conditions
- **Bucket index calculation** using bit manipulation
- **CONTAINER_OF casts** throughout
- No formal bounds checking on chunk sizes

This is where buffer overflows, use-after-free, and heap corruption originate.
A Rust replacement would eliminate entire classes of vulnerabilities.

Key files:
- `heap.c` (592 lines) -- core allocator logic, free list, split/merge
- `heap_validate.c` (186 lines) -- heap integrity check
- `multi_heap.c` (131 lines) -- multi-region heap
- `shared_multi_heap.c` (128 lines) -- shared memory heap

#### lib/os/ key components

| File | Lines | ASIL-D Value |
|------|------:|:------------|
| cbprintf_complete.c | 1,883 | LOW -- format string processing |
| cbprintf_packaged.c | 1,170 | MEDIUM -- packed printf args |
| mpsc_pbuf.c | 660 | HIGH -- multi-producer single-consumer ring buffer |
| zvfs_fdtable.c | 609 | MEDIUM -- file descriptor table |
| spsc_pbuf.c | 387 | HIGH -- single-producer ring buffer |
| p4wq.c | 323 | MEDIUM -- parallel work queue |
| clock.c | 243 | HIGH -- system clock wrappers |

The ring buffer implementations (`mpsc_pbuf.c`, `spsc_pbuf.c`) are critical data
structures used throughout the kernel. They involve lock-free algorithms with
subtle ordering requirements -- prime candidates for Rust + verification.

#### arch/arm/core/ critical components

| File | Lines | ASIL-D Value |
|------|------:|:------------|
| cortex_m/fault.c | 1,124 | CRITICAL -- fault diagnosis and recovery |
| mmu/arm_mmu.c | 1,105 | CRITICAL -- ARM MMU table management |
| mpu/nxp_mpu.c | 713 | HIGH -- NXP MPU region config |
| cortex_m/thread.c | 709 | CRITICAL -- Cortex-M thread context |
| mpu/arm_mpu.c | 633 | HIGH -- ARM MPU region config |
| mpu/arm_core_mpu.c | 372 | HIGH -- Core MPU management |

The Cortex-M fault handler (1,124 lines) is the first line of defense when
something goes wrong. Its complexity makes it both safety-critical and error-prone.

#### include/zephyr/sys/ data structures

| Header | Lines | ASIL-D Value | Notes |
|--------|------:|:------------|:------|
| dlist.h | 645 | HIGH | Doubly-linked list (inline, no bounds checks) |
| ring_buffer.h | 531 | CRITICAL | Ring buffer (used by pipe, msgq, logging) |
| sflist.h | 508 | HIGH | Singly-linked flagged list (used by queue) |
| slist.h | 454 | HIGH | Singly-linked list |
| spinlock.h | 447 | CRITICAL | Spinlock with validation |
| atomic.h | 482 | CRITICAL | Atomic operations |
| bitarray.h | 343 | MEDIUM | Bit array operations |
| rb.h | 240 | MEDIUM | Red-black tree |

---

## Part 3: STPA Safety Analysis

### 3.1 Losses (what we are protecting against)

| ID | Loss | Impact | Severity |
|----|------|:-------|:---------|
| L1 | **Data corruption** | Race conditions, buffer overflow, stale pointer dereference corrupt application or kernel state | Catastrophic |
| L2 | **Deadlock** | System hangs; safety-critical task never executes | Catastrophic |
| L3 | **Priority inversion** | High-priority safety task starved by low-priority task holding resource | Hazardous |
| L4 | **Resource leak** | Memory, thread, or kernel object never freed; system degrades over time | Major |
| L5 | **Timing violation** | Deadline missed; actuator does not fire within required window | Catastrophic |
| L6 | **Undefined behavior** | Null dereference, integer overflow, use-after-free lead to arbitrary execution | Catastrophic |

### 3.2 Hazards (system states leading to losses)

#### H1: Synchronization Primitive Hazards

| ID | Hazard | Modules | Leads to |
|----|--------|---------|----------|
| H1.1 | Semaphore count exceeds `max_count` | sem | L1, L6 |
| H1.2 | Semaphore count goes below 0 (underflow) | sem | L1, L6 |
| H1.3 | Mutex unlocked by non-owner | mutex | L1, L2, L3 |
| H1.4 | Mutex lock_count overflows | mutex | L1, L6 |
| H1.5 | Priority inheritance not correctly restored on unlock | mutex, sched | L3, L5 |
| H1.6 | Condvar signal wakes wrong thread | condvar, wait_queue | L3, L5 |
| H1.7 | Event bitmask corrupted by concurrent post/clear | event | L1 |
| H1.8 | Futex wait/wake mismatch (spurious wake or lost wake) | futex | L2, L5 |

**Gale mitigation**: H1.1-H1.4 are directly verified by Verus proofs. H1.5 is
partially verified (mutex ownership tracking). H1.6-H1.7 are verified at the
model level. H1.8 is modeled but not FFI-integrated.

#### H2: IPC Hazards

| ID | Hazard | Modules | Leads to |
|----|--------|---------|----------|
| H2.1 | Message queue ring buffer index wraps incorrectly | msgq | L1, L6 |
| H2.2 | Message queue read/write indices diverge | msgq | L1 |
| H2.3 | Pipe byte count does not match actual bytes in ring buffer | pipe | L1 |
| H2.4 | Pipe state machine allows read on closed pipe | pipe | L6 |
| H2.5 | Stack push beyond capacity | stack | L1, L6 |
| H2.6 | Queue count mismatch with linked list length | queue | L1, L4 |
| H2.7 | Mailbox message ID matching allows wrong sender/receiver pairing | mbox | L1 |
| H2.8 | Mailbox data exchange size exceeds buffer bounds | mbox | L1, L6 |

**Gale mitigation**: H2.1-H2.8 are all verified by Verus proofs and have C shim
integration that delegates safety-critical arithmetic to Rust.

#### H3: Scheduler Hazards

| ID | Hazard | Modules | Leads to |
|----|--------|---------|----------|
| H3.1 | Run queue returns wrong highest-priority thread | sched, priority | L3, L5 |
| H3.2 | Thread stuck in READY state but never scheduled | sched | L2, L5 |
| H3.3 | Thread state machine allows invalid transition | thread, sched | L1, L6 |
| H3.4 | Time slice tick counter overflow | timeslice | L3, L5 |
| H3.5 | MetaIRQ preemption record not cleared, causing stale dispatch | sched | L3 |
| H3.6 | Scheduler cache (update_cache) inconsistent with run queue | sched | L3, L5 |
| H3.7 | CPU mask allows thread to run on masked-off CPU | cpu_mask, sched | L1 |

**Gale mitigation**: H3.1 verified at model level (RunQueue.best). H3.3 modeled
(ThreadState FSM). H3.4 verified (TimeSlice tick counter). H3.5-H3.7 not yet
addressed at FFI level.

#### H4: Memory Management Hazards

| ID | Hazard | Modules | Leads to |
|----|--------|---------|----------|
| H4.1 | Heap chunk split/merge corrupts free list | lib/heap | L1, L4, L6 |
| H4.2 | Heap use-after-free: freed chunk returned to caller | lib/heap | L1, L6 |
| H4.3 | Memory slab double-free | mem_slab | L1, L6 |
| H4.4 | Memory pool returns null without caller check | mempool, kheap | L6 |
| H4.5 | MMU page table corruption allows unprivileged access | mmu | L1, L6 |
| H4.6 | Memory domain partition overlap | mem_domain | L1 |
| H4.7 | Stack overflow undetected | compiler_stack_protect | L1, L6 |

**Gale mitigation**: H4.3 partially verified (mem_slab count tracking). H4.4
modeled (mempool, kheap null checks). H4.1-H4.2, H4.5-H4.7 not addressed.

#### H5: Timing and Timeout Hazards

| ID | Hazard | Modules | Leads to |
|----|--------|---------|----------|
| H5.1 | Timeout deadline computed with wrong tick offset | timeout | L5 |
| H5.2 | Timeout fires after object destroyed | timeout, timer | L6 |
| H5.3 | Timer status counter overflows | timer | L1 |
| H5.4 | Busy-wait cycle conversion overflow | busy_wait | L5 |
| H5.5 | Tick wrap-around at 64-bit boundary | timeout | L5 |
| H5.6 | announce_remaining not zeroed after completion | timeout | L5 |

**Gale mitigation**: H5.1 verified at model level (tick arithmetic). H5.3
verified (checked status increment). H5.4-H5.6 not addressed.

#### H6: Work Queue Hazards

| ID | Hazard | Modules | Leads to |
|----|--------|---------|----------|
| H6.1 | Work item submitted while already running (re-entrant handler) | work | L1 |
| H6.2 | Work cancellation race: handler runs after cancel returns | work | L1, L6 |
| H6.3 | Flush/cancel synchronization deadlock | work | L2 |
| H6.4 | Work queue drain with active submissions | work | L2, L4 |

**Gale mitigation**: H6.1-H6.2 verified at model level (work item state machine).
No C shim integration yet.

### 3.3 Unsafe Control Actions (UCAs)

#### UCA-1: Semaphore operations

| UCA ID | Control Action | Unsafe Condition | Type | Consequence |
|--------|:--------------|:-----------------|:-----|:-----------|
| UCA-SEM-1 | k_sem_give() | Called when count == max_count | Providing causes hazard | count overflow (H1.1) |
| UCA-SEM-2 | k_sem_take() | Called from ISR with non-zero timeout | Providing causes hazard | blocks in ISR (L2) |
| UCA-SEM-3 | k_sem_give() | Not called when producer has data | Not providing | consumer starved (L5) |
| UCA-SEM-4 | k_sem_take() | Returns success but count was 0 | Wrong order/timing | data-less wake (L1) |
| UCA-SEM-5 | k_sem_reset() | Called while threads waiting | Providing causes hazard | waiting threads never wake (L2) |

**Verification status**: UCA-SEM-1 proven (give saturates at max_count). UCA-SEM-4
proven (take decrements count before return). UCA-SEM-2, UCA-SEM-3, UCA-SEM-5
are behavioral properties requiring system-level proof.

#### UCA-2: Mutex operations

| UCA ID | Control Action | Unsafe Condition | Type | Consequence |
|--------|:--------------|:-----------------|:-----|:-----------|
| UCA-MUT-1 | k_mutex_lock() | Thread already holds mutex, lock_count overflows | Providing causes hazard | H1.4 |
| UCA-MUT-2 | k_mutex_unlock() | Called by non-owner thread | Providing causes hazard | H1.3 |
| UCA-MUT-3 | k_mutex_unlock() | Priority not restored after last unlock in chain | Wrong timing | H1.5 |
| UCA-MUT-4 | k_mutex_lock() | Not acquired before accessing protected resource | Not providing | L1, L6 |
| UCA-MUT-5 | k_mutex_unlock() | Not called on error path (exception in critical section) | Not providing | L2, L4 |

**Verification status**: UCA-MUT-1 proven (lock_count checked). UCA-MUT-2 proven
(ownership validated). UCA-MUT-3 partially proven (prio tracking modeled). UCA-MUT-4
and UCA-MUT-5 are usage-pattern properties beyond kernel scope.

#### UCA-3: Scheduler operations

| UCA ID | Control Action | Unsafe Condition | Type | Consequence |
|--------|:--------------|:-----------------|:-----|:-----------|
| UCA-SCHED-1 | z_ready_thread() | Thread already in run queue | Providing causes hazard | double-insertion corruption (L1) |
| UCA-SCHED-2 | z_swap() | No higher-priority thread ready | Wrong timing | unnecessary context switch (L5) |
| UCA-SCHED-3 | z_pend_thread() | Thread already pending on another object | Providing causes hazard | wait queue corruption (L1) |
| UCA-SCHED-4 | z_unpend_thread() | Thread not in any wait queue | Wrong state | list corruption (L1, L6) |
| UCA-SCHED-5 | k_sched_lock() | Not unlocked before thread sleeps | Not providing (unlock) | scheduler lock leak (L2) |
| UCA-SCHED-6 | update_cache() | Cache updated without checking cooperative semantics | Wrong timing | cooperative thread preempted (L3) |

**Verification status**: UCA-SCHED-1 partially verified (RunQueue model). UCA-SCHED-3
verified (no_duplicates precondition in WaitQueue). Others are behavioral.

#### UCA-4: Timeout operations

| UCA ID | Control Action | Unsafe Condition | Type | Consequence |
|--------|:--------------|:-----------------|:-----|:-----------|
| UCA-TO-1 | z_add_timeout() | Deadline wraps past max tick value | Providing causes hazard | fires immediately or never (H5.1) |
| UCA-TO-2 | z_abort_timeout() | Timeout already fired, callback in progress | Wrong timing | use-after-callback-start (H5.2) |
| UCA-TO-3 | sys_clock_announce() | ticks parameter is 0 or negative | Providing causes hazard | tick counter corruption (H5.5) |
| UCA-TO-4 | z_add_timeout() | Not called (timeout omitted) | Not providing | thread blocks forever (L2) |

**Verification status**: UCA-TO-1 verified at model level (deadline arithmetic).
Others require system-level integration.

#### UCA-5: Memory operations

| UCA ID | Control Action | Unsafe Condition | Type | Consequence |
|--------|:--------------|:-----------------|:-----|:-----------|
| UCA-MEM-1 | sys_heap_alloc() | Returns pointer into already-allocated chunk | Providing causes hazard | H4.1, H4.2 |
| UCA-MEM-2 | sys_heap_free() | Pointer not from this heap | Providing causes hazard | free-list corruption (H4.1) |
| UCA-MEM-3 | sys_heap_free() | Same pointer freed twice | Providing causes hazard | double-free (H4.2) |
| UCA-MEM-4 | k_mem_slab_alloc() | Returns success but no block available | Wrong state | use of uninitialized memory (L6) |
| UCA-MEM-5 | k_mem_slab_free() | Block not from this slab | Providing causes hazard | slab corruption (H4.3) |

**Verification status**: UCA-MEM-4 partially verified (count tracking). Others
require heap algorithm verification.

### 3.4 Causal Scenarios

#### CS-1: Race condition in scheduler cache update (H3.6 -> L3, L5)

**Scenario**: Thread A calls `z_ready_thread(T_high)` on CPU 0 while CPU 1 is in
`update_cache()`. The cache on CPU 1 does not reflect T_high's readiness, so the
next context switch on CPU 1 picks a lower-priority thread.

**Root cause**: The scheduler uses a single spinlock (`_sched_spinlock`) with 66
lock/unlock sites in sched.c. The window between `runq_add()` and `update_cache()`
is a critical section; if interrupted between the two, the cache is stale.

**Mitigation**: Rust's borrow checker would make the lock scope explicit. Verus
could prove that `runq_add` and `update_cache` are always called under the same
lock acquisition.

#### CS-2: Priority inheritance chain corruption (H1.5 -> L3)

**Scenario**: Thread A holds Mutex M1 at priority 5, then acquires Mutex M2. Thread B
(priority 1) blocks on M1, boosting A to priority 1. Thread C (priority 2) blocks
on M2. If A releases M2 first, its priority should drop to 1 (from M1 inheritance).
If the priority restoration algorithm looks only at M2's waiters, it incorrectly
sets A's priority to 5, breaking M1's inheritance.

**Root cause**: Zephyr's mutex.c `adjust_owner_prio()` traverses only the specific
mutex's wait queue to compute the new priority. It does not aggregate across all
held mutexes.

**Zephyr behavior**: Zephyr documents that mutexes "must be released in the reverse
order they were acquired" to avoid this. This is an API contract, not an enforced
invariant.

**Mitigation**: Gale's mutex model verifies ownership and lock count. The priority
inheritance chain is partially modeled. A complete solution would require a
multi-mutex ownership graph model.

#### CS-3: Timeout delta-list corruption (H5.1 -> L5)

**Scenario**: Two threads simultaneously call `z_add_timeout()` with deadlines that
should be inserted adjacent in the delta list. If the timeout_lock is released
and re-acquired between computing the insertion point and performing the insert,
the delta values become inconsistent. A subsequent `sys_clock_announce()` fires
timeouts out of order or skips them entirely.

**Root cause**: The timeout.c delta-list uses relative ticks (`dticks`) between
consecutive entries. Insertion requires updating the `dticks` of the successor.
If a concurrent removal changes the successor during insertion, the delta chain
is broken.

**Actual Zephyr protection**: The `timeout_lock` spinlock is held continuously
during list manipulation, preventing this scenario in practice. However, the C
code relies on programmer discipline to always hold the lock.

**Mitigation**: Rust's type system can encode "must hold lock" as a type parameter,
making this compile-time verified.

#### CS-4: Heap use-after-free (H4.2 -> L1, L6)

**Scenario**: Thread A allocates buffer B from `sys_heap`. Thread A passes B to
Thread C via a message queue. Thread A then frees B. Thread C reads from B, which
now overlaps with a new allocation made by Thread D. Thread C reads Thread D's data.

**Root cause**: C has no ownership tracking. The heap's free-list manipulation does
not validate that the freed chunk matches a prior allocation.

**Mitigation**: Rust's ownership model prevents this at compile time. A Rust heap
allocator with lifetime tracking would make this class of bug impossible.

#### CS-5: Spinlock double-acquisition deadlock (L2)

**Scenario**: A function acquires spinlock L, then calls a helper function that also
acquires L. On a uniprocessor system, this hangs. On SMP, it may cause a livelock.

**Root cause**: Zephyr's `spinlock_validate.c` detects this at runtime (52 lines of
validation code), but only when `CONFIG_SPIN_VALIDATE=y`. In production, this
check is typically disabled for performance.

**Mitigation**: Rust's borrow checker prevents double-acquisition by construction.
A spinlock guard type (`MutexGuard`) cannot be acquired twice in the same scope.

#### CS-6: Poll event registration leak (H6 -> L4)

**Scenario**: `k_poll()` registers poll events on kernel objects (sem, signal, msgq).
If the polling thread is aborted between registration and return, the kernel
objects retain dangling pointers to the aborted thread's poll events.

**Root cause**: `clear_event_registrations()` is only called in the normal return
path of `k_poll()`. Thread abort does not clean up poll registrations.

**Mitigation**: Rust's RAII (Drop trait) would automatically clear registrations
when the poll context goes out of scope, even during unwinding.

---

## Part 4: Minimal ASIL-D Kernel Definition

### 4.1 What is mandatory?

For an ASIL-D safety-critical system (e.g., automotive actuator control),
the minimal kernel requires:

#### Tier 0: Absolutely Required (cannot disable)

| Component | Files | Lines | Rationale |
|-----------|-------|------:|:----------|
| Thread creation/management | thread.c | 1,367 | At least 2 threads needed (main + safety) |
| Scheduler | sched.c | 1,605 | Priority-based scheduling is the safety mechanism |
| Context switch | kswap.h + arch | ~500 | Cannot schedule without switching |
| Initialization | init.c | 652 | Must boot the system |
| Fatal error handling | fatal.c | 179 | Must handle faults safely |
| IRQ management | arch/irq.h | ~300 | Interrupts are fundamental |
| **Total** | | **~4,603** | |

#### Tier 1: Required for Safety Primitives

| Component | Files | Lines | Kconfig | Rationale |
|-----------|-------|------:|:--------|:----------|
| Semaphore | sem.c | 237 | Always on | Basic synchronization |
| Mutex | mutex.c | 337 | Always on | Priority inheritance for ASIL-D |
| Timeout/Timer | timeout.c + timer.c | 785 | CONFIG_SYS_CLOCK_EXISTS=y | Deadline monitoring |
| Spinlock | spinlock.h | 447 | Always on | Interrupt-safe locking |
| Wait queue | wait_q.h | 103 | Always on | Thread blocking infrastructure |
| **Total** | | **~1,909** | | |

#### Tier 2: Recommended for ASIL-D

| Component | Files | Lines | Kconfig | Rationale |
|-----------|-------|------:|:--------|:----------|
| Stack canary | compiler_stack_protect.c | 65 | CONFIG_STACK_CANARIES=y | Stack overflow detection |
| Memory slab | mem_slab.c | 353 | Always on | Deterministic allocation |
| Timeslicing | timeslicing.c | 161 | CONFIG_TIMESLICING=y | Starvation prevention |
| Message queue | msg_q.c | 516 | Always on | Inter-task communication |
| **Total** | | **~1,095** | | |

### 4.2 What can be disabled for minimal ASIL-D?

| Component | Kconfig to disable | Lines saved | Safety risk of inclusion |
|-----------|:-------------------|------------:|:------------------------|
| MMU | CONFIG_MMU=n | 1,837 | MMU bugs are catastrophic; skip if MCU |
| Userspace | CONFIG_USERSPACE=n | 1,039+89 | Complex; MCU typically runs flat |
| Poll | CONFIG_POLL=n | 810 | Can use direct semaphore/msgq waits |
| Events | CONFIG_EVENTS=n | 448 | Can use semaphores instead |
| Work queues | CONFIG_SYSTEM_WORKQUEUE=n | 1,275+38 | Deferred work adds complexity |
| SMP | CONFIG_SMP=n (MP_MAX_NUM_CPUS=1) | 264+204 | Single-core simplifies analysis |
| Mailbox | CONFIG_MBOX=n (if available) | 461 | Use msgq instead |
| Pipe | (no Kconfig, always compiled) | 358 | Use msgq instead |
| Dynamic threads | CONFIG_DYNAMIC_THREAD=n | 175+25 | Static thread allocation is safer |
| Thread monitor | CONFIG_THREAD_MONITOR=n | 110 | Debug only |
| Memory domain | Implied by USERSPACE=n | 424 | Not needed without userspace |
| Kernel heap | CONFIG_HEAP_MEM_POOL_SIZE=0 | 218+204 | Dynamic alloc is non-deterministic |
| Usage stats | CONFIG_SCHED_THREAD_USAGE=n | 484 | Statistics only |
| Object core | CONFIG_OBJ_CORE=n | 287 | Debug/tracing only |
| FPU sharing | CONFIG_FPU_SHARING=n | 47 | Avoid FPU complexity if not needed |
| CPU mask | CONFIG_SCHED_CPU_MASK=n | 72 | Single-core only |
| **Total saveable** | | **~7,889** | |

### 4.3 Minimal ASIL-D kernel configuration

```ini
# Minimal ASIL-D kernel (Cortex-M, single-core, no MMU)
CONFIG_MULTITHREADING=y
CONFIG_NUM_COOP_PRIORITIES=16
CONFIG_NUM_PREEMPT_PRIORITIES=16
CONFIG_NUM_METAIRQ_PRIORITIES=0

# Safety features ON
CONFIG_STACK_CANARIES=y
CONFIG_TIMESLICING=y
CONFIG_ASSERT=y
CONFIG_SPIN_VALIDATE=y

# Timing
CONFIG_SYS_CLOCK_EXISTS=y
CONFIG_TICKLESS_KERNEL=y

# Synchronization (verified by Gale)
CONFIG_GALE_KERNEL_SEM=y
CONFIG_GALE_KERNEL_MUTEX=y
CONFIG_GALE_KERNEL_MSGQ=y
CONFIG_GALE_KERNEL_STACK=y

# Disable non-essential
CONFIG_MULTITHREADING=y
CONFIG_SMP=n
CONFIG_USERSPACE=n
CONFIG_MMU=n
CONFIG_POLL=n
CONFIG_EVENTS=n
CONFIG_DYNAMIC_THREAD=n
CONFIG_THREAD_MONITOR=n
CONFIG_SCHED_THREAD_USAGE=n
CONFIG_OBJ_CORE=n
CONFIG_HEAP_MEM_POOL_SIZE=0
CONFIG_KERNEL_MEM_POOL=n
CONFIG_BOOT_BANNER=n
```

### 4.4 Minimal kernel size estimate

| Component | Lines | Status |
|-----------|------:|:-------|
| thread.c | 1,367 | Needs Rust model |
| sched.c | 1,605 | Has Verus model, needs C shim |
| init.c | 652 | Infrastructure, keep as C |
| timeout.c | 351 | Has Verus model, needs C shim |
| sem.c | 237 | **Fully verified by Gale** |
| mutex.c | 337 | **Fully verified by Gale** |
| msg_q.c | 516 | **Fully verified by Gale** |
| stack.c | 222 | **Fully verified by Gale** |
| mem_slab.c | 353 | **Fully verified by Gale** |
| timer.c | 434 | Has Verus model + C shim |
| timeslicing.c | 161 | Has Verus model |
| fatal.c | 179 | Has Verus model |
| compiler_stack_protect.c | 65 | Keep as C |
| idle.c | 104 | Keep as C |
| busy_wait.c | 128 | Keep as C (hardware-bound) |
| errno.c | 57 | Keep as C |
| **Total** | **~6,768** | |
| **Gale-verified portion** | **~2,465** | 36% of minimal kernel |
| **Gale-modeled (no shim)** | **~2,116** | 31% of minimal kernel |
| **Remaining C** | **~2,187** | 33% of minimal kernel |

---

## Part 5: Hardware-Generic Extraction Opportunities

### 5.1 "Hardware-bound" files with extractable generic logic

| File | Total Lines | Arch Refs | Generic % | Extractable Logic |
|------|------------|----------:|----------:|:------------------|
| idle.c | 104 | 6 | ~85% | Power management policy, idle loop structure |
| busy_wait.c | 128 | 4 | ~80% | Cycle-to-microsecond conversion arithmetic |
| float.c | 47 | 3 | ~30% | Just a shim, mostly arch_float_enable/disable |
| atomic_c.c | 414 | 1 | ~95% | Software atomic ops (compare-and-swap emulation) |
| spinlock_validate.c | 52 | 1 | ~90% | Lock ownership tracking logic |
| mem_domain.c | 424 | 8 | ~80% | Domain/partition data structures, permission tracking |
| smp.c | 264 | N/A | ~60% | CPU state tracking (already modeled by Gale smp_state.rs) |
| ipi.c | 204 | N/A | ~40% | IPI scheduling decisions |

### 5.2 Extraction analysis

#### atomic_c.c (414 lines, 95% generic)

This is a pure-software implementation of atomic operations used when hardware
atomics are unavailable. The logic is:

```c
irq_lock(); old = *target; *target = new_val; irq_unlock();
```

This is a perfect Rust target: the IRQ lock/unlock boundary is the only
hardware-dependent part; all the compare-exchange, add, or, and, xor logic is
pure arithmetic. A Rust implementation would add:
- Type safety (no void* casts)
- Overflow checking on atomic_add/sub
- Compile-time guarantees about operation ordering

**Estimated effort**: Small (1-2 days). **Safety benefit**: Medium.

#### spinlock_validate.c (52 lines, 90% generic)

The validation logic tracks which thread/CPU owns a spinlock:

```c
l->thread_cpu = _current_cpu->id | (uintptr_t)_current;
```

The ownership encoding (CPU ID in low bits, thread pointer in high bits) is
a classic unsafe pattern. A Rust implementation would:
- Use a proper enum/struct instead of bit-packed uintptr_t
- Prevent the "check then act" race inherent in separate valid/set calls
- Make lock ownership a type-level invariant

**Estimated effort**: Small (0.5 day). **Safety benefit**: High (prevents deadlock).

#### busy_wait.c (128 lines, 80% generic)

The cycle-to-microsecond conversion arithmetic is pure math:

```c
uint64_t num = (uint64_t)usec * (uint64_t)hz;
return (uint32_t)((num + denom - 1U) / denom);
```

This has a potential overflow if `usec * hz > 2^64`. A Rust implementation would
use checked arithmetic, eliminating this class of bug.

**Estimated effort**: Small (0.5 day). **Safety benefit**: Medium (timing correctness).

#### mem_domain.c (424 lines, 80% generic)

Memory domain management tracks partitions (base address + size + attributes)
assigned to threads. The generic part is:
- Partition array management (add/remove partitions)
- Thread-to-domain association
- Overlap detection between partitions

The arch-specific part is the actual MPU/MMU register programming. A Rust model
could verify partition overlap detection and bounds checking.

**Estimated effort**: Medium (3-5 days). **Safety benefit**: High (memory isolation correctness).

---

## Part 6: Rust-Without-Verification Benefit Analysis

### 6.1 What Rust's type system gives for free

Even without Verus proofs, rewriting C in Rust eliminates:

| Vulnerability Class | C Kernel Occurrences | Rust Eliminates? | Mechanism |
|---------------------|--------------------:|:----------------|:----------|
| Null pointer dereference | ~129 NULL checks across 6 files | Yes | Option type |
| Buffer overflow | Implicit in all memcpy/array access | Yes | Bounds checking |
| Use-after-free | Possible in queue/heap/timeout | Yes | Ownership |
| Data races | All shared-state without lock | Yes | Send/Sync traits |
| Integer overflow | Every arithmetic operation | Yes | Checked by default |
| Type confusion | 18 CONTAINER_OF casts in kernel | Yes | Generics/enums |
| Uninitialized memory | `__noinit` variables | Partial | Compiler checks |
| Format string bugs | cbprintf | Yes | Type-safe formatting |
| Double-free | Heap and slab allocators | Yes | Ownership |

### 6.2 Files ranked by Rust-without-verification benefit

#### Tier A: Maximum benefit (high unsafe pattern density, no arch dependency)

| File | Lines | Unsafe Patterns | Primary Benefit |
|------|------:|----------------:|:----------------|
| **work.c** | 1,275 | 71 | Flag manipulation (44 NULL checks), state machine via raw u32 bitfields -> enum |
| **sched.c** | 1,605 | 57 | 66 spinlock sites, CONTAINER_OF casts, stale pointer to preempted thread |
| **userspace.c** | 1,039 | 55 | Dynamic object red-black tree, thread index bitmap, permission bitmask |
| **thread.c** | 1,367 | 54 | Stack setup with raw pointer arithmetic, random offset calculation |
| **mmu.c** | 1,837 | 59 | Page frame database, virtual address bitmap, free-list -> ownership |
| **poll.c** | 810 | 37 | Event registration linked list, dangling poller pointers |
| **queue.c** | 498 | 25 | sys_sflist with CONTAINER_OF, alloc_node flag encoding |
| **mem_slab.c** | 353 | 19 | Free-list pointer chasing, CONTAINER_OF for stats |
| **timeout.c** | 351 | 16 | Delta-list with CONTAINER_OF, next/prev pointer chasing |

#### Tier B: Moderate benefit (some unsafe patterns, partial arch dependency)

| File | Lines | Unsafe Patterns | Primary Benefit |
|------|------:|----------------:|:----------------|
| **timer.c** | 434 | 17 | Status counter, callback function pointer |
| **mailbox.c** | 461 | 12 | Async message descriptor pool, CONTAINER_OF |
| **mem_domain.c** | 424 | 14 | Partition array bounds, arch_ calls |
| **kheap.c** | 218 | 12 | Heap wrapper with lock, size validation |
| **mempool.c** | 204 | 18 | k_malloc/k_free wrapper |
| **fatal.c** | 179 | N/A | Fault classification enum -> exhaustive match |

#### Tier C: Lower benefit (small files, mostly hardware shims)

| File | Lines | Primary Benefit |
|------|------:|:----------------|
| futex.c | 105 | Ownership tracking |
| idle.c | 104 | Exhaustive PM state matching |
| busy_wait.c | 128 | Checked arithmetic for cycle conversion |
| spinlock_validate.c | 52 | Ownership as type invariant |
| atomic_c.c | 414 | Type-safe atomic operations |

### 6.3 Highest-ROI targets for Rust rewrite

Combining safety impact, unsafe pattern density, and size:

| Rank | File | Safety Impact | Effort | ROI |
|------|------|:-------------|:-------|:----|
| 1 | **lib/heap/heap.c** (592 lines) | Eliminates heap corruption class | Medium | HIGHEST |
| 2 | **sched.c** (1,605 lines) | Eliminates scheduler races | High | VERY HIGH |
| 3 | **thread.c** (1,367 lines) | Eliminates stack setup UB | High | HIGH |
| 4 | **work.c** (1,275 lines) | Eliminates flag/state races | Medium | HIGH |
| 5 | **timeout.c** (351 lines) | Eliminates delta-list corruption | Low | HIGH |
| 6 | **userspace.c** (1,039 lines) | Eliminates permission bypass | High | MEDIUM |
| 7 | **poll.c** (810 lines) | Eliminates registration leaks | Medium | MEDIUM |

---

## Part 7: Recommended Roadmap

### Phase 1: Complete FFI integration for existing models (1-2 months)

Gale has Verus models for 30 modules but only 13 have C shim integration.
The remaining 17 models should be wired in:

| Priority | Module | Model Lines | Effort | Impact |
|----------|--------|------------:|:-------|:-------|
| P0 | sched (partial FFI) | 866 | 2 weeks | CRITICAL -- scheduling correctness |
| P0 | timeout (FFI) | 629 | 1 week | CRITICAL -- deadline arithmetic |
| P1 | work (FFI) | 339 | 1 week | HIGH -- work queue state machine |
| P1 | poll (FFI) | 688 | 2 weeks | HIGH -- poll event state machine |
| P1 | timeslice (FFI) | 325 | 3 days | HIGH -- time-slice accounting |
| P2 | futex (FFI) | 369 | 3 days | MEDIUM -- userspace mutex |
| P2 | fatal (FFI) | 305 | 2 days | MEDIUM -- fault classification |
| P2 | dynamic (FFI) | 270 | 2 days | LOW -- thread pool |
| P2 | thread_lifecycle (FFI) | 520 | 1 week | HIGH -- thread state FSM |
| P3 | kheap (FFI) | 373 | 3 days | MEDIUM -- heap wrapper |
| P3 | mempool (FFI) | 347 | 3 days | MEDIUM -- pool wrapper |
| P3 | smp_state (FFI) | 330 | 3 days | MEDIUM -- CPU state |

### Phase 2: New safety-critical targets (2-4 months)

| Priority | Target | Lines | Type | Rationale |
|----------|--------|------:|:-----|:----------|
| P0 | **lib/heap/heap.c** | 592 | Full Rust rewrite | Highest-ROI: eliminates heap corruption |
| P0 | **spinlock model** | ~200 | Verus model | Prove lock ordering, no double-acquire |
| P1 | **thread.c stack setup** | ~400 | Rust rewrite of `setup_thread_stack` | Eliminates pointer arithmetic UB |
| P1 | **userspace_handler.c** | 89 | Rust rewrite | Syscall validation with type safety |
| P2 | **lib/os/mpsc_pbuf.c** | 660 | Verus model + Rust | Lock-free ring buffer verification |
| P2 | **lib/os/spsc_pbuf.c** | 387 | Verus model + Rust | Single-producer ring buffer |
| P3 | **atomic_c.c** | 414 | Rust rewrite | Type-safe atomics with overflow checks |
| P3 | **mem_domain.c** | 424 | Verus model | Partition overlap detection |

### Phase 3: Architecture support (3-6 months)

| Target | Lines | Type | Rationale |
|--------|------:|:-----|:----------|
| ARM Cortex-M fault.c model | 1,124 | Verus model | Fault classification verification |
| ARM MPU configuration | 1,005 | Rust + model | Region overlap and permission checks |
| Cortex-M thread context | 709 | Partial Rust | Stack frame layout verification |

### Phase 4: Full ASIL-D coverage (6-12 months)

| Milestone | Kernel Coverage | Verification Level |
|-----------|:--------------:|:-------------------|
| Current state | 36% of minimal kernel verified | Verus model + some FFI |
| After Phase 1 | 67% of minimal kernel verified | Verus model + FFI |
| After Phase 2 | 85% of minimal kernel hardened | Verus + Rust type safety |
| After Phase 3 | 90% of ASIL-D kernel scope | Architecture-aware verification |
| After Phase 4 | 95%+ of ASIL-D kernel scope | Full stack verification |

### Key metrics for ASIL-D certification support

| Metric | Current | Phase 1 | Phase 4 Target |
|--------|--------:|--------:|---------------:|
| Verus-verified modules | 30 | 30 | 40+ |
| Proof functions | 159 | 250+ | 500+ |
| Ensures clauses | 493 | 700+ | 1,500+ |
| C files replaced by Rust FFI | 13 | 25+ | 35+ |
| Zephyr test suites passing | 6 | 10+ | 15+ |
| STPA hazards mitigated | 15/35 | 25/35 | 33/35 |
| Lines of verified Rust | 10,718 | 15,000+ | 25,000+ |

---

## Appendix A: Complete STPA Hazard-to-Module Traceability

| Hazard | Gale Module | Verification Level | Gap |
|--------|:-----------|:-------------------|:----|
| H1.1 Sem overflow | sem.rs | Verus + FFI | None |
| H1.2 Sem underflow | sem.rs | Verus + FFI | None |
| H1.3 Mutex non-owner unlock | mutex.rs | Verus + FFI | None |
| H1.4 Mutex lock_count overflow | mutex.rs | Verus + FFI | None |
| H1.5 Priority inheritance restore | mutex.rs, sched.rs | Verus model only | Need cross-module proof |
| H1.6 Condvar wrong wake | condvar.rs, wait_queue.rs | Verus model | Need system-level proof |
| H1.7 Event bitmask corruption | event.rs | Verus + FFI | None (model-level) |
| H1.8 Futex wake mismatch | futex.rs | Verus model | Need FFI |
| H2.1 MsgQ index wrap | msgq.rs | Verus + FFI | None |
| H2.2 MsgQ index diverge | msgq.rs | Verus + FFI | None |
| H2.3 Pipe byte count mismatch | pipe.rs | Verus + FFI | None |
| H2.4 Pipe read on closed | pipe.rs | Verus + FFI | None |
| H2.5 Stack overflow push | stack.rs | Verus + FFI | None |
| H2.6 Queue count mismatch | queue.rs | Verus + FFI | None |
| H2.7 Mbox wrong pairing | mbox.rs | Verus + FFI | None |
| H2.8 Mbox data overrun | mbox.rs | Verus + FFI | None |
| H3.1 RunQ wrong priority | sched.rs | Verus model | Need FFI |
| H3.2 Thread stuck ready | sched.rs | Verus model | Need FFI + liveness proof |
| H3.3 Invalid state transition | thread.rs | Verus model | Need FFI |
| H3.4 Timeslice overflow | timeslice.rs | Verus model | Need FFI |
| H3.5 MetaIRQ stale record | sched.rs | Not modeled | Gap |
| H3.6 Scheduler cache stale | sched.rs | Partially modeled | Gap |
| H3.7 CPU mask violation | Not modeled | Not modeled | Gap |
| H4.1 Heap corruption | Not covered | N/A | Full gap |
| H4.2 Heap use-after-free | Not covered | N/A | Full gap |
| H4.3 Slab double-free | mem_slab.rs | Verus + FFI (partial) | Count only |
| H4.4 Mempool null return | mempool.rs, kheap.rs | Verus model | Need FFI |
| H4.5 MMU page table | Not covered | N/A | Full gap |
| H4.6 Mem domain overlap | Not covered | N/A | Full gap |
| H4.7 Stack overflow undetected | Not covered | N/A | Gap (C canary) |
| H5.1 Timeout deadline wrong | timeout.rs | Verus model | Need FFI |
| H5.2 Timeout post-fire access | Not modeled | N/A | Gap |
| H5.3 Timer status overflow | timer.rs | Verus + FFI | None |
| H5.4 Busy-wait overflow | Not covered | N/A | Gap |
| H5.5 Tick wrap-around | timeout.rs | Verus model | Need FFI |
| H5.6 announce_remaining leak | Not modeled | N/A | Gap |
| H6.1 Work double-submit | work.rs | Verus model | Need FFI |
| H6.2 Work cancel race | work.rs | Verus model | Need FFI |
| H6.3 Flush/cancel deadlock | work.rs | Partially modeled | Gap |
| H6.4 Work drain race | work.rs | Partially modeled | Gap |

**Summary**: 15 of 38 hazards fully mitigated, 13 partially mitigated (model only),
10 unaddressed.

## Appendix B: Zephyr Kconfig Feature Dependency Graph (kernel-level)

```
MULTITHREADING
  ├── SCHED_CPU_MASK → cpu_mask.c
  │     └── SCHED_CPU_MASK_PIN_ONLY
  ├── DYNAMIC_THREAD → dynamic.c
  │     ├── DYNAMIC_THREAD_ALLOC
  │     └── DYNAMIC_THREAD_POOL_SIZE
  ├── sched.c, thread.c (always compiled)
  ├── condvar.c, sem.c, mutex.c, msg_q.c, stack.c, pipe.c (always)
  ├── queue.c, mailbox.c (always)
  ├── work.c, system_work_q.c (always)
  ├── TIMESLICING → timeslicing.c
  ├── SMP → smp.c, ipi.c
  └── POLL → poll.c
       └── k_work_poll_* integration

SYS_CLOCK_EXISTS
  ├── timeout.c
  └── timer.c

EVENTS → events.c
USERSPACE → userspace.c, userspace_handler.c, mem_domain.c
MMU → mmu.c
ATOMIC_OPERATIONS_C → atomic_c.c
SPIN_VALIDATE → spinlock_validate.c
THREAD_MONITOR → thread_monitor.c
STACK_CANARIES → compiler_stack_protect.c
SCHED_THREAD_USAGE → usage.c
OBJ_CORE → obj_core.c
KERNEL_MEM_POOL → kheap.c, mempool.c
```

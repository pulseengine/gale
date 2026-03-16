# Zephyr Kernel C Coverage Analysis

Analysis date: 2026-03-16

## Part 1: All Zephyr Kernel C Files

| File | Lines | Gale Model | C Shim | Tests Pass | Category |
|------|------:|:----------:|:------:|:----------:|----------|
| sem.c | 238 | Yes | Yes | Yes (24/24) | Sync primitive |
| mutex.c | 338 | Yes | Yes | Yes (12/12) | Sync primitive |
| condvar.c | 172 | Yes | No (pure wrapper) | Yes (11/11) | Sync primitive |
| msg_q.c | 517 | Yes | Yes | Yes (13/13) | IPC |
| stack.c | 223 | Yes | Yes | Yes (12/12) | IPC |
| pipe.c | 359 | Yes | Yes | Yes (18/18) | IPC |
| timer.c | 435 | Yes | Yes | No | Time |
| events.c | 449 | Yes | Yes | No | Sync primitive |
| mem_slab.c | 354 | Yes | Yes | No | Memory |
| queue.c | 499 | Yes | Yes | No | IPC |
| mailbox.c | 462 | Yes | Yes | No | IPC |
| futex.c | 106 | Yes | No | No | Sync primitive |
| timeout.c | 352 | Yes | No | No | Time |
| poll.c | 811 | Yes | No | No | Async |
| sched.c | 1348 | Yes | No | No | Scheduler |
| thread.c | 1153 | No | No | No | Thread mgmt |
| userspace.c | 868 | No | No | No | Security |
| work.c | 1039 | No | No | No | Work queues |
| mmu.c | 1589 | No | No | No | Memory mgmt |
| mem_domain.c | 334 | No | No | No | Memory mgmt |
| smp.c | 212 | No | No | No | Multi-core |
| init.c | 552 | No | No | No | Boot |
| idle.c | 93 | No | No | No | Power mgmt |
| device.c | 226 | No | No | No | Device |
| obj_core.c | 228 | No | No | No | Debug |
| usage.c | 361 | No | No | No | Statistics |
| kheap.c | 170 | No | No | No | Memory |
| fatal.c | 157 | No | No | No | Error handling |
| mempool.c | 158 | No | No | No | Memory |
| ipi.c | 163 | No | No | No | Multi-core |
| dynamic.c | 145 | No | No | No | Thread mgmt |
| timeslicing.c | 137 | No | No | No | Scheduling |
| busy_wait.c | 109 | No | No | No | Time |
| thread_monitor.c | 94 | No | No | No | Debug |
| boot_args.c | 79 | No | No | No | Boot |
| userspace_handler.c | 74 | No | No | No | Security |
| nothread.c | 66 | No | No | No | Config |
| compiler_stack_protect.c | 58 | No | No | No | Security |
| cpu_mask.c | 56 | No | No | No | Multi-core |
| errno.c | 47 | No | No | No | Error handling |
| spinlock_validate.c | 45 | No | No | No | Debug |
| sys_clock_hw_cycles.c | 43 | No | No | No | Time |
| float.c | 42 | No | No | No | FPU |
| banner.c | 42 | No | No | No | Boot |
| system_work_q.c | 31 | No | No | No | Work queues |
| priority_queues.c | 25 | No | No | No | Scheduling |
| main_weak.c | 26 | No | No | No | Boot |
| paging/statistics.c | 215 | No | No | No | Memory mgmt |
| atomic_c.c | 341 | No | No | No | Atomics |
| irq_offload.c | 21 | No | No | No | Interrupts |
| dynamic_disabled.c | 19 | No | No | No | Config |
| version.c | 19 | No | No | No | Metadata |

## Part 2: Coverage Summary

### Lines with Gale verified models

| Kernel file | Lines | What Gale verifies |
|-------------|------:|---------------------|
| sem.c | 238 | Count/limit arithmetic, invariant 0<=count<=limit, give/take/reset state transitions |
| mutex.c | 338 | Ownership validation, lock_count arithmetic, recursive lock tracking |
| condvar.c | 172 | Wait queue semantics (pure wrapper, no FFI -- verified by composition) |
| msg_q.c | 517 | Ring buffer index arithmetic (read_idx, write_idx, used_msgs) |
| stack.c | 223 | Count/capacity arithmetic (next vs top bounds) |
| pipe.c | 359 | State machine (OPEN/CLOSED/RESETTING flags), byte count (used vs size) |
| timer.c | 435 | Status counter increment (overflow-safe), status read-reset |
| events.c | 449 | Bitmask operations (post, set, clear, set_masked, wait condition checks) |
| mem_slab.c | 354 | Block count tracking (num_used increment/decrement bounds) |
| queue.c | 499 | Queue counter arithmetic (append/prepend/get count tracking) |
| mailbox.c | 462 | Send validation, message matching, data exchange size computation |
| futex.c | 106 | Atomic compare-and-wait semantics, wake count tracking |
| timeout.c | 352 | Delta-tick list insertion arithmetic, remaining/expiry computation |
| poll.c | 811 | Event registration tracking, condition matching, signal state |
| sched.c | 1348 | Priority comparison, readyq insertion, pend/unpend state transitions |
| **Subtotal** | **6,663** | |

### Lines without Gale coverage

| Kernel file | Lines | Why not covered |
|-------------|------:|-----------------|
| thread.c | 1,153 | Thread lifecycle (create/join/abort) -- arch-specific stack setup |
| userspace.c | 868 | Syscall validation, MPU config -- arch/hardware dependent |
| work.c | 1,039 | Work queue scheduling -- complex callback chains |
| mmu.c | 1,589 | Page table manipulation -- hardware-specific |
| mem_domain.c | 334 | Memory protection regions -- hardware-specific |
| smp.c | 212 | IPI, CPU startup -- hardware-specific |
| init.c | 552 | Boot sequence -- hardware-specific |
| idle.c | 93 | Power management -- hardware-specific |
| device.c | 226 | Device model -- hardware-specific |
| obj_core.c | 228 | Debug/introspection object tracking |
| usage.c | 361 | Runtime statistics collection |
| kheap.c | 170 | Kernel heap wrapper (delegates to sys_heap) |
| fatal.c | 157 | Fault handling -- arch-specific |
| mempool.c | 158 | Memory pool wrapper (delegates to sys_heap) |
| ipi.c | 163 | Inter-processor interrupt -- hardware-specific |
| dynamic.c | 145 | Dynamic thread creation -- involves thread.c |
| timeslicing.c | 137 | Preemption time tracking -- arch/timer dependent |
| busy_wait.c | 109 | Spin-wait loops -- hardware timer dependent |
| thread_monitor.c | 94 | Debug thread tracking |
| boot_args.c | 79 | Boot arguments -- board-specific |
| userspace_handler.c | 74 | Syscall handler trampoline -- arch-specific |
| nothread.c | 66 | Threadless kernel config |
| compiler_stack_protect.c | 58 | Stack canary -- compiler/arch dependent |
| cpu_mask.c | 56 | CPU affinity mask -- SMP only |
| errno.c | 47 | TLS errno storage |
| spinlock_validate.c | 45 | Debug spinlock checking |
| sys_clock_hw_cycles.c | 43 | Hardware cycle counter -- arch-specific |
| float.c | 42 | FPU context save/restore -- arch-specific |
| banner.c | 42 | Boot banner printing |
| system_work_q.c | 31 | System work queue init |
| priority_queues.c | 25 | Priority queue implementation |
| main_weak.c | 26 | Weak main() symbol |
| paging/statistics.c | 215 | Demand paging stats -- MMU-specific |
| atomic_c.c | 341 | Software atomic fallbacks -- arch-specific |
| irq_offload.c | 21 | Test helper -- arch-specific |
| dynamic_disabled.c | 19 | Config stub |
| version.c | 19 | Build version string |
| **Subtotal** | **7,302** | |

### Totals

| Metric | Lines | Percentage |
|--------|------:|-----------:|
| Total kernel C code | 13,965 | 100.0% |
| Lines with Gale model | 6,663 | 47.7% |
| Lines without Gale model | 7,302 | 52.3% |

### Tested and verified end-to-end (C shim + Zephyr test suite passing)

| Metric | Lines | Percentage |
|--------|------:|-----------:|
| Verified + tested on qemu_cortex_m3 | 1,847 | 13.2% |
| Verified model only (no integration test) | 4,816 | 34.5% |
| No coverage | 7,302 | 52.3% |

The 1,847 lines with full integration = sem (238) + mutex (338) + condvar (172) + msg_q (517) + stack (223) + pipe (359).

## Part 3: Gale Verus Source Inventory

| Verus module | Lines (verus) | Lines (plain) | Maps to C file |
|--------------|-------------:|---------------:|----------------|
| sem.rs | 433 | 232 | sem.c |
| mutex.rs | 415 | 209 | mutex.c |
| condvar.rs | 259 | 157 | condvar.c |
| msgq.rs | 596 | 292 | msg_q.c |
| stack.rs | 266 | 118 | stack.c |
| pipe.rs | 361 | 187 | pipe.c |
| timer.rs | 260 | 126 | timer.c |
| event.rs | 228 | 109 | events.c |
| mem_slab.rs | 269 | 122 | mem_slab.c |
| queue.rs | 268 | 132 | queue.c |
| fifo.rs | 232 | 110 | queue.c (fifo portion) |
| lifo.rs | 236 | 115 | queue.c (lifo portion) |
| mbox.rs | 333 | 196 | mailbox.c |
| futex.rs | 341 | 242 | futex.c |
| timeout.rs | 588 | 265 | timeout.c |
| poll.rs | 621 | 327 | poll.c |
| sched.rs | 787 | 597 | sched.c |
| wait_queue.rs | 428 | -- | (shared infrastructure) |
| priority.rs | 95 | -- | (shared infrastructure) |
| thread.rs | 153 | -- | (shared infrastructure) |
| error.rs | 21 | -- | (shared infrastructure) |
| lib.rs | 54 | -- | (module root) |

Total Verus source: ~7,194 lines
Total plain (runtime) source: ~3,536 lines

## Part 4: What Gale Actually Verifies vs What Stays in C

Gale's approach is **arithmetic/state machine modeling** -- it verifies the mathematical properties of kernel object state, not the full C implementation. For each covered primitive:

### Fully replaced C files (C shim calls Rust FFI)

These 6 primitives have **passing Zephyr test suites**:

| Primitive | Verified in Rust | Stays in C (shim) |
|-----------|-----------------|-------------------|
| **sem** | Count/limit invariant, give/take/reset arithmetic | Spinlock, wait queue ops (z_unpend/z_pend_curr), tracing, poll events, userspace, obj_core |
| **mutex** | Ownership check, lock_count tracking | Spinlock, priority inheritance (adjust_owner_prio), wait queue, tracing, userspace |
| **condvar** | (No FFI) Verified by composition -- WaitQueue proofs cover condvar's wait_q ops | Entire condvar.c still compiles; Gale proves its wait queue operations are safe |
| **msgq** | Ring buffer index arithmetic (read_idx, write_idx, used_msgs, wrap-around) | Spinlock, memcpy, wait queue, tracing, poll, userspace, alloc_init |
| **stack** | Count vs capacity bounds (next vs top) | Spinlock, wait queue, data pointer management, tracing, userspace |
| **pipe** | State machine (OPEN/CLOSED/RESETTING flags), byte count (used vs size) | Ring buffer (ring_buf_put/get), spinlock, wait queues, direct-copy, tracing, userspace |

### Models exist but not yet integration-tested

| Primitive | Verified in Rust | Stays in C |
|-----------|-----------------|------------|
| **timer** | Status counter increment (overflow-safe), status read-reset | Timeout scheduling, expiry callbacks, period computation, tracing |
| **events** | Bitmask operations, wait condition matching | Wait queue walk, scheduling, tracing, userspace |
| **mem_slab** | Block count (num_used) bounds checking | Free-list pointer management, pointer validation, alloc/free, statistics |
| **queue/fifo/lifo** | Queue counter arithmetic | Linked list (sflist) management, alloc nodes, polling, tracing |
| **mbox** | Message matching, data size validation | Wait queues, async descriptors, data copy, scheduling |
| **futex** | Atomic compare semantics, wake count | k_object lookup, spinlock, wait queue |
| **timeout** | Delta-tick insertion arithmetic, remaining/expiry computation | sys_clock_announce loop, timer driver interface |
| **poll** | Event registration state tracking, condition matching | Spinlock, wait queue, triggered work, userspace validation |
| **sched** | Priority comparison, readyq state, pend/unpend transitions | Actual context switch, spinlock, SMP, timeslicing, thread state |

### Verification depth estimate

For the 6 fully tested primitives, roughly:
- **30-50%** of each C file's logic is verified (arithmetic, state transitions, invariants)
- **50-70%** stays in C (spinlocks, wait queue manipulation, scheduling calls, tracing, userspace validation, poll integration)

The verification targets the **highest-risk code**: the arithmetic that could overflow, the state machines that could enter invalid states, and the invariants that guard data integrity. The C code that remains is mostly "plumbing" -- calling Zephyr internal APIs (z_pend_curr, z_unpend_first_thread, z_reschedule) that are shared kernel infrastructure.

## Part 5: Next Targets Ranked by Safety Impact

### Tier 1: High safety impact, good fit for Gale's approach

| File | Lines | Rationale |
|------|------:|-----------|
| timeslicing.c | 137 | Time-slice accounting arithmetic; bounded integer tracking |
| priority_queues.c | 25 | Priority queue ordering invariants; small and self-contained |
| kheap.c | 170 | Heap alloc/free count tracking; bounded arithmetic |

### Tier 2: High safety impact, partial fit

| File | Lines | Rationale |
|------|------:|-----------|
| thread.c | 1,153 | Thread state machine is verifiable; stack setup is arch-specific |
| work.c | 1,039 | Work queue state machine; callback dispatch stays in C |
| fatal.c | 157 | Fault classification logic; arch-specific handlers stay in C |

### Tier 3: Important but hardware-dependent

| File | Lines | Rationale |
|------|------:|-----------|
| mmu.c | 1,589 | Page table arithmetic could be modeled; but address space layout is arch-specific |
| userspace.c | 868 | Permission checks could be verified; MPU config is hardware |
| mem_domain.c | 334 | Partition arithmetic; hardware region config stays in C |
| smp.c | 212 | CPU state tracking could be modeled; IPI is hardware |
| init.c | 552 | Init ordering could be verified; hardware init stays in C |

### Not verifiable with Gale's approach

| File | Lines | Why |
|------|------:|-----|
| atomic_c.c | 341 | Software atomic fallbacks -- correctness depends on compiler barriers and hardware memory model |
| idle.c | 93 | Power state management -- hardware-specific (WFI/WFE instructions) |
| float.c | 42 | FPU context save/restore -- pure register manipulation |
| busy_wait.c | 109 | Hardware timer polling loops |
| compiler_stack_protect.c | 58 | Stack canary checking -- compiler/linker dependent |
| irq_offload.c | 21 | Interrupt injection -- pure hardware |
| sys_clock_hw_cycles.c | 43 | Hardware cycle counter reading |
| boot_args.c | 79 | Board-specific boot parameter passing |
| userspace_handler.c | 74 | Architecture-specific syscall trampoline |

## Part 6: Immediate Priority -- Complete Integration Testing

The biggest ROI is getting the existing models through integration testing:

1. **timer** (435 lines) -- Verus model + C shim exist; needs test suite run
2. **events** (449 lines) -- Verus model + C shim exist; needs test suite run
3. **mem_slab** (354 lines) -- Verus model + C shim exist; needs test suite run
4. **queue/fifo/lifo** (499 lines) -- Verus model + C shim exist; needs test suite run
5. **mailbox** (462 lines) -- Verus model + C shim exist; needs test suite run

Completing these would bring verified+tested coverage from 1,847 to 4,046 lines (29.0% of kernel).

Models without C shims (futex, timeout, poll, sched) total 2,617 lines. These require
writing C shims before integration testing, and sched.c (1,348 lines) is the most complex
single file in the kernel.

# Gale Kernel Coverage

Comprehensive accounting of every `.c` file in `zephyr/kernel/` and its Gale status.

Total kernel source files: 51 (excluding `dynamic_disabled.c`, which is the
`!CONFIG_DYNAMIC_THREAD` fallback for `dynamic.c`).

## Status Legend

| Status | Meaning |
|--------|---------|
| REPLACED | Gale C shim replaces upstream; CONFIG guard excludes original |
| HELPERS | Gale provides verified decision helpers; upstream .c still compiled |
| EXCLUDED | Assessed and intentionally excluded from Gale scope |

## File Coverage Table

| # | File | Lines | Status | Detail |
|---|------|------:|--------|--------|
| 1 | `sem.c` | 237 | REPLACED | `CONFIG_GALE_KERNEL_SEM` — verified semaphore state machine |
| 2 | `mutex.c` | 337 | REPLACED | `CONFIG_GALE_KERNEL_MUTEX` — verified ownership + lock_count |
| 3 | `msg_q.c` | 516 | REPLACED | `CONFIG_GALE_KERNEL_MSGQ` — verified ring buffer index arithmetic |
| 4 | `stack.c` | 222 | REPLACED | `CONFIG_GALE_KERNEL_STACK` — verified LIFO count/capacity |
| 5 | `pipe.c` | 358 | REPLACED | `CONFIG_GALE_KERNEL_PIPE` — verified state machine + byte count |
| 6 | `events.c` | 448 | REPLACED | `CONFIG_GALE_KERNEL_EVENT` — verified bitmask operations |
| 7 | `poll.c` | 810 | REPLACED | `CONFIG_GALE_KERNEL_POLL` — verified event state machine |
| 8 | `futex.c` | 105 | REPLACED | `CONFIG_GALE_KERNEL_FUTEX` — verified value comparison + wake count |
| 9 | `timeslicing.c` | 161 | REPLACED | `CONFIG_GALE_KERNEL_TIMESLICE` — verified tick accounting |
| 10 | `kheap.c` | 218 | REPLACED | `CONFIG_GALE_KERNEL_KHEAP` — verified byte-level allocation tracking |
| 11 | `fatal.c` | 179 | REPLACED | `CONFIG_GALE_KERNEL_FATAL` — verified error classification |
| 12 | `mempool.c` | 204 | REPLACED | `CONFIG_GALE_KERNEL_MEMPOOL` — verified block pool allocation |
| 13 | `dynamic.c` | 175 | REPLACED | `CONFIG_GALE_KERNEL_DYNAMIC` — verified thread pool tracking |
| 14 | `smp.c` | 264 | REPLACED | `CONFIG_GALE_KERNEL_SMP_STATE` — verified CPU start/stop count |
| 15 | `sched.c` | 1605 | REPLACED | `CONFIG_GALE_KERNEL_SCHED` — verified next_up + preemption |
| 16 | `thread.c` | 1367 | REPLACED | `CONFIG_GALE_KERNEL_THREAD_LIFECYCLE` — verified create/exit counting |
| 17 | `mem_slab.c` | 353 | REPLACED | `CONFIG_GALE_KERNEL_MEM_SLAB` — verified block count tracking |
| 18 | `queue.c` | 498 | REPLACED | `CONFIG_GALE_KERNEL_QUEUE` — verified unbounded queue counter |
| 19 | `mailbox.c` | 461 | REPLACED | `CONFIG_GALE_KERNEL_MBOX` — verified send validation + ID matching |
| 20 | `condvar.c` | 171 | HELPERS | Verified by composition (pure wait queue wrapper, no FFI) |
| 21 | `work.c` | 1275 | HELPERS | 2 decision helpers (submit/cancel state flags); upstream compiled |
| 22 | `timeout.c` | 351 | HELPERS | Verified tick arithmetic; upstream compiled (defines z_abort_timeout) |
| 23 | `timer.c` | 434 | HELPERS | Verified status counter; upstream compiled (defines sys_clock_announce) |
| 24 | `banner.c` | 48 | EXCLUDED | Prints startup text, no kernel state, no safety impact |
| 25 | `version.c` | 21 | EXCLUDED | Returns compile-time constant KERNELVERSION, no logic |
| 26 | `main_weak.c` | 30 | EXCLUDED | Weak default main() stub, linker fallback only |
| 27 | `errno.c` | 57 | EXCLUDED | Per-thread errno accessor, no kernel state mutation |
| 28 | `float.c` | 47 | EXCLUDED | FPU context save/restore, delegates to arch_float_*, no logic |
| 29 | `busy_wait.c` | 128 | EXCLUDED | Cycle-counting spin loop, hardware-dependent, no kernel state |
| 30 | `boot_args.c` | 94 | EXCLUDED | Bootargs string parser, pre-kernel init only, no safety impact |
| 31 | `compiler_stack_protect.c` | 65 | EXCLUDED | Stack canary handler + guard variable, compiler infrastructure |
| 32 | `irq_offload.c` | 23 | EXCLUDED | Semaphore-guarded arch_irq_offload wrapper, test infrastructure |
| 33 | `sys_clock_hw_cycles.c` | 52 | EXCLUDED | Hardware cycles-per-sec variable + weak update function, driver glue |
| 34 | `nothread.c` | 83 | EXCLUDED | Single-threaded fallback (k_sleep/k_is_in_isr), !MULTITHREADING only |
| 35 | `usage.c` | 484 | EXCLUDED | Thread runtime stats (cycle counting), monitoring/debug only |
| 36 | `obj_core.c` | 287 | EXCLUDED | Object core framework (linked list + stats), debug/introspection only |
| 37 | `thread_monitor.c` | 110 | EXCLUDED | Thread list iteration (k_thread_foreach), debug/monitoring only |
| 38 | `spinlock_validate.c` | 52 | EXCLUDED | Debug spinlock validation (CONFIG_SPIN_VALIDATE), debug-only |
| 39 | `device.c` | 277 | EXCLUDED | Device model init/lookup, no kernel primitive state |
| 40 | `init.c` | 652 | EXCLUDED | Kernel boot sequence orchestration, calls into all subsystems |
| 41 | `idle.c` | 104 | EXCLUDED | Idle thread loop, power management glue, no kernel state |
| 42 | `system_work_q.c` | 38 | EXCLUDED | Defines single system workqueue thread, trivial init |
| 43 | `priority_queues.c` | 30 | EXCLUDED | Red-black tree wrappers for CONFIG_SCHED_SCALABLE, data structure glue |
| 44 | `ipi.c` | 204 | EXCLUDED | Inter-processor interrupt signaling, SMP hardware glue |
| 45 | `atomic_c.c` | 414 | EXCLUDED | Software atomic operations fallback, compiler/arch intrinsics |
| 46 | `mmu.c` | 1837 | EXCLUDED | Memory management unit, arch-specific page table management |
| 47 | `mem_domain.c` | 424 | EXCLUDED | Memory domain/partition management, userspace infrastructure |
| 48 | `userspace.c` | 1039 | EXCLUDED | Userspace syscall handling + object permission framework |
| 49 | `userspace_handler.c` | 89 | EXCLUDED | Userspace handler stubs, syscall verification helpers |
| 50 | `cpu_mask.c` | 72 | EXCLUDED | CPU affinity mask get/set, SMP scheduling attribute only |
| 51 | `paging/statistics.c` | 251 | EXCLUDED | Demand paging statistics counters, monitoring only |

## Exclusion Rationale Summary

### Trivial / Boilerplate (files 24-34)

These files contain no safety-critical kernel state. They are startup
boilerplate, compiler infrastructure, hardware abstraction glue, or test
support. Replacing them would add zero safety value.

| File | Rationale |
|------|-----------|
| `banner.c` | Prints startup text, no kernel state, no safety impact |
| `version.c` | Returns compile-time constant, pure accessor |
| `main_weak.c` | Weak default main(), linker fallback only |
| `errno.c` | Per-thread errno accessor, standard C runtime glue |
| `float.c` | FPU enable/disable delegates entirely to arch layer |
| `busy_wait.c` | Hardware cycle-counting spin loop, no kernel state mutation |
| `boot_args.c` | Parses bootargs string at pre-kernel init, no runtime impact |
| `compiler_stack_protect.c` | Compiler-generated canary check, calls z_except_reason on fail |
| `irq_offload.c` | Test helper wrapping arch_irq_offload with a semaphore |
| `sys_clock_hw_cycles.c` | Stores hw cycles-per-sec, weak update for runtime frequency changes |
| `nothread.c` | Fallback k_sleep/k_is_in_isr for single-threaded (!MULTITHREADING) configs |

### Monitoring / Debug (files 35-38)

These files are compiled only under optional debug/monitoring Kconfig
options. They observe kernel state but do not participate in scheduling,
synchronization, or resource management decisions.

| File | Rationale |
|------|-----------|
| `usage.c` | Thread/CPU cycle accounting for runtime stats; read-only observation |
| `obj_core.c` | Object core registry (linked list + stats); introspection framework |
| `thread_monitor.c` | Thread list walk (k_thread_foreach); debug iteration only |
| `spinlock_validate.c` | Debug-only spinlock owner tracking (CONFIG_SPIN_VALIDATE) |

### Infrastructure / Platform (files 39-51)

These files provide kernel infrastructure (boot, device model, memory
management, userspace) that operates below or outside the kernel primitive
layer that Gale targets.

| File | Rationale |
|------|-----------|
| `device.c` | Device model init, lookup, and power management |
| `init.c` | Kernel boot orchestration, calls all subsystem init |
| `idle.c` | Idle thread loop and power management hooks |
| `system_work_q.c` | System workqueue thread creation (3 lines of init code) |
| `priority_queues.c` | Red-black tree node wrappers for scalable run queues |
| `ipi.c` | Inter-processor interrupt signaling for SMP |
| `atomic_c.c` | Software fallback for atomic operations (compiler intrinsic replacement) |
| `mmu.c` | Page table management, virtual memory mapping |
| `mem_domain.c` | Memory domain/partition management for userspace isolation |
| `userspace.c` | Syscall dispatch, object permission tables, userspace framework |
| `userspace_handler.c` | Syscall verification handler stubs |
| `cpu_mask.c` | CPU affinity bitmask for SMP thread pinning |
| `paging/statistics.c` | Demand paging hit/miss/eviction counters |

## Coverage Summary

| Category | Files | Lines |
|----------|------:|------:|
| REPLACED (CONFIG guard active) | 19 | 8,518 |
| HELPERS (verified decision logic) | 4 | 2,231 |
| **Total Gale-covered** | **23** | **10,749** |
| EXCLUDED (trivial/boilerplate) | 11 | 648 |
| EXCLUDED (monitoring/debug) | 4 | 933 |
| EXCLUDED (infrastructure/platform) | 13 | 5,431 |
| **Total excluded** | **28** | **7,012** |
| **Grand total** | **51** | **17,761** |

**Coverage: 23 / 51 files = 45% by file count, 10,749 / 17,761 lines = 61% by line count.**

All 28 excluded files have been individually assessed and confirmed to
contain no safety-critical kernel primitive logic. They fall into three
categories: trivial boilerplate (no state), debug/monitoring (read-only
observation), or platform infrastructure (below the kernel primitive layer).

The 23 Gale-covered files account for all kernel synchronization primitives
(semaphore, mutex, condvar, message queue, stack, pipe, event, poll, futex),
all resource management (memory slab, kheap, mempool, queue, mailbox, work),
and all scheduling/lifecycle (scheduler, thread, timer, timeout, timeslice,
fatal, dynamic, SMP state).

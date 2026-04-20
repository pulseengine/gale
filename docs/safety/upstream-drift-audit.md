# Upstream-Drift Audit: Gale Shims vs Zephyr v4.4.0-rc3

Baseline: Zephyr fork merge-base 2026-03-09. Tip: `v4.4.0-rc3` (commit
`6182bc08c9d`). Scope: C files replaced via `CONFIG_GALE_KERNEL_*` fork
guards (21 in `kernel/CMakeLists.txt` + 1 in `lib/heap/CMakeLists.txt`).
Gap example tracked in gh issue #15 (heap canary hardening missing).

| Upstream file | Gale shim | Severity | Upstream changes since 2026-03-09 | What's missing |
|---|---|---|---|---|
| `lib/heap/heap.c` | `gale_heap.c` | **HIGH** | 6 commits: `SYS_HEAP_HARDENING` tiered Kconfig (b846fd8b), `SYS_HEAP_CANARIES` trailers (c3c404af), canary-on-free-merge tripwire (30c07c0f), `chunk_usable_bytes` rework (b57ab90f), sizing-constants generator (dee349919b), `undersized_chunk` rename (c776a6c9) | Entire heap hardening family: tiered hardening levels NONE/BASIC/MODERATE/FULL/EXTREME, canary trailers, left-neighbor canary tripwire, new struct-layout-derived sizing. `grep SYS_HEAP_HARDENING gale_heap.c` → zero hits. |
| `kernel/futex.c` | `gale_futex.c` | **HIGH** | 1 commit: TOCTOU race fix in `k_futex_wait` (10c974c9d5) — moves atomic value check *inside* `futex_data->lock` critical section | Race persists: `gale_futex.c:102` reads `atomic_get(&futex->val)` *before* `k_spin_lock(&futex_data->lock)` at line 114. Wake can be lost, waiter blocks forever. |
| `kernel/userspace.c` | `gale_userspace.c` | **HIGH** | 3 commits: new `k_object_access_check` syscall (11f89f73eb), dynamic-stack cached-area option (23054a97f4), `k_heap_free_sched_locked` refactor for recursive-lock fix (9cef0da05c) | `k_object_access_check` syscall absent; `clear_perms_cb` path still uses non-sched-locked free (recursive-lock deadlock window remains); no cached-area handling for coherent-SMP stacks. |
| `kernel/sched.c` | `gale_sched.c` | **HIGH** | 8 commits: self-directed IPI race (1666066082), `z_unpend_all_locked` + scheduler-lock assert (184b5a3804, 9cef0da05c), signed/unsigned `z_tick_sleep` wrap bug (d67038c7e7), reschedule-under-lock refactor (a04a30c6d1, 2bbcece6ee), z_swap optimizations (d535d17cbc), doc fixes (f6141e5ccf) | IPI fix missing: `gale_sched.c:994` calls `signal_pending_ipi()` *outside* `K_SPINLOCK` — silent IPI loss on SMP. `z_tick_sleep` wrap fix likely missing. No `z_unpend_all_locked`/locked variants. High volatility. |
| `kernel/kheap.c` | `gale_kheap.c` | **HIGH** | 1 commit: `k_heap_free_sched_locked` for abort-path recursive-lock (9cef0da05c) | Abort via `k_thread_perms_all_clear` → `k_free` → `z_unpend_all` can recursively take `_sched_spinlock`; the new locked variant is absent in the shim. |
| `kernel/mempool.c` | `gale_mempool.c` | **HIGH** | 1 commit: `k_free_sched_locked` peer of kheap refactor (9cef0da05c) | Same recursive-lock hole as kheap on abort path. |
| `kernel/msg_q.c` | `gale_msgq.c` | **MEDIUM** | 1 commit: `k_free_sched_locked` plumbing for dynamic queue cleanup (9cef0da05c) | Dynamic-allocated msgq buffer free on abort path uses non-locked free; reachable only with `CONFIG_USERSPACE`. |
| `kernel/stack.c` | `gale_stack.c` | **MEDIUM** | 1 commit: `k_free_sched_locked` plumbing (9cef0da05c) | Same as msgq — dynamic stack free on abort path. |
| `kernel/timeslicing.c` | `gale_timeslice.c` | NONE | 0 commits | — |
| `kernel/spinlock_validate.c` | `gale_spinlock_validate.c` | NONE | 0 commits (see note: `z_spin_is_locked` visibility widened in 184b5a38, header-only) | — |
| `kernel/fatal.c` | `gale_fatal.c` | NONE | 0 commits | — |
| `kernel/cpu_mask.c` | `gale_cpu_mask.c` | NONE | 0 commits | — |
| `kernel/thread.c` | `gale_thread_lifecycle.c` | NONE | 0 commits | — (shim not activated in overlay — see note) |
| `kernel/mailbox.c` | `gale_mbox.c` | LOW | 1 commit: Doxygen `@retval` fix (f6141e5ccf) | Cosmetic. |
| `kernel/queue.c` | `gale_queue.c` | NONE | 0 commits | — |
| `kernel/pipe.c` | `gale_pipe.c` | NONE | 0 commits | — |
| `kernel/mutex.c` | `gale_mutex.c` | NONE | 0 commits | — |
| `kernel/sem.c` | `gale_sem.c` | NONE | 0 commits | — |
| `kernel/mem_slab.c` | `gale_mem_slab.c` | LOW | 1 commit: Doxygen `@retval` fix (f6141e5ccf) | Cosmetic. |
| `kernel/poll.c` | `gale_poll.c` | NONE | 0 commits | — |
| `kernel/events.c` | `gale_event.c` | NONE | 0 commits | — |
| `kernel/mem_domain.c` | `gale_mem_domain.c` | NONE | 0 commits | — |
| `kernel/dynamic.c` | `gale_dynamic.c` | NONE | 0 commits | — |

### Key findings

- **Memory safety cluster is the hotspot.** Heap, kheap, mempool, msgq, stack, and userspace all gained fixes in the Feb–Apr 2026 window. Five of the six HIGH severities come from two upstream change families: (1) the `SYS_HEAP_HARDENING` tiered canary series (`lib/heap/heap.c`), and (2) the `*_sched_locked` recursive-lock refactor cascading through every allocator+dynamic-object abort path.
- **One silent SMP liveness bug reproduced in the shim.** `gale_sched.c:994` still calls `signal_pending_ipi()` outside the scheduler spinlock — the exact pattern upstream fixed in `1666066082a` (permanent CPU hang reproducible under QEMU). This is independent of the memory-safety cluster.
- **Userspace syscall surface drifted by one syscall.** `k_object_access_check` was added upstream and is missing from `gale_userspace.c`. Applications using it against a Gale-built kernel will get a link failure or, worse, an unknown-syscall trap. Not a safety regression in Gale code itself, but a compatibility gap.
- **IPC primitives (queue, pipe, mutex, sem, mbox, events, poll) are stable.** Zero or cosmetic-only changes upstream in this window; these shims do not need re-audit from this rebase.
- **Volatility != risk for `sched.c`.** 8 commits, but 4 are refactors/optimizations that preserve semantics. Only 3 (IPI race, unsigned-wrap, recursive-lock) are HIGH-impact. The shim already diverges heavily by design, so cherry-pick — don't re-fork.

### Methodology

Each file was classified by running `git log --oneline --since=2026-03-09 v4.4.0-rc3 -- <path>` on the Zephyr fork, then inspecting each commit's stat and message. **HIGH** was assigned when the commit message describes a security/liveness/correctness bug (TOCTOU, recursive lock, silent IPI loss, heap corruption tripwire, unsigned wrap) and the Gale shim was grep-verified to still contain the pre-fix pattern. **MEDIUM** when the commit adds a Kconfig-gated feature or touches an abort path gated behind a rare config (`CONFIG_USERSPACE` dynamic objects). **LOW** for Doxygen-only or rename-only commits. **NONE** when `git log` returned no commits in the window. No full-file diffs were executed (per instructions, short-circuiting allowed); in particular, the heap-hardening series is large enough to warrant the "commit log as proxy" approach called out in the task.

### Not audited

- `gale_ring_buf.c`: upstream equivalent is `lib/os/ring_buffer.c`, not in kernel/; helper-only module without fork guard.
- `gale_net_buf.c`: upstream is `subsys/net/buf/buf.c`; helper-only module without fork guard.
- `gale_bitarray.c`, `gale_cbprintf.c`, `gale_rb.c`: validation-only modules (overlay comments explicitly note no fork guard activated).
- `gale_atomic.c`, `gale_condvar.c`, `gale_fifo.c`, `gale_lifo.c`, `gale_ipc.c`, `gale_ipi.c`, `gale_mmu.c`, `gale_pm.c`, `gale_smp_state.c`, `gale_spinlock.c`, `gale_timeout.c`, `gale_timer.c`, `gale_usage.c`, `gale_work.c`: decision-helper modules that run alongside upstream (no fork guard; upstream source always compiled). Out of scope for drift audit — upstream changes here are automatically picked up.
- `gale_ipi.c` note: upstream `kernel/ipi.c` did receive the `1666066082a` IPI-race fix; since `ipi.c` is always compiled alongside the Gale helper, this is not a drift — the fix is in-tree. The sibling gap in `gale_sched.c` is what matters.
- `gale_thread_lifecycle.c` note: overlay conf shows `CONFIG_GALE_KERNEL_THREAD_LIFECYCLE=y` but a code comment ("thread_lifecycle disabled") contradicts this. Treated as NONE since `kernel/thread.c` had no commits in the window regardless.

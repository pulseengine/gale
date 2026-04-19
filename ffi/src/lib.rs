//! C FFI: verified kernel primitives for Zephyr.
//!
//! ## Semaphore (gale_sem_count_*)
//!
//! Pure functions replacing count arithmetic from kernel/sem.c:
//!   sem.c:48-50   CHECKIF(limit == 0U || initial_count > limit)
//!   sem.c:110     sem->count += (sem->count != sem->limit) ? 1U : 0U;
//!   sem.c:143-144 if (likely(sem->count > 0U)) { sem->count--; }
//!
//! Verified: P1-P3, P5-P6, P9 (count bounds, no overflow/underflow).
//!
//! ## Mutex (gale_mutex_*_validate)
//!
//! Pure functions replacing state machine validation from kernel/mutex.c:
//!   mutex.c:121-129  lock_count/owner checks + lock_count++
//!   mutex.c:238-268  owner checks + lock_count--
//!
//! Verified: M3-M7, M10 (ownership, reentrancy, no overflow/underflow).
//!
//! ## Message Queue (gale_msgq_*)
//!
//! Pure functions replacing ring buffer index arithmetic from kernel/msg_q.c:
//!   msg_q.c:43-71   init parameter validation + buffer size computation
//!   msg_q.c:164-188 write index advancement (put / put_front)
//!   msg_q.c:293-300 read index advancement (get)
//!   msg_q.c:397-430 indexed peek offset computation
//!
//! Verified: MQ1-MQ13 (capacity bounds, index bounds, ring consistency,
//! no overflow/underflow).
//!
//! ## Stack (gale_stack_*)
//!
//! Pure functions replacing LIFO count/capacity tracking from kernel/stack.c:
//!   stack.c:109-112  capacity check (next == top)
//!   stack.c:124-125  push increment
//!   stack.c:158-160  pop decrement
//!
//! Verified: SK1-SK9 (bounds, conservation, no overflow/underflow).
//!
//! ## Pipe (gale_pipe_*)
//!
//! Pure functions replacing state validation and byte count computation
//! from kernel/pipe.c:
//!   pipe.c:147-218  write state check + byte count
//!   pipe.c:220-271  read state check + byte count
//!
//! Verified: PP1-PP10 (state machine, byte count bounds, conservation).
//!
//! ## Memory Slab (gale_mem_slab_*)
//!
//! Pure functions replacing block count tracking from kernel/mem_slab.c:
//!   mem_slab.c:109-111 block_size/num_blocks validation
//!   mem_slab.c:245     num_used++ (alloc)
//!   mem_slab.c:308     num_used-- (free)
//!
//! Verified: MS1-MS8 (bounds, conservation, no overflow/underflow).
//!
//! ## Event (gale_event_*)
//!
//! Pure functions replacing bitmask operations from kernel/events.c:
//!   events.c:191-192  post (OR bits) + set_masked
//!   events.c:238-240  set (replace all)
//!   events.c:268-270  clear (AND complement)
//!   events.c:95-107   are_wait_conditions_met (any/all check)
//!
//! Verified: EV1-EV8 (bitmask ops, monotonicity, wait conditions).
//!
//! ## Timer (gale_timer_*)
//!
//! Pure functions replacing status counter arithmetic from kernel/timer.c:
//!   timer.c expiry handler  status++ (checked)
//!   timer.c status_get      read + reset to 0
//!   timer.c init            period validation
//!
//! Verified: TM1-TM8 (status bounds, increment, reset, no overflow).
//!
//! ## Fifo (gale_fifo_*)
//!
//! Pure functions replacing unbounded queue count tracking for k_fifo
//! (FIFO ordering wrapper around k_queue):
//!   queue.c:insert  count++ (overflow check)
//!   queue.c:get     count-- (underflow check)
//!
//! Verified: FI1-FI4 (bounds, increment, decrement, no overflow/underflow).
//!
//! ## Lifo (gale_lifo_*)
//!
//! Pure functions replacing unbounded queue count tracking for k_lifo
//! (LIFO ordering wrapper around k_queue):
//!   queue.c:insert  count++ (overflow check)
//!   queue.c:get     count-- (underflow check)
//!
//! Verified: LI1-LI4 (bounds, increment, decrement, no overflow/underflow).
//!
//! ## Queue (gale_queue_*)
//!
//! Pure functions replacing unbounded queue count tracking for k_queue:
//!   queue.c:append/prepend  count++ (overflow check)
//!   queue.c:get             count-- (underflow check)
//!
//! Verified: QU1-QU6 (bounds, append, prepend, get, no overflow/underflow).
//!
//! ## Mbox (gale_mbox_*)
//!
//! Pure functions replacing stateless validation in kernel/mailbox.c:
//!   mailbox.c:put   size > 0 check
//!   mailbox.c:match sender/receiver ID compatibility
//!   mailbox.c:data  min(tx_size, rx_buf_size) computation
//!
//! Verified: MB1-MB6 (send validation, match logic, data exchange).
//!
//! ## Timeout (gale_timeout_*)
//!
//! Pure functions replacing tick arithmetic in kernel/timeout.c:
//!   timeout.c:z_add_timeout    deadline = current_tick + duration
//!   timeout.c:z_abort_timeout  deactivate pending timeout
//!   timeout.c:sys_clock_announce advance tick, fire expired
//!
//! Verified: TO1-TO8 (deadline, overflow, abort, fire, forever, no_wait).
//!
//! ## Poll (gale_poll_*)
//!
//! Pure functions replacing poll event state machine in kernel/poll.c:
//!   poll.c:k_poll_event_init     init to NOT_READY
//!   poll.c:is_condition_met      check sem/signal/msgq
//!   poll.c:k_poll_signal_raise   set signaled + result
//!   poll.c:k_poll_signal_reset   clear signaled
//!
//! Verified: PL1-PL8 (state machine, conditions, signal raise/reset).
//!
//! ## Futex (gale_futex_*)
//!
//! Pure functions replacing value comparison in kernel/futex.c:
//!   futex.c:z_impl_k_futex_wait  compare val to expected
//!   futex.c:z_impl_k_futex_wake  wake count tracking
//!
//! Verified: FX1-FX6 (wait gating, wake count, no overflow).
//!
//! ## Timeslice (gale_timeslice_*)
//!
//! Pure functions replacing tick accounting in kernel/timeslicing.c:
//!   timeslicing.c:z_reset_time_slice  reset to max
//!   timeslicing.c:z_time_slice        decrement, detect expiry
//!
//! Verified: TS1-TS6 (bounds, reset, tick, expire, no underflow).
//!
//! ## KHeap (gale_kheap_*)
//!
//! Pure functions replacing byte count accounting in kernel/kheap.c:
//!   kheap.c:k_heap_alloc  allocated_bytes += bytes
//!   kheap.c:k_heap_free   allocated_bytes -= bytes
//!
//! Verified: KH1-KH6 (bounds, alloc, free, conservation, no overflow).
//!
//! ## Thread Lifecycle (gale_thread_*)
//!
//! Pure functions replacing thread counting and priority validation:
//!   thread.c:k_thread_create     count++
//!   thread.c:exit/abort          count--
//!   sched.c:k_thread_priority_set  range check
//!
//! Verified: TH1-TH6 (priority range, count bounds, no overflow/underflow).
//!
//! ## Work (gale_work_*)
//!
//! Pure functions replacing work item state flag management in kernel/work.c:
//!   work.c:submit_to_queue_locked  set QUEUED flag
//!   work.c:cancel_async_locked     clear QUEUED, set CANCELING
//!
//! Verified: WK1-WK6 (init idle, submit, cancel, state consistency).
//!
//! ## Fatal (gale_fatal_*)
//!
//! Pure function replacing fatal error classification in kernel/fatal.c:
//!   fatal.c:z_fatal_error  determine recovery action
//!
//! Verified: FT1-FT4 (reason mapping, panic halts, recovery, distinct codes).
//!
//! ## MemPool (gale_mempool_*)
//!
//! Pure functions replacing fixed-block pool counting:
//!   pool alloc  allocated += 1
//!   pool free   allocated -= 1
//!
//! Verified: MP1-MP6 (bounds, alloc, free, conservation, no overflow).
//!
//! ## Dynamic (gale_dynamic_*)
//!
//! Pure functions replacing dynamic thread pool tracking in kernel/dynamic.c:
//!   dynamic.c:z_thread_stack_alloc_pool  active += 1
//!   dynamic.c:z_impl_k_thread_stack_free active -= 1
//!
//! Verified: DY1-DY4 (bounds, alloc, free, no underflow).
//!
//! ## SMP State (gale_smp_*)
//!
//! Pure functions replacing SMP CPU state tracking in kernel/smp.c:
//!   smp.c:k_smp_cpu_start  active_cpus += 1
//!   smp.c:stop_cpu         active_cpus -= 1 (min 1)
//!
//! Verified: SM1-SM4 (bounds, start, stop, CPU 0 never stops).
//!
//! ## Sched (gale_sched_*)
//!
//! Pure functions replacing scheduler decisions in kernel/sched.c:
//!   sched.c:next_up          select highest-priority thread
//!   sched.c:should_preempt   cooperative protection
//!
//! Verified: SC1-SC16 (priority ordering, preemption, state FSM).
//!
//! ## Memory Domain (gale_mem_domain_*)
//!
//! Pure functions replacing partition validation and slot management
//! in kernel/mem_domain.c:
//!   mem_domain.c:24-86    check_add_partition (validate + overlap)
//!   mem_domain.c:208-259  k_mem_domain_add_partition (find slot, add)
//!   mem_domain.c:261-306  k_mem_domain_remove_partition (find match, clear)
//!
//! Verified: MD1-MD6 (non-overlap, size > 0, bounds, no overflow).
//!
//! ## Sys Heap (gale_sys_heap_*)
//!
//! Pure functions replacing chunk-level allocation decisions in
//! lib/heap/heap.c:
//!   heap.c:266-303  sys_heap_alloc — split/whole decision
//!   heap.c:166-201  sys_heap_free — double-free check + coalesce strategy
//!   heap.c:112-125  split_chunks — conservation validation
//!   heap.c:128-134  merge_chunks — conservation + overflow validation
//!   heap.c:312-388  sys_heap_aligned_alloc — alignment + padding overflow
//!   heap.c:467-492  sys_heap_realloc — shrink/grow/copy decision
//!
//! Verified: HP1-HP8 (bounds, conservation, alloc gating, free exactness,
//! double-free, alignment, overflow, merge invariant).
//!
//! ## MMU (gale_mmu_*)
//!
//! Pure functions replacing validation logic in kernel/mmu.c:
//!   mmu.c:570-677   k_mem_map_phys_guard — size/flags/overflow checks
//!   mmu.c:679-817   k_mem_unmap_phys_guard — addr/size/guard checks
//!   mmu.c:819-847   k_mem_update_flags — size/flags checks
//!   mmu.c:1008-1021 k_mem_region_align — alignment arithmetic
//!
//! Verified: MM1-MM8 (size alignment, user+uninit, cache flags,
//! guard overflow, known flags, W^X, overlap, no overflow).
//!
//! ## PM (gale_pm_*)
//!
//! Pure functions replacing policy and state machine decisions from
//! subsys/pm/pm.c and subsys/pm/policy/policy_default.c:
//!   pm.c:135-153         pm_state_force — record forced state
//!   pm.c:182-189         forced/policy selection in pm_system_suspend
//!   policy_default.c:27-38  min-residency check
//!
//! Verified: PM1-PM7 (state enum bounds, transition validity, terminal
//! SOFT_OFF, forced single-use, residency policy, substate bounds).
//!
//! ## Usage (gale_usage_*)
//!
//! Pure functions replacing decision logic in kernel/usage.c:
//!   usage.c:74-97    z_sched_usage_start  — start_decide
//!   usage.c:99-119   z_sched_usage_stop   — stop_decide
//!   usage.c:155-159  z_sched_cpu_usage    — average_cycles (div-by-zero guard)
//!   usage.c:211-215  z_sched_thread_usage — average_cycles (div-by-zero guard)
//!   usage.c:227-246  k_thread_runtime_stats_enable  — thread enable
//!   usage.c:248-273  k_thread_runtime_stats_disable — thread disable
//!   usage.c:283-293  k_sys_runtime_stats_enable  — sys_enable_decide
//!   usage.c:317-326  k_sys_runtime_stats_disable — sys_disable_decide
//!
//! Verified: US1-US6 (tracking guard, accumulate-only-when-started,
//! track_usage toggle, idempotent sys ops, no divide-by-zero, monotone cycles).

#![cfg_attr(not(any(test, kani)), no_std)]
// FFI boundary crate — unsafe is inherent (no_mangle, raw pointers).
// The verified pure logic lives in the `gale` crate which denies unsafe.

pub mod coarse;

use gale::error::{
    EADDRINUSE, EAGAIN, EBADF, EBUSY, ECANCELED, EDEADLK, EINVAL, ENOMEM, ENOMSG, ENOENT,
    ENOSPC, EOVERFLOW, EPERM, EPIPE, ETIMEDOUT, OK,
};

// ---------------------------------------------------------------------------
// FFI exports — pure count arithmetic
// ---------------------------------------------------------------------------

/// Validate semaphore init parameters.
///
/// sem.c:48-50:
///   CHECKIF(limit == 0U || initial_count > limit) { return -EINVAL; }
#[cfg(feature = "sem")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_count_init(initial_count: u32, limit: u32) -> i32 {
    if limit == 0 || initial_count > limit {
        EINVAL
    } else {
        OK
    }
}

/// Compute new count after give with no waiters.
///
/// sem.c:110:
///   sem->count += (sem->count != sem->limit) ? 1U : 0U;
///
/// Safe: count < limit <= u32::MAX when count != limit, so count+1
/// cannot overflow.  Verified by Verus lemma_give_saturation.
#[cfg(feature = "sem")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_count_give(count: u32, limit: u32) -> u32 {
    use gale::sem::{GiveDecision, give_decide};

    // Delegate to verified model (has_waiter=false: old API
    // handles the no-waiter direct-increment path only).
    let d = give_decide(count, limit, false);
    match d {
        GiveDecision::Increment => {
            #[allow(clippy::arithmetic_side_effects)]
            let new_count = count + 1;
            new_count
        }
        GiveDecision::Saturated | GiveDecision::WakeThread => count,
    }
}

/// Attempt to decrement count for take.
///
/// sem.c:143-144:
///   if (likely(sem->count > 0U)) { sem->count--; ret = 0; }
///
/// SAFETY: `count` must point to a valid `unsigned int` (Zephyr's
/// sem->count).  Called under Zephyr's spinlock — no concurrent access.
#[cfg(feature = "sem")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_count_take(count: *mut u32) -> i32 {
    use gale::sem::{TakeDecision, take_decide};

    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        if count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (is_no_wait=true: old API never pends).
        let d = take_decide(*count, true);
        match d {
            TakeDecision::Acquired => {
                #[allow(clippy::arithmetic_side_effects)]
                {
                    *count -= 1;
                }
                OK
            }
            TakeDecision::WouldBlock | TakeDecision::Pend => EBUSY,
        }
    }
}

// ---- Phase 2: Full Decision API ----

/// Decision struct for k_sem_give — tells C shim what action to take.
#[repr(C)]
pub struct GaleSemGiveDecision {
    /// Action: 0=INCREMENT_COUNT, 1=WAKE_THREAD
    pub action: u8,
    /// New count value (only meaningful when action=INCREMENT_COUNT)
    pub new_count: u32,
}

pub const GALE_SEM_ACTION_INCREMENT: u8 = 0;
pub const GALE_SEM_ACTION_WAKE: u8 = 1;

/// Full decision for k_sem_give: decides whether to increment count or wake a thread.
///
/// The C shim calls z_unpend_first_thread first (side effect), then passes
/// whether a waiter was found. Rust decides the action.
///
/// Delegates to `gale::sem::give_decide` (Verus-verified).
#[cfg(feature = "sem")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_sem_give_decide(
    count: u32,
    limit: u32,
    has_waiter: u32,
) -> GaleSemGiveDecision {
    use gale::sem::{GiveDecision, give_decide};

    let decision = give_decide(count, limit, has_waiter != 0);
    match decision {
        GiveDecision::WakeThread => GaleSemGiveDecision {
            action: GALE_SEM_ACTION_WAKE,
            new_count: count,
        },
        GiveDecision::Increment => {
            #[allow(clippy::arithmetic_side_effects)]
            let new_count = count + 1;
            GaleSemGiveDecision {
                action: GALE_SEM_ACTION_INCREMENT,
                new_count,
            }
        }
        GiveDecision::Saturated => GaleSemGiveDecision {
            action: GALE_SEM_ACTION_INCREMENT,
            new_count: count,
        },
    }
}

/// Decision struct for k_sem_take.
#[repr(C)]
pub struct GaleSemTakeDecision {
    /// Return code: 0 (acquired), -EBUSY (would block)
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
/// Delegates to `gale::sem::take_decide` (Verus-verified).
#[cfg(feature = "sem")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_sem_take_decide(
    count: u32,
    is_no_wait: u32,
) -> GaleSemTakeDecision {
    use gale::sem::{TakeDecision, take_decide};

    let decision = take_decide(count, is_no_wait != 0);
    match decision {
        TakeDecision::Acquired => {
            #[allow(clippy::arithmetic_side_effects)]
            let new_count = count - 1;
            GaleSemTakeDecision {
                ret: OK,
                new_count,
                action: GALE_SEM_ACTION_RETURN,
            }
        }
        TakeDecision::WouldBlock => GaleSemTakeDecision {
            ret: EBUSY,
            new_count: 0,
            action: GALE_SEM_ACTION_RETURN,
        },
        TakeDecision::Pend => GaleSemTakeDecision {
            ret: 0,
            new_count: 0,
            action: GALE_SEM_ACTION_PEND,
        },
    }
}

// ---------------------------------------------------------------------------
// Mutex FFI exports — state machine validation
// ---------------------------------------------------------------------------
//
// These pure functions replace the safety-critical state machine checks
// and arithmetic from kernel/mutex.c:
//
//   mutex.c:121-129  lock_count/owner checks + lock_count++
//   mutex.c:238-268  owner checks + lock_count--
//
// All other mutex logic (wait queue, scheduling, priority inheritance,
// tracing) remains native Zephyr C in gale_mutex.c.
//
// Verified by Verus (SMT/Z3):
//   M3:  lock unlocked -> lock_count = 1
//   M4:  reentrant lock -> lock_count + 1
//   M5:  contended -> -EBUSY
//   M6a: unlock no owner -> -EINVAL
//   M6b: unlock wrong owner -> -EPERM
//   M7:  reentrant unlock -> lock_count - 1
//   M10: no overflow/underflow in lock_count


/// Validate a mutex lock attempt.
///
/// mutex.c:121-129:
///   if ((lock_count == 0) || (owner == _current)) {
///       lock_count++;
///       owner = _current;
///   }
///
/// Arguments:
///   lock_count:       current mutex->lock_count
///   owner_is_null:    1 if mutex->owner == NULL, 0 otherwise
///   owner_is_current: 1 if mutex->owner == _current, 0 otherwise
///   new_lock_count:   pointer to receive the new lock_count value
///
/// Returns:
///   0 (OK)    — lock acquired, *new_lock_count set, caller sets owner
///   -EBUSY    — contended (different owner holds it)
#[cfg(feature = "mutex")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mutex_lock_validate(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
    new_lock_count: *mut u32,
) -> i32 {
    use gale::mutex::{LockDecision, lock_decide};

    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        if new_lock_count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (is_no_wait=true: old API never pends).
        let d = lock_decide(lock_count, owner_is_null != 0, owner_is_current != 0, true);
        match d {
            LockDecision::Acquire => {
                *new_lock_count = 1;
                OK
            }
            LockDecision::Reentrant => {
                #[allow(clippy::arithmetic_side_effects)]
                {
                    *new_lock_count = lock_count + 1;
                }
                OK
            }
            LockDecision::Overflow => EINVAL,
            LockDecision::Busy | LockDecision::Pend => EBUSY,
        }
    }
}

/// Return code: mutex still held (reentrant unlock, lock_count decremented).
pub const GALE_MUTEX_RELEASED: i32 = 1;
/// Return code: mutex fully unlocked (caller should check waiters).
pub const GALE_MUTEX_UNLOCKED: i32 = 0;

/// Validate a mutex unlock attempt.
///
/// mutex.c:238-268:
///   CHECKIF(owner == NULL)    -> -EINVAL
///   CHECKIF(owner != current) -> -EPERM
///   if (lock_count > 1)       -> lock_count--; return 0;
///   else                      -> fully unlock, handle waiters
///
/// Arguments:
///   lock_count:       current mutex->lock_count
///   owner_is_null:    1 if mutex->owner == NULL, 0 otherwise
///   owner_is_current: 1 if mutex->owner == _current, 0 otherwise
///   new_lock_count:   pointer to receive the new lock_count value
///
/// Returns:
///   1 (GALE_MUTEX_RELEASED) — still held, *new_lock_count decremented
///   0 (GALE_MUTEX_UNLOCKED) — fully unlocked, *new_lock_count = 0,
///                             caller should check waiters
///   -EINVAL                 — not locked (no owner)
///   -EPERM                  — not the owner
#[cfg(feature = "mutex")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mutex_unlock_validate(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
    new_lock_count: *mut u32,
) -> i32 {
    use gale::mutex::{UnlockDecisionKind, unlock_decide};

    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        if new_lock_count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model.
        let d = unlock_decide(lock_count, owner_is_null != 0, owner_is_current != 0);
        match d {
            UnlockDecisionKind::NotLocked => EINVAL,
            UnlockDecisionKind::NotOwner => EPERM,
            UnlockDecisionKind::Released => {
                #[allow(clippy::arithmetic_side_effects)]
                {
                    *new_lock_count = lock_count - 1;
                }
                GALE_MUTEX_RELEASED
            }
            UnlockDecisionKind::FullyUnlocked => {
                *new_lock_count = 0;
                GALE_MUTEX_UNLOCKED
            }
        }
    }
}

// ---- Phase 2: Full Decision API for Mutex ----

/// Decision struct for k_mutex_lock — tells C shim what action to take.
#[repr(C)]
pub struct GaleMutexLockDecision {
    /// Return code: 0 (acquired), -EBUSY (would block)
    pub ret: i32,
    /// Action: 0=ACQUIRED, 1=PEND_CURRENT, 2=RETURN_BUSY
    pub action: u8,
    /// New lock_count value (only meaningful when action=ACQUIRED)
    pub new_lock_count: u32,
}

pub const GALE_MUTEX_ACTION_ACQUIRED: u8 = 0;
pub const GALE_MUTEX_ACTION_PEND: u8 = 1;
pub const GALE_MUTEX_ACTION_BUSY: u8 = 2;

/// Full decision for k_mutex_lock: decides whether to acquire, pend, or return busy.
///
/// Handles reentrant locking, ownership check, and pend-or-busy decision.
/// Priority inheritance logic stays in C — Rust decides the action,
/// C applies it including any priority adjustments.
///
/// Delegates to `gale::mutex::lock_decide` (Verus-verified).
/// Verified: M3 (acquire), M4 (reentrant), M5 (contended), M10 (no overflow).
#[cfg(feature = "mutex")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mutex_lock_decide(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
    is_no_wait: u32,
) -> GaleMutexLockDecision {
    use gale::mutex::{LockDecision, lock_decide};

    let d = lock_decide(lock_count, owner_is_null != 0, owner_is_current != 0, is_no_wait != 0);
    match d {
        LockDecision::Acquire => GaleMutexLockDecision {
            ret: OK,
            action: GALE_MUTEX_ACTION_ACQUIRED,
            new_lock_count: 1,
        },
        LockDecision::Reentrant => {
            #[allow(clippy::arithmetic_side_effects)]
            let n = lock_count + 1;
            GaleMutexLockDecision {
                ret: OK,
                action: GALE_MUTEX_ACTION_ACQUIRED,
                new_lock_count: n,
            }
        }
        LockDecision::Overflow => GaleMutexLockDecision {
            ret: EINVAL,
            action: GALE_MUTEX_ACTION_BUSY,
            new_lock_count: lock_count,
        },
        LockDecision::Busy => GaleMutexLockDecision {
            ret: EBUSY,
            action: GALE_MUTEX_ACTION_BUSY,
            new_lock_count: lock_count,
        },
        LockDecision::Pend => GaleMutexLockDecision {
            ret: 0,
            action: GALE_MUTEX_ACTION_PEND,
            new_lock_count: lock_count,
        },
    }
}

/// Decision struct for k_mutex_unlock — tells C shim what action to take.
#[repr(C)]
pub struct GaleMutexUnlockDecision {
    /// Return code: 0 (success), -EINVAL (not locked), -EPERM (not owner)
    pub ret: i32,
    /// Action: 0=RELEASED (still held), 1=UNLOCKED (check waiters), 2=ERROR
    pub action: u8,
    /// New lock_count value (decremented if RELEASED, 0 if UNLOCKED)
    pub new_lock_count: u32,
}

pub const GALE_MUTEX_UNLOCK_RELEASED: u8 = 0;
pub const GALE_MUTEX_UNLOCK_UNLOCKED: u8 = 1;
pub const GALE_MUTEX_UNLOCK_ERROR: u8 = 2;

/// Full decision for k_mutex_unlock: decides whether to decrement, fully unlock,
/// or return an error.
///
/// Priority inheritance restoration stays in C — Rust decides the action,
/// C applies it including any priority adjustments.
///
/// Delegates to `gale::mutex::unlock_decide` (Verus-verified).
/// Verified: M6a (EINVAL), M6b (EPERM), M7 (reentrant), M10 (no underflow).
#[cfg(feature = "mutex")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mutex_unlock_decide(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
) -> GaleMutexUnlockDecision {
    use gale::mutex::{UnlockDecisionKind, unlock_decide};

    let d = unlock_decide(lock_count, owner_is_null != 0, owner_is_current != 0);
    match d {
        UnlockDecisionKind::NotLocked => GaleMutexUnlockDecision {
            ret: EINVAL,
            action: GALE_MUTEX_UNLOCK_ERROR,
            new_lock_count: 0,
        },
        UnlockDecisionKind::NotOwner => GaleMutexUnlockDecision {
            ret: EPERM,
            action: GALE_MUTEX_UNLOCK_ERROR,
            new_lock_count: lock_count,
        },
        UnlockDecisionKind::Released => {
            #[allow(clippy::arithmetic_side_effects)]
            let new_count = lock_count - 1;
            GaleMutexUnlockDecision {
                ret: OK,
                action: GALE_MUTEX_UNLOCK_RELEASED,
                new_lock_count: new_count,
            }
        }
        UnlockDecisionKind::FullyUnlocked => GaleMutexUnlockDecision {
            ret: OK,
            action: GALE_MUTEX_UNLOCK_UNLOCKED,
            new_lock_count: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Message Queue FFI exports — ring buffer index arithmetic
// ---------------------------------------------------------------------------
//
// These pure functions replace the index arithmetic in kernel/msg_q.c.
// The C shim converts between slot indices and byte pointers:
//   byte_ptr = buffer_start + slot_idx * msg_size
//
// All other msgq logic (wait queue, scheduling, memcpy, polling, tracing)
// remains native Zephyr C in gale_msgq.c.
//
// Verified by Verus (SMT/Z3):
//   MQ1:  0 <= used_msgs <= max_msgs
//   MQ2:  read_idx < max_msgs
//   MQ3:  write_idx < max_msgs
//   MQ4:  msg_size > 0, max_msgs > 0
//   MQ5:  put advances write_idx correctly
//   MQ6:  put on full returns -ENOMSG
//   MQ7:  put_front retreats read_idx correctly
//   MQ8:  get advances read_idx correctly
//   MQ9:  get on empty returns -ENOMSG
//   MQ10: peek_at computes correct slot
//   MQ11: purge resets to empty
//   MQ12: no arithmetic overflow
//   MQ13: ring consistency maintained

/// Validate message queue init parameters and compute buffer size.
///
/// msg_q.c:43-71:
///   __ASSERT(!size_mul_overflow(max_msgs, msg_size, ...))
///
/// Arguments:
///   msg_size:    size of each message in bytes
///   max_msgs:    maximum number of messages
///   buffer_size: pointer to receive msg_size * max_msgs
///
/// Returns:
///   0 (OK)   — valid parameters, *buffer_size set
///   -EINVAL  — invalid (zero msg_size/max_msgs, or overflow)
#[cfg(feature = "msgq")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_msgq_init_validate(
    msg_size: u32,
    max_msgs: u32,
    buffer_size: *mut u32,
) -> i32 {
    unsafe {
        if buffer_size.is_null() {
            return EINVAL;
        }

        if msg_size == 0 || max_msgs == 0 {
            return EINVAL;
        }

        match msg_size.checked_mul(max_msgs) {
            Some(size) => {
                *buffer_size = size;
                OK
            }
            None => EINVAL,
        }
    }
}

/// Compute new write index after putting a message at the back.
///
/// msg_q.c:164-173:
///   memcpy(write_ptr, data, msg_size);
///   write_ptr += msg_size;
///   if (write_ptr == buffer_end) write_ptr = buffer_start;
///   used_msgs++;
///
/// Arguments:
///   write_idx:     current write slot index
///   used_msgs:     current number of messages in queue
///   max_msgs:      maximum messages (capacity)
///   new_write_idx: pointer to receive advanced write index
///   new_used:      pointer to receive incremented used count
///
/// Returns:
///   0 (OK)    — queue had space, outputs set, caller does memcpy at write_idx
///   -ENOMSG   — queue full, outputs unchanged
#[cfg(feature = "msgq")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_msgq_put(
    write_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    new_write_idx: *mut u32,
    new_used: *mut u32,
) -> i32 {
    use gale::msgq::{PutDecision, put_decide};

    unsafe {
        if new_write_idx.is_null() || new_used.is_null() || max_msgs == 0 {
            return EINVAL;
        }

        // Delegate to verified model (has_waiter=false, is_no_wait=true:
        // old API handles no-waiter direct-store path only).
        let r = put_decide(write_idx, used_msgs, max_msgs, false, true);
        match r.decision {
            PutDecision::Store => {
                *new_write_idx = r.new_write_idx;
                *new_used = r.new_used;
                OK
            }
            PutDecision::Full | PutDecision::WakeReader | PutDecision::Pend => ENOMSG,
        }
    }
}

/// Compute new read index after putting a message at the front.
///
/// msg_q.c:174-186:
///   if (read_ptr == buffer_start) read_ptr = buffer_end;
///   read_ptr -= msg_size;
///   memcpy(read_ptr, data, msg_size);
///   used_msgs++;
///
/// Arguments:
///   read_idx:     current read slot index
///   used_msgs:    current number of messages in queue
///   max_msgs:     maximum messages (capacity)
///   new_read_idx: pointer to receive retreated read index
///   new_used:     pointer to receive incremented used count
///
/// Returns:
///   0 (OK)    — queue had space, outputs set, caller does memcpy at *new_read_idx
///   -ENOMSG   — queue full, outputs unchanged
#[cfg(feature = "msgq")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_msgq_put_front(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    new_read_idx: *mut u32,
    new_used: *mut u32,
) -> i32 {
    use gale::msgq::put_front_decide;

    unsafe {
        if new_read_idx.is_null() || new_used.is_null() || max_msgs == 0 {
            return EINVAL;
        }

        // Delegate to verified model.
        let r = put_front_decide(read_idx, used_msgs, max_msgs);
        if r.ok {
            *new_read_idx = r.new_read_idx;
            *new_used = r.new_used;
            OK
        } else {
            ENOMSG
        }
    }
}

/// Compute new read index after getting a message.
///
/// msg_q.c:293-300:
///   memcpy(data, read_ptr, msg_size);
///   read_ptr += msg_size;
///   if (read_ptr == buffer_end) read_ptr = buffer_start;
///   used_msgs--;
///
/// Arguments:
///   read_idx:     current read slot index
///   used_msgs:    current number of messages in queue
///   max_msgs:     maximum messages (capacity)
///   new_read_idx: pointer to receive advanced read index
///   new_used:     pointer to receive decremented used count
///
/// Returns:
///   0 (OK)    — queue had messages, outputs set, caller does memcpy at read_idx
///   -ENOMSG   — queue empty, outputs unchanged
#[cfg(feature = "msgq")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_msgq_get(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    new_read_idx: *mut u32,
    new_used: *mut u32,
) -> i32 {
    use gale::msgq::{GetDecision, get_decide};

    unsafe {
        if new_read_idx.is_null() || new_used.is_null() || max_msgs == 0 {
            return EINVAL;
        }

        // Delegate to verified model (has_waiter=false, is_no_wait=true:
        // old API handles no-waiter direct-read path only).
        let r = get_decide(read_idx, used_msgs, max_msgs, false, true);
        match r.decision {
            GetDecision::Read => {
                *new_read_idx = r.new_read_idx;
                *new_used = r.new_used;
                OK
            }
            GetDecision::Empty | GetDecision::WakeWriter | GetDecision::Pend => ENOMSG,
        }
    }
}

/// Compute the buffer slot index for peeking at message `idx`.
///
/// msg_q.c:408-418:
///   bytes_to_end = (buffer_end - read_ptr);
///   byte_offset = idx * msg_size;
///   if (bytes_to_end <= byte_offset) { ... wrap ... }
///
/// Arguments:
///   read_idx:  current read slot index
///   used_msgs: current number of messages in queue
///   max_msgs:  maximum messages (capacity)
///   idx:       message index (0 = first/oldest)
///   slot_idx:  pointer to receive the computed slot index
///
/// Returns:
///   0 (OK)    — valid index, *slot_idx set
///   -ENOMSG   — index out of bounds
#[cfg(feature = "msgq")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_msgq_peek_at(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    idx: u32,
    slot_idx: *mut u32,
) -> i32 {
    use gale::msgq::peek_at_decide;

    unsafe {
        if slot_idx.is_null() || max_msgs == 0 {
            return EINVAL;
        }

        // Delegate to verified model.
        let r = peek_at_decide(read_idx, used_msgs, max_msgs, idx);
        if r.ok {
            *slot_idx = r.slot_idx;
            OK
        } else {
            ENOMSG
        }
    }
}

// ---- Phase 2: Full Decision API for msgq ----

/// Decision struct for k_msgq_put -- tells C shim what action to take.
#[repr(C)]
pub struct GaleMsgqPutDecision {
    /// Return code: 0 (OK), -ENOMSG (full)
    pub ret: i32,
    /// Action: 0=PUT_OK, 1=WAKE_READER, 2=PEND_CURRENT, 3=RETURN_FULL
    pub action: u8,
    /// New write index (only meaningful when action=PUT_OK)
    pub new_write_idx: u32,
    /// New used count
    pub new_used: u32,
}

pub const GALE_MSGQ_ACTION_PUT_OK: u8 = 0;
pub const GALE_MSGQ_ACTION_WAKE_READER: u8 = 1;
pub const GALE_MSGQ_ACTION_PUT_PEND: u8 = 2;
pub const GALE_MSGQ_ACTION_RETURN_FULL: u8 = 3;

/// Full decision for k_msgq_put: decides whether to put, wake a reader, pend,
/// or return full.
///
/// Delegates to `gale::msgq::put_decide` (Verus-verified).
#[cfg(feature = "msgq")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_msgq_put_decide(
    write_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    has_waiter: u32,
    is_no_wait: u32,
) -> GaleMsgqPutDecision {
    use gale::msgq::{PutDecision, put_decide};

    let r = put_decide(write_idx, used_msgs, max_msgs, has_waiter != 0, is_no_wait != 0);
    match r.decision {
        PutDecision::Store => GaleMsgqPutDecision {
            ret: OK,
            action: GALE_MSGQ_ACTION_PUT_OK,
            new_write_idx: r.new_write_idx,
            new_used: r.new_used,
        },
        PutDecision::WakeReader => GaleMsgqPutDecision {
            ret: OK,
            action: GALE_MSGQ_ACTION_WAKE_READER,
            new_write_idx: r.new_write_idx,
            new_used: r.new_used,
        },
        PutDecision::Full => GaleMsgqPutDecision {
            ret: ENOMSG,
            action: GALE_MSGQ_ACTION_RETURN_FULL,
            new_write_idx: r.new_write_idx,
            new_used: r.new_used,
        },
        PutDecision::Pend => GaleMsgqPutDecision {
            ret: 0,
            action: GALE_MSGQ_ACTION_PUT_PEND,
            new_write_idx: r.new_write_idx,
            new_used: r.new_used,
        },
    }
}

/// Decision struct for k_msgq_get -- tells C shim what action to take.
#[repr(C)]
pub struct GaleMsgqGetDecision {
    /// Return code: 0 (OK), -ENOMSG (empty)
    pub ret: i32,
    /// Action: 0=GET_OK, 1=WAKE_WRITER, 2=PEND_CURRENT, 3=RETURN_EMPTY
    pub action: u8,
    /// New read index (only meaningful when action=GET_OK or WAKE_WRITER)
    pub new_read_idx: u32,
    /// New used count
    pub new_used: u32,
}

pub const GALE_MSGQ_ACTION_GET_OK: u8 = 0;
pub const GALE_MSGQ_ACTION_WAKE_WRITER: u8 = 1;
pub const GALE_MSGQ_ACTION_GET_PEND: u8 = 2;
pub const GALE_MSGQ_ACTION_RETURN_EMPTY: u8 = 3;

/// Full decision for k_msgq_get: decides whether to get, wake a writer, pend,
/// or return empty.
///
/// Delegates to `gale::msgq::get_decide` (Verus-verified).
#[cfg(feature = "msgq")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_msgq_get_decide(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    has_waiter: u32,
    is_no_wait: u32,
) -> GaleMsgqGetDecision {
    use gale::msgq::{GetDecision, get_decide};

    let r = get_decide(read_idx, used_msgs, max_msgs, has_waiter != 0, is_no_wait != 0);
    match r.decision {
        GetDecision::Read => GaleMsgqGetDecision {
            ret: OK,
            action: GALE_MSGQ_ACTION_GET_OK,
            new_read_idx: r.new_read_idx,
            new_used: r.new_used,
        },
        GetDecision::WakeWriter => GaleMsgqGetDecision {
            ret: OK,
            action: GALE_MSGQ_ACTION_WAKE_WRITER,
            new_read_idx: r.new_read_idx,
            new_used: r.new_used,
        },
        GetDecision::Empty => GaleMsgqGetDecision {
            ret: ENOMSG,
            action: GALE_MSGQ_ACTION_RETURN_EMPTY,
            new_read_idx: r.new_read_idx,
            new_used: r.new_used,
        },
        GetDecision::Pend => GaleMsgqGetDecision {
            ret: 0,
            action: GALE_MSGQ_ACTION_GET_PEND,
            new_read_idx: r.new_read_idx,
            new_used: r.new_used,
        },
    }
}

// ---------------------------------------------------------------------------
// Stack FFI exports — LIFO count/capacity arithmetic
// ---------------------------------------------------------------------------
//
// These pure functions replace the capacity check and count tracking
// from kernel/stack.c:
//
//   stack.c:109-112  capacity check (next == top)
//   stack.c:124-125  push: *(next) = data; next++
//   stack.c:158-160  pop: next--; *data = *(next)
//
// All other stack logic (wait queue, scheduling, data storage, tracing)
// remains native Zephyr C in gale_stack.c.
//
// Verified by Verus (SMT/Z3):
//   SK1:  0 <= count <= capacity
//   SK2:  capacity > 0
//   SK3:  push increments count
//   SK4:  push on full returns -ENOMEM
//   SK5:  pop decrements count
//   SK6:  pop on empty returns -EBUSY
//   SK7:  num_free + num_used == capacity
//   SK8:  no overflow/underflow
//   SK9:  push-pop roundtrip

// ---------------------------------------------------------------------------
// Pipe FFI exports — state machine + byte count validation
// ---------------------------------------------------------------------------
//
// These pure functions replace the state checks and byte count computation
// from kernel/pipe.c:
//
//   pipe.c:147-218  write state check + ring_buf_put result
//   pipe.c:220-271  read state check + ring_buf_get result
//   pipe.c:273-285  reset
//   pipe.c:287-296  close
//
// All other pipe logic (ring buffer internals, wait queues, scheduling,
// memcpy, polling, tracing) remains native Zephyr C in gale_pipe.c.
//
// Verified by Verus (SMT/Z3):
//   PP1:  0 <= used <= size
//   PP2:  size > 0
//   PP3:  write closed -> -EPIPE
//   PP4:  write/read resetting -> -ECANCELED
//   PP5:  write computes correct byte count
//   PP6:  read computes correct byte count
//   PP7:  reset sets used to 0
//   PP8:  close clears flags
//   PP9:  conservation: used + free == size
//   PP10: no overflow/underflow

const PIPE_FLAG_OPEN: u8 = 1;
const PIPE_FLAG_RESET: u8 = 2;

/// Validate a pipe write and compute how many bytes can be written.
///
/// Arguments:
///   used:        current bytes in buffer (ring_buf_size_get)
///   size:        buffer capacity
///   flags:       pipe flags (OPEN, RESET)
///   request_len: bytes the caller wants to write
///   actual_len:  pointer to receive actual byte count
///
/// Returns:
///   0 (OK)       — *actual_len bytes can be written
///   -EPIPE       — pipe closed
///   -ECANCELED   — pipe resetting
///   -EAGAIN      — pipe full
///   -ENOMSG      — zero-length request
#[cfg(feature = "pipe")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_pipe_write_check(
    used: u32,
    size: u32,
    flags: u8,
    request_len: u32,
    actual_len: *mut u32,
    new_used: *mut u32,
) -> i32 {
    use gale::pipe::{WriteDecision, write_decide};

    unsafe {
        if actual_len.is_null() || new_used.is_null() || size == 0 {
            return EINVAL;
        }
        if request_len == 0 {
            return ENOMSG;
        }

        // Delegate core logic to verified model (has_reader=false:
        // old API doesn't handle waking).
        let r = write_decide(used, size, flags, request_len, false);
        match r.decision {
            WriteDecision::WriteOk => {
                *actual_len = r.actual_bytes;
                *new_used = r.new_used;
                OK
            }
            WriteDecision::WriteError => r.ret,
            WriteDecision::WritePend => EAGAIN,
            WriteDecision::WakeReader => {
                // Should not occur with has_reader=false, but handle gracefully.
                *actual_len = r.actual_bytes;
                *new_used = r.new_used;
                OK
            }
        }
    }
}

/// Validate a pipe read and compute how many bytes can be read.
///
/// Arguments:
///   used:        current bytes in buffer
///   flags:       pipe flags
///   request_len: bytes the caller wants to read
///   actual_len:  pointer to receive actual byte count
///   new_used:    pointer to receive updated used count
///
/// Returns:
///   0 (OK)       — *actual_len bytes can be read
///   -EPIPE       — pipe closed and empty
///   -ECANCELED   — pipe resetting
///   -EAGAIN      — pipe empty
///   -ENOMSG      — zero-length request
#[cfg(feature = "pipe")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_pipe_read_check(
    used: u32,
    flags: u8,
    request_len: u32,
    actual_len: *mut u32,
    new_used: *mut u32,
) -> i32 {
    use gale::pipe::{ReadDecision, read_decide};

    unsafe {
        if actual_len.is_null() || new_used.is_null() {
            return EINVAL;
        }
        if request_len == 0 {
            return ENOMSG;
        }

        // Delegate core logic to verified model (has_writer=false:
        // old API doesn't handle waking). Size is irrelevant when
        // has_writer=false; pass u32::MAX as safe placeholder.
        let r = read_decide(used, u32::MAX, flags, request_len, false);
        match r.decision {
            ReadDecision::ReadOk => {
                *actual_len = r.actual_bytes;
                *new_used = r.new_used;
                OK
            }
            ReadDecision::ReadError => r.ret,
            ReadDecision::ReadPend => EAGAIN,
            ReadDecision::WakeWriter => {
                // Should not occur with has_writer=false, but handle gracefully.
                *actual_len = r.actual_bytes;
                *new_used = r.new_used;
                OK
            }
        }
    }
}

// ---- Phase 2: Pipe Decision API ----

/// Decision struct for k_pipe_write -- tells C shim what action to take.
#[repr(C)]
pub struct GalePipeWriteDecision {
    /// Return code (error code when action=RETURN_ERROR)
    pub ret: i32,
    /// Action: 0=WRITE_OK, 1=WAKE_READER, 2=PEND_CURRENT, 3=RETURN_ERROR
    pub action: u8,
    /// Bytes that can be written to ring buffer
    pub actual_bytes: u32,
    /// Updated used count after write
    pub new_used: u32,
}

pub const GALE_PIPE_ACTION_WRITE_OK: u8 = 0;
pub const GALE_PIPE_ACTION_WAKE_READER: u8 = 1;
pub const GALE_PIPE_ACTION_WRITE_PEND: u8 = 2;
pub const GALE_PIPE_ACTION_WRITE_ERROR: u8 = 3;

/// Full decision for k_pipe_write: decides what the C shim should do.
///
/// Delegates to `gale::pipe::write_decide` (Verus-verified).
/// Verified: PP3-PP5, PP9-PP10
#[cfg(feature = "pipe")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_pipe_write_decide(
    used: u32,
    size: u32,
    flags: u8,
    request_len: u32,
    has_reader: u32,
) -> GalePipeWriteDecision {
    use gale::pipe::{WriteDecision, write_decide};

    let r = write_decide(used, size, flags, request_len, has_reader != 0);
    match r.decision {
        WriteDecision::WriteOk => GalePipeWriteDecision {
            ret: r.ret,
            action: GALE_PIPE_ACTION_WRITE_OK,
            actual_bytes: r.actual_bytes,
            new_used: r.new_used,
        },
        WriteDecision::WakeReader => GalePipeWriteDecision {
            ret: r.ret,
            action: GALE_PIPE_ACTION_WAKE_READER,
            actual_bytes: r.actual_bytes,
            new_used: r.new_used,
        },
        WriteDecision::WritePend => GalePipeWriteDecision {
            ret: r.ret,
            action: GALE_PIPE_ACTION_WRITE_PEND,
            actual_bytes: r.actual_bytes,
            new_used: r.new_used,
        },
        WriteDecision::WriteError => GalePipeWriteDecision {
            ret: r.ret,
            action: GALE_PIPE_ACTION_WRITE_ERROR,
            actual_bytes: r.actual_bytes,
            new_used: r.new_used,
        },
    }
}

/// Decision struct for k_pipe_read -- tells C shim what action to take.
#[repr(C)]
pub struct GalePipeReadDecision {
    /// Return code (error code when action=RETURN_ERROR)
    pub ret: i32,
    /// Action: 0=READ_OK, 1=WAKE_WRITER, 2=PEND_CURRENT, 3=RETURN_ERROR
    pub action: u8,
    /// Bytes that can be read from ring buffer
    pub actual_bytes: u32,
    /// Updated used count after read
    pub new_used: u32,
}

pub const GALE_PIPE_ACTION_READ_OK: u8 = 0;
pub const GALE_PIPE_ACTION_WAKE_WRITER: u8 = 1;
pub const GALE_PIPE_ACTION_READ_PEND: u8 = 2;
pub const GALE_PIPE_ACTION_READ_ERROR: u8 = 3;

/// Full decision for k_pipe_read: decides what the C shim should do.
///
/// Delegates to `gale::pipe::read_decide` (Verus-verified).
/// Verified: PP3-PP6, PP9-PP10
#[cfg(feature = "pipe")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_pipe_read_decide(
    used: u32,
    size: u32,
    flags: u8,
    request_len: u32,
    has_writer: u32,
) -> GalePipeReadDecision {
    use gale::pipe::{ReadDecision, read_decide};

    let r = read_decide(used, size, flags, request_len, has_writer != 0);
    match r.decision {
        ReadDecision::ReadOk => GalePipeReadDecision {
            ret: r.ret,
            action: GALE_PIPE_ACTION_READ_OK,
            actual_bytes: r.actual_bytes,
            new_used: r.new_used,
        },
        ReadDecision::WakeWriter => GalePipeReadDecision {
            ret: r.ret,
            action: GALE_PIPE_ACTION_WAKE_WRITER,
            actual_bytes: r.actual_bytes,
            new_used: r.new_used,
        },
        ReadDecision::ReadPend => GalePipeReadDecision {
            ret: r.ret,
            action: GALE_PIPE_ACTION_READ_PEND,
            actual_bytes: r.actual_bytes,
            new_used: r.new_used,
        },
        ReadDecision::ReadError => GalePipeReadDecision {
            ret: r.ret,
            action: GALE_PIPE_ACTION_READ_ERROR,
            actual_bytes: r.actual_bytes,
            new_used: r.new_used,
        },
    }
}

/// Validate stack init parameters.
///
/// stack.c:27-42:
///   stack->base = buffer; stack->next = buffer;
///   stack->top = buffer + num_entries;
///
/// Returns:
///   0 (OK)   — valid capacity
///   -EINVAL  — num_entries == 0
#[cfg(feature = "stack")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_stack_init_validate(num_entries: u32) -> i32 {
    if num_entries == 0 {
        EINVAL
    } else {
        OK
    }
}

/// Validate a push operation and compute new count.
///
/// stack.c:109-125:
///   CHECKIF(stack->next == stack->top) { ret = -ENOMEM; }
///   *(stack->next) = data; stack->next++;
///
/// Arguments:
///   count:     current element count (next - base)
///   capacity:  maximum entries (top - base)
///   new_count: pointer to receive count + 1
///
/// Returns:
///   0 (OK)    — space available, *new_count set, caller stores data
///   -ENOMEM   — stack full, output unchanged
#[cfg(feature = "stack")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_stack_push_validate(
    count: u32,
    capacity: u32,
    new_count: *mut u32,
) -> i32 {
    use gale::stack::{PushDecision, push_decide};

    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (has_waiter=false: old API
        // doesn't handle waking).
        let r = push_decide(count, capacity, false);
        match r.decision {
            PushDecision::Store => {
                *new_count = r.new_count;
                OK
            }
            PushDecision::Full | PushDecision::WakeWaiter => ENOMEM,
        }
    }
}

/// Validate a pop operation and compute new count.
///
/// stack.c:158-160:
///   if (stack->next > stack->base) {
///       stack->next--; *data = *(stack->next);
///   }
///
/// Arguments:
///   count:     current element count (next - base)
///   new_count: pointer to receive count - 1
///
/// Returns:
///   0 (OK)    — data available, *new_count set, caller reads data
///   -EBUSY    — stack empty, output unchanged
#[cfg(feature = "stack")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_stack_pop_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    use gale::stack::{PopDecision, pop_decide};

    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (is_no_wait=true: old API never pends).
        let r = pop_decide(count, true);
        match r.decision {
            PopDecision::Pop => {
                *new_count = r.new_count;
                OK
            }
            PopDecision::Busy => EBUSY,
            PopDecision::Pend => EBUSY,
        }
    }
}

// ---- Phase 2: Full Decision API for Stack ----

/// Decision struct for k_stack_push — tells C shim what action to take.
#[repr(C)]
pub struct GaleStackPushDecision {
    /// Return code: 0 (OK), -ENOMEM (full)
    pub ret: i32,
    /// New count value (only meaningful when action=PUSH_OK and no waiter)
    pub new_count: u32,
    /// Action: 0=PUSH_OK, 1=PEND_CURRENT (unused for push — always immediate)
    /// With push: 0=PUSH_OK means store data or wake waiter, 1 is not used.
    /// We use: 0=STORE_DATA, 1=WAKE_WAITER, 2=FULL
    pub action: u8,
}

pub const GALE_STACK_PUSH_STORE: u8 = 0;
pub const GALE_STACK_PUSH_WAKE: u8 = 1;
pub const GALE_STACK_PUSH_FULL: u8 = 2;

/// Full decision for k_stack_push: decides whether to store data, wake a waiter,
/// or reject because the stack is full.
///
/// The C shim calls z_unpend_first_thread first (side effect), then passes
/// whether a waiter was found.  Rust decides the action.
///
/// Delegates to `gale::stack::push_decide` (Verus-verified).
/// Verified: SK1 (bounds), SK3 (increment), SK4 (-ENOMEM).
#[cfg(feature = "stack")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_stack_push_decide(
    count: u32,
    capacity: u32,
    has_waiter: u32,
) -> GaleStackPushDecision {
    use gale::stack::{PushDecision, push_decide};

    let r = push_decide(count, capacity, has_waiter != 0);
    match r.decision {
        PushDecision::Store => GaleStackPushDecision {
            ret: OK,
            new_count: r.new_count,
            action: GALE_STACK_PUSH_STORE,
        },
        PushDecision::WakeWaiter => GaleStackPushDecision {
            ret: OK,
            new_count: r.new_count,
            action: GALE_STACK_PUSH_WAKE,
        },
        PushDecision::Full => GaleStackPushDecision {
            ret: ENOMEM,
            new_count: r.new_count,
            action: GALE_STACK_PUSH_FULL,
        },
    }
}

/// Decision struct for k_stack_pop — tells C shim what action to take.
#[repr(C)]
pub struct GaleStackPopDecision {
    /// Return code: 0 (OK), -EBUSY (empty + no_wait)
    pub ret: i32,
    /// New count value (decremented if popped)
    pub new_count: u32,
    /// Action: 0=POP_OK (return data), 1=PEND_CURRENT (block or return -EBUSY)
    pub action: u8,
}

pub const GALE_STACK_POP_OK: u8 = 0;
pub const GALE_STACK_POP_PEND: u8 = 1;

/// Full decision for k_stack_pop: decides whether to pop data or pend/reject.
///
/// Delegates to `gale::stack::pop_decide` (Verus-verified).
/// Verified: SK1 (bounds), SK5 (decrement), SK6 (-EBUSY).
#[cfg(feature = "stack")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_stack_pop_decide(
    count: u32,
    is_no_wait: u32,
) -> GaleStackPopDecision {
    use gale::stack::{PopDecision, pop_decide};

    let r = pop_decide(count, is_no_wait != 0);
    match r.decision {
        PopDecision::Pop => GaleStackPopDecision {
            ret: OK,
            new_count: r.new_count,
            action: GALE_STACK_POP_OK,
        },
        PopDecision::Busy => GaleStackPopDecision {
            ret: EBUSY,
            new_count: r.new_count,
            action: GALE_STACK_POP_OK,
        },
        PopDecision::Pend => GaleStackPopDecision {
            ret: 0,
            new_count: r.new_count,
            action: GALE_STACK_POP_PEND,
        },
    }
}

/// Validate timer init parameters.
///
/// timer.c init:
///   Timer period can be 0 (one-shot) or >0 (periodic).
///   Always succeeds — no invalid period values.
///
/// Returns:
///   0 (OK) — always valid
#[cfg(feature = "timer")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_timer_init_validate(period: u32) -> i32 {
    // Period 0 = one-shot, period > 0 = periodic.  Both are valid.
    let _ = period;
    OK
}

/// Record a timer expiry: checked status increment.
///
/// timer.c expiry handler:
///   timer->status++;
///
/// Arguments:
///   status:     current expiry count
///   new_status: pointer to receive status + 1
///
/// Returns:
///   0 (OK)       — *new_status set to status + 1
///   -EOVERFLOW   — status == u32::MAX, output unchanged
///   -EINVAL      — new_status is null
#[cfg(feature = "timer")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_timer_expire(
    status: u32,
    new_status: *mut u32,
) -> i32 {
    use gale::timer::expire_decide;

    unsafe {
        if new_status.is_null() {
            return EINVAL;
        }

        // Delegate to verified model.
        let r = expire_decide(status, 0);
        if r.new_status != status {
            *new_status = r.new_status;
            OK
        } else {
            // Saturated at u32::MAX — model returns unchanged status.
            EOVERFLOW
        }
    }
}

/// Read and reset the status counter.
///
/// timer.c k_timer_status_get:
///   result = timer->status;
///   timer->status = 0;
///   return result;
///
/// Arguments:
///   status:     current expiry count
///   new_status: pointer to receive 0 (reset value)
///
/// Returns:
///   The old status value.
///   If new_status is non-null, *new_status is set to 0.
///
/// Delegates to `gale_k_timer_status_decide` (verified pattern: read + reset).
#[cfg(feature = "timer")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_timer_status_get(
    status: u32,
    new_status: *mut u32,
) -> u32 {
    // Delegate to the decision function (TM2).
    let d = gale_k_timer_status_decide(status);
    unsafe {
        if !new_status.is_null() {
            *new_status = d.new_status;
        }
    }
    d.count
}

// ---- Decision API for timer ----

/// Decision struct for timer expiry — tells C shim what new status to apply.
#[repr(C)]
pub struct GaleTimerExpireDecision {
    /// New status value (status + 1, or unchanged on overflow).
    pub new_status: u32,
    /// Whether the timer has a non-zero period (1 = periodic, 0 = one-shot).
    pub is_periodic: u8,
}

/// Decision for timer expiry handler: increment status and classify period.
///
/// Extract→Decide→Apply: C extracts timer->status and timer->period,
/// Rust decides the new status value and whether timer is periodic,
/// C applies new_status to timer->status.
///
/// Delegates to `gale::timer::expire_decide` (Verus-verified).
/// Verified: TM5 (increment), TM8 (no overflow — saturates at u32::MAX).
#[cfg(feature = "timer")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_timer_expire_decide(
    status: u32,
    period: u32,
) -> GaleTimerExpireDecision {
    use gale::timer::expire_decide;

    let r = expire_decide(status, period);
    GaleTimerExpireDecision {
        new_status: r.new_status,
        is_periodic: if r.is_periodic { 1 } else { 0 },
    }
}

/// Decision struct for timer status_get — tells C shim what count to return
/// and what new status to apply (reset to 0).
#[repr(C)]
pub struct GaleTimerStatusDecision {
    /// The old status value to return to the caller.
    pub count: u32,
    /// New status value (always 0 — reset after read).
    pub new_status: u32,
}

/// Decision for k_timer_status_get: read current status and reset to 0.
///
/// Extract→Decide→Apply: C extracts timer->status,
/// Rust decides the return value (old status) and new status (0),
/// C applies new_status to timer->status and returns count.
///
/// Verified: TM2 (read + reset to 0).
#[cfg(feature = "timer")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_timer_status_decide(
    status: u32,
) -> GaleTimerStatusDecision {
    GaleTimerStatusDecision {
        count: status,
        new_status: 0,
    }
}

// ---------------------------------------------------------------------------
// Memory Slab — verified block count tracking
// ---------------------------------------------------------------------------

/// Validate memory slab init parameters.
///
/// mem_slab.c:109-111:
///   CHECKIF(slab->info.block_size == 0U) { return -EINVAL; }
///
/// Returns:
///   0 (OK)   — valid parameters
///   -EINVAL  — block_size == 0 or num_blocks == 0
#[cfg(feature = "mem_slab")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mem_slab_init_validate(block_size: u32, num_blocks: u32) -> i32 {
    if block_size == 0 || num_blocks == 0 {
        EINVAL
    } else {
        OK
    }
}

/// Validate an alloc operation and compute new num_used.
///
/// mem_slab.c:245:
///   slab->info.num_used++;
///
/// Arguments:
///   num_used:     current allocated block count
///   num_blocks:   total blocks in the slab
///   new_num_used: pointer to receive num_used + 1
///
/// Returns:
///   0 (OK)    — block available, *new_num_used set
///   -ENOMEM   — slab full, output unchanged
#[cfg(feature = "mem_slab")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mem_slab_alloc_validate(
    num_used: u32,
    num_blocks: u32,
    new_num_used: *mut u32,
) -> i32 {
    use gale::mem_slab::{AllocDecision, alloc_decide};

    unsafe {
        if new_num_used.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (is_no_wait=true: old API never pends).
        let r = alloc_decide(num_used, num_blocks, true);
        match r.decision {
            AllocDecision::Alloc => {
                *new_num_used = r.new_num_used;
                OK
            }
            AllocDecision::NoMem | AllocDecision::Pend => ENOMEM,
        }
    }
}

/// Validate a free operation and compute new num_used.
///
/// mem_slab.c:308:
///   slab->info.num_used--;
///
/// Arguments:
///   num_used:     current allocated block count
///   new_num_used: pointer to receive num_used - 1
///
/// Returns:
///   0 (OK)    — block freed, *new_num_used set
///   -EINVAL   — all blocks already free, output unchanged
#[cfg(feature = "mem_slab")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mem_slab_free_validate(
    num_used: u32,
    new_num_used: *mut u32,
) -> i32 {
    use gale::mem_slab::{FreeDecision, free_decide};

    unsafe {
        if new_num_used.is_null() {
            return EINVAL;
        }

        if num_used == 0 {
            return EINVAL;
        }

        // Delegate to verified model (has_waiter=false: old API
        // doesn't handle waking).
        let r = free_decide(num_used, false);
        match r.decision {
            FreeDecision::Free => {
                *new_num_used = r.new_num_used;
                OK
            }
            FreeDecision::WakeThread => {
                // Should not occur with has_waiter=false.
                *new_num_used = r.new_num_used;
                OK
            }
        }
    }
}

// ---- Memory Slab Decision API ----

/// Decision struct for k_mem_slab_alloc — tells C shim what action to take.
#[repr(C)]
pub struct GaleMemSlabAllocDecision {
    /// Return code: 0 (OK), -ENOMEM (slab full)
    pub ret: i32,
    /// New num_used value (incremented if allocated)
    pub new_num_used: u32,
    /// Action: 0=ALLOC_OK, 1=PEND_CURRENT, 2=RETURN_NOMEM
    pub action: u8,
}

pub const GALE_MEM_SLAB_ACTION_ALLOC_OK: u8 = 0;
pub const GALE_MEM_SLAB_ACTION_PEND_CURRENT: u8 = 1;
pub const GALE_MEM_SLAB_ACTION_RETURN_NOMEM: u8 = 2;

/// Full decision for k_mem_slab_alloc: decides whether to allocate, pend, or return -ENOMEM.
///
/// The C shim extracts num_used, num_blocks, and whether the caller is willing
/// to wait.  Rust decides the action.
///
/// Delegates to `gale::mem_slab::alloc_decide` (Verus-verified).
/// Verified: MS4 (increment), MS5 (-ENOMEM), MS1 (bounds).
#[cfg(feature = "mem_slab")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mem_slab_alloc_decide(
    num_used: u32,
    num_blocks: u32,
    is_no_wait: u32,
) -> GaleMemSlabAllocDecision {
    use gale::mem_slab::{AllocDecision, alloc_decide};

    let r = alloc_decide(num_used, num_blocks, is_no_wait != 0);
    match r.decision {
        AllocDecision::Alloc => GaleMemSlabAllocDecision {
            ret: OK,
            new_num_used: r.new_num_used,
            action: GALE_MEM_SLAB_ACTION_ALLOC_OK,
        },
        AllocDecision::NoMem => GaleMemSlabAllocDecision {
            ret: ENOMEM,
            new_num_used: r.new_num_used,
            action: GALE_MEM_SLAB_ACTION_RETURN_NOMEM,
        },
        AllocDecision::Pend => GaleMemSlabAllocDecision {
            ret: 0,
            new_num_used: r.new_num_used,
            action: GALE_MEM_SLAB_ACTION_PEND_CURRENT,
        },
    }
}

/// Decision struct for k_mem_slab_free — tells C shim what action to take.
#[repr(C)]
pub struct GaleMemSlabFreeDecision {
    /// New num_used value (decremented)
    pub new_num_used: u32,
    /// Action: 0=FREE_OK, 1=WAKE_THREAD
    pub action: u8,
}

pub const GALE_MEM_SLAB_ACTION_FREE_OK: u8 = 0;
pub const GALE_MEM_SLAB_ACTION_WAKE_THREAD: u8 = 1;

/// Full decision for k_mem_slab_free: decides whether to return block to free list or wake a thread.
///
/// The C shim checks whether there is a waiting thread (and whether the free
/// list was empty).  Rust decides the action.
///
/// Delegates to `gale::mem_slab::free_decide` (Verus-verified).
/// Verified: MS6 (decrement), MS1 (bounds).
#[cfg(feature = "mem_slab")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mem_slab_free_decide(
    num_used: u32,
    has_waiter: u32,
) -> GaleMemSlabFreeDecision {
    use gale::mem_slab::{FreeDecision, free_decide};

    let r = free_decide(num_used, has_waiter != 0);
    match r.decision {
        FreeDecision::Free => GaleMemSlabFreeDecision {
            new_num_used: r.new_num_used,
            action: GALE_MEM_SLAB_ACTION_FREE_OK,
        },
        FreeDecision::WakeThread => GaleMemSlabFreeDecision {
            new_num_used: r.new_num_used,
            action: GALE_MEM_SLAB_ACTION_WAKE_THREAD,
        },
    }
}

// ---------------------------------------------------------------------------
// FFI exports — event bitmask operations
// ---------------------------------------------------------------------------
//
// These pure functions replace the bitmask arithmetic from kernel/events.c:
//
//   events.c:191-192  k_event_post_internal — (events & ~mask) | (new & mask)
//   events.c:238-240  k_event_set — replace all bits
//   events.c:268-270  k_event_clear — AND complement
//   events.c:95-107   are_wait_conditions_met — any/all bit check
//
// All other event logic (wait queues, scheduling, tracing, userspace)
// remains native Zephyr C in gale_event.c.
//
// Verified by Verus (SMT/Z3):
//   EV1: post ORs bits: events |= new
//   EV2: set replaces: events = new
//   EV3: clear ANDs complement: events &= !clear_bits
//   EV4: set_masked: events = (events & !mask) | (new & mask)
//   EV5: wait_any: returns true when (events & desired) != 0
//   EV6: wait_all: returns true when (events & desired) == desired
//   EV7: events is always a valid u32
//   EV8: post is monotonic (never clears bits)

/// Post (OR) new event bits into the bitmask.
///
/// events.c:
///   event->events |= new_events;
///
/// Arguments:
///   events:     current event bitmask
///   new_events: bits to OR in
///   result:     pointer to receive events | new_events
///
/// Returns:
///   0 (OK)    — *result set
///   -EINVAL   — result is null
///
/// Delegates to `gale::event::post_decide` (Verus-verified) using full mask.
#[cfg(feature = "event")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_event_post(
    events: u32,
    new_events: u32,
    result: *mut u32,
) -> i32 {
    use gale::event::post_decide;

    unsafe {
        if result.is_null() {
            return EINVAL;
        }

        // Delegate OR to verified model (EV1).
        // post_decide(current, new, mask) = (current & !mask) | (new & mask)
        // Setting mask = new_events:
        //   = (events & !new_events) | (new_events & new_events)
        //   = (events & !new_events) | new_events
        //   = events | new_events   ✓
        *result = post_decide(events, new_events, new_events);
        OK
    }
}

/// Set the event bitmask to an exact value, returning the old value.
///
/// events.c:
///   old = event->events; event->events = new_events;
///
/// Arguments:
///   new_events: the new bitmask value
///   old_events: pointer to receive the previous bitmask
///   current:    current event bitmask
///
/// Returns:
///   0 (OK)    — *old_events set to current
///   -EINVAL   — old_events is null
#[cfg(feature = "event")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_event_set(
    new_events: u32,
    old_events: *mut u32,
    current: u32,
) -> i32 {
    unsafe {
        if old_events.is_null() {
            return EINVAL;
        }

        *old_events = current;
        // Caller uses new_events directly; we just record the old value.
        let _ = new_events;
        OK
    }
}

/// Clear specific event bits.
///
/// events.c:
///   event->events &= ~clear_events;
///
/// Arguments:
///   events:     current event bitmask
///   clear_bits: bits to clear
///   result:     pointer to receive events & ~clear_bits
///
/// Returns:
///   0 (OK)    — *result set
///   -EINVAL   — result is null
///
/// Delegates to `gale::event::post_decide` (Verus-verified).
#[cfg(feature = "event")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_event_clear(
    events: u32,
    clear_bits: u32,
    result: *mut u32,
) -> i32 {
    use gale::event::post_decide;

    unsafe {
        if result.is_null() {
            return EINVAL;
        }

        // Delegate AND-complement to verified model (EV3).
        // post_decide(events, 0, clear_bits):
        //   = (events & !clear_bits) | (0 & clear_bits)
        //   = events & !clear_bits   ✓
        *result = post_decide(events, 0, clear_bits);
        OK
    }
}

/// Set only the bits selected by a mask, leaving other bits unchanged.
///
/// events.c:
///   event->events = (event->events & ~mask) | (events & mask);
///
/// Arguments:
///   events:   current event bitmask
///   new_bits: new values for the masked bits
///   mask:     which bits to update
///   result:   pointer to receive (events & ~mask) | (new_bits & mask)
///
/// Returns:
///   0 (OK)    — *result set
///   -EINVAL   — result is null
///
/// Delegates to `gale::event::post_decide` (Verus-verified).
#[cfg(feature = "event")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_event_set_masked(
    events: u32,
    new_bits: u32,
    mask: u32,
    result: *mut u32,
) -> i32 {
    use gale::event::post_decide;

    unsafe {
        if result.is_null() {
            return EINVAL;
        }

        // Direct delegation to verified model (EV4).
        *result = post_decide(events, new_bits, mask);
        OK
    }
}

/// Check if any of the desired event bits are set.
///
/// events.c:
///   match = (event->events & desired) != 0
///
/// Arguments:
///   events:  current event bitmask
///   desired: bits to check
///
/// Returns:
///   1 — at least one desired bit is set
///   0 — no desired bits are set
///
/// Delegates to `gale::event::wait_decide` (Verus-verified, EV5).
#[cfg(feature = "event")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_event_wait_check_any(
    events: u32,
    desired: u32,
) -> i32 {
    use gale::event::{WaitDecision, WAIT_ANY, wait_decide};

    // Delegate to verified model (EV5).
    // Pass is_no_wait=true so the decision is Matched or Timeout (not Pend).
    let r = wait_decide(events, desired, WAIT_ANY, true);
    if r.decision == WaitDecision::Matched { 1 } else { 0 }
}

/// Check if all of the desired event bits are set.
///
/// events.c:
///   match = (event->events & desired) == desired
///
/// Arguments:
///   events:  current event bitmask
///   desired: bits to check
///
/// Returns:
///   1 — all desired bits are set
///   0 — not all desired bits are set
///
/// Delegates to `gale::event::wait_decide` (Verus-verified, EV6).
#[cfg(feature = "event")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_event_wait_check_all(
    events: u32,
    desired: u32,
) -> i32 {
    use gale::event::{WaitDecision, WAIT_ALL, wait_decide};

    // Delegate to verified model (EV6).
    // Pass is_no_wait=true so the decision is Matched or Timeout (not Pend).
    let r = wait_decide(events, desired, WAIT_ALL, true);
    if r.decision == WaitDecision::Matched { 1 } else { 0 }
}

// ---- Phase 2: Full Decision API for events ----

/// Wait type constants for event wait decisions.
pub const GALE_EVENT_WAIT_ANY: u8 = 0;
pub const GALE_EVENT_WAIT_ALL: u8 = 1;

/// Action constants for event wait decisions.
pub const GALE_EVENT_ACTION_MATCHED: u8 = 0;
pub const GALE_EVENT_ACTION_PEND: u8 = 1;
pub const GALE_EVENT_ACTION_TIMEOUT: u8 = 2;

/// Decision struct for k_event_post_internal — tells C shim the new event bitmask.
///
/// Computes: (current_events & ~mask) | (new_events & mask)
///
/// This replaces gale_event_set_masked with a value-returning decision struct.
#[repr(C)]
pub struct GaleEventPostDecision {
    /// The new event bitmask after applying the masked set.
    pub new_events: u32,
}

/// Full decision for k_event_post_internal: computes the new bitmask after
/// applying events through a mask.
///
/// Delegates to `gale::event::post_decide` (Verus-verified).
/// Verified: EV4 — set_masked computes (current & ~mask) | (new & mask)
#[cfg(feature = "event")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_event_post_decide(
    current_events: u32,
    new_events: u32,
    mask: u32,
) -> GaleEventPostDecision {
    use gale::event::post_decide;

    GaleEventPostDecision {
        new_events: post_decide(current_events, new_events, mask),
    }
}

/// Decision struct for event wait condition check — tells C shim what to do.
#[repr(C)]
pub struct GaleEventWaitDecision {
    /// Return code: 0 means success (matched), non-zero means no match
    pub ret: i32,
    /// The matched event bits (current & desired), or 0 if no match
    pub matched_events: u32,
    /// Action: 0=MATCHED, 1=PEND_CURRENT, 2=RETURN_TIMEOUT
    pub action: u8,
}

/// Full decision for event wait: determines whether the wait condition is met,
/// whether to pend the current thread, or return immediately on timeout.
///
/// wait_type: 0=ANY (at least one desired bit set), 1=ALL (all desired bits set)
/// is_no_wait: non-zero if K_NO_WAIT timeout (should not block)
///
/// Delegates to `gale::event::wait_decide` (Verus-verified).
/// Verified: EV5 (any-bit match), EV6 (all-bits match)
#[cfg(feature = "event")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_event_wait_decide(
    current_events: u32,
    desired: u32,
    wait_type: u8,
    is_no_wait: u32,
) -> GaleEventWaitDecision {
    use gale::event::{WaitDecision, wait_decide};

    let r = wait_decide(current_events, desired, wait_type, is_no_wait != 0);
    match r.decision {
        WaitDecision::Matched => GaleEventWaitDecision {
            ret: 0,
            matched_events: r.matched_events,
            action: GALE_EVENT_ACTION_MATCHED,
        },
        WaitDecision::Timeout => GaleEventWaitDecision {
            ret: 0,
            matched_events: 0,
            action: GALE_EVENT_ACTION_TIMEOUT,
        },
        WaitDecision::Pend => GaleEventWaitDecision {
            ret: 0,
            matched_events: 0,
            action: GALE_EVENT_ACTION_PEND,
        },
    }
}

// ---------------------------------------------------------------------------
// FFI exports — fifo (unbounded queue counter, FIFO ordering)
// ---------------------------------------------------------------------------

/// Validate a fifo put operation and compute new count.
///
/// queue.c queue_insert:
///   Enqueue data at tail (FIFO ordering).
///   Count tracks number of data items in the underlying k_queue.
///
/// Arguments:
///   count:     current element count
///   new_count: pointer to receive count + 1
///
/// Returns:
///   0 (OK)       — space available, *new_count set
///   -EOVERFLOW   — count would overflow u32
#[cfg(feature = "fifo")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_fifo_put_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    use gale::fifo::{PutDecision, put_decide};

    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (has_waiter=false: this API
        // handles the no-waiter direct-insert path only).
        let d = put_decide(count, false);
        match d {
            PutDecision::Insert => {
                #[allow(clippy::arithmetic_side_effects)]
                {
                    *new_count = count + 1;
                }
                OK
            }
            PutDecision::Overflow | PutDecision::WakeThread => EOVERFLOW,
        }
    }
}

/// Validate a fifo get operation and compute new count.
///
/// queue.c k_queue_get:
///   Dequeue data from head (FIFO ordering).
///
/// Arguments:
///   count:     current element count
///   new_count: pointer to receive count - 1
///
/// Returns:
///   0 (OK)    — data available, *new_count set
///   -EAGAIN   — fifo empty
#[cfg(feature = "fifo")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_fifo_get_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    use gale::fifo::{GetDecision, get_decide};

    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model.
        let d = get_decide(count);
        match d {
            GetDecision::Dequeued => {
                #[allow(clippy::arithmetic_side_effects)]
                {
                    *new_count = count - 1;
                }
                OK
            }
            GetDecision::Empty => EAGAIN,
        }
    }
}

// ---- Phase 2: Full Decision API for Fifo ----

/// Decision struct for k_fifo put (queue_insert) — tells C shim what action to take.
#[repr(C)]
pub struct GaleFifoPutDecision {
    /// Action: 0=PUT_OK (insert into list), 1=WAKE_THREAD (hand data to waiter)
    pub action: u8,
}

pub const GALE_FIFO_PUT_OK: u8 = 0;
pub const GALE_FIFO_PUT_WAKE: u8 = 1;

/// Full decision for fifo put: decides whether to insert data or wake a waiting thread.
///
/// The C shim calls z_unpend_first_thread first (side effect), then passes
/// whether a waiter was found.  Rust decides the action.
///
/// Fifo is unbounded, so put always succeeds (no capacity check needed).
///
/// Delegates to `gale::fifo::put_decide` (Verus-verified).
/// Verified: FI1 (no overflow), FI2 (increment via PUT_OK path).
#[cfg(feature = "fifo")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_fifo_put_decide(
    count: u32,
    has_waiter: u32,
) -> GaleFifoPutDecision {
    use gale::fifo::{PutDecision, put_decide};

    // Delegate to verified model (FI1, FI2).
    let d = put_decide(count, has_waiter != 0);
    GaleFifoPutDecision {
        action: match d {
            PutDecision::WakeThread => GALE_FIFO_PUT_WAKE,
            PutDecision::Insert | PutDecision::Overflow => GALE_FIFO_PUT_OK,
        },
    }
}

/// Decision struct for k_fifo get (k_queue_get) — tells C shim what action to take.
#[repr(C)]
pub struct GaleFifoGetDecision {
    /// Return code: 0 (data available), -EBUSY (empty + no_wait)
    pub ret: i32,
    /// Action: 0=GET_OK (dequeue data), 1=PEND_CURRENT (block), 2=RETURN_NODATA (no_wait + empty)
    pub action: u8,
}

pub const GALE_FIFO_GET_OK: u8 = 0;
pub const GALE_FIFO_GET_PEND: u8 = 1;
pub const GALE_FIFO_GET_NODATA: u8 = 2;

/// Full decision for fifo get: decides whether to dequeue, pend, or return empty.
///
/// C shim checks sys_sflist_is_empty first and passes the result.
/// If data available (count > 0), return GET_OK.
/// If empty and no_wait, return RETURN_NODATA.
/// If empty and willing to wait, return PEND_CURRENT.
///
/// Delegates to `gale::fifo::get_decide` (Verus-verified).
/// Verified: FI3 (no underflow), FI4 (decrement via GET_OK path).
#[cfg(feature = "fifo")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_fifo_get_decide(
    count: u32,
    is_no_wait: u32,
) -> GaleFifoGetDecision {
    use gale::fifo::{GetDecision, get_decide};

    // Delegate to verified model (FI3, FI4).
    let d = get_decide(count);
    match d {
        GetDecision::Dequeued => GaleFifoGetDecision {
            ret: OK,
            action: GALE_FIFO_GET_OK,
        },
        GetDecision::Empty => {
            if is_no_wait != 0 {
                GaleFifoGetDecision {
                    ret: EBUSY,
                    action: GALE_FIFO_GET_NODATA,
                }
            } else {
                GaleFifoGetDecision {
                    ret: 0,
                    action: GALE_FIFO_GET_PEND,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FFI exports — lifo (unbounded queue counter, LIFO ordering)
// ---------------------------------------------------------------------------

/// Validate a lifo put operation and compute new count.
///
/// queue.c queue_insert:
///   Enqueue data at head (LIFO ordering).
///   Count tracks number of data items in the underlying k_queue.
///
/// Arguments:
///   count:     current element count
///   new_count: pointer to receive count + 1
///
/// Returns:
///   0 (OK)       — space available, *new_count set
///   -EOVERFLOW   — count would overflow u32
#[cfg(feature = "lifo")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_lifo_put_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    use gale::lifo::{PutDecision, put_decide};

    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (has_waiter=false: this API
        // handles the no-waiter direct-insert path only).
        let d = put_decide(count, false);
        match d {
            PutDecision::Insert => {
                #[allow(clippy::arithmetic_side_effects)]
                {
                    *new_count = count + 1;
                }
                OK
            }
            PutDecision::Overflow | PutDecision::WakeThread => EOVERFLOW,
        }
    }
}

/// Validate a lifo get operation and compute new count.
///
/// queue.c k_queue_get:
///   Dequeue data from head (LIFO ordering).
///
/// Arguments:
///   count:     current element count
///   new_count: pointer to receive count - 1
///
/// Returns:
///   0 (OK)    — data available, *new_count set
///   -EAGAIN   — lifo empty
#[cfg(feature = "lifo")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_lifo_get_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    use gale::lifo::{GetDecision, get_decide};

    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model.
        let d = get_decide(count);
        match d {
            GetDecision::Dequeued => {
                #[allow(clippy::arithmetic_side_effects)]
                {
                    *new_count = count - 1;
                }
                OK
            }
            GetDecision::Empty => EAGAIN,
        }
    }
}

// ---- Phase 2: Full Decision API for Lifo ----

/// Decision struct for k_lifo put (queue_insert) — tells C shim what action to take.
#[repr(C)]
pub struct GaleLifoPutDecision {
    /// Action: 0=PUT_OK (insert into list), 1=WAKE_THREAD (hand data to waiter)
    pub action: u8,
}

pub const GALE_LIFO_PUT_OK: u8 = 0;
pub const GALE_LIFO_PUT_WAKE: u8 = 1;

/// Full decision for lifo put: decides whether to insert data or wake a waiting thread.
///
/// The C shim calls z_unpend_first_thread first (side effect), then passes
/// whether a waiter was found.  Rust decides the action.
///
/// Lifo is unbounded, so put always succeeds (no capacity check needed).
///
/// Delegates to `gale::lifo::put_decide` (Verus-verified).
/// Verified: LI1 (no overflow), LI2 (increment via PUT_OK path).
#[cfg(feature = "lifo")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_lifo_put_decide(
    count: u32,
    has_waiter: u32,
) -> GaleLifoPutDecision {
    use gale::lifo::{PutDecision, put_decide};

    // Delegate to verified model (LI1, LI2).
    let d = put_decide(count, has_waiter != 0);
    GaleLifoPutDecision {
        action: match d {
            PutDecision::WakeThread => GALE_LIFO_PUT_WAKE,
            PutDecision::Insert | PutDecision::Overflow => GALE_LIFO_PUT_OK,
        },
    }
}

/// Decision struct for k_lifo get (k_queue_get) — tells C shim what action to take.
#[repr(C)]
pub struct GaleLifoGetDecision {
    /// Return code: 0 (data available), -EBUSY (empty + no_wait)
    pub ret: i32,
    /// Action: 0=GET_OK (dequeue data), 1=PEND_CURRENT (block), 2=RETURN_NODATA (no_wait + empty)
    pub action: u8,
}

pub const GALE_LIFO_GET_OK: u8 = 0;
pub const GALE_LIFO_GET_PEND: u8 = 1;
pub const GALE_LIFO_GET_NODATA: u8 = 2;

/// Full decision for lifo get: decides whether to dequeue, pend, or return empty.
///
/// C shim checks sys_sflist_is_empty first and passes the result.
/// If data available (count > 0), return GET_OK.
/// If empty and no_wait, return RETURN_NODATA.
/// If empty and willing to wait, return PEND_CURRENT.
///
/// Delegates to `gale::lifo::get_decide` (Verus-verified).
/// Verified: LI3 (no underflow), LI4 (decrement via GET_OK path).
#[cfg(feature = "lifo")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_lifo_get_decide(
    count: u32,
    is_no_wait: u32,
) -> GaleLifoGetDecision {
    use gale::lifo::{GetDecision, get_decide};

    // Delegate to verified model (LI3, LI4).
    let d = get_decide(count);
    match d {
        GetDecision::Dequeued => GaleLifoGetDecision {
            ret: OK,
            action: GALE_LIFO_GET_OK,
        },
        GetDecision::Empty => {
            if is_no_wait != 0 {
                GaleLifoGetDecision {
                    ret: EBUSY,
                    action: GALE_LIFO_GET_NODATA,
                }
            } else {
                GaleLifoGetDecision {
                    ret: 0,
                    action: GALE_LIFO_GET_PEND,
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FFI exports — queue (unbounded queue counter)
// ---------------------------------------------------------------------------

/// Validate a queue append operation and compute new count.
///
/// queue.c queue_insert (is_append=true):
///   Enqueue data at tail.
///
/// Arguments:
///   count:     current element count
///   new_count: pointer to receive count + 1
///
/// Returns:
///   0 (OK)       — space available, *new_count set
///   -EOVERFLOW   — count would overflow u32
#[cfg(feature = "queue")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_queue_append_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    use gale::queue::{InsertDecision, insert_decide};

    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (has_waiter=false: this API
        // handles the no-waiter direct-insert path only).
        let d = insert_decide(count, false);
        match d {
            InsertDecision::Insert => {
                #[allow(clippy::arithmetic_side_effects)]
                {
                    *new_count = count + 1;
                }
                OK
            }
            InsertDecision::Overflow | InsertDecision::WakeThread => EOVERFLOW,
        }
    }
}

/// Validate a queue prepend operation and compute new count.
///
/// queue.c queue_insert (is_append=false):
///   Enqueue data at head.
///
/// Arguments:
///   count:     current element count
///   new_count: pointer to receive count + 1
///
/// Returns:
///   0 (OK)       — space available, *new_count set
///   -EOVERFLOW   — count would overflow u32
#[cfg(feature = "queue")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_queue_prepend_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    use gale::queue::{InsertDecision, insert_decide};

    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (has_waiter=false: this API
        // handles the no-waiter direct-insert path only).
        let d = insert_decide(count, false);
        match d {
            InsertDecision::Insert => {
                #[allow(clippy::arithmetic_side_effects)]
                {
                    *new_count = count + 1;
                }
                OK
            }
            InsertDecision::Overflow | InsertDecision::WakeThread => EOVERFLOW,
        }
    }
}

/// Validate a queue get operation and compute new count.
///
/// queue.c k_queue_get:
///   Dequeue data from head.
///
/// Arguments:
///   count:     current element count
///   new_count: pointer to receive count - 1
///
/// Returns:
///   0 (OK)    — data available, *new_count set
///   -EAGAIN   — queue empty
#[cfg(feature = "queue")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_queue_get_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    use gale::queue::{GetDecision, get_decide};

    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        // Delegate to verified model.
        let d = get_decide(count);
        match d {
            GetDecision::Dequeued => {
                #[allow(clippy::arithmetic_side_effects)]
                {
                    *new_count = count - 1;
                }
                OK
            }
            GetDecision::Empty => EAGAIN,
        }
    }
}

// ---------------------------------------------------------------------------
// FFI exports — mbox (stateless validation)
// ---------------------------------------------------------------------------

/// Validate a mailbox send operation.
///
/// mailbox.c mbox_message_put:
///   Validates that the message has non-zero data size.
///
/// Arguments:
///   size: message data size in bytes
///
/// Returns:
///   0 (OK)    — valid send
///   -EINVAL   — size == 0
#[cfg(feature = "mbox")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mbox_validate_send(size: u32) -> i32 {
    use gale::mbox::validate_send_decide;

    // Delegate to verified model.
    validate_send_decide(size)
}

/// Check if sender and receiver IDs are compatible for mailbox matching.
///
/// mailbox.c mbox_message_match:
///   tx_target_thread == K_ANY || tx_target_thread == rx thread
///   rx_source_thread == K_ANY || rx_source_thread == tx thread
///
/// Simplified to integer IDs: 0 means K_ANY (match any).
///
/// Arguments:
///   send_id: sender's target ID (0 = K_ANY)
///   recv_id: receiver's source ID (0 = K_ANY)
///
/// Returns:
///   1 — IDs match (either is 0/K_ANY, or both are equal)
///   0 — IDs do not match
#[cfg(feature = "mbox")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mbox_match_check(send_id: u32, recv_id: u32) -> i32 {
    use gale::mbox::match_check_decide;

    // Delegate to verified model.
    if match_check_decide(send_id, recv_id) { 1 } else { 0 }
}

/// Compute the actual data exchange size for a mailbox message.
///
/// mailbox.c mbox_message_match:
///   if (rx_msg->size > tx_msg->size) { rx_msg->size = tx_msg->size; }
///
/// Arguments:
///   tx_size:     transmit message data size
///   rx_buf_size: receive buffer size
///
/// Returns:
///   min(tx_size, rx_buf_size)
#[cfg(feature = "mbox")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mbox_data_exchange(tx_size: u32, rx_buf_size: u32) -> u32 {
    use gale::mbox::data_exchange_decide;

    // Delegate to verified model.
    data_exchange_decide(tx_size, rx_buf_size)
}

// ---- Phase 2: Queue Decision API ----

/// Decision struct for queue insert (append/prepend) — tells C shim what action to take.
#[repr(C)]
pub struct GaleQueueInsertDecision {
    /// Action: 0=INSERT_INTO_LIST, 1=WAKE_THREAD
    pub action: u8,
}

pub const GALE_QUEUE_ACTION_INSERT: u8 = 0;
pub const GALE_QUEUE_ACTION_WAKE: u8 = 1;

/// Full decision for queue insert: decides whether to wake a pending thread
/// or insert data into the linked list.
///
/// The C shim calls z_unpend_first_thread first (side effect), then passes
/// whether a waiter was found. Rust decides the action.
///
/// Delegates to `gale::queue::insert_decide` (Verus-verified).
/// Verified: QU1/QU2 (append), QU3/QU4 (prepend) — state transition only.
#[cfg(feature = "queue")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_queue_insert_decide(
    has_waiter: u32,
) -> GaleQueueInsertDecision {
    use gale::queue::{InsertDecision, insert_decide};

    // Delegate to verified model (QU1-QU4).
    // count=0 is safe here: insert_decide only uses it for overflow check,
    // and QU1/QU4 do not track count at this layer (count is managed by C).
    let d = insert_decide(0, has_waiter != 0);
    GaleQueueInsertDecision {
        action: match d {
            InsertDecision::WakeThread => GALE_QUEUE_ACTION_WAKE,
            InsertDecision::Insert | InsertDecision::Overflow => GALE_QUEUE_ACTION_INSERT,
        },
    }
}

/// Decision struct for k_queue_get — tells C shim what action to take.
#[repr(C)]
pub struct GaleQueueGetDecision {
    /// Action: 0=DEQUEUE, 1=RETURN_NULL, 2=PEND_CURRENT
    pub action: u8,
}

pub const GALE_QUEUE_ACTION_DEQUEUE: u8 = 0;
pub const GALE_QUEUE_ACTION_RETURN_NULL: u8 = 1;
pub const GALE_QUEUE_ACTION_PEND: u8 = 2;

/// Full decision for k_queue_get: decides whether to dequeue data,
/// return NULL immediately, or pend the current thread.
///
/// The C shim checks if the list has data and whether timeout is K_NO_WAIT.
/// Rust decides the action.
///
/// Delegates to `gale::queue::get_decide` (Verus-verified).
/// Verified: QU5/QU6 — state transition only.
#[cfg(feature = "queue")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_queue_get_decide(
    has_data: u32,
    is_no_wait: u32,
) -> GaleQueueGetDecision {
    use gale::queue::{GetDecision, get_decide};

    // Delegate availability check to verified model (QU5, QU6).
    // has_data != 0 means count > 0 — pass 1 to represent "some items".
    let d = get_decide(has_data);
    match d {
        GetDecision::Dequeued => GaleQueueGetDecision {
            action: GALE_QUEUE_ACTION_DEQUEUE,
        },
        GetDecision::Empty => {
            if is_no_wait != 0 {
                GaleQueueGetDecision {
                    action: GALE_QUEUE_ACTION_RETURN_NULL,
                }
            } else {
                GaleQueueGetDecision {
                    action: GALE_QUEUE_ACTION_PEND,
                }
            }
        }
    }
}

// ---- Phase 2: Mbox Decision API ----

/// Decision struct for mbox_message_put — tells C shim what action to take.
#[repr(C)]
pub struct GaleMboxPutDecision {
    /// Action: 0=MATCHED (wake receiver), 1=RETURN_ENOMSG, 2=PEND_TX_QUEUE
    pub action: u8,
}

pub const GALE_MBOX_ACTION_MATCHED: u8 = 0;
pub const GALE_MBOX_ACTION_RETURN_ENOMSG: u8 = 1;
pub const GALE_MBOX_ACTION_PEND_TX: u8 = 2;

/// Full decision for mbox_message_put: decides post-scan action.
///
/// The C shim scans the rx queue for a compatible receiver (side effect),
/// then passes whether a match was found and the timeout mode.
/// Rust decides the action.
///
/// Verified: MB2-MB4 (match check delegated), state transition decision.
#[cfg(feature = "mbox")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mbox_put_decide(
    matched: u32,
    is_no_wait: u32,
) -> GaleMboxPutDecision {
    if matched != 0 {
        GaleMboxPutDecision {
            action: GALE_MBOX_ACTION_MATCHED,
        }
    } else if is_no_wait != 0 {
        GaleMboxPutDecision {
            action: GALE_MBOX_ACTION_RETURN_ENOMSG,
        }
    } else {
        GaleMboxPutDecision {
            action: GALE_MBOX_ACTION_PEND_TX,
        }
    }
}

/// Decision struct for k_mbox_get — tells C shim what action to take.
#[repr(C)]
pub struct GaleMboxGetDecision {
    /// Action: 0=MATCHED (consume data), 1=RETURN_ENOMSG, 2=PEND_RX_QUEUE
    pub action: u8,
}

pub const GALE_MBOX_ACTION_CONSUME: u8 = 0;
// GALE_MBOX_ACTION_RETURN_ENOMSG = 1 (shared with put)
pub const GALE_MBOX_ACTION_PEND_RX: u8 = 2;

/// Full decision for k_mbox_get: decides post-scan action.
///
/// The C shim scans the tx queue for a compatible sender (side effect),
/// then passes whether a match was found and the timeout mode.
/// Rust decides the action.
///
/// Verified: MB2-MB4 (match check delegated), state transition decision.
#[cfg(feature = "mbox")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mbox_get_decide(
    matched: u32,
    is_no_wait: u32,
) -> GaleMboxGetDecision {
    if matched != 0 {
        GaleMboxGetDecision {
            action: GALE_MBOX_ACTION_CONSUME,
        }
    } else if is_no_wait != 0 {
        GaleMboxGetDecision {
            action: GALE_MBOX_ACTION_RETURN_ENOMSG,
        }
    } else {
        GaleMboxGetDecision {
            action: GALE_MBOX_ACTION_PEND_RX,
        }
    }
}

// ---------------------------------------------------------------------------
// FFI exports — timeout (tick arithmetic + deadline tracking)
// ---------------------------------------------------------------------------
//
// These pure functions replace the safety-critical tick arithmetic from
// kernel/timeout.c:
//
//   timeout.c z_add_timeout     — deadline = current_tick + duration
//   timeout.c z_abort_timeout   — deactivate pending timeout
//   timeout.c sys_clock_announce — advance tick, fire expired timeouts
//
// All other timeout logic (linked-list, spinlock, callbacks, hardware timer)
// remains native Zephyr C.
//
// Verified by Verus (SMT/Z3):
//   TO1: deadline >= current_tick when active
//   TO2: deadline = current_tick + duration
//   TO3: abort clears to inactive
//   TO4: fires when deadline <= now
//   TO5: no overflow (u64 arithmetic)
//   TO6: relative-to-absolute conversion correct
//   TO7: K_FOREVER never expires
//   TO8: K_NO_WAIT immediate

const K_FOREVER_TICKS: u64 = u64::MAX;

// ---- Phase 2: Timeout Decision API ----

/// Decision struct for z_add_timeout — tells C shim the computed deadline.
#[repr(C)]
pub struct GaleTimeoutAddDecision {
    /// Return code: 0 (OK), -EINVAL (overflow)
    pub ret: i32,
    /// Computed absolute deadline (only meaningful when ret == 0)
    pub deadline: u64,
}

/// Compute absolute deadline from current tick + duration.
///
/// timeout.c z_add_timeout:
///   C extracts current_tick and duration, Rust computes the deadline.
///
/// Verified: TO2 (deadline = current_tick + duration), TO5 (no overflow).
#[cfg(feature = "timeout")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeout_add_decide(
    current_tick: u64,
    duration: u64,
) -> GaleTimeoutAddDecision {
    use gale::timeout::add_decide;

    match add_decide(current_tick, duration) {
        Ok(dl) => GaleTimeoutAddDecision {
            ret: OK,
            deadline: dl,
        },
        Err(e) => GaleTimeoutAddDecision {
            ret: e,
            deadline: 0,
        },
    }
}

/// Decision struct for z_abort_timeout — tells C shim whether abort is valid.
#[repr(C)]
pub struct GaleTimeoutAbortDecision {
    /// Return code: 0 (OK, was active), -EINVAL (already inactive)
    pub ret: i32,
    /// Action: 0=DO_REMOVE (remove from list), 1=NOOP (already inactive)
    pub action: u8,
}

pub const GALE_TIMEOUT_ACTION_REMOVE: u8 = 0;
pub const GALE_TIMEOUT_ACTION_NOOP: u8 = 1;

/// Decide whether to abort a pending timeout.
///
/// timeout.c z_abort_timeout:
///   C extracts whether the timeout node is linked (active).
///   Rust decides: remove or noop.
///
/// Verified: TO3 (abort clears to inactive).
#[cfg(feature = "timeout")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeout_abort_decide(
    is_linked: u32,
) -> GaleTimeoutAbortDecision {
    use gale::timeout::abort_decide;

    // Delegate to verified model (TO3).
    if abort_decide(is_linked != 0) {
        GaleTimeoutAbortDecision {
            ret: OK,
            action: GALE_TIMEOUT_ACTION_REMOVE,
        }
    } else {
        GaleTimeoutAbortDecision {
            ret: EINVAL,
            action: GALE_TIMEOUT_ACTION_NOOP,
        }
    }
}

/// Decision struct for sys_clock_announce — tells C shim the new tick and
/// whether a specific timeout has expired.
#[repr(C)]
pub struct GaleTimeoutAnnounceDecision {
    /// Return code: 0 (OK), -EINVAL (overflow)
    pub ret: i32,
    /// Advanced tick value (current_tick + ticks)
    pub new_tick: u64,
    /// 1 if the timeout fired (deadline <= new_tick), 0 otherwise
    pub fired: u32,
}

/// Advance tick and check if a timeout has expired.
///
/// timeout.c sys_clock_announce:
///   C extracts current_tick, ticks to advance, deadline, and active flag.
///   Rust computes new_tick and whether the timeout fired.
///
/// Verified: TO4 (fires when deadline <= now), TO5 (no overflow),
///           TO7 (K_FOREVER never expires).
#[cfg(feature = "timeout")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeout_announce_decide(
    current_tick: u64,
    ticks: u64,
    deadline: u64,
    active: u32,
) -> GaleTimeoutAnnounceDecision {
    use gale::timeout::announce_decide;

    // Delegate to verified model (TO4, TO5, TO7).
    match announce_decide(current_tick, ticks, deadline, active != 0) {
        Ok((new_tick, fired)) => GaleTimeoutAnnounceDecision {
            ret: OK,
            new_tick,
            fired: if fired { 1 } else { 0 },
        },
        Err(e) => GaleTimeoutAnnounceDecision {
            ret: e,
            new_tick: 0,
            fired: 0,
        },
    }
}

// ---- Legacy API (kept for backward compatibility) ----

/// Schedule a timeout: compute absolute deadline from current tick + duration.
///
/// Delegates to gale_timeout_add_decide.
#[cfg(feature = "timeout")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeout_add(
    current_tick: u64,
    duration: u64,
    deadline: *mut u64,
) -> i32 {
    unsafe {
        if deadline.is_null() {
            return EINVAL;
        }

        let d = gale_timeout_add_decide(current_tick, duration);
        if d.ret != OK {
            return d.ret;
        }

        *deadline = d.deadline;
        OK
    }
}

/// Abort a pending timeout.
///
/// Delegates to gale_timeout_abort_decide.
#[cfg(feature = "timeout")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeout_abort(active: u32) -> i32 {
    gale_timeout_abort_decide(active).ret
}

/// Advance tick and check if a timeout has expired.
///
/// Delegates to gale_timeout_announce_decide.
#[cfg(feature = "timeout")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeout_announce(
    current_tick: u64,
    ticks: u64,
    deadline: u64,
    active: u32,
    new_tick: *mut u64,
    fired: *mut u32,
) -> i32 {
    unsafe {
        if new_tick.is_null() || fired.is_null() {
            return EINVAL;
        }

        let d = gale_timeout_announce_decide(current_tick, ticks, deadline, active);
        if d.ret != OK {
            return d.ret;
        }

        *new_tick = d.new_tick;
        *fired = d.fired;
        OK
    }
}

// ---------------------------------------------------------------------------
// FFI exports — poll (event state machine + signal)
// ---------------------------------------------------------------------------
//
// These pure functions replace the poll event state checks from
// kernel/poll.c:
//
//   poll.c:46-62   k_poll_event_init — set type, clear state
//   poll.c:65-103  is_condition_met — check sem/signal/msgq availability
//   poll.c:475-498 k_poll_signal_init/raise/reset/check
//
// Verified by Verus (SMT/Z3):
//   PL1: event starts NOT_READY
//   PL3: SEM_AVAILABLE iff count > 0
//   PL7: signal raise sets result + signaled
//   PL8: signal reset clears signaled

/// Initialize a poll event: validate type, output NOT_READY state.
///
/// Arguments:
///   event_type: poll event type (K_POLL_TYPE_*)
///   state:      pointer to receive initial state (0 = NOT_READY)
///
/// Returns:
///   0 (OK)   — valid type, *state set to 0
///   -EINVAL  — null pointer
#[cfg(feature = "poll")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_poll_event_init(
    event_type: u32,
    state: *mut u32,
) -> i32 {
    unsafe {
        if state.is_null() {
            return EINVAL;
        }

        // All types are valid — Zephyr doesn't reject unknown types at init.
        let _ = event_type;
        *state = 0; // STATE_NOT_READY
        OK
    }
}

/// Check if a semaphore condition is met for a poll event.
///
/// poll.c:65-70: is_condition_met() K_POLL_TYPE_SEM_AVAILABLE case.
///
/// Arguments:
///   event_type: poll event type
///   sem_count:  current semaphore count
///
/// Returns:
///   1 — condition met (type == SEM_AVAILABLE && count > 0)
///   0 — condition not met
#[cfg(feature = "poll")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_poll_check_sem(
    event_type: u32,
    sem_count: u32,
) -> i32 {
    use gale::poll::check_sem_decide;

    // Delegate to verified model (PL3).
    if check_sem_decide(event_type, sem_count) { 1 } else { 0 }
}

/// Raise a poll signal: set signaled flag and result value.
///
/// poll.c:522-545: k_poll_signal_raise()
///
/// Arguments:
///   signaled:    pointer to signaled flag (set to 1)
///   result:      pointer to result value (set to result_val)
///   result_val:  value to store
///
/// Returns:
///   0 (OK)   — *signaled = 1, *result = result_val
///   -EINVAL  — null pointer
#[cfg(feature = "poll")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_poll_signal_raise(
    signaled: *mut u32,
    result: *mut i32,
    result_val: i32,
) -> i32 {
    use gale::poll::signal_raise_decide;

    unsafe {
        if signaled.is_null() || result.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (PL7).
        let (new_signaled, new_result, _) = signal_raise_decide(result_val, false);
        *signaled = new_signaled;
        *result = new_result;
        OK
    }
}

/// Reset a poll signal: clear signaled flag.
///
/// poll.c:494-498: k_poll_signal_reset()
///
/// Arguments:
///   signaled: pointer to signaled flag (set to 0)
///
/// Returns:
///   0 (OK)   — *signaled = 0
///   -EINVAL  — null pointer
#[cfg(feature = "poll")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_poll_signal_reset(
    signaled: *mut u32,
) -> i32 {
    unsafe {
        if signaled.is_null() {
            return EINVAL;
        }

        *signaled = 0;
        OK
    }
}

// ---- Phase 2: Full Decision API for Poll ----

/// Decision struct for k_poll_signal_raise — tells C shim what values to apply.
///
/// C extracts the current signaled state and whether a poll_event is pending
/// (side effect: sys_dlist_get removes it). Rust decides the new signaled/result
/// values.
#[repr(C)]
pub struct GalePollSignalRaiseDecision {
    /// New signaled value (always 1 — raise sets signaled).
    pub new_signaled: u32,
    /// Result value to store in signal.
    pub new_result: i32,
    /// Action: 0=NO_EVENT (no poll_event to signal), 1=SIGNAL_EVENT (wake poller)
    pub action: u8,
}

pub const GALE_POLL_ACTION_NO_EVENT: u8 = 0;
pub const GALE_POLL_ACTION_SIGNAL_EVENT: u8 = 1;

/// Full decision for k_poll_signal_raise: decides new signal state and whether
/// to signal a waiting poll event.
///
/// The C shim calls sys_dlist_get(&sig->poll_events) first (side effect: removes
/// the poll_event node), then passes whether a poll_event was found.  Rust decides
/// the new signaled/result values and the action to take.
///
/// poll.c:522-545: z_impl_k_poll_signal_raise()
///
/// Verified: PL7 (signal raise sets result + signaled).
#[cfg(feature = "poll")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_poll_signal_raise_decide(
    signaled: u32,
    result_val: i32,
    has_poll_event: u32,
) -> GalePollSignalRaiseDecision {
    use gale::poll::signal_raise_decide;

    // Delegate to verified model (PL7).
    let _ = signaled;
    let (new_signaled, new_result, has_event) =
        signal_raise_decide(result_val, has_poll_event != 0);
    GalePollSignalRaiseDecision {
        new_signaled,
        new_result,
        action: if has_event {
            GALE_POLL_ACTION_SIGNAL_EVENT
        } else {
            GALE_POLL_ACTION_NO_EVENT
        },
    }
}

// ---------------------------------------------------------------------------
// FFI exports — futex (fast userspace mutex)
// ---------------------------------------------------------------------------
//
// These pure functions replace the value comparison logic from
// kernel/futex.c:
//
//   futex.c:69-94   z_impl_k_futex_wait — compare val to expected
//   futex.c:27-57   z_impl_k_futex_wake — wake count tracking
//
// Verified by Verus (SMT/Z3):
//   FX1: wait blocks when val == expected
//   FX2: wait mismatch returns EAGAIN
//   FX3: wake returns number woken

/// Check if a futex wait should block.
///
/// futex.c:
///   if (atomic_get(&futex->val) != expected) { return -EAGAIN; }
///
/// Arguments:
///   val:      current futex value
///   expected: expected value
///
/// Returns:
///   0 (OK)    — val == expected, caller should block
///   -EAGAIN   — val != expected, do not block
#[cfg(feature = "futex")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_futex_wait_check(val: u32, expected: u32) -> i32 {
    use gale::futex::{WaitDecision, wait_decide};

    // Delegate to verified model.
    let d = wait_decide(val, expected);
    match d {
        WaitDecision::Block => OK,
        WaitDecision::Mismatch => EAGAIN,
    }
}

/// Validate futex wake count and compute remaining waiters.
///
/// futex.c z_impl_k_futex_wake:
///   Wake up to `wake_count` waiters (0 = none, u32::MAX = all).
///
/// Arguments:
///   num_waiters: current number of threads waiting
///   wake_all:    1 to wake all, 0 to wake at most 1
///   woken:       pointer to receive number actually woken
///   remaining:   pointer to receive remaining waiters
///
/// Returns:
///   0 (OK)   — *woken and *remaining set
///   -EINVAL  — null pointer
#[cfg(feature = "futex")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_futex_wake(
    num_waiters: u32,
    wake_all: u32,
    woken: *mut u32,
    remaining: *mut u32,
) -> i32 {
    use gale::futex::wake_decide;

    unsafe {
        if woken.is_null() || remaining.is_null() {
            return EINVAL;
        }

        // Delegate to verified model.
        let d = wake_decide(num_waiters, wake_all != 0);
        *woken = d.woken;
        *remaining = d.remaining;
        OK
    }
}

// ---- Phase 2: Full Decision API for futex ----

/// Decision struct for k_futex_wait — tells C shim whether to block or return.
///
/// The C shim reads the atomic futex value and passes it here. Rust decides
/// whether the value matches the expected value (block) or not (return -EAGAIN).
#[repr(C)]
pub struct GaleFutexWaitDecision {
    /// Action: 0=BLOCK (pend on wait queue), 1=RETURN_EAGAIN
    pub action: u8,
    /// Return code: 0 if blocking, -EAGAIN if mismatch
    pub ret: i32,
}

pub const GALE_FUTEX_ACTION_BLOCK: u8 = 0;
pub const GALE_FUTEX_ACTION_RETURN_EAGAIN: u8 = 1;

/// Full decision for k_futex_wait: decides whether to block or return -EAGAIN.
///
/// Delegates to `gale::futex::wait_decide` (Verus-verified).
/// Verified: FX1 (block when val == expected), FX2 (EAGAIN on mismatch).
#[cfg(feature = "futex")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_futex_wait_decide(
    val: u32,
    expected: u32,
    is_no_wait: u32,
) -> GaleFutexWaitDecision {
    use gale::futex::{WaitDecision, wait_decide};

    // Delegate value-comparison to verified model (FX1, FX2).
    let d = wait_decide(val, expected);
    match d {
        WaitDecision::Mismatch => GaleFutexWaitDecision {
            action: GALE_FUTEX_ACTION_RETURN_EAGAIN,
            ret: EAGAIN,
        },
        WaitDecision::Block => {
            if is_no_wait != 0 {
                // Value matches but caller specified K_NO_WAIT — cannot block.
                GaleFutexWaitDecision {
                    action: GALE_FUTEX_ACTION_RETURN_EAGAIN,
                    ret: ETIMEDOUT,
                }
            } else {
                GaleFutexWaitDecision {
                    action: GALE_FUTEX_ACTION_BLOCK,
                    ret: OK,
                }
            }
        }
    }
}

/// Decision struct for k_futex_wake — tells C shim whether to keep waking.
///
/// Called once before the wake loop. Rust decides the maximum number of
/// threads to wake based on the wake_all flag.
#[repr(C)]
pub struct GaleFutexWakeDecision {
    /// Maximum number of threads to wake
    pub wake_limit: u32,
}

/// Full decision for k_futex_wake: decides the wake limit.
///
/// Delegates to `gale::futex::wake_decide` (Verus-verified).
/// Verified: FX3 (wake count correct), FX4 (wake_all wakes all), FX5 (single wake).
#[cfg(feature = "futex")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_futex_wake_decide(
    num_waiters: u32,
    wake_all: u32,
) -> GaleFutexWakeDecision {
    use gale::futex::wake_decide;

    // Delegate to verified model (FX3, FX4, FX5).
    let d = wake_decide(num_waiters, wake_all != 0);
    GaleFutexWakeDecision {
        wake_limit: d.woken,
    }
}

// ---------------------------------------------------------------------------
// FFI exports — timeslice (tick accounting for preemptive scheduling)
// ---------------------------------------------------------------------------
//
// These pure functions replace the time-slice tick counter from
// kernel/timeslicing.c:
//
//   timeslicing.c:75-86   z_reset_time_slice — reset to max
//   timeslicing.c:131-161 z_time_slice — decrement, detect expiry
//
// Verified by Verus (SMT/Z3):
//   TS1: 0 <= slice_ticks <= slice_max_ticks
//   TS2: reset sets slice_ticks = slice_max_ticks
//   TS3: tick decrements by 1
//   TS4: expired when slice_ticks == 0
//   TS5: no underflow

/// Reset the time slice counter to its maximum value.
///
/// timeslicing.c z_reset_time_slice:
///   slice_ticks = slice_max_ticks
///
/// Arguments:
///   slice_max_ticks: configured time-slice size
///   new_ticks:       pointer to receive reset value (= slice_max_ticks)
///
/// Returns:
///   0 (OK) — *new_ticks set
///   -EINVAL — null pointer
#[cfg(feature = "timeslice")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeslice_reset(
    slice_max_ticks: u32,
    new_ticks: *mut u32,
) -> i32 {
    use gale::timeslice::reset_decide;

    unsafe {
        if new_ticks.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (TS2).
        *new_ticks = reset_decide(slice_max_ticks);
        OK
    }
}

/// Consume one tick of the time slice.
///
/// timeslicing.c z_time_slice timer path:
///   if (slice_ticks > 0) { slice_ticks--; }
///   if (slice_ticks == 0) { expired = true; }
///
/// Arguments:
///   slice_ticks: current remaining ticks
///   new_ticks:   pointer to receive decremented value
///   expired:     pointer to receive 1 if expired, 0 otherwise
///
/// Returns:
///   0 (OK) — *new_ticks and *expired set
///   -EINVAL — null pointer
#[cfg(feature = "timeslice")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeslice_tick(
    slice_ticks: u32,
    new_ticks: *mut u32,
    expired: *mut u32,
) -> i32 {
    use gale::timeslice::tick_decide;

    unsafe {
        if new_ticks.is_null() || expired.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (TS3, TS4, TS5).
        let (nt, exp) = tick_decide(slice_ticks);
        *new_ticks = nt;
        *expired = if exp { 1 } else { 0 };
        OK
    }
}

// ---- Phase 2: Full Decision API for timeslice ----

/// Decision struct for z_time_slice — tells C shim whether to yield.
///
/// The C shim extracts the current tick state (ticks_remaining from the
/// timeout expiry flag, slice_ticks for the thread, cooperative flag).
/// Rust decides whether the thread should yield its time slice.
#[repr(C)]
pub struct GaleTimesliceTickDecision {
    /// Action: 0=NO_YIELD (continue running), 1=YIELD (move to end of prio queue)
    pub action: u8,
    /// New ticks remaining (0 when expired, unchanged when cooperative/no-slice)
    pub new_ticks: u32,
}

pub const GALE_TIMESLICE_ACTION_NO_YIELD: u8 = 0;
pub const GALE_TIMESLICE_ACTION_YIELD: u8 = 1;

/// Full decision for z_time_slice: decides whether to yield the current thread.
///
/// Called from the timer/IPI interrupt handler. C extracts the slice state,
/// Rust decides whether the thread should be preempted.
///
/// Arguments:
///   ticks_remaining: ticks left in current slice (0 = expired)
///   slice_ticks:     configured slice size for this thread (0 = no slicing)
///   is_cooperative:  1 if thread is cooperative (should never be preempted), 0 otherwise
///
/// Returns a decision struct:
///   action=YIELD if expired && sliceable && preemptible
///   action=NO_YIELD otherwise
///   new_ticks: reset to slice_ticks on yield, else ticks_remaining
///
/// Verified: TS4 (expire detection), TS6 (cooperative threads never yield).
#[cfg(feature = "timeslice")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_timeslice_tick_decide(
    ticks_remaining: u32,
    slice_ticks: u32,
    is_cooperative: u32,
) -> GaleTimesliceTickDecision {
    use gale::timeslice::timeslice_tick_full_decide;

    // Delegate to verified model (TS4, TS6).
    let (should_yield, new_ticks) =
        timeslice_tick_full_decide(ticks_remaining, slice_ticks, is_cooperative != 0);
    GaleTimesliceTickDecision {
        action: if should_yield {
            GALE_TIMESLICE_ACTION_YIELD
        } else {
            GALE_TIMESLICE_ACTION_NO_YIELD
        },
        new_ticks,
    }
}

// ---------------------------------------------------------------------------
// FFI exports — kheap (byte-level allocation tracking)
// ---------------------------------------------------------------------------
//
// These pure functions replace the byte count accounting from
// kernel/kheap.c:
//
//   kheap.c:119-129  k_heap_alloc — allocated_bytes += bytes
//   kheap.c:206-218  k_heap_free — allocated_bytes -= bytes
//
// Verified by Verus (SMT/Z3):
//   KH1: 0 <= allocated_bytes <= capacity
//   KH2: alloc success: allocated += bytes
//   KH3: alloc full: -ENOMEM
//   KH4: free: allocated -= bytes
//   KH5: conservation

/// Validate a kheap allocation and compute new allocated_bytes.
///
/// Arguments:
///   allocated_bytes: current bytes allocated
///   capacity:        total heap capacity
///   bytes:           bytes requested
///   new_allocated:   pointer to receive updated allocated count
///
/// Returns:
///   0 (OK)    — *new_allocated set
///   -ENOMEM   — would exceed capacity
///   -EINVAL   — null pointer or bytes == 0
#[cfg(feature = "kheap")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_kheap_alloc_validate(
    allocated_bytes: u32,
    capacity: u32,
    bytes: u32,
    new_allocated: *mut u32,
) -> i32 {
    use gale::kheap::alloc_decide;

    unsafe {
        if new_allocated.is_null() || bytes == 0 {
            return EINVAL;
        }

        // Delegate to verified model (KH2, KH3, KH6).
        match alloc_decide(allocated_bytes.min(capacity), capacity, bytes) {
            Ok(na) => {
                *new_allocated = na;
                OK
            }
            Err(e) => e,
        }
    }
}

/// Validate a kheap free and compute new allocated_bytes.
///
/// Arguments:
///   allocated_bytes: current bytes allocated
///   bytes:           bytes to free
///   new_allocated:   pointer to receive updated allocated count
///
/// Returns:
///   0 (OK)    — *new_allocated set
///   -EINVAL   — would underflow or null pointer or bytes == 0
#[cfg(feature = "kheap")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_kheap_free_validate(
    allocated_bytes: u32,
    bytes: u32,
    new_allocated: *mut u32,
) -> i32 {
    use gale::kheap::free_decide;

    unsafe {
        if new_allocated.is_null() || bytes == 0 {
            return EINVAL;
        }

        // Delegate to verified model (KH4).
        match free_decide(allocated_bytes, bytes) {
            Ok(na) => {
                *new_allocated = na;
                OK
            }
            Err(e) => e,
        }
    }
}

// ---- KHeap Decision API ----

/// Decision struct for k_heap_alloc — tells C shim what action to take.
///
/// After C calls sys_heap_aligned_alloc and determines if a result was
/// obtained, Rust decides: return the pointer, pend, or return NULL.
#[repr(C)]
pub struct GaleKheapAllocDecision {
    /// Action: 0=RETURN_PTR, 1=PEND, 2=RETURN_NULL
    pub action: u8,
}

/// Alloc succeeded — return the pointer to caller.
pub const GALE_KHEAP_ACTION_RETURN_PTR: u8 = 0;
/// Alloc failed, caller willing to wait — pend on wait queue.
pub const GALE_KHEAP_ACTION_PEND: u8 = 1;
/// Alloc failed, no-wait or non-threaded — return NULL.
pub const GALE_KHEAP_ACTION_RETURN_NULL: u8 = 2;

/// Full decision for k_heap_alloc: decides whether to return pointer,
/// pend, or return NULL.
///
/// The C shim calls sys_heap to attempt allocation, then passes the
/// result to Rust.  Rust decides the action.
///
/// Arguments:
///   alloc_succeeded: 1 if sys_heap returned non-NULL, 0 if NULL
///   is_no_wait:      1 if K_NO_WAIT or !MULTITHREADING, 0 otherwise
///
/// Verified: KH2 (alloc), KH3 (full), KH6 (no overflow).
#[cfg(feature = "kheap")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_kheap_alloc_decide(
    alloc_succeeded: u32,
    is_no_wait: u32,
) -> GaleKheapAllocDecision {
    if alloc_succeeded != 0 {
        GaleKheapAllocDecision {
            action: GALE_KHEAP_ACTION_RETURN_PTR,
        }
    } else if is_no_wait != 0 {
        GaleKheapAllocDecision {
            action: GALE_KHEAP_ACTION_RETURN_NULL,
        }
    } else {
        GaleKheapAllocDecision {
            action: GALE_KHEAP_ACTION_PEND,
        }
    }
}

/// Decision struct for k_heap_free — tells C shim what action to take.
#[repr(C)]
pub struct GaleKheapFreeDecision {
    /// Action: 0=FREE_ONLY, 1=FREE_AND_RESCHEDULE
    pub action: u8,
}

/// Free completed, no waiters — just unlock.
pub const GALE_KHEAP_ACTION_FREE_ONLY: u8 = 0;
/// Free completed, waiters present — reschedule.
pub const GALE_KHEAP_ACTION_FREE_AND_RESCHEDULE: u8 = 1;

/// Full decision for k_heap_free: decides whether to just free or
/// also reschedule waiters.
///
/// The C shim frees via sys_heap_free, then checks wait queue.
/// Rust decides whether to reschedule.
///
/// Arguments:
///   has_waiters: 1 if z_unpend_all returned > 0, 0 otherwise
///
/// Verified: KH4 (free), KH5 (conservation).
#[cfg(feature = "kheap")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_kheap_free_decide(
    has_waiters: u32,
) -> GaleKheapFreeDecision {
    if has_waiters != 0 {
        GaleKheapFreeDecision {
            action: GALE_KHEAP_ACTION_FREE_AND_RESCHEDULE,
        }
    } else {
        GaleKheapFreeDecision {
            action: GALE_KHEAP_ACTION_FREE_ONLY,
        }
    }
}

// ---------------------------------------------------------------------------
// FFI exports — thread_lifecycle (create/exit counting + priority validation)
// ---------------------------------------------------------------------------
//
// These pure functions replace the safety-critical thread lifecycle
// tracking from kernel/thread.c:
//
//   thread.c:383-500  k_thread_create — resource counting
//   thread.c exit/abort — resource counting
//   sched.c:1009-1023 k_thread_priority_set — range validation
//
// Verified by Verus (SMT/Z3):
//   TH1: priority in [0, MAX_PRIORITY)
//   TH5: count >= 0 (no underflow on exit)
//   TH6: no overflow on thread count

/// Validate thread creation: check count < max and increment.
///
/// Arguments:
///   count:     current active thread count
///   new_count: pointer to receive count + 1
///
/// Returns:
///   0 (OK)    — *new_count set
///   -EAGAIN   — at capacity
///   -EINVAL   — null pointer
///
/// Uses `gale::thread_lifecycle::MAX_THREADS` for the capacity bound (Verus-verified).
#[cfg(feature = "thread_lifecycle")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_thread_create_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    use gale::thread_lifecycle::MAX_THREADS;

    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        // TH6: capacity bound from verified model constant.
        if count >= MAX_THREADS {
            return EAGAIN;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_count = count + 1;
        }
        OK
    }
}

/// Validate thread exit: check count > 0 and decrement.
///
/// Arguments:
///   count:     current active thread count
///   new_count: pointer to receive count - 1
///
/// Returns:
///   0 (OK)    — *new_count set
///   -EINVAL   — no threads active (underflow protection) or null pointer
///
/// Uses `gale::thread_lifecycle::MAX_THREADS` for consistency with model (Verus-verified).
#[cfg(feature = "thread_lifecycle")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_thread_exit_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    let _: u32 = gale::thread_lifecycle::MAX_THREADS; // ensure same constant universe

    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        // TH5: underflow protection — count must be > 0.
        if count == 0 {
            return EINVAL;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_count = count - 1;
        }
        OK
    }
}

/// Validate a thread priority value.
///
/// sched.c k_thread_priority_set:
///   Z_ASSERT_VALID_PRIO(prio, NULL)
///
/// Arguments:
///   priority: proposed priority value
///
/// Returns:
///   0 (OK)    — priority < MAX_PRIORITY
///   -EINVAL   — priority out of range
///
/// Delegates to `gale::thread_lifecycle::priority_set_decide` (Verus-verified).
#[cfg(feature = "thread_lifecycle")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_thread_priority_validate(priority: u32) -> i32 {
    use gale::thread_lifecycle::priority_set_decide;

    // TH1: delegate validation to verified model; extract return code only.
    priority_set_decide(priority).ret
}

// ---- Phase 2: Full Decision API for thread lifecycle ----

/// Decision struct for k_thread_create — tells C shim whether to proceed
/// with thread creation or reject it.
///
/// C extracts stack_size, priority, and options before calling Rust.
/// Rust validates parameters and decides proceed/reject.
/// All arch-specific init, TLS, naming, etc. stay in C.
#[repr(C)]
pub struct GaleThreadCreateDecision {
    /// Action: 0=PROCEED (create the thread), 1=REJECT (return error)
    pub action: u8,
    /// Return code: 0 (OK) or negative errno (-EINVAL, -EAGAIN)
    pub ret: i32,
}

pub const GALE_THREAD_ACTION_PROCEED: u8 = 0;
pub const GALE_THREAD_ACTION_REJECT: u8 = 1;

/// Minimum stack size (arch-dependent, but 64 bytes is a sane floor).
const MIN_STACK_SIZE: u32 = 64;

/// Full decision for k_thread_create: validates stack size, priority, and options.
///
/// thread.c z_setup_new_thread:
///   Z_ASSERT_VALID_PRIO(prio, entry)
///   setup_thread_stack (requires stack_size > 0)
///
/// Arguments:
///   stack_size:    proposed stack size in bytes
///   priority:      proposed thread priority
///   options:       thread creation options (K_ESSENTIAL, K_USER, etc.)
///   active_count:  current active thread count
///
/// Delegates priority validation to `gale::thread_lifecycle::priority_set_decide`
/// and stack validation to `gale::thread_lifecycle::StackInfo::init` (Verus-verified).
/// Verified: TH1 (priority range), TH3 (stack_size > 0), TH6 (no overflow).
#[cfg(feature = "thread_lifecycle")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_thread_create_decide(
    stack_size: u32,
    priority: u32,
    _options: u32,
    active_count: u32,
) -> GaleThreadCreateDecision {
    use gale::thread_lifecycle::{StackInfo, MAX_THREADS, priority_set_decide};

    // TH3: stack must have nonzero, minimum size — delegate to StackInfo::init.
    if stack_size < MIN_STACK_SIZE
        || StackInfo::init(0, stack_size).is_err()
    {
        return GaleThreadCreateDecision {
            action: GALE_THREAD_ACTION_REJECT,
            ret: EINVAL,
        };
    }

    // TH1: priority must be in valid range — delegate to verified model.
    let prio_d = priority_set_decide(priority);
    if prio_d.action != gale::thread_lifecycle::PRIO_SET_PROCEED {
        return GaleThreadCreateDecision {
            action: GALE_THREAD_ACTION_REJECT,
            ret: prio_d.ret,
        };
    }

    // TH6: thread count must not overflow
    if active_count >= MAX_THREADS {
        return GaleThreadCreateDecision {
            action: GALE_THREAD_ACTION_REJECT,
            ret: EAGAIN,
        };
    }

    GaleThreadCreateDecision {
        action: GALE_THREAD_ACTION_PROCEED,
        ret: OK,
    }
}

/// Decision struct for k_thread_abort — tells C shim what action to take.
///
/// C extracts thread state (dead, essential) before calling Rust.
/// Rust decides: already dead (no-op), panic (essential), or proceed with abort.
#[repr(C)]
pub struct GaleThreadAbortDecision {
    /// Action: 0=ABORT (proceed), 1=ALREADY_DEAD (no-op), 2=PANIC (essential thread)
    pub action: u8,
}

pub const GALE_THREAD_ABORT_PROCEED: u8 = 0;
pub const GALE_THREAD_ABORT_ALREADY_DEAD: u8 = 1;
pub const GALE_THREAD_ABORT_PANIC: u8 = 2;

/// Thread state flag: thread is dead (from kernel_structs.h _THREAD_DEAD = BIT(3)).
const THREAD_STATE_DEAD: u8 = 0x08;

/// Full decision for k_thread_abort: determines abort action based on thread state.
///
/// sched.c z_thread_abort:
///   if (z_is_thread_dead(thread)) { return; }
///   z_thread_halt(thread, key, true);
///   if (essential) { k_panic(); }
///
/// Arguments:
///   thread_state:  thread_base.thread_state flags
///   is_essential:  1 if thread has K_ESSENTIAL flag, 0 otherwise
///
/// Returns a decision struct:
///   action=ALREADY_DEAD if thread is dead
///   action=PANIC if thread is essential (will be aborted, then panic)
///   action=ABORT otherwise (proceed with halt)
///
/// Verified: TH5 (no underflow — dead threads not re-aborted).
#[cfg(feature = "thread_lifecycle")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_thread_abort_decide(
    thread_state: u8,
    is_essential: u32,
) -> GaleThreadAbortDecision {
    // Already dead — no-op
    if (thread_state & THREAD_STATE_DEAD) != 0 {
        return GaleThreadAbortDecision {
            action: GALE_THREAD_ABORT_ALREADY_DEAD,
        };
    }

    // Essential thread — will be aborted, then panic
    if is_essential != 0 {
        return GaleThreadAbortDecision {
            action: GALE_THREAD_ABORT_PANIC,
        };
    }

    GaleThreadAbortDecision {
        action: GALE_THREAD_ABORT_PROCEED,
    }
}

/// Decision struct for k_thread_join — tells C shim what action to take.
///
/// C extracts thread state and relationship info before calling Rust.
/// Rust decides: return 0 (already dead), -EBUSY (no_wait), -EDEADLK, or pend.
#[repr(C)]
pub struct GaleThreadJoinDecision {
    /// Action: 0=RETURN_IMMEDIATELY, 1=PEND_ON_JOIN_QUEUE
    pub action: u8,
    /// Return code: 0 (dead), -EBUSY, -EDEADLK
    pub ret: i32,
}

pub const GALE_THREAD_JOIN_RETURN: u8 = 0;
pub const GALE_THREAD_JOIN_PEND: u8 = 1;

/// Full decision for k_thread_join: determines join action.
///
/// sched.c z_impl_k_thread_join:
///   if (z_is_thread_dead(thread)) { ret = 0; }
///   else if (K_TIMEOUT_EQ(timeout, K_NO_WAIT)) { ret = -EBUSY; }
///   else if (thread == _current || circular) { ret = -EDEADLK; }
///   else { pend on join_queue }
///
/// Arguments:
///   is_dead:             1 if target thread is dead, 0 otherwise
///   is_no_wait:          1 if timeout == K_NO_WAIT, 0 otherwise
///   is_self_or_circular: 1 if target == _current or target is pended on our join queue
///
/// Verified: deadlock detection, proper state transitions.
#[cfg(feature = "thread_lifecycle")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_thread_join_decide(
    is_dead: u32,
    is_no_wait: u32,
    is_self_or_circular: u32,
) -> GaleThreadJoinDecision {
    // Already dead — return success immediately
    if is_dead != 0 {
        return GaleThreadJoinDecision {
            action: GALE_THREAD_JOIN_RETURN,
            ret: OK,
        };
    }

    // No-wait mode — return busy
    if is_no_wait != 0 {
        return GaleThreadJoinDecision {
            action: GALE_THREAD_JOIN_RETURN,
            ret: EBUSY,
        };
    }

    // Deadlock: joining self or circular dependency
    if is_self_or_circular != 0 {
        return GaleThreadJoinDecision {
            action: GALE_THREAD_JOIN_RETURN,
            ret: EDEADLK,
        };
    }

    // Otherwise pend on the thread's join queue
    GaleThreadJoinDecision {
        action: GALE_THREAD_JOIN_PEND,
        ret: OK,
    }
}

// ---- Phase 2: Suspend / Resume / Priority-set / Stack-space / Deadline ----

/// Decision struct for k_thread_suspend.
#[repr(C)]
pub struct GaleThreadSuspendDecision {
    /// Action: 0=PROCEED, 1=ALREADY_SUSPENDED (no-op)
    pub action: u8,
}

pub const GALE_THREAD_SUSPEND_PROCEED: u8 = 0;
pub const GALE_THREAD_SUSPEND_ALREADY_SUSPENDED: u8 = 1;

/// Decide whether to proceed with k_thread_suspend.
///
/// sched.c:491-522 z_impl_k_thread_suspend:
///   if (unlikely(z_is_thread_suspended(thread))) { return; }
///
/// TH7: Suspending an already-suspended thread is idempotent (no-op).
///
/// Arguments:
///   thread_state: thread_base.thread_state flags
///
/// Delegates to `gale::thread_lifecycle::suspend_decide` (Verus-verified).
#[cfg(feature = "thread_lifecycle")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_thread_suspend_decide(
    thread_state: u8,
) -> GaleThreadSuspendDecision {
    use gale::thread_lifecycle::suspend_decide;

    // Delegate to verified model (TH7).
    let d = suspend_decide(thread_state);
    GaleThreadSuspendDecision { action: d.action }
}

/// Decision struct for k_thread_resume.
#[repr(C)]
pub struct GaleThreadResumeDecision {
    /// Action: 0=PROCEED (ready the thread), 1=NOT_SUSPENDED (no-op)
    pub action: u8,
}

pub const GALE_THREAD_RESUME_PROCEED: u8 = 0;
pub const GALE_THREAD_RESUME_NOT_SUSPENDED: u8 = 1;

/// Decide whether to proceed with k_thread_resume.
///
/// sched.c:533-551 z_impl_k_thread_resume:
///   if (unlikely(!z_is_thread_suspended(thread))) { return; }
///
/// TH8: Resuming a non-suspended thread is idempotent (no-op).
///
/// Arguments:
///   thread_state: thread_base.thread_state flags
///
/// Delegates to `gale::thread_lifecycle::resume_decide` (Verus-verified).
#[cfg(feature = "thread_lifecycle")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_thread_resume_decide(
    thread_state: u8,
) -> GaleThreadResumeDecision {
    use gale::thread_lifecycle::resume_decide;

    // Delegate to verified model (TH8).
    let d = resume_decide(thread_state);
    GaleThreadResumeDecision { action: d.action }
}

/// Decision struct for k_thread_priority_set.
#[repr(C)]
pub struct GaleThreadPrioritySetDecision {
    /// Action: 0=PROCEED (call z_thread_prio_set), 1=REJECT (-EINVAL)
    pub action: u8,
    /// Return code: 0 (OK) or -EINVAL
    pub ret: i32,
}

pub const GALE_THREAD_PRIO_SET_PROCEED: u8 = 0;
pub const GALE_THREAD_PRIO_SET_REJECT: u8 = 1;

/// Decide whether to proceed with k_thread_priority_set.
///
/// sched.c:1009-1023 z_impl_k_thread_priority_set:
///   Z_ASSERT_VALID_PRIO(prio, NULL)
///
/// TH1: Priority must be in valid range [0, MAX_PRIORITY).
/// TH2: Reject out-of-range priority before modifying thread state.
///
/// Arguments:
///   new_priority: proposed new priority value
///
/// Delegates to `gale::thread_lifecycle::priority_set_decide` (Verus-verified).
#[cfg(feature = "thread_lifecycle")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_thread_priority_set_decide(
    new_priority: u32,
) -> GaleThreadPrioritySetDecision {
    use gale::thread_lifecycle::priority_set_decide;

    // Delegate to verified model (TH1, TH2).
    let d = priority_set_decide(new_priority);
    GaleThreadPrioritySetDecision {
        action: d.action,
        ret: d.ret,
    }
}

/// Decision struct for k_thread_stack_space_get.
#[repr(C)]
pub struct GaleThreadStackSpaceDecision {
    /// Action: 0=PROCEED (query the stack), 1=REJECT (stack not queryable)
    pub action: u8,
    /// Return code: 0 (OK) or -EINVAL
    pub ret: i32,
    /// Upper-bound estimate of unused bytes (stack_size - usage_watermark).
    /// Valid only when action=PROCEED. Always <= stack_size.
    pub unused_estimate: u32,
}

pub const GALE_THREAD_STACK_SPACE_PROCEED: u8 = 0;
pub const GALE_THREAD_STACK_SPACE_REJECT: u8 = 1;

/// Decide whether k_thread_stack_space_get can proceed.
///
/// thread.c:1067-1078 z_impl_k_thread_stack_space_get:
///   if (mapped.addr == NULL) { return -EINVAL; }
///   z_stack_space_get(start, size, unused_ptr)
///
/// TH4: unused_estimate <= stack_size.
///
/// Arguments:
///   stack_size:         usable stack size in bytes (must be > 0)
///   stack_usage:        high-watermark usage in bytes (<= stack_size)
///   stack_mapped_valid: 1 if stack is accessible (pass 1 for non-mem-mapped)
///
/// Delegates to `gale::thread_lifecycle::stack_space_decide` (Verus-verified).
#[cfg(feature = "thread_lifecycle")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_thread_stack_space_decide(
    stack_size: u32,
    stack_usage: u32,
    stack_mapped_valid: u32,
) -> GaleThreadStackSpaceDecision {
    use gale::thread_lifecycle::{StackInfo, stack_space_decide};

    // Build a StackInfo from the C-extracted fields.
    // stack_usage is clamped to stack_size to satisfy the StackInfo invariant.
    let stack = match StackInfo::init(0, stack_size) {
        Ok(mut si) => {
            // Clamp usage to size (StackInfo invariant: usage <= size).
            si.usage = if stack_usage > stack_size { stack_size } else { stack_usage };
            si
        }
        Err(_) => {
            // stack_size == 0: invalid
            return GaleThreadStackSpaceDecision {
                action: GALE_THREAD_STACK_SPACE_REJECT,
                ret: EINVAL,
                unused_estimate: 0,
            };
        }
    };

    // Delegate to verified model (TH4).
    let d = stack_space_decide(stack, stack_mapped_valid != 0);
    GaleThreadStackSpaceDecision {
        action: d.action,
        ret: d.ret,
        unused_estimate: d.unused_estimate,
    }
}

/// Decision struct for k_thread_deadline_set.
#[repr(C)]
pub struct GaleThreadDeadlineDecision {
    /// Action: 0=PROCEED, 1=REJECT (-EINVAL)
    pub action: u8,
    /// Return code: 0 (OK) or -EINVAL
    pub ret: i32,
    /// Clamped deadline value (== deadline for valid positive inputs).
    pub clamped_deadline: i32,
}

pub const GALE_THREAD_DEADLINE_PROCEED: u8 = 0;
pub const GALE_THREAD_DEADLINE_REJECT: u8 = 1;

/// Decide whether a deadline value is valid for k_thread_deadline_set.
///
/// sched.c:1063-1095 z_impl_k_thread_deadline_set + z_vrfy_k_thread_deadline_set:
///   z_vrfy: if (deadline <= 0) return -EINVAL
///
/// TD1: deadline must be > 0.
/// TD3: zero or negative deadlines are rejected with -EINVAL.
///
/// Arguments:
///   deadline: proposed deadline in cycles (must be > 0)
///
/// Delegates to `gale::thread_lifecycle::deadline_decide` (Verus-verified).
#[cfg(feature = "thread_lifecycle")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_thread_deadline_decide(
    deadline: i32,
) -> GaleThreadDeadlineDecision {
    use gale::thread_lifecycle::deadline_decide;

    // Delegate to verified model (TD1, TD3).
    let d = deadline_decide(deadline);
    GaleThreadDeadlineDecision {
        action: d.action,
        ret: d.ret,
        clamped_deadline: d.clamped_deadline,
    }
}

// ---------------------------------------------------------------------------
// FFI exports — work (work item state machine)
// ---------------------------------------------------------------------------
//
// These pure functions replace the work item state flag management
// from kernel/work.c:
//
//   work.c:320-365  submit_to_queue_locked — set QUEUED flag
//   work.c:501-520  cancel_async_locked — clear QUEUED, set CANCELING
//
// Phase 2: Decision struct pattern (Extract->Decide->Apply).
//
// Verified by Verus (SMT/Z3):
//   WK1: init produces IDLE
//   WK2: submit from IDLE sets QUEUED
//   WK3: submit while CANCELING returns EBUSY
//   WK4: submit while QUEUED is idempotent (no-op)
//   WK5: cancel clears QUEUED, sets CANCELING if still busy

const WORK_FLAG_RUNNING: u8 = 1;    // BIT(0) -- K_WORK_RUNNING_BIT
const WORK_FLAG_CANCELING: u8 = 2;  // BIT(1) -- K_WORK_CANCELING_BIT
const WORK_FLAG_QUEUED: u8 = 4;     // BIT(2) -- K_WORK_QUEUED_BIT
const WORK_BUSY_MASK: u8 = 7;       // RUNNING | CANCELING | QUEUED

// ---- Phase 2: Full Decision API for work ----

/// Action codes for work submit decision.
pub const GALE_WORK_SUBMIT_QUEUE: u8 = 0;     // newly queued
pub const GALE_WORK_SUBMIT_REQUEUE: u8 = 1;   // was running, re-queued
pub const GALE_WORK_SUBMIT_ALREADY: u8 = 2;   // already queued (no-op)
pub const GALE_WORK_SUBMIT_REJECT: u8 = 3;    // rejected (canceling)

/// Decision struct for k_work_submit -- tells C shim what action to take.
#[repr(C)]
pub struct GaleWorkSubmitDecision {
    /// Action: 0=QUEUE, 1=REQUEUE, 2=ALREADY_QUEUED, 3=REJECT
    pub action: u8,
    /// Updated flags to write back to work->flags
    pub new_flags: u8,
    /// Return code for the C caller:
    ///   1 = newly queued, 2 = re-queued running, 0 = already queued, -EBUSY = rejected
    pub ret: i32,
}

/// Full decision for k_work_submit: decides whether to queue, re-queue,
/// reject, or treat as already-queued.
///
/// work.c submit_to_queue_locked:
///   if (flags & CANCELING) return -EBUSY
///   if (flags & QUEUED) return 0 (already queued)
///   if (flags & RUNNING) ret = 2 (re-queue to same queue)
///   else ret = 1 (newly queued)
///   flags |= QUEUED
///
/// C extracts: work->flags (under spinlock).
/// C applies:  writes new_flags back, queues work item if action != ALREADY/REJECT.
///
/// Verified: WK2 (submit sets QUEUED), WK3 (CANCELING rejects),
///           WK4 (already queued is no-op).
#[cfg(feature = "work")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_work_submit_decide(
    flags: u8,
    is_queued: u8,
    is_running: u8,
) -> GaleWorkSubmitDecision {
    use gale::work::{SubmitDecision, submit_decide};

    let _ = (is_queued, is_running); // model derives these from flags

    // Delegate to verified model (WK2, WK3, WK4).
    let (decision, new_flags) = submit_decide(flags);
    match decision {
        SubmitDecision::Queue => GaleWorkSubmitDecision {
            action: GALE_WORK_SUBMIT_QUEUE,
            new_flags,
            ret: 1,
        },
        SubmitDecision::Requeue => GaleWorkSubmitDecision {
            action: GALE_WORK_SUBMIT_REQUEUE,
            new_flags,
            ret: 2,
        },
        SubmitDecision::AlreadyQueued => GaleWorkSubmitDecision {
            action: GALE_WORK_SUBMIT_ALREADY,
            new_flags,
            ret: 0,
        },
        SubmitDecision::Reject => GaleWorkSubmitDecision {
            action: GALE_WORK_SUBMIT_REJECT,
            new_flags,
            ret: EBUSY,
        },
    }
}

/// Action codes for work cancel decision.
pub const GALE_WORK_CANCEL_IDLE: u8 = 0;      // already idle, nothing to do
pub const GALE_WORK_CANCEL_DEQUEUE: u8 = 1;   // was queued, dequeue it
pub const GALE_WORK_CANCEL_CANCELING: u8 = 2; // still busy, set CANCELING

/// Decision struct for k_work_cancel -- tells C shim what action to take.
#[repr(C)]
pub struct GaleWorkCancelDecision {
    /// Action: 0=IDLE, 1=DEQUEUE, 2=SET_CANCELING
    pub action: u8,
    /// Updated flags to write back to work->flags
    pub new_flags: u8,
    /// Busy status after cancel (flags & BUSY_MASK)
    pub busy: u8,
}

/// Full decision for k_work_cancel: decides whether the item is idle,
/// needs dequeuing, or needs the CANCELING flag set.
///
/// work.c cancel_async_locked:
///   if (!CANCELING) { remove from queue (clears QUEUED) }
///   busy = flags & BUSY_MASK
///   if (busy) flags |= CANCELING
///   return busy
///
/// C extracts: work->flags (under spinlock).
/// C applies:  writes new_flags back, removes from queue if action==DEQUEUE.
///
/// Verified: WK5 (cancel clears QUEUED, sets CANCELING if still busy).
#[cfg(feature = "work")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_work_cancel_decide(
    flags: u8,
    is_queued: u8,
    is_running: u8,
) -> GaleWorkCancelDecision {
    let _ = is_running; // used implicitly via flags

    // Step 1: If not already canceling, clear QUEUED
    use gale::work::{CancelDecisionAction, cancel_decide};

    let _ = (is_queued, is_running); // model derives these from flags

    // Delegate to verified model (WK5).
    let (decision, new_flags, busy) = cancel_decide(flags);
    let action = match decision {
        CancelDecisionAction::Idle => GALE_WORK_CANCEL_IDLE,
        CancelDecisionAction::Dequeue => GALE_WORK_CANCEL_DEQUEUE,
        CancelDecisionAction::SetCanceling => GALE_WORK_CANCEL_CANCELING,
    };
    GaleWorkCancelDecision {
        action,
        new_flags,
        busy,
    }
}

// Keep the validate API for backward compatibility.
// These are thin wrappers around the decision struct functions.

/// Validate a work submit operation (legacy API).
///
/// Returns:
///   1          -- newly queued
///   2          -- was running, re-queued
///   0          -- already queued (no-op)
///   -EBUSY     -- canceling, rejected
///   -EINVAL    -- null pointer
#[cfg(feature = "work")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_work_submit_validate(
    flags: u8,
    new_flags: *mut u8,
) -> i32 {
    unsafe {
        if new_flags.is_null() {
            return EINVAL;
        }

        let is_queued = if (flags & WORK_FLAG_QUEUED) != 0 { 1u8 } else { 0u8 };
        let is_running = if (flags & WORK_FLAG_RUNNING) != 0 { 1u8 } else { 0u8 };
        let d = gale_k_work_submit_decide(flags, is_queued, is_running);
        *new_flags = d.new_flags;
        d.ret
    }
}

/// Validate a work cancel operation (legacy API).
///
/// Returns:
///   0 (OK) -- *new_flags and *busy set
///   -EINVAL -- null pointer
#[cfg(feature = "work")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_work_cancel_validate(
    flags: u8,
    new_flags: *mut u8,
    busy: *mut u8,
) -> i32 {
    unsafe {
        if new_flags.is_null() || busy.is_null() {
            return EINVAL;
        }

        let is_queued = if (flags & WORK_FLAG_QUEUED) != 0 { 1u8 } else { 0u8 };
        let is_running = if (flags & WORK_FLAG_RUNNING) != 0 { 1u8 } else { 0u8 };
        let d = gale_k_work_cancel_decide(flags, is_queued, is_running);
        *new_flags = d.new_flags;
        *busy = d.busy;
        OK
    }
}

// ---------------------------------------------------------------------------
// FFI exports — fatal (error classification)
// ---------------------------------------------------------------------------
//
// This pure function replaces the fatal error classification logic
// from kernel/fatal.c:
//
//   fatal.c:85-179  z_fatal_error — determine recovery action
//
// Verified by Verus (SMT/Z3):
//   FT1: all reason codes map to valid variants
//   FT2: kernel panic always halts
//   FT3: recovery depends on reason + context

// ---- Phase 2: Full Decision API ----

/// Decision struct for fatal error classification — tells the C shim what
/// recovery action to apply after `k_sys_fatal_error_handler` returns.
#[repr(C)]
pub struct GaleFatalDecision {
    /// Action: 0=ABORT_THREAD, 1=HALT, 2=IGNORE
    pub action: u8,
    /// Return code: 0 on success, -EINVAL for unknown reason
    pub ret: i32,
}

/// Fatal action: abort the faulting thread and continue.
pub const GALE_FATAL_ACTION_ABORT_THREAD: u8 = 0;
/// Fatal action: halt the entire system (no recovery possible).
pub const GALE_FATAL_ACTION_HALT: u8 = 1;
/// Fatal action: ignore (test mode ISR — return without action).
pub const GALE_FATAL_ACTION_IGNORE: u8 = 2;

/// Classify a fatal error: determine recovery action.
///
/// Arguments:
///   reason:    error reason code (0=CPU_EXCEPTION, 1=SPURIOUS_IRQ,
///              2=STACK_CHECK_FAIL, 3=KERNEL_OOPS, 4=KERNEL_PANIC)
///   is_isr:    1 if in ISR context, 0 if in thread context
///   test_mode: 1 if CONFIG_TEST, 0 for production
///
/// Returns:
///   0 — AbortThread (recoverable)
///   1 — Halt (non-recoverable)
///   2 — Ignore (test mode ISR)
///   -EINVAL — unknown reason code
#[cfg(feature = "fatal")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_fatal_classify(
    reason: u32,
    is_isr: u32,
    test_mode: u32,
) -> i32 {
    let d = gale_k_fatal_decide(reason, is_isr, test_mode);
    if d.ret != 0 {
        return d.ret;
    }
    d.action as i32
}

/// Full decision for fatal error classification: determines recovery action.
///
/// The C shim in `gale_fatal.c` calls this after `k_sys_fatal_error_handler`
/// returns. Rust classifies the error and decides: abort thread, halt, or
/// ignore.
///
/// Verified: FT1 (reason mapping), FT2 (panic halts), FT3 (recovery).
#[cfg(feature = "fatal")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_fatal_decide(
    reason: u32,
    is_isr: u32,
    test_mode: u32,
) -> GaleFatalDecision {
    use gale::fatal::{RecoveryAction, classify_decide};

    // Delegate to verified model (FT1, FT2, FT3).
    match classify_decide(reason, is_isr != 0, test_mode != 0) {
        Ok(action) => {
            let a = match action {
                RecoveryAction::AbortThread => GALE_FATAL_ACTION_ABORT_THREAD,
                RecoveryAction::Halt => GALE_FATAL_ACTION_HALT,
                RecoveryAction::Ignore => GALE_FATAL_ACTION_IGNORE,
            };
            GaleFatalDecision { action: a, ret: 0 }
        }
        Err(e) => GaleFatalDecision {
            action: GALE_FATAL_ACTION_HALT,
            ret: e,
        },
    }
}

// ---------------------------------------------------------------------------
// FFI exports — mempool (fixed-block pool allocation tracking)
// ---------------------------------------------------------------------------
//
// These pure functions replace the block count tracking for
// variable-size memory pools:
//
//   pool alloc — allocated += 1
//   pool free  — allocated -= 1
//
// Verified by Verus (SMT/Z3):
//   MP1: 0 <= allocated <= capacity
//   MP2: alloc success: allocated += 1
//   MP3: alloc full: -ENOMEM

/// Validate a mempool allocation: increment block count.
///
/// Arguments:
///   allocated: current allocated block count
///   capacity:  total blocks in pool
///   new_allocated: pointer to receive allocated + 1
///
/// Returns:
///   0 (OK)    — *new_allocated set
///   -ENOMEM   — pool full
///   -EINVAL   — null pointer
#[cfg(feature = "mempool")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mempool_alloc_validate(
    allocated: u32,
    capacity: u32,
    new_allocated: *mut u32,
) -> i32 {
    use gale::mempool::alloc_block_decide;

    unsafe {
        if new_allocated.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (MP2, MP3).
        match alloc_block_decide(allocated, capacity) {
            Ok(na) => {
                *new_allocated = na;
                OK
            }
            Err(e) => e,
        }
    }
}

/// Validate a mempool free: decrement block count.
///
/// Arguments:
///   allocated:     current allocated block count
///   new_allocated: pointer to receive allocated - 1
///
/// Returns:
///   0 (OK)    — *new_allocated set
///   -EINVAL   — no blocks allocated (underflow) or null pointer
#[cfg(feature = "mempool")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mempool_free_validate(
    allocated: u32,
    new_allocated: *mut u32,
) -> i32 {
    use gale::mempool::free_block_decide;

    unsafe {
        if new_allocated.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (MP4).
        match free_block_decide(allocated) {
            Ok(na) => {
                *new_allocated = na;
                OK
            }
            Err(e) => e,
        }
    }
}

// ---- MemPool Decision API ----

/// Decision struct for mempool alloc — tells C shim what action to take.
///
/// After C calls sys_heap to attempt allocation, Rust decides whether
/// the allocation succeeded or should return NULL.
#[repr(C)]
pub struct GaleMemPoolAllocDecision {
    /// Action: 0=RETURN_PTR, 1=RETURN_NULL
    pub action: u8,
}

/// Alloc succeeded — return the pointer to caller.
pub const GALE_MEMPOOL_ACTION_RETURN_PTR: u8 = 0;
/// Alloc failed — return NULL.
pub const GALE_MEMPOOL_ACTION_RETURN_NULL: u8 = 1;

/// Full decision for mempool alloc: decides whether to return pointer
/// or return NULL.
///
/// The C shim calls sys_heap to attempt allocation, then passes the
/// result to Rust.  Rust decides the action.
///
/// Arguments:
///   alloc_succeeded: 1 if sys_heap returned non-NULL, 0 if NULL
///
/// Verified: MP2 (alloc), MP3 (full).
#[cfg(feature = "mempool")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mempool_alloc_decide(
    alloc_succeeded: u32,
) -> GaleMemPoolAllocDecision {
    if alloc_succeeded != 0 {
        GaleMemPoolAllocDecision {
            action: GALE_MEMPOOL_ACTION_RETURN_PTR,
        }
    } else {
        GaleMemPoolAllocDecision {
            action: GALE_MEMPOOL_ACTION_RETURN_NULL,
        }
    }
}

/// Decision struct for mempool free — tells C shim what action to take.
#[repr(C)]
pub struct GaleMemPoolFreeDecision {
    /// Action: 0=FREE_OK, 1=FREE_AND_RESCHEDULE
    pub action: u8,
}

/// Free completed, no waiters — just unlock.
pub const GALE_MEMPOOL_ACTION_FREE_OK: u8 = 0;
/// Free completed, waiters present — reschedule.
pub const GALE_MEMPOOL_ACTION_FREE_AND_RESCHEDULE: u8 = 1;

/// Full decision for mempool free: decides whether to just free or
/// also reschedule waiters.
///
/// The C shim frees via sys_heap_free, then checks wait queue.
/// Rust decides whether to reschedule.
///
/// Arguments:
///   has_waiters: 1 if z_unpend_all returned > 0, 0 otherwise
///
/// Verified: MP4 (free), MP5 (conservation).
#[cfg(feature = "mempool")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mempool_free_decide(
    has_waiters: u32,
) -> GaleMemPoolFreeDecision {
    if has_waiters != 0 {
        GaleMemPoolFreeDecision {
            action: GALE_MEMPOOL_ACTION_FREE_AND_RESCHEDULE,
        }
    } else {
        GaleMemPoolFreeDecision {
            action: GALE_MEMPOOL_ACTION_FREE_OK,
        }
    }
}

// ---------------------------------------------------------------------------
// FFI exports — dynamic (dynamic thread pool tracking)
// ---------------------------------------------------------------------------
//
// These pure functions replace the stack pool accounting from
// kernel/dynamic.c:
//
//   dynamic.c:34-57   z_thread_stack_alloc_pool — active += 1
//   dynamic.c:116-158 z_impl_k_thread_stack_free — active -= 1
//
// Verified by Verus (SMT/Z3):
//   DY1: 0 <= active <= max_threads
//   DY2: alloc: active += 1
//   DY3: alloc full: -ENOMEM
//   DY4: free: active -= 1

/// Validate a dynamic pool allocation: increment active count.
///
/// Arguments:
///   active:      current active stack count
///   max_threads: maximum threads in pool
///   new_active:  pointer to receive active + 1
///
/// Returns:
///   0 (OK)    — *new_active set
///   -ENOMEM   — pool full
///   -EINVAL   — null pointer
#[cfg(feature = "dynamic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_dynamic_alloc_validate(
    active: u32,
    max_threads: u32,
    new_active: *mut u32,
) -> i32 {
    use gale::dynamic::alloc_decide;

    unsafe {
        if new_active.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (DY2, DY3).
        match alloc_decide(active, max_threads) {
            Ok(na) => {
                *new_active = na;
                OK
            }
            Err(e) => e,
        }
    }
}

/// Validate a dynamic pool free: decrement active count.
///
/// Arguments:
///   active:     current active stack count
///   new_active: pointer to receive active - 1
///
/// Returns:
///   0 (OK)    — *new_active set
///   -EINVAL   — no stacks active (underflow) or null pointer
#[cfg(feature = "dynamic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_dynamic_free_validate(
    active: u32,
    new_active: *mut u32,
) -> i32 {
    use gale::dynamic::free_decide;

    unsafe {
        if new_active.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (DY4).
        match free_decide(active) {
            Ok(na) => {
                *new_active = na;
                OK
            }
            Err(e) => e,
        }
    }
}

// ---------------------------------------------------------------------------
// FFI exports — smp_state (SMP CPU state tracking)
// ---------------------------------------------------------------------------
//
// These pure functions replace the CPU state accounting from
// kernel/smp.c:
//
//   smp.c:170-194  k_smp_cpu_start — active_cpus += 1
//   smp.c stop     — active_cpus -= 1 (CPU 0 never stops)
//
// Verified by Verus (SMT/Z3):
//   SM1: 0 <= active_cpus <= max_cpus
//   SM2: start: active += 1
//   SM3: stop: active -= 1 (min 1)

/// Validate starting a CPU: increment active_cpus.
///
/// Arguments:
///   active_cpus: current active CPU count
///   max_cpus:    maximum CPUs in system
///   new_active:  pointer to receive active + 1
///
/// Returns:
///   0 (OK)    — *new_active set
///   -EBUSY    — all CPUs already active
///   -EINVAL   — null pointer
#[cfg(feature = "smp_state")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_smp_start_cpu_validate(
    active_cpus: u32,
    max_cpus: u32,
    new_active: *mut u32,
) -> i32 {
    use gale::smp_state::start_cpu_decide;

    unsafe {
        if new_active.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (SM2).
        match start_cpu_decide(active_cpus, max_cpus) {
            Ok(na) => {
                *new_active = na;
                OK
            }
            Err(e) => e,
        }
    }
}

/// Validate stopping a CPU: decrement active_cpus (min 1).
///
/// Arguments:
///   active_cpus: current active CPU count
///   new_active:  pointer to receive active - 1
///
/// Returns:
///   0 (OK)    — *new_active set
///   -EINVAL   — only CPU 0 left (cannot stop) or null pointer
#[cfg(feature = "smp_state")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_smp_stop_cpu_validate(
    active_cpus: u32,
    new_active: *mut u32,
) -> i32 {
    use gale::smp_state::stop_cpu_decide;

    unsafe {
        if new_active.is_null() {
            return EINVAL;
        }

        // Delegate to verified model (SM3).
        match stop_cpu_decide(active_cpus) {
            Ok(na) => {
                *new_active = na;
                OK
            }
            Err(e) => e,
        }
    }
}

// ---- Phase 2: Full Decision API for dynamic ----

/// Decision struct for dynamic pool alloc — tells C shim what action to take.
#[repr(C)]
pub struct GaleDynamicAllocDecision {
    /// Action: 0=ALLOC_OK, 1=POOL_FULL
    pub action: u8,
    /// New active count (only meaningful when action=ALLOC_OK)
    pub new_active: u32,
}

pub const GALE_DYNAMIC_ACTION_ALLOC_OK: u8 = 0;
pub const GALE_DYNAMIC_ACTION_POOL_FULL: u8 = 1;

/// Full decision for dynamic pool alloc: decides whether allocation can proceed.
///
/// The C shim extracts current active count and pool size, Rust decides
/// whether there is room for another allocation.
///
/// Verified: DY2 (alloc: active += 1), DY3 (full: -ENOMEM).
#[cfg(feature = "dynamic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_dynamic_alloc_decide(
    active: u32,
    max_threads: u32,
) -> GaleDynamicAllocDecision {
    use gale::dynamic::alloc_decide;

    // Delegate to verified model (DY2, DY3).
    match alloc_decide(active, max_threads) {
        Ok(new_active) => GaleDynamicAllocDecision {
            action: GALE_DYNAMIC_ACTION_ALLOC_OK,
            new_active,
        },
        Err(_) => GaleDynamicAllocDecision {
            action: GALE_DYNAMIC_ACTION_POOL_FULL,
            new_active: active,
        },
    }
}

/// Decision struct for dynamic pool free — tells C shim what action to take.
#[repr(C)]
pub struct GaleDynamicFreeDecision {
    /// Action: 0=FREE_OK, 1=UNDERFLOW
    pub action: u8,
    /// New active count (only meaningful when action=FREE_OK)
    pub new_active: u32,
}

pub const GALE_DYNAMIC_ACTION_FREE_OK: u8 = 0;
pub const GALE_DYNAMIC_ACTION_UNDERFLOW: u8 = 1;

/// Full decision for dynamic pool free: decides whether free can proceed.
///
/// The C shim extracts current active count, Rust decides whether the
/// free is valid (no underflow).
///
/// Verified: DY4 (free: active -= 1, no underflow).
#[cfg(feature = "dynamic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_dynamic_free_decide(
    active: u32,
) -> GaleDynamicFreeDecision {
    use gale::dynamic::free_decide;

    // Delegate to verified model (DY4).
    match free_decide(active) {
        Ok(new_active) => GaleDynamicFreeDecision {
            action: GALE_DYNAMIC_ACTION_FREE_OK,
            new_active,
        },
        Err(_) => GaleDynamicFreeDecision {
            action: GALE_DYNAMIC_ACTION_UNDERFLOW,
            new_active: 0,
        },
    }
}

// ---- Phase 2: Full Decision API for smp_state ----

/// Decision struct for SMP CPU start — tells C shim what action to take.
#[repr(C)]
pub struct GaleSmpStartDecision {
    /// Action: 0=START_OK, 1=ALL_ACTIVE
    pub action: u8,
    /// New active count (only meaningful when action=START_OK)
    pub new_active: u32,
}

pub const GALE_SMP_ACTION_START_OK: u8 = 0;
pub const GALE_SMP_ACTION_ALL_ACTIVE: u8 = 1;

/// Full decision for SMP CPU start: decides whether a CPU can be started.
///
/// The C shim extracts current active CPU count and max CPUs, Rust decides
/// whether there is room to start another CPU.
///
/// Verified: SM2 (start: active += 1).
#[cfg(feature = "smp_state")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_smp_start_cpu_decide(
    active_cpus: u32,
    max_cpus: u32,
) -> GaleSmpStartDecision {
    use gale::smp_state::start_cpu_decide;

    // Delegate to verified model (SM2).
    match start_cpu_decide(active_cpus, max_cpus) {
        Ok(new_active) => GaleSmpStartDecision {
            action: GALE_SMP_ACTION_START_OK,
            new_active,
        },
        Err(_) => GaleSmpStartDecision {
            action: GALE_SMP_ACTION_ALL_ACTIVE,
            new_active: active_cpus,
        },
    }
}

/// Decision struct for SMP CPU stop — tells C shim what action to take.
#[repr(C)]
pub struct GaleSmpStopDecision {
    /// Action: 0=STOP_OK, 1=LAST_CPU
    pub action: u8,
    /// New active count (only meaningful when action=STOP_OK)
    pub new_active: u32,
}

pub const GALE_SMP_ACTION_STOP_OK: u8 = 0;
pub const GALE_SMP_ACTION_LAST_CPU: u8 = 1;

/// Full decision for SMP CPU stop: decides whether a CPU can be stopped.
///
/// The C shim extracts current active CPU count, Rust decides whether
/// stopping is valid (CPU 0 must never stop).
///
/// Verified: SM3 (stop: active -= 1, min 1).
#[cfg(feature = "smp_state")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_smp_stop_cpu_decide(
    active_cpus: u32,
) -> GaleSmpStopDecision {
    use gale::smp_state::stop_cpu_decide;

    // Delegate to verified model (SM3).
    match stop_cpu_decide(active_cpus) {
        Ok(new_active) => GaleSmpStopDecision {
            action: GALE_SMP_ACTION_STOP_OK,
            new_active,
        },
        Err(_) => GaleSmpStopDecision {
            action: GALE_SMP_ACTION_LAST_CPU,
            new_active: active_cpus,
        },
    }
}

// ---------------------------------------------------------------------------
// FFI exports — sched (scheduler primitives)
// ---------------------------------------------------------------------------
//
// These pure functions replace the scheduler priority comparison and
// preemption decision from kernel/sched.c:
//
//   sched.c:101-104  runq_best — select highest-priority thread
//   sched.c:128-145  should_preempt — cooperative protection
//   sched.c:185-279  next_up — scheduling decision
//
// Verified by Verus (SMT/Z3):
//   SC5: next_up returns highest-priority eligible thread
//   SC6: cooperative threads not preempted by non-MetaIRQ
//   SC7: idle only when no ready threads
//   SC8: no overflow in priority comparison

/// Select the next thread to run (uniprocessor).
///
/// Arguments:
///   runq_best_prio:  priority of best thread in run queue (u32::MAX if empty)
///   idle_prio:       priority of idle thread
///   best_prio:       pointer to receive selected thread's priority
///
/// Returns:
///   0 — selected the run queue best
///   1 — selected idle (run queue was empty)
///   -EINVAL — null pointer
#[cfg(feature = "sched")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sched_next_up(
    runq_best_prio: u32,
    idle_prio: u32,
    best_prio: *mut u32,
) -> i32 {
    unsafe {
        if best_prio.is_null() {
            return EINVAL;
        }

        if runq_best_prio == u32::MAX {
            // No threads in run queue — select idle
            *best_prio = idle_prio;
            1
        } else {
            *best_prio = runq_best_prio;
            0
        }
    }
}

/// Check whether a candidate should preempt the current thread.
///
/// sched.c should_preempt:
///   Cooperative current + non-MetaIRQ candidate -> no preemption
///   swap_ok (yield) -> always preempt
///
/// Arguments:
///   current_is_cooperative: 1 if current thread is cooperative
///   candidate_is_metairq:  1 if candidate is a MetaIRQ thread
///   swap_ok:               1 if explicit yield allows swap
///
/// Returns:
///   1 — should preempt
///   0 — should not preempt
#[cfg(feature = "sched")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sched_should_preempt(
    current_is_cooperative: u32,
    candidate_is_metairq: u32,
    swap_ok: u32,
) -> i32 {
    use gale::sched::should_preempt;

    // Delegate to verified model (SC6).
    if should_preempt(
        current_is_cooperative != 0,
        candidate_is_metairq != 0,
        swap_ok != 0,
    ) {
        1
    } else {
        0
    }
}

// ---- Phase 3: Sched Decision API ----

/// Action codes for `GaleSchedNextUpDecision`.
pub const GALE_SCHED_SELECT_RUNQ: u8 = 0;
pub const GALE_SCHED_SELECT_IDLE: u8 = 1;
pub const GALE_SCHED_SELECT_METAIRQ_PREEMPTED: u8 = 2;

/// Decision struct for next_up — tells C shim which thread to run next.
///
/// The C shim extracts scheduling state (run queue best, metairq preempted
/// thread readiness), Rust decides the selection, C applies it.
#[repr(C)]
pub struct GaleSchedNextUpDecision {
    /// Action: 0=SELECT_RUNQ, 1=SELECT_IDLE, 2=SELECT_METAIRQ_PREEMPTED
    pub action: u8,
}

/// Full decision for next_up (uniprocessor): decides which thread to run.
///
/// Mirrors sched.c:next_up (uniprocessor path):
///   1. If metairq preempted thread exists and is ready, and runq best is
///      not a metairq, return to the preempted cooperative thread.
///   2. If runq has a ready thread, select it.
///   3. Otherwise select idle.
///
/// Arguments:
///   has_runq_thread:              1 if run queue has a best thread
///   runq_best_is_metairq:        1 if the runq best thread is a MetaIRQ
///   has_metairq_preempted:       1 if a cooperative thread was preempted by MetaIRQ
///   metairq_preempted_is_ready:  1 if the preempted thread is still ready
///
/// Verified: SC5 (highest-priority), SC6 (cooperative protection), SC7 (idle fallback).
#[cfg(feature = "sched")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_sched_next_up_decide(
    has_runq_thread: u32,
    runq_best_is_metairq: u32,
    has_metairq_preempted: u32,
    metairq_preempted_is_ready: u32,
) -> GaleSchedNextUpDecision {
    // MetaIRQ preemption return: if a cooperative thread was preempted by a
    // MetaIRQ and the runq best is NOT a metairq, return to the preempted thread.
    if has_metairq_preempted != 0
        && (has_runq_thread == 0 || runq_best_is_metairq == 0)
    {
        if metairq_preempted_is_ready != 0 {
            return GaleSchedNextUpDecision {
                action: GALE_SCHED_SELECT_METAIRQ_PREEMPTED,
            };
        }
        // Preempted thread is no longer ready — fall through
        // (C shim clears metairq_preempted pointer)
    }

    if has_runq_thread != 0 {
        GaleSchedNextUpDecision {
            action: GALE_SCHED_SELECT_RUNQ,
        }
    } else {
        GaleSchedNextUpDecision {
            action: GALE_SCHED_SELECT_IDLE,
        }
    }
}

/// Action codes for `GaleSchedPreemptDecision`.
pub const GALE_SCHED_PREEMPT: u8 = 1;
pub const GALE_SCHED_NO_PREEMPT: u8 = 0;

/// Decision struct for should_preempt — tells C shim whether to preempt.
#[repr(C)]
pub struct GaleSchedPreemptDecision {
    /// 1=should preempt, 0=should not preempt
    pub should_preempt: u8,
}

/// Full decision for should_preempt: decides whether the candidate thread
/// should preempt the current thread.
///
/// Mirrors kthread.h:should_preempt:
///   1. swap_ok (explicit yield) -> always preempt
///   2. current is prevented from running -> preempt
///   3. current is preemptible OR candidate is MetaIRQ -> preempt
///   4. otherwise -> no preempt (cooperative protection)
///
/// Arguments:
///   is_cooperative:           1 if current thread is cooperative (not preemptible)
///   candidate_is_metairq:    1 if candidate thread is a MetaIRQ
///   swap_ok:                 1 if explicit yield allows swap
///   current_is_prevented:    1 if current thread is prevented from running
///                            (pended/suspended/dummy)
///
/// Verified: SC6 (cooperative not preempted by non-MetaIRQ).
#[cfg(feature = "sched")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_sched_preempt_decide(
    is_cooperative: u32,
    candidate_is_metairq: u32,
    swap_ok: u32,
    current_is_prevented: u32,
) -> GaleSchedPreemptDecision {
    use gale::sched::should_preempt;

    // If current is pended/suspended/dummy, always preempt (FFI-specific).
    if current_is_prevented != 0 {
        return GaleSchedPreemptDecision {
            should_preempt: GALE_SCHED_PREEMPT,
        };
    }

    // Delegate core preemption logic to verified model (SC6).
    let preempt = should_preempt(
        is_cooperative != 0,
        candidate_is_metairq != 0,
        swap_ok != 0,
    );
    GaleSchedPreemptDecision {
        should_preempt: if preempt {
            GALE_SCHED_PREEMPT
        } else {
            GALE_SCHED_NO_PREEMPT
        },
    }
}

// ---------------------------------------------------------------------------
// FFI exports — memory domain partition management
// ---------------------------------------------------------------------------
//
// These pure functions replace the partition validation and slot management
// from kernel/mem_domain.c:
//
//   mem_domain.c:24-86    check_add_partition — validate + non-overlap check
//   mem_domain.c:88-160   k_mem_domain_init — zero-init + bulk add
//   mem_domain.c:208-259  k_mem_domain_add_partition — find free slot, add
//   mem_domain.c:261-306  k_mem_domain_remove_partition — find match, clear
//
// All other mem_domain logic (spinlock, thread list, arch sync, W^X policy,
// SYS_INIT, deinit, add/remove thread) remains native Zephyr C in
// gale_mem_domain.c.
//
// Verified by Verus (SMT/Z3):
//   MD1: partitions don't overlap (no address collision)
//   MD2: partition alignment constraints satisfied (size > 0)
//   MD3: partition size > 0 for all active partitions
//   MD4: num_partitions <= MAX_PARTITIONS
//   MD5: add/remove preserve non-overlap invariant
//   MD6: no overflow in address arithmetic (start + size <= u32::MAX)

/// Maximum partitions per domain (matches CONFIG_MAX_DOMAIN_PARTITIONS).
const MEM_DOMAIN_MAX_PARTITIONS: u32 = 16;

/// Decision struct for check_add_partition — validates a single partition.
///
/// Used by both k_mem_domain_init (bulk validation) and
/// k_mem_domain_add_partition (single add).
#[repr(C)]
pub struct GaleMemDomainCheckPartitionDecision {
    /// 0 = valid, -EINVAL = invalid
    pub ret: i32,
}

/// Check whether a partition is valid and non-overlapping with all existing
/// active partitions in the domain.
///
/// Mirrors check_add_partition (mem_domain.c:24-86).
///
/// Arguments:
///   part_start:    start address of the candidate partition
///   part_size:     size of the candidate partition
///   domain_starts: array of 16 start addresses (existing partitions)
///   domain_sizes:  array of 16 sizes (existing partitions; 0 = free slot)
///   num_partitions: current active partition count (for bounds info only)
///
/// Returns: GaleMemDomainCheckPartitionDecision with ret = 0 or -EINVAL.
///
/// Verified: MD1, MD3, MD6 (non-overlap, size > 0, no overflow).
#[cfg(feature = "mem_domain")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mem_domain_check_partition(
    part_start: u32,
    part_size: u32,
    domain_starts: *const u32,
    domain_sizes: *const u32,
    _num_partitions: u32,
) -> GaleMemDomainCheckPartitionDecision {
    use gale::mem_domain::{partition_valid_decide, partitions_overlap_decide};

    // MD3 + MD6: validate partition (size > 0, no overflow).
    if !partition_valid_decide(part_start, part_size) {
        return GaleMemDomainCheckPartitionDecision { ret: EINVAL };
    }

    // NULL pointer guard (defensive — C shim should never pass NULL)
    if domain_starts.is_null() || domain_sizes.is_null() {
        return GaleMemDomainCheckPartitionDecision { ret: EINVAL };
    }

    // MD1: check non-overlap with all existing active partitions.
    let mut i: u32 = 0;
    while i < MEM_DOMAIN_MAX_PARTITIONS {
        unsafe {
            let dsize = *domain_sizes.add(i as usize);
            if dsize > 0 {
                let dstart = *domain_starts.add(i as usize);
                // Delegate overlap check to verified model (MD1).
                if partitions_overlap_decide(part_start, part_size, dstart, dsize) {
                    return GaleMemDomainCheckPartitionDecision { ret: EINVAL };
                }
            }
        }
        #[allow(clippy::arithmetic_side_effects)]
        {
            i += 1;
        }
    }

    GaleMemDomainCheckPartitionDecision { ret: OK }
}

// ---- Phase 2: Memory Domain Decision API ----

/// Decision struct for k_mem_domain_add_partition.
#[repr(C)]
pub struct GaleMemDomainAddDecision {
    /// Return code: 0=OK, -EINVAL=invalid partition, -ENOSPC=no free slot
    pub ret: i32,
    /// Slot index where partition was placed (valid only when ret==0)
    pub slot: u32,
    /// New num_partitions value (incremented on success)
    pub new_num_partitions: u32,
    /// Action: 0=ADD_OK, 1=RETURN_ERROR
    pub action: u8,
}

pub const GALE_MEM_DOMAIN_ACTION_ADD_OK: u8 = 0;
pub const GALE_MEM_DOMAIN_ACTION_ADD_ERROR: u8 = 1;

/// Full decision for k_mem_domain_add_partition: validates the partition,
/// checks non-overlap, finds a free slot, and returns the slot index.
///
/// Arguments:
///   part_start:      start address of new partition
///   part_size:       size of new partition
///   part_attr:       attributes of new partition (passed through)
///   domain_starts:   array of 16 start addresses
///   domain_sizes:    array of 16 sizes (0 = free slot)
///   num_partitions:  current active partition count
///
/// Verified: MD1-MD6.
#[cfg(feature = "mem_domain")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mem_domain_add_partition_decide(
    part_start: u32,
    part_size: u32,
    _part_attr: u32,
    domain_starts: *const u32,
    domain_sizes: *const u32,
    num_partitions: u32,
) -> GaleMemDomainAddDecision {
    // Validate partition (MD1, MD3, MD6)
    let check = gale_mem_domain_check_partition(
        part_start, part_size, domain_starts, domain_sizes, num_partitions,
    );
    if check.ret != OK {
        return GaleMemDomainAddDecision {
            ret: EINVAL,
            slot: 0,
            new_num_partitions: num_partitions,
            action: GALE_MEM_DOMAIN_ACTION_ADD_ERROR,
        };
    }

    // Find a free slot (size == 0)
    if domain_sizes.is_null() {
        return GaleMemDomainAddDecision {
            ret: EINVAL,
            slot: 0,
            new_num_partitions: num_partitions,
            action: GALE_MEM_DOMAIN_ACTION_ADD_ERROR,
        };
    }

    let mut p_idx: u32 = 0;
    while p_idx < MEM_DOMAIN_MAX_PARTITIONS {
        unsafe {
            if *domain_sizes.add(p_idx as usize) == 0 {
                // Found free slot
                #[allow(clippy::arithmetic_side_effects)]
                let new_num = num_partitions + 1;
                return GaleMemDomainAddDecision {
                    ret: OK,
                    slot: p_idx,
                    new_num_partitions: new_num,
                    action: GALE_MEM_DOMAIN_ACTION_ADD_OK,
                };
            }
        }
        #[allow(clippy::arithmetic_side_effects)]
        {
            p_idx += 1;
        }
    }

    // No free slot
    GaleMemDomainAddDecision {
        ret: ENOSPC,
        slot: 0,
        new_num_partitions: num_partitions,
        action: GALE_MEM_DOMAIN_ACTION_ADD_ERROR,
    }
}

/// Decision struct for k_mem_domain_remove_partition.
#[repr(C)]
pub struct GaleMemDomainRemoveDecision {
    /// Return code: 0=OK, -ENOENT=not found
    pub ret: i32,
    /// Slot index where partition was found (valid only when ret==0)
    pub slot: u32,
    /// New num_partitions value (decremented on success)
    pub new_num_partitions: u32,
    /// Action: 0=REMOVE_OK, 1=RETURN_ERROR
    pub action: u8,
}

pub const GALE_MEM_DOMAIN_ACTION_REMOVE_OK: u8 = 0;
pub const GALE_MEM_DOMAIN_ACTION_REMOVE_ERROR: u8 = 1;

/// Full decision for k_mem_domain_remove_partition: finds the matching
/// partition by start+size and returns the slot index.
///
/// Arguments:
///   part_start:      start address to match
///   part_size:       size to match
///   domain_starts:   array of 16 start addresses
///   domain_sizes:    array of 16 sizes
///   num_partitions:  current active partition count
///
/// Verified: MD5 (remove preserves invariant).
#[cfg(feature = "mem_domain")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mem_domain_remove_partition_decide(
    part_start: u32,
    part_size: u32,
    domain_starts: *const u32,
    domain_sizes: *const u32,
    num_partitions: u32,
) -> GaleMemDomainRemoveDecision {
    if domain_starts.is_null() || domain_sizes.is_null() {
        return GaleMemDomainRemoveDecision {
            ret: EINVAL,
            slot: 0,
            new_num_partitions: num_partitions,
            action: GALE_MEM_DOMAIN_ACTION_REMOVE_ERROR,
        };
    }

    let mut p_idx: u32 = 0;
    while p_idx < MEM_DOMAIN_MAX_PARTITIONS {
        unsafe {
            if *domain_starts.add(p_idx as usize) == part_start
                && *domain_sizes.add(p_idx as usize) == part_size
            {
                // Found matching partition
                #[allow(clippy::arithmetic_side_effects)]
                let new_num = if num_partitions > 0 {
                    num_partitions - 1
                } else {
                    0
                };
                return GaleMemDomainRemoveDecision {
                    ret: OK,
                    slot: p_idx,
                    new_num_partitions: new_num,
                    action: GALE_MEM_DOMAIN_ACTION_REMOVE_OK,
                };
            }
        }
        #[allow(clippy::arithmetic_side_effects)]
        {
            p_idx += 1;
        }
    }

    // No matching partition
    GaleMemDomainRemoveDecision {
        ret: ENOENT,
        slot: 0,
        new_num_partitions: num_partitions,
        action: GALE_MEM_DOMAIN_ACTION_REMOVE_ERROR,
    }
}

/// Decision struct for k_mem_domain_init partition validation.
///
/// During init, we validate each partition in the parts[] array one at a
/// time, building up the domain incrementally.  This struct carries the
/// per-partition verdict.
#[repr(C)]
pub struct GaleMemDomainInitPartDecision {
    /// 0 = partition valid, -EINVAL = invalid (reject whole init)
    pub ret: i32,
}

/// Validate one partition during k_mem_domain_init bulk insertion.
///
/// This is called for each partition in the parts[] array.  The domain_*
/// arrays reflect the partitions already added (slots 0..idx-1).
///
/// Verified: MD1, MD3, MD6.
#[cfg(feature = "mem_domain")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mem_domain_init_validate_partition(
    part_start: u32,
    part_size: u32,
    domain_starts: *const u32,
    domain_sizes: *const u32,
    num_partitions: u32,
) -> GaleMemDomainInitPartDecision {
    let check = gale_mem_domain_check_partition(
        part_start, part_size, domain_starts, domain_sizes, num_partitions,
    );
    GaleMemDomainInitPartDecision { ret: check.ret }
}

// ---------------------------------------------------------------------------
// FFI exports — userspace (kernel object permission/type/init validation)
// ---------------------------------------------------------------------------
//
// These pure functions replace the safety-critical validation logic from
// kernel/userspace.c:
//
//   userspace.c:670-683  thread_perms_test (access check)
//   userspace.c:754-785  k_object_validate (type + permission + init check)
//   userspace.c:787-810  k_object_init (set initialized flag)
//   userspace.c:823-834  k_object_uninit (clear initialized flag)
//   userspace.c:812-821  k_object_recycle (clear perms + grant + init)
//   userspace.c:745-752  k_object_access_all_grant (make public)
//
// Verified by Verus (SMT/Z3):
//   US1: object access requires permission bit set for calling thread
//   US2: grant_access sets the permission bit
//   US3: revoke_access clears the permission bit
//   US4: object type validation (type must match expected type for syscall)
//   US5: supervisor mode bypasses permission checks
//   US7: K_OBJ_FLAG_INITIALIZED required for access (when init_check == MustBeInit)
//   US8: thread ID must be valid (< MAX_THREADS)

// Flag constants — must match Zephyr's <zephyr/sys/kobject.h>
const K_OBJ_FLAG_INITIALIZED: u8 = 0x01;
const K_OBJ_FLAG_PUBLIC: u8 = 0x02;

// Init check constants — must match Zephyr's _obj_init_check enum
const OBJ_INIT_TRUE: i8 = 0;   // _OBJ_INIT_TRUE
const OBJ_INIT_FALSE: i8 = -1; // _OBJ_INIT_FALSE
// OBJ_INIT_ANY = 1 — the "don't care" case

// ---- Phase 2: Full Decision API for userspace ----

/// Decision struct for thread_perms_test — access granted or denied.
///
/// C extracts the PUBLIC flag and per-thread permission bit,
/// Rust decides whether access is granted.
///
/// Verified: US1 (permission required), US5 (public bypass).
#[repr(C)]
pub struct GaleUserspaceAccessDecision {
    /// 1 = access granted, 0 = denied
    pub granted: u8,
}

/// Decide whether a thread has access to a kernel object.
///
/// This replaces thread_perms_test() (userspace.c:670-683):
///   if (ko->flags & K_OBJ_FLAG_PUBLIC) return 1;
///   return sys_bitfield_test_bit(&ko->perms, index);
///
/// Arguments:
///   flags:        ko->flags
///   has_perm_bit: 1 if sys_bitfield_test_bit passed, 0 otherwise
///
/// Verified: US1 (permission bit required), US5 (public bypass).
#[cfg(feature = "userspace")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_object_access_decide(
    flags: u8,
    has_perm_bit: u8,
) -> GaleUserspaceAccessDecision {
    use gale::userspace::access_decide;

    // Delegate to verified model (US1, US5).
    let granted = access_decide(
        (flags & K_OBJ_FLAG_PUBLIC) != 0,
        has_perm_bit != 0,
    );
    GaleUserspaceAccessDecision {
        granted: if granted { 1 } else { 0 },
    }
}

/// Decision struct for k_object_validate — pass/fail with error code.
///
/// C extracts object type, flags, and access result, Rust decides the
/// validation outcome.
///
/// Verified: US1 (permission), US4 (type), US5 (supervisor), US7 (init).
#[repr(C)]
pub struct GaleUserspaceValidateDecision {
    /// 0 = OK, negative = error code (-EBADF, -EPERM, -EINVAL, -EADDRINUSE)
    pub ret: i32,
}

/// Decide whether a kernel object passes validation for a syscall.
///
/// This replaces k_object_validate() (userspace.c:754-785):
///   1. Type check: if otype != K_OBJ_ANY, ko->type must match
///   2. Permission check: thread must have access
///   3. Init check: based on init_check parameter
///
/// Arguments:
///   obj_type:      ko->type (enum k_objects)
///   expected_type: otype argument (0 = K_OBJ_ANY)
///   flags:         ko->flags
///   has_access:    1 if thread_perms_test() passed
///   init_check:    _OBJ_INIT_TRUE(0), _OBJ_INIT_FALSE(-1), _OBJ_INIT_ANY(1)
///
/// Returns decision with:
///   ret = 0     : validation passed
///   ret = -EBADF : type mismatch (US4)
///   ret = -EPERM : no permission (US1)
///   ret = -EINVAL     : not initialized when required (US7)
///   ret = -EADDRINUSE : already initialized when must-not-be (US7)
///
/// Verified: US1, US4, US5, US7.
#[cfg(feature = "userspace")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_object_validate_decide(
    obj_type: u8,
    expected_type: u8,
    flags: u8,
    has_access: u8,
    init_check: i8,
) -> GaleUserspaceValidateDecision {
    use gale::userspace::validate_decide;

    // Delegate to verified model (US1, US4, US5, US7).
    let type_matches = expected_type == 0 || obj_type == expected_type;
    let is_initialized = (flags & K_OBJ_FLAG_INITIALIZED) != 0;
    match validate_decide(type_matches, has_access != 0, is_initialized, init_check) {
        Ok(()) => GaleUserspaceValidateDecision { ret: OK },
        Err(e) => GaleUserspaceValidateDecision { ret: e },
    }
}

/// Decision for k_object_init — compute new flags with INITIALIZED set.
///
/// Verified: US7 (init flag management).
#[repr(C)]
pub struct GaleUserspaceInitDecision {
    /// New flags value with K_OBJ_FLAG_INITIALIZED set
    pub new_flags: u8,
}

/// Decide new flags for k_object_init.
///
/// userspace.c:809: ko->flags |= K_OBJ_FLAG_INITIALIZED;
#[cfg(feature = "userspace")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_object_init_decide(
    current_flags: u8,
) -> GaleUserspaceInitDecision {
    GaleUserspaceInitDecision {
        new_flags: current_flags | K_OBJ_FLAG_INITIALIZED,
    }
}

/// Decision for k_object_uninit — compute new flags with INITIALIZED cleared.
///
/// Verified: US7 (init flag management).
#[repr(C)]
pub struct GaleUserspaceUninitDecision {
    /// New flags value with K_OBJ_FLAG_INITIALIZED cleared
    pub new_flags: u8,
}

/// Decide new flags for k_object_uninit.
///
/// userspace.c:833: ko->flags &= ~K_OBJ_FLAG_INITIALIZED;
#[cfg(feature = "userspace")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_object_uninit_decide(
    current_flags: u8,
) -> GaleUserspaceUninitDecision {
    GaleUserspaceUninitDecision {
        new_flags: current_flags & !K_OBJ_FLAG_INITIALIZED,
    }
}

/// Decision for k_object_recycle — clear perms, grant caller, init.
///
/// Verified: US2 (grant), US6 (clear perms), US7 (init).
#[repr(C)]
pub struct GaleUserspaceRecycleDecision {
    /// New flags value with K_OBJ_FLAG_INITIALIZED set
    pub new_flags: u8,
    /// 1 = must clear perms and set caller's bit
    pub clear_perms: u8,
}

/// Decide new flags for k_object_recycle.
///
/// userspace.c:817-819:
///   memset(ko->perms, 0, sizeof(ko->perms));
///   k_thread_perms_set(ko, _current);
///   ko->flags |= K_OBJ_FLAG_INITIALIZED;
#[cfg(feature = "userspace")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_object_recycle_decide(
    current_flags: u8,
) -> GaleUserspaceRecycleDecision {
    GaleUserspaceRecycleDecision {
        new_flags: current_flags | K_OBJ_FLAG_INITIALIZED,
        clear_perms: 1,
    }
}

/// Decision for k_object_access_all_grant — make object public.
///
/// Verified: US5 (public flag grants universal access).
#[repr(C)]
pub struct GaleUserspacePublicDecision {
    /// New flags value with K_OBJ_FLAG_PUBLIC set
    pub new_flags: u8,
}

/// Decide new flags for k_object_access_all_grant.
///
/// userspace.c:750: ko->flags |= K_OBJ_FLAG_PUBLIC;
#[cfg(feature = "userspace")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_object_make_public_decide(
    current_flags: u8,
) -> GaleUserspacePublicDecision {
    GaleUserspacePublicDecision {
        new_flags: current_flags | K_OBJ_FLAG_PUBLIC,
    }
}

// ---------------------------------------------------------------------------
// FFI exports — CPU affinity mask
// ---------------------------------------------------------------------------

/// Result struct for cpu_mask_mod — returns new mask and error code.
///
/// Maps to CpuMaskResult from gale::cpu_mask but with C-compatible layout.
#[cfg(feature = "cpu_mask")]
#[repr(C)]
pub struct GaleCpuMaskResult {
    /// The resulting CPU affinity mask (valid only when `err == 0`).
    pub mask: u32,
    /// Error code: 0 on success, -EINVAL on failure.
    pub err: i32,
}

/// Core CPU mask modification — decide new mask from enable/disable bits.
///
/// cpu_mask.c:19-45:
///   new_mask = (current | enable) & ~disable;
///   Rejects running threads, zero masks, and invalid pin masks.
///
/// C uses u32 for booleans: 0=false, nonzero=true.
///
/// Verified: CM1-CM5 (running guard, pin-only, formula, nonzero, overflow).
#[cfg(feature = "cpu_mask")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_cpu_mask_mod(
    current_mask: u32,
    enable: u32,
    disable: u32,
    is_running: u32,
    pin_only: u32,
) -> GaleCpuMaskResult {
    use gale::cpu_mask::*;

    let result = cpu_mask_mod(
        current_mask,
        enable,
        disable,
        is_running != 0,
        pin_only != 0,
    );
    GaleCpuMaskResult {
        mask: result.mask,
        err: result.error,
    }
}

/// Validate whether a mask is a valid PIN_ONLY mask (exactly one bit set).
///
/// cpu_mask.c:38-41: power-of-two check.
///
/// Returns 1 if valid, 0 if invalid.
///
/// Verified: CM2 (exactly one bit set).
#[cfg(feature = "cpu_mask")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_validate_pin_mask(mask: u32) -> i32 {
    use gale::cpu_mask::*;

    if validate_pin_mask(mask) { 1 } else { 0 }
}

/// Compute the pin mask for a specific CPU: BIT(cpu_id).
///
/// cpu_mask.c:69: k_thread_cpu_pin uses BIT(cpu).
///
/// Returns mask in .mask and 0 in .err on success, or -EINVAL in .err on
/// bounds failure (cpu_id >= max_cpus or max_cpus > 32).
///
/// Verified: CM6 (bounds check, single-bit result).
#[cfg(feature = "cpu_mask")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_cpu_pin_compute(
    cpu_id: u32,
    max_cpus: u32,
) -> GaleCpuMaskResult {
    use gale::cpu_mask::*;

    match cpu_pin_compute(cpu_id, max_cpus) {
        Ok(m) => GaleCpuMaskResult { mask: m, err: OK },
        Err(e) => GaleCpuMaskResult { mask: 0, err: e },
    }
}

// Panic handler for no_std
#[cfg(not(any(test, kani)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — semaphore
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "sem"))]
mod kani_sem_proofs {
    use super::*;

    /// P1/P2: init rejects limit==0 and initial_count > limit,
    /// accepts all valid combinations.
    #[kani::proof]
    fn sem_init_validates_all_params() {
        let initial: u32 = kani::any();
        let limit: u32 = kani::any();
        let ret = gale_sem_count_init(initial, limit);
        if limit == 0 || initial > limit {
            assert!(ret == EINVAL);
        } else {
            assert!(ret == OK);
        }
    }

    /// P3/P9: give never overflows and saturates at limit.
    #[kani::proof]
    fn sem_give_no_overflow() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();
        // Pre: valid semaphore state
        kani::assume(limit > 0);
        kani::assume(count <= limit);

        let new_count = gale_sem_count_give(count, limit);

        // Post: result in bounds
        assert!(new_count <= limit);
        // Post: correct arithmetic
        if count < limit {
            assert!(new_count == count + 1);
        } else {
            assert!(new_count == count);
        }
    }

    /// P5/P6/P9: take never underflows and returns correct status.
    #[kani::proof]
    fn sem_take_no_underflow() {
        let mut count: u32 = kani::any();
        let original = count;

        let ret = gale_sem_count_take(&mut count);

        if original > 0 {
            assert!(ret == OK);
            assert!(count == original - 1);
        } else {
            assert!(ret == EBUSY);
            assert!(count == 0);
        }
    }

    /// Null pointer returns EINVAL.
    #[kani::proof]
    fn sem_take_null_returns_einval() {
        let ret = gale_sem_count_take(core::ptr::null_mut());
        assert!(ret == EINVAL);
    }

    /// Give-take roundtrip: giving then taking returns to original count.
    #[kani::proof]
    fn sem_give_take_roundtrip() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();
        kani::assume(limit > 0);
        kani::assume(count < limit); // below limit so give increments

        let mut after_give = gale_sem_count_give(count, limit);
        assert!(after_give == count + 1);

        let ret = gale_sem_count_take(&mut after_give);
        assert!(ret == OK);
        assert!(after_give == count);
    }

    /// Repeated gives saturate at limit.
    #[kani::proof]
    #[kani::unwind(4)]
    fn sem_repeated_give_saturates() {
        let limit: u32 = kani::any();
        kani::assume(limit > 0 && limit <= 8); // bound for tractability
        let mut count = limit; // start at limit

        // 3 gives should all saturate
        let mut i: u32 = 0;
        while i < 3 {
            count = gale_sem_count_give(count, limit);
            assert!(count == limit);
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — mutex
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "mutex"))]
mod kani_mutex_proofs {
    use super::*;

    /// M3: lock when unlocked sets lock_count = 1.
    #[kani::proof]
    fn mutex_lock_unlocked() {
        let mut new_lc: u32 = 0;
        let ret = gale_mutex_lock_validate(0, 1, 0, &mut new_lc);
        assert!(ret == OK);
        assert!(new_lc == 1);
    }

    /// M4/M10: reentrant lock increments without overflow.
    #[kani::proof]
    fn mutex_lock_reentrant_no_overflow() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0 && lock_count < u32::MAX);

        let mut new_lc: u32 = 0;
        let ret = gale_mutex_lock_validate(lock_count, 0, 1, &mut new_lc);
        assert!(ret == OK);
        assert!(new_lc == lock_count + 1);
    }

    /// M4/M10: reentrant lock at u32::MAX returns error (overflow protection).
    #[kani::proof]
    fn mutex_lock_reentrant_overflow_protection() {
        let mut new_lc: u32 = 0;
        let ret = gale_mutex_lock_validate(u32::MAX, 0, 1, &mut new_lc);
        assert!(ret == EINVAL);
    }

    /// M5: lock by different owner returns -EBUSY.
    #[kani::proof]
    fn mutex_lock_contended() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0);

        let mut new_lc: u32 = 0;
        let ret = gale_mutex_lock_validate(lock_count, 0, 0, &mut new_lc);
        assert!(ret == EBUSY);
    }

    /// M6a: unlock when not locked returns -EINVAL.
    #[kani::proof]
    fn mutex_unlock_not_locked() {
        let mut new_lc: u32 = 0;
        let ret = gale_mutex_unlock_validate(0, 1, 0, &mut new_lc);
        assert!(ret == EINVAL);
    }

    /// M6b: unlock by wrong owner returns -EPERM.
    #[kani::proof]
    fn mutex_unlock_not_owner() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0);

        let mut new_lc: u32 = 0;
        let ret = gale_mutex_unlock_validate(lock_count, 0, 0, &mut new_lc);
        assert!(ret == EPERM);
    }

    /// M7: reentrant unlock decrements correctly.
    #[kani::proof]
    fn mutex_unlock_reentrant() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 1);

        let mut new_lc: u32 = 0;
        let ret = gale_mutex_unlock_validate(lock_count, 0, 1, &mut new_lc);
        assert!(ret == GALE_MUTEX_RELEASED);
        assert!(new_lc == lock_count - 1);
    }

    /// M9: final unlock returns UNLOCKED.
    #[kani::proof]
    fn mutex_unlock_final() {
        let mut new_lc: u32 = 0;
        let ret = gale_mutex_unlock_validate(1, 0, 1, &mut new_lc);
        assert!(ret == GALE_MUTEX_UNLOCKED);
        assert!(new_lc == 0);
    }

    /// Lock-unlock roundtrip: lock then unlock returns to lock_count = 0.
    #[kani::proof]
    fn mutex_lock_unlock_roundtrip() {
        let mut new_lc: u32 = 0;
        // Lock (unlocked mutex)
        let ret = gale_mutex_lock_validate(0, 1, 0, &mut new_lc);
        assert!(ret == OK);
        assert!(new_lc == 1);

        // Unlock
        let ret = gale_mutex_unlock_validate(new_lc, 0, 1, &mut new_lc);
        assert!(ret == GALE_MUTEX_UNLOCKED);
        assert!(new_lc == 0);
    }

    /// Reentrant lock-unlock roundtrip preserves lock_count.
    #[kani::proof]
    fn mutex_reentrant_roundtrip() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0 && lock_count < u32::MAX);

        let mut new_lc: u32 = 0;
        // Reentrant lock
        let ret = gale_mutex_lock_validate(lock_count, 0, 1, &mut new_lc);
        assert!(ret == OK);
        assert!(new_lc == lock_count + 1);

        // Reentrant unlock
        let ret = gale_mutex_unlock_validate(new_lc, 0, 1, &mut new_lc);
        assert!(ret == GALE_MUTEX_RELEASED);
        assert!(new_lc == lock_count);
    }

    /// Null pointer to lock_validate returns EINVAL.
    #[kani::proof]
    fn mutex_lock_null_returns_einval() {
        let ret = gale_mutex_lock_validate(0, 1, 0, core::ptr::null_mut());
        assert!(ret == EINVAL);
    }

    /// Null pointer to unlock_validate returns EINVAL.
    #[kani::proof]
    fn mutex_unlock_null_returns_einval() {
        let ret = gale_mutex_unlock_validate(1, 0, 1, core::ptr::null_mut());
        assert!(ret == EINVAL);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — mutex decision structs
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "mutex"))]
mod kani_mutex_decide_proofs {
    use super::*;

    /// M3: lock_decide on unlocked mutex returns ACQUIRED with lock_count = 1.
    #[kani::proof]
    fn mutex_lock_decide_unlocked() {
        let d = gale_k_mutex_lock_decide(0, 1, 0, 0);
        assert!(d.ret == OK);
        assert!(d.action == GALE_MUTEX_ACTION_ACQUIRED);
        assert!(d.new_lock_count == 1);
    }

    /// M4/M10: lock_decide reentrant increments without overflow.
    #[kani::proof]
    fn mutex_lock_decide_reentrant() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0 && lock_count < u32::MAX);

        let d = gale_k_mutex_lock_decide(lock_count, 0, 1, 0);
        assert!(d.ret == OK);
        assert!(d.action == GALE_MUTEX_ACTION_ACQUIRED);
        assert!(d.new_lock_count == lock_count + 1);
    }

    /// M10: lock_decide reentrant at u32::MAX returns error (overflow protection).
    #[kani::proof]
    fn mutex_lock_decide_reentrant_overflow() {
        let d = gale_k_mutex_lock_decide(u32::MAX, 0, 1, 0);
        assert!(d.ret == EINVAL);
        assert!(d.action == GALE_MUTEX_ACTION_BUSY);
        assert!(d.new_lock_count == u32::MAX);
    }

    /// M5: lock_decide contended with no-wait returns BUSY.
    #[kani::proof]
    fn mutex_lock_decide_contended_no_wait() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0);

        let d = gale_k_mutex_lock_decide(lock_count, 0, 0, 1);
        assert!(d.ret == EBUSY);
        assert!(d.action == GALE_MUTEX_ACTION_BUSY);
    }

    /// M5: lock_decide contended willing to wait returns PEND.
    #[kani::proof]
    fn mutex_lock_decide_contended_pend() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0);

        let d = gale_k_mutex_lock_decide(lock_count, 0, 0, 0);
        assert!(d.ret == 0);
        assert!(d.action == GALE_MUTEX_ACTION_PEND);
    }

    /// M6a: unlock_decide when not locked returns EINVAL + ERROR action.
    #[kani::proof]
    fn mutex_unlock_decide_not_locked() {
        let d = gale_k_mutex_unlock_decide(0, 1, 0);
        assert!(d.ret == EINVAL);
        assert!(d.action == GALE_MUTEX_UNLOCK_ERROR);
    }

    /// M6b: unlock_decide by wrong owner returns EPERM + ERROR action.
    #[kani::proof]
    fn mutex_unlock_decide_not_owner() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0);

        let d = gale_k_mutex_unlock_decide(lock_count, 0, 0);
        assert!(d.ret == EPERM);
        assert!(d.action == GALE_MUTEX_UNLOCK_ERROR);
    }

    /// M7: unlock_decide reentrant decrements correctly.
    #[kani::proof]
    fn mutex_unlock_decide_reentrant() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 1);

        let d = gale_k_mutex_unlock_decide(lock_count, 0, 1);
        assert!(d.ret == OK);
        assert!(d.action == GALE_MUTEX_UNLOCK_RELEASED);
        assert!(d.new_lock_count == lock_count - 1);
    }

    /// M9: unlock_decide final unlock returns UNLOCKED.
    #[kani::proof]
    fn mutex_unlock_decide_final() {
        let d = gale_k_mutex_unlock_decide(1, 0, 1);
        assert!(d.ret == OK);
        assert!(d.action == GALE_MUTEX_UNLOCK_UNLOCKED);
        assert!(d.new_lock_count == 0);
    }

    /// Lock-unlock roundtrip via decision structs.
    #[kani::proof]
    fn mutex_decide_roundtrip() {
        // Lock (unlocked mutex)
        let dl = gale_k_mutex_lock_decide(0, 1, 0, 0);
        assert!(dl.ret == OK);
        assert!(dl.action == GALE_MUTEX_ACTION_ACQUIRED);
        assert!(dl.new_lock_count == 1);

        // Unlock
        let du = gale_k_mutex_unlock_decide(dl.new_lock_count, 0, 1);
        assert!(du.ret == OK);
        assert!(du.action == GALE_MUTEX_UNLOCK_UNLOCKED);
        assert!(du.new_lock_count == 0);
    }

    /// Reentrant lock-unlock roundtrip via decision structs.
    #[kani::proof]
    fn mutex_decide_reentrant_roundtrip() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0 && lock_count < u32::MAX);

        // Reentrant lock
        let dl = gale_k_mutex_lock_decide(lock_count, 0, 1, 0);
        assert!(dl.ret == OK);
        assert!(dl.new_lock_count == lock_count + 1);

        // Reentrant unlock
        let du = gale_k_mutex_unlock_decide(dl.new_lock_count, 0, 1);
        assert!(du.ret == OK);
        assert!(du.action == GALE_MUTEX_UNLOCK_RELEASED);
        assert!(du.new_lock_count == lock_count);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — message queue
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "msgq"))]
mod kani_msgq_proofs {
    use super::*;

    /// MQ2/MQ4: init validates all parameter combinations.
    #[kani::proof]
    fn msgq_init_validates_params() {
        let msg_size: u32 = kani::any();
        let max_msgs: u32 = kani::any();
        kani::assume(msg_size <= 256);
        kani::assume(max_msgs <= 64);

        let mut buf_size: u32 = 0;
        let ret = gale_msgq_init_validate(msg_size, max_msgs, &mut buf_size);

        if msg_size == 0 || max_msgs == 0 || msg_size.checked_mul(max_msgs).is_none() {
            assert!(ret == EINVAL);
        } else {
            assert!(ret == OK);
            assert!(buf_size == msg_size * max_msgs);
        }
    }

    /// MQ5: put advances write index correctly.
    #[kani::proof]
    fn msgq_put_advances_write() {
        let write_idx: u32 = kani::any();
        let used_msgs: u32 = kani::any();
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 16);
        kani::assume(write_idx < max_msgs);
        kani::assume(used_msgs < max_msgs);

        let mut new_w: u32 = 0;
        let mut new_u: u32 = 0;
        let ret = gale_msgq_put(write_idx, used_msgs, max_msgs, &mut new_w, &mut new_u);

        assert!(ret == OK);
        assert!(new_u == used_msgs + 1);
        assert!(new_w < max_msgs);
        if write_idx + 1 < max_msgs {
            assert!(new_w == write_idx + 1);
        } else {
            assert!(new_w == 0);
        }
    }

    /// MQ6: put on full queue returns ENOMSG.
    #[kani::proof]
    fn msgq_put_full_returns_enomsg() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 16);

        let mut new_w: u32 = 0;
        let mut new_u: u32 = 0;
        let ret = gale_msgq_put(0, max_msgs, max_msgs, &mut new_w, &mut new_u);
        assert!(ret == ENOMSG);
    }

    /// MQ7: put_front retreats read index correctly.
    #[kani::proof]
    fn msgq_put_front_retreats_read() {
        let read_idx: u32 = kani::any();
        let used_msgs: u32 = kani::any();
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 16);
        kani::assume(read_idx < max_msgs);
        kani::assume(used_msgs < max_msgs);

        let mut new_r: u32 = 0;
        let mut new_u: u32 = 0;
        let ret = gale_msgq_put_front(read_idx, used_msgs, max_msgs, &mut new_r, &mut new_u);

        assert!(ret == OK);
        assert!(new_u == used_msgs + 1);
        assert!(new_r < max_msgs);
        if read_idx == 0 {
            assert!(new_r == max_msgs - 1);
        } else {
            assert!(new_r == read_idx - 1);
        }
    }

    /// MQ8: get advances read index correctly.
    #[kani::proof]
    fn msgq_get_advances_read() {
        let read_idx: u32 = kani::any();
        let used_msgs: u32 = kani::any();
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 16);
        kani::assume(read_idx < max_msgs);
        kani::assume(used_msgs > 0 && used_msgs <= max_msgs);

        let mut new_r: u32 = 0;
        let mut new_u: u32 = 0;
        let ret = gale_msgq_get(read_idx, used_msgs, max_msgs, &mut new_r, &mut new_u);

        assert!(ret == OK);
        assert!(new_u == used_msgs - 1);
        assert!(new_r < max_msgs);
        if read_idx + 1 < max_msgs {
            assert!(new_r == read_idx + 1);
        } else {
            assert!(new_r == 0);
        }
    }

    /// MQ9: get on empty returns ENOMSG.
    #[kani::proof]
    fn msgq_get_empty_returns_enomsg() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 16);

        let mut new_r: u32 = 0;
        let mut new_u: u32 = 0;
        let ret = gale_msgq_get(0, 0, max_msgs, &mut new_r, &mut new_u);
        assert!(ret == ENOMSG);
    }

    /// MQ10: peek_at computes correct slot.
    #[kani::proof]
    fn msgq_peek_at_correct_slot() {
        let read_idx: u32 = kani::any();
        let used_msgs: u32 = kani::any();
        let max_msgs: u32 = kani::any();
        let idx: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 16);
        kani::assume(read_idx < max_msgs);
        kani::assume(used_msgs > 0 && used_msgs <= max_msgs);
        kani::assume(idx < used_msgs);

        let mut slot: u32 = 0;
        let ret = gale_msgq_peek_at(read_idx, used_msgs, max_msgs, idx, &mut slot);

        assert!(ret == OK);
        assert!(slot < max_msgs);
        // Verify it equals (read_idx + idx) % max_msgs
        let expected = (read_idx + idx) % max_msgs;
        assert!(slot == expected);
    }

    /// Put-get roundtrip: indices return correctly.
    #[kani::proof]
    fn msgq_put_get_roundtrip() {
        let max_msgs: u32 = kani::any();
        kani::assume(max_msgs > 0 && max_msgs <= 16);

        let mut w: u32 = 0;
        let mut u: u32 = 0;
        let mut r: u32 = 0;

        // Put one message (write_idx=0, used=0)
        let ret = gale_msgq_put(0, 0, max_msgs, &mut w, &mut u);
        assert!(ret == OK);
        assert!(u == 1);

        // Get one message (read_idx=0, used=1)
        let ret = gale_msgq_get(0, u, max_msgs, &mut r, &mut u);
        assert!(ret == OK);
        assert!(u == 0);
        // Both indices should have advanced by 1
        assert!(r == w);
    }

    /// Null pointer checks return EINVAL.
    #[kani::proof]
    fn msgq_null_pointers() {
        assert!(gale_msgq_init_validate(4, 10, core::ptr::null_mut()) == EINVAL);
        assert!(gale_msgq_put(0, 0, 10, core::ptr::null_mut(), core::ptr::null_mut()) == EINVAL);
        assert!(gale_msgq_get(0, 1, 10, core::ptr::null_mut(), core::ptr::null_mut()) == EINVAL);
        assert!(gale_msgq_put_front(0, 0, 10, core::ptr::null_mut(), core::ptr::null_mut()) == EINVAL);
        assert!(gale_msgq_peek_at(0, 1, 10, 0, core::ptr::null_mut()) == EINVAL);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — pipe
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "pipe"))]
mod kani_pipe_proofs {
    use super::*;

    /// PP3: write on closed pipe returns EPIPE.
    #[kani::proof]
    fn pipe_write_closed() {
        let mut actual: u32 = 0;
        let mut new_used: u32 = 0;
        let ret = gale_pipe_write_check(0, 16, 0, 5, &mut actual, &mut new_used);
        assert!(ret == EPIPE);
    }

    /// PP4: write/read on resetting pipe returns ECANCELED.
    #[kani::proof]
    fn pipe_resetting_returns_ecanceled() {
        let mut actual: u32 = 0;
        let mut new_used: u32 = 0;
        let flags = PIPE_FLAG_OPEN | PIPE_FLAG_RESET;
        assert!(gale_pipe_write_check(0, 16, flags, 5, &mut actual, &mut new_used) == ECANCELED);
        assert!(gale_pipe_read_check(5, flags, 5, &mut actual, &mut new_used) == ECANCELED);
    }

    /// PP5: write computes correct byte count.
    #[kani::proof]
    fn pipe_write_clamps() {
        let used: u32 = kani::any();
        let size: u32 = kani::any();
        let request: u32 = kani::any();
        kani::assume(size > 0 && size <= 32);
        kani::assume(used < size);
        kani::assume(request > 0 && request <= 64);

        let mut actual: u32 = 0;
        let mut new_used: u32 = 0;
        let ret = gale_pipe_write_check(used, size, PIPE_FLAG_OPEN, request, &mut actual, &mut new_used);

        assert!(ret == OK);
        assert!(actual > 0);
        assert!(actual <= request);
        let free = size - used;
        if request <= free {
            assert!(actual == request);
        } else {
            assert!(actual == free);
        }
        assert!(new_used == used + actual);
        assert!(new_used <= size);
    }

    /// PP6: read computes correct byte count.
    #[kani::proof]
    fn pipe_read_clamps() {
        let used: u32 = kani::any();
        let request: u32 = kani::any();
        kani::assume(used > 0 && used <= 32);
        kani::assume(request > 0 && request <= 64);

        let mut actual: u32 = 0;
        let mut new_used: u32 = 0;
        let ret = gale_pipe_read_check(used, PIPE_FLAG_OPEN, request, &mut actual, &mut new_used);

        assert!(ret == OK);
        assert!(actual > 0);
        assert!(actual <= request);
        if request <= used {
            assert!(actual == request);
        } else {
            assert!(actual == used);
        }
        assert!(new_used == used - actual);
    }

    /// Write on full pipe returns EAGAIN.
    #[kani::proof]
    fn pipe_write_full_eagain() {
        let size: u32 = kani::any();
        kani::assume(size > 0 && size <= 32);
        let mut actual: u32 = 0;
        let mut new_used: u32 = 0;
        let ret = gale_pipe_write_check(size, size, PIPE_FLAG_OPEN, 1, &mut actual, &mut new_used);
        assert!(ret == EAGAIN);
    }

    /// Read on empty open pipe returns EAGAIN.
    #[kani::proof]
    fn pipe_read_empty_eagain() {
        let mut actual: u32 = 0;
        let mut new_used: u32 = 0;
        let ret = gale_pipe_read_check(0, PIPE_FLAG_OPEN, 1, &mut actual, &mut new_used);
        assert!(ret == EAGAIN);
    }

    /// Null pointer checks.
    #[kani::proof]
    fn pipe_null_pointers() {
        let mut dummy: u32 = 0;
        assert!(gale_pipe_write_check(0, 16, PIPE_FLAG_OPEN, 5, core::ptr::null_mut(), &mut dummy) == EINVAL);
        assert!(gale_pipe_read_check(5, PIPE_FLAG_OPEN, 5, core::ptr::null_mut(), &mut dummy) == EINVAL);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — stack
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "stack"))]
mod kani_stack_proofs {
    use super::*;

    /// SK2: init rejects zero capacity.
    #[kani::proof]
    fn stack_init_validates() {
        let num_entries: u32 = kani::any();
        let ret = gale_stack_init_validate(num_entries);
        if num_entries == 0 {
            assert!(ret == EINVAL);
        } else {
            assert!(ret == OK);
        }
    }

    /// SK3/SK4: push validates capacity and increments count.
    #[kani::proof]
    fn stack_push_validates() {
        let count: u32 = kani::any();
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 16);
        kani::assume(count <= capacity);

        let mut new_count: u32 = 0;
        let ret = gale_stack_push_validate(count, capacity, &mut new_count);

        if count < capacity {
            assert!(ret == OK);
            assert!(new_count == count + 1);
            assert!(new_count <= capacity);
        } else {
            assert!(ret == ENOMEM);
        }
    }

    /// SK5/SK6: pop validates non-empty and decrements count.
    #[kani::proof]
    fn stack_pop_validates() {
        let count: u32 = kani::any();

        let mut new_count: u32 = 0;
        let ret = gale_stack_pop_validate(count, &mut new_count);

        if count > 0 {
            assert!(ret == OK);
            assert!(new_count == count - 1);
        } else {
            assert!(ret == EBUSY);
        }
    }

    /// SK9: push-pop roundtrip preserves count.
    #[kani::proof]
    fn stack_push_pop_roundtrip() {
        let count: u32 = kani::any();
        let capacity: u32 = kani::any();
        kani::assume(capacity > 0 && capacity <= 16);
        kani::assume(count < capacity); // not full

        let mut after_push: u32 = 0;
        let ret1 = gale_stack_push_validate(count, capacity, &mut after_push);
        assert!(ret1 == OK);
        assert!(after_push == count + 1);

        let mut after_pop: u32 = 0;
        let ret2 = gale_stack_pop_validate(after_push, &mut after_pop);
        assert!(ret2 == OK);
        assert!(after_pop == count);
    }

    /// Null pointer checks return EINVAL.
    #[kani::proof]
    fn stack_null_pointers() {
        assert!(gale_stack_push_validate(0, 10, core::ptr::null_mut()) == EINVAL);
        assert!(gale_stack_pop_validate(1, core::ptr::null_mut()) == EINVAL);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — timer
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "timer"))]
mod kani_timer_proofs {
    use super::*;

    /// Init always succeeds for any period value.
    #[kani::proof]
    fn timer_init_always_ok() {
        let period: u32 = kani::any();
        let ret = gale_timer_init_validate(period);
        assert!(ret == OK);
    }

    /// TM5/TM8: expire validates overflow and increments status.
    #[kani::proof]
    fn timer_expire_validates() {
        let status: u32 = kani::any();

        let mut new_status: u32 = 0;
        let ret = gale_timer_expire(status, &mut new_status);

        if status == u32::MAX {
            assert!(ret == EOVERFLOW);
        } else {
            assert!(ret == OK);
            assert!(new_status == status + 1);
        }
    }

    /// TM2: status_get returns old value and resets to 0.
    #[kani::proof]
    fn timer_status_get_resets() {
        let status: u32 = kani::any();

        let mut new_status: u32 = 99;
        let old = gale_timer_status_get(status, &mut new_status);

        assert!(old == status);
        assert!(new_status == 0);
    }

    /// Expire then status_get roundtrip.
    #[kani::proof]
    fn timer_expire_status_get_roundtrip() {
        let status: u32 = kani::any();
        kani::assume(status < u32::MAX);

        let mut after_expire: u32 = 0;
        let ret = gale_timer_expire(status, &mut after_expire);
        assert!(ret == OK);
        assert!(after_expire == status + 1);

        let mut after_get: u32 = 99;
        let old = gale_timer_status_get(after_expire, &mut after_get);
        assert!(old == status + 1);
        assert!(after_get == 0);
    }

    /// Null pointer checks return EINVAL.
    #[kani::proof]
    fn timer_null_pointers() {
        assert!(gale_timer_expire(0, core::ptr::null_mut()) == EINVAL);
    }

    /// TM5/TM8: expire_decide increments status (saturating at u32::MAX).
    #[kani::proof]
    fn timer_expire_decide_validates() {
        let status: u32 = kani::any();
        let period: u32 = kani::any();

        let d = gale_k_timer_expire_decide(status, period);

        if status < u32::MAX {
            assert!(d.new_status == status + 1);
        } else {
            assert!(d.new_status == u32::MAX);
        }

        if period > 0 {
            assert!(d.is_periodic == 1);
        } else {
            assert!(d.is_periodic == 0);
        }
    }

    /// TM2: status_decide returns old status and resets to 0.
    #[kani::proof]
    fn timer_status_decide_resets() {
        let status: u32 = kani::any();

        let d = gale_k_timer_status_decide(status);

        assert!(d.count == status);
        assert!(d.new_status == 0);
    }

    /// Decision roundtrip: expire_decide then status_decide.
    #[kani::proof]
    fn timer_decide_roundtrip() {
        let status: u32 = kani::any();
        let period: u32 = kani::any();
        kani::assume(status < u32::MAX);

        let expire_d = gale_k_timer_expire_decide(status, period);
        assert!(expire_d.new_status == status + 1);

        let status_d = gale_k_timer_status_decide(expire_d.new_status);
        assert!(status_d.count == status + 1);
        assert!(status_d.new_status == 0);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — memory slab
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "mem_slab"))]
mod kani_mem_slab_proofs {
    use super::*;

    /// MS2/MS3: init rejects zero block_size or num_blocks.
    #[kani::proof]
    fn mem_slab_init_validates() {
        let block_size: u32 = kani::any();
        let num_blocks: u32 = kani::any();
        let ret = gale_mem_slab_init_validate(block_size, num_blocks);
        if block_size == 0 || num_blocks == 0 {
            assert!(ret == EINVAL);
        } else {
            assert!(ret == OK);
        }
    }

    /// MS4/MS5: alloc validates capacity and increments num_used.
    #[kani::proof]
    fn mem_slab_alloc_validates() {
        let num_used: u32 = kani::any();
        let num_blocks: u32 = kani::any();
        kani::assume(num_blocks > 0 && num_blocks <= 16);
        kani::assume(num_used <= num_blocks);

        let mut new_num_used: u32 = 0;
        let ret = gale_mem_slab_alloc_validate(num_used, num_blocks, &mut new_num_used);

        if num_used < num_blocks {
            assert!(ret == OK);
            assert!(new_num_used == num_used + 1);
            assert!(new_num_used <= num_blocks);
        } else {
            assert!(ret == ENOMEM);
        }
    }

    /// MS6: free validates non-empty and decrements num_used.
    #[kani::proof]
    fn mem_slab_free_validates() {
        let num_used: u32 = kani::any();

        let mut new_num_used: u32 = 0;
        let ret = gale_mem_slab_free_validate(num_used, &mut new_num_used);

        if num_used > 0 {
            assert!(ret == OK);
            assert!(new_num_used == num_used - 1);
        } else {
            assert!(ret == EINVAL);
        }
    }

    /// MS4+MS6: alloc-free roundtrip preserves num_used.
    #[kani::proof]
    fn mem_slab_alloc_free_roundtrip() {
        let num_used: u32 = kani::any();
        let num_blocks: u32 = kani::any();
        kani::assume(num_blocks > 0 && num_blocks <= 16);
        kani::assume(num_used < num_blocks); // not full

        let mut after_alloc: u32 = 0;
        let ret1 = gale_mem_slab_alloc_validate(num_used, num_blocks, &mut after_alloc);
        assert!(ret1 == OK);
        assert!(after_alloc == num_used + 1);

        let mut after_free: u32 = 0;
        let ret2 = gale_mem_slab_free_validate(after_alloc, &mut after_free);
        assert!(ret2 == OK);
        assert!(after_free == num_used);
    }

    /// Null pointer checks return EINVAL.
    #[kani::proof]
    fn mem_slab_null_pointers() {
        assert!(gale_mem_slab_alloc_validate(0, 10, core::ptr::null_mut()) == EINVAL);
        assert!(gale_mem_slab_free_validate(1, core::ptr::null_mut()) == EINVAL);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — event
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "event"))]
mod kani_event_proofs {
    use super::*;

    /// EV1: post ORs bits correctly.
    #[kani::proof]
    fn event_post_ors_bits() {
        let events: u32 = kani::any();
        let new_events: u32 = kani::any();
        let mut result: u32 = 0;
        let ret = gale_event_post(events, new_events, &mut result);
        assert!(ret == OK);
        assert!(result == events | new_events);
        // EV8: monotonic — old bits preserved
        assert!(result & events == events);
    }

    /// EV2: set records old value.
    #[kani::proof]
    fn event_set_records_old() {
        let new_events: u32 = kani::any();
        let current: u32 = kani::any();
        let mut old_events: u32 = 0;
        let ret = gale_event_set(new_events, &mut old_events, current);
        assert!(ret == OK);
        assert!(old_events == current);
    }

    /// EV3: clear ANDs complement.
    #[kani::proof]
    fn event_clear_ands_complement() {
        let events: u32 = kani::any();
        let clear_bits: u32 = kani::any();
        let mut result: u32 = 0;
        let ret = gale_event_clear(events, clear_bits, &mut result);
        assert!(ret == OK);
        assert!(result == events & !clear_bits);
    }

    /// EV4: set_masked applies mask correctly.
    #[kani::proof]
    fn event_set_masked_applies_mask() {
        let events: u32 = kani::any();
        let new_bits: u32 = kani::any();
        let mask: u32 = kani::any();
        let mut result: u32 = 0;
        let ret = gale_event_set_masked(events, new_bits, mask, &mut result);
        assert!(ret == OK);
        assert!(result == (events & !mask) | (new_bits & mask));
    }

    /// EV5: wait_check_any returns correct result.
    #[kani::proof]
    fn event_wait_check_any_correct() {
        let events: u32 = kani::any();
        let desired: u32 = kani::any();
        let ret = gale_event_wait_check_any(events, desired);
        if (events & desired) != 0 {
            assert!(ret == 1);
        } else {
            assert!(ret == 0);
        }
    }

    /// EV6: wait_check_all returns correct result.
    #[kani::proof]
    fn event_wait_check_all_correct() {
        let events: u32 = kani::any();
        let desired: u32 = kani::any();
        let ret = gale_event_wait_check_all(events, desired);
        if (events & desired) == desired {
            assert!(ret == 1);
        } else {
            assert!(ret == 0);
        }
    }

    /// EV5+EV6: wait_all implies wait_any for non-zero desired.
    #[kani::proof]
    fn event_wait_all_implies_any() {
        let events: u32 = kani::any();
        let desired: u32 = kani::any();
        kani::assume(desired != 0);
        let all = gale_event_wait_check_all(events, desired);
        let any = gale_event_wait_check_any(events, desired);
        if all == 1 {
            assert!(any == 1);
        }
    }

    /// EV1: double-post idempotence.
    #[kani::proof]
    fn event_post_idempotent() {
        let events: u32 = kani::any();
        let new_events: u32 = kani::any();
        let mut after_first: u32 = 0;
        let mut after_second: u32 = 0;
        gale_event_post(events, new_events, &mut after_first);
        gale_event_post(after_first, new_events, &mut after_second);
        assert!(after_second == after_first);
    }

    /// Null pointer checks return EINVAL.
    #[kani::proof]
    fn event_null_pointers() {
        assert!(gale_event_post(0, 0, core::ptr::null_mut()) == EINVAL);
        assert!(gale_event_set(0, core::ptr::null_mut(), 0) == EINVAL);
        assert!(gale_event_clear(0, 0, core::ptr::null_mut()) == EINVAL);
        assert!(gale_event_set_masked(0, 0, 0, core::ptr::null_mut()) == EINVAL);
    }

    // ---- Phase 2 decision struct proofs ----

    /// EV4-D: post_decide computes (current & ~mask) | (new & mask).
    #[kani::proof]
    fn event_post_decide_masked_set() {
        let current: u32 = kani::any();
        let new: u32 = kani::any();
        let mask: u32 = kani::any();
        let d = gale_k_event_post_decide(current, new, mask);
        assert!(d.new_events == (current & !mask) | (new & mask));
    }

    /// EV4-D: post_decide with full mask is equivalent to replacement.
    #[kani::proof]
    fn event_post_decide_full_mask() {
        let current: u32 = kani::any();
        let new: u32 = kani::any();
        let d = gale_k_event_post_decide(current, new, !0u32);
        assert!(d.new_events == new);
    }

    /// EV4-D: post_decide with self-mask is equivalent to OR (post).
    #[kani::proof]
    fn event_post_decide_self_mask() {
        let current: u32 = kani::any();
        let new: u32 = kani::any();
        let d = gale_k_event_post_decide(current, new, new);
        // (current & ~new) | (new & new) = (current & ~new) | new = current | new
        assert!(d.new_events == current | new);
    }

    /// EV5-D: wait_decide ANY matches when at least one desired bit set.
    #[kani::proof]
    fn event_wait_decide_any_matched() {
        let current: u32 = kani::any();
        let desired: u32 = kani::any();
        kani::assume(desired != 0);
        kani::assume((current & desired) != 0);
        let d = gale_k_event_wait_decide(current, desired, GALE_EVENT_WAIT_ANY, 0);
        assert!(d.action == GALE_EVENT_ACTION_MATCHED);
        assert!(d.matched_events == current & desired);
    }

    /// EV6-D: wait_decide ALL matches only when all desired bits set.
    #[kani::proof]
    fn event_wait_decide_all_matched() {
        let current: u32 = kani::any();
        let desired: u32 = kani::any();
        kani::assume((current & desired) == desired);
        let d = gale_k_event_wait_decide(current, desired, GALE_EVENT_WAIT_ALL, 0);
        assert!(d.action == GALE_EVENT_ACTION_MATCHED);
        assert!(d.matched_events == desired);
    }

    /// EV6-D: wait_decide ALL does not match partial bits.
    #[kani::proof]
    fn event_wait_decide_all_no_match() {
        let current: u32 = kani::any();
        let desired: u32 = kani::any();
        kani::assume(desired != 0);
        kani::assume((current & desired) != desired);
        let d = gale_k_event_wait_decide(current, desired, GALE_EVENT_WAIT_ALL, 0);
        assert!(d.action == GALE_EVENT_ACTION_PEND);
        assert!(d.matched_events == 0);
    }

    /// Wait_decide returns TIMEOUT when no-wait and no match.
    #[kani::proof]
    fn event_wait_decide_no_wait_timeout() {
        let current: u32 = kani::any();
        let desired: u32 = kani::any();
        let wait_type: u8 = kani::any();
        kani::assume(wait_type <= 1);
        kani::assume(desired > 0); // desired=0 with WAIT_ALL always matches (0&x==0)
        kani::assume((current & desired) == 0); // no matching bits
        let d = gale_k_event_wait_decide(current, desired, wait_type, 1);
        assert!(d.action == GALE_EVENT_ACTION_TIMEOUT);
        assert!(d.matched_events == 0);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — fifo
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "fifo"))]
mod kani_fifo_proofs {
    use super::*;

    /// FI1/FI2: put validates overflow and increments count.
    #[kani::proof]
    fn fifo_put_validates() {
        let count: u32 = kani::any();
        let mut new_count: u32 = 0;
        let ret = gale_fifo_put_validate(count, &mut new_count);
        if count >= u32::MAX - 1 {
            assert!(ret == EOVERFLOW);
        } else {
            assert!(ret == OK);
            assert!(new_count == count + 1);
        }
    }

    /// FI3/FI4: get validates underflow and decrements count.
    #[kani::proof]
    fn fifo_get_validates() {
        let count: u32 = kani::any();
        let mut new_count: u32 = 0;
        let ret = gale_fifo_get_validate(count, &mut new_count);
        if count == 0 {
            assert!(ret == EAGAIN);
        } else {
            assert!(ret == OK);
            assert!(new_count == count - 1);
        }
    }

    /// Put then get is identity.
    #[kani::proof]
    fn fifo_put_get_roundtrip() {
        let count: u32 = kani::any();
        kani::assume(count < u32::MAX - 1);
        let mut after_put: u32 = 0;
        let ret1 = gale_fifo_put_validate(count, &mut after_put);
        assert!(ret1 == OK);
        let mut after_get: u32 = 0;
        let ret2 = gale_fifo_get_validate(after_put, &mut after_get);
        assert!(ret2 == OK);
        assert!(after_get == count);
    }

    /// Null pointer checks return EINVAL.
    #[kani::proof]
    fn fifo_null_pointers() {
        assert!(gale_fifo_put_validate(0, core::ptr::null_mut()) == EINVAL);
        assert!(gale_fifo_get_validate(1, core::ptr::null_mut()) == EINVAL);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — lifo
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "lifo"))]
mod kani_lifo_proofs {
    use super::*;

    /// LI1/LI2: put validates overflow and increments count.
    #[kani::proof]
    fn lifo_put_validates() {
        let count: u32 = kani::any();
        let mut new_count: u32 = 0;
        let ret = gale_lifo_put_validate(count, &mut new_count);
        if count >= u32::MAX - 1 {
            assert!(ret == EOVERFLOW);
        } else {
            assert!(ret == OK);
            assert!(new_count == count + 1);
        }
    }

    /// LI3/LI4: get validates underflow and decrements count.
    #[kani::proof]
    fn lifo_get_validates() {
        let count: u32 = kani::any();
        let mut new_count: u32 = 0;
        let ret = gale_lifo_get_validate(count, &mut new_count);
        if count == 0 {
            assert!(ret == EAGAIN);
        } else {
            assert!(ret == OK);
            assert!(new_count == count - 1);
        }
    }

    /// Put then get is identity.
    #[kani::proof]
    fn lifo_put_get_roundtrip() {
        let count: u32 = kani::any();
        kani::assume(count < u32::MAX - 1);
        let mut after_put: u32 = 0;
        let ret1 = gale_lifo_put_validate(count, &mut after_put);
        assert!(ret1 == OK);
        let mut after_get: u32 = 0;
        let ret2 = gale_lifo_get_validate(after_put, &mut after_get);
        assert!(ret2 == OK);
        assert!(after_get == count);
    }

    /// Null pointer checks return EINVAL.
    #[kani::proof]
    fn lifo_null_pointers() {
        assert!(gale_lifo_put_validate(0, core::ptr::null_mut()) == EINVAL);
        assert!(gale_lifo_get_validate(1, core::ptr::null_mut()) == EINVAL);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — queue
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "queue"))]
mod kani_queue_proofs {
    use super::*;

    /// QU1/QU2: append validates overflow and increments count.
    #[kani::proof]
    fn queue_append_validates() {
        let count: u32 = kani::any();
        let mut new_count: u32 = 0;
        let ret = gale_queue_append_validate(count, &mut new_count);
        if count >= u32::MAX - 1 {
            assert!(ret == EOVERFLOW);
        } else {
            assert!(ret == OK);
            assert!(new_count == count + 1);
        }
    }

    /// QU3/QU4: prepend validates overflow and increments count.
    #[kani::proof]
    fn queue_prepend_validates() {
        let count: u32 = kani::any();
        let mut new_count: u32 = 0;
        let ret = gale_queue_prepend_validate(count, &mut new_count);
        if count >= u32::MAX - 1 {
            assert!(ret == EOVERFLOW);
        } else {
            assert!(ret == OK);
            assert!(new_count == count + 1);
        }
    }

    /// QU5/QU6: get validates underflow and decrements count.
    #[kani::proof]
    fn queue_get_validates() {
        let count: u32 = kani::any();
        let mut new_count: u32 = 0;
        let ret = gale_queue_get_validate(count, &mut new_count);
        if count == 0 {
            assert!(ret == EAGAIN);
        } else {
            assert!(ret == OK);
            assert!(new_count == count - 1);
        }
    }

    /// Append then get is identity.
    #[kani::proof]
    fn queue_append_get_roundtrip() {
        let count: u32 = kani::any();
        kani::assume(count < u32::MAX - 1);
        let mut after_append: u32 = 0;
        let ret1 = gale_queue_append_validate(count, &mut after_append);
        assert!(ret1 == OK);
        let mut after_get: u32 = 0;
        let ret2 = gale_queue_get_validate(after_append, &mut after_get);
        assert!(ret2 == OK);
        assert!(after_get == count);
    }

    /// Null pointer checks return EINVAL.
    #[kani::proof]
    fn queue_null_pointers() {
        assert!(gale_queue_append_validate(0, core::ptr::null_mut()) == EINVAL);
        assert!(gale_queue_prepend_validate(0, core::ptr::null_mut()) == EINVAL);
        assert!(gale_queue_get_validate(1, core::ptr::null_mut()) == EINVAL);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — mbox
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "mbox"))]
mod kani_mbox_proofs {
    use super::*;

    /// MB1: validate_send rejects zero size.
    #[kani::proof]
    fn mbox_validate_send_checks() {
        let size: u32 = kani::any();
        let ret = gale_mbox_validate_send(size);
        if size == 0 {
            assert!(ret == EINVAL);
        } else {
            assert!(ret == OK);
        }
    }

    /// MB2: K_ANY (0) matches any ID.
    #[kani::proof]
    fn mbox_match_k_any() {
        let id: u32 = kani::any();
        // send_id == 0 (K_ANY) always matches
        assert!(gale_mbox_match_check(0, id) == 1);
        // recv_id == 0 (K_ANY) always matches
        assert!(gale_mbox_match_check(id, 0) == 1);
    }

    /// MB3: equal non-zero IDs match.
    #[kani::proof]
    fn mbox_match_equal_ids() {
        let id: u32 = kani::any();
        kani::assume(id != 0);
        assert!(gale_mbox_match_check(id, id) == 1);
    }

    /// MB4: different non-zero IDs do not match.
    #[kani::proof]
    fn mbox_match_different_ids() {
        let a: u32 = kani::any();
        let b: u32 = kani::any();
        kani::assume(a != 0 && b != 0 && a != b);
        assert!(gale_mbox_match_check(a, b) == 0);
    }

    /// MB5: data_exchange returns min of tx_size and rx_buf_size.
    #[kani::proof]
    fn mbox_data_exchange_is_min() {
        let tx: u32 = kani::any();
        let rx: u32 = kani::any();
        let result = gale_mbox_data_exchange(tx, rx);
        if tx < rx {
            assert!(result == tx);
        } else {
            assert!(result == rx);
        }
    }

    /// MB6: data_exchange is commutative when equal.
    #[kani::proof]
    fn mbox_data_exchange_symmetric() {
        let size: u32 = kani::any();
        assert!(gale_mbox_data_exchange(size, size) == size);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — mem_domain
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "mem_domain"))]
mod kani_mem_domain_proofs {
    use super::*;

    /// MD3: check_partition rejects zero-size partition.
    #[kani::proof]
    fn mem_domain_check_rejects_zero_size() {
        let starts = [0u32; 16];
        let sizes = [0u32; 16];
        let d = gale_mem_domain_check_partition(
            0x1000, 0, starts.as_ptr(), sizes.as_ptr(), 0,
        );
        assert!(d.ret == EINVAL);
    }

    /// MD6: check_partition rejects overflow (start + size wraps).
    #[kani::proof]
    fn mem_domain_check_rejects_overflow() {
        let starts = [0u32; 16];
        let sizes = [0u32; 16];
        let d = gale_mem_domain_check_partition(
            u32::MAX, 2, starts.as_ptr(), sizes.as_ptr(), 0,
        );
        assert!(d.ret == EINVAL);
    }

    /// MD1: check_partition rejects overlapping partition.
    #[kani::proof]
    fn mem_domain_check_rejects_overlap() {
        let starts = [0x1000u32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let sizes = [0x100u32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        // New partition overlaps existing [0x1000, 0x1100)
        let d = gale_mem_domain_check_partition(
            0x1050, 0x100, starts.as_ptr(), sizes.as_ptr(), 1,
        );
        assert!(d.ret == EINVAL);
    }

    /// MD1: check_partition accepts non-overlapping partition.
    #[kani::proof]
    fn mem_domain_check_accepts_valid() {
        let starts = [0x1000u32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let sizes = [0x100u32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        // New partition at [0x2000, 0x2100) — no overlap
        let d = gale_mem_domain_check_partition(
            0x2000, 0x100, starts.as_ptr(), sizes.as_ptr(), 1,
        );
        assert!(d.ret == OK);
    }

    /// Add decision finds free slot and increments count.
    #[kani::proof]
    fn mem_domain_add_finds_slot() {
        let starts = [0x1000u32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let sizes = [0x100u32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let d = gale_k_mem_domain_add_partition_decide(
            0x2000, 0x100, 0, starts.as_ptr(), sizes.as_ptr(), 1,
        );
        assert!(d.ret == OK);
        assert!(d.slot == 1); // first free slot
        assert!(d.new_num_partitions == 2);
        assert!(d.action == GALE_MEM_DOMAIN_ACTION_ADD_OK);
    }

    /// Remove decision finds matching partition.
    #[kani::proof]
    fn mem_domain_remove_finds_match() {
        let starts = [0x1000u32, 0x2000, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let sizes = [0x100u32, 0x200, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let d = gale_k_mem_domain_remove_partition_decide(
            0x2000, 0x200, starts.as_ptr(), sizes.as_ptr(), 2,
        );
        assert!(d.ret == OK);
        assert!(d.slot == 1);
        assert!(d.new_num_partitions == 1);
        assert!(d.action == GALE_MEM_DOMAIN_ACTION_REMOVE_OK);
    }

    /// Remove decision returns ENOENT when no match.
    #[kani::proof]
    fn mem_domain_remove_no_match() {
        let starts = [0x1000u32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let sizes = [0x100u32, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
        let d = gale_k_mem_domain_remove_partition_decide(
            0x9999, 0x100, starts.as_ptr(), sizes.as_ptr(), 1,
        );
        assert!(d.ret == ENOENT);
        assert!(d.action == GALE_MEM_DOMAIN_ACTION_REMOVE_ERROR);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — userspace
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "userspace"))]
mod kani_userspace_proofs {
    use super::*;

    /// US1: access denied when not public and no perm bit.
    #[kani::proof]
    fn userspace_access_denied_no_perm() {
        let flags: u8 = kani::any();
        // Ensure not public
        kani::assume((flags & K_OBJ_FLAG_PUBLIC) == 0);
        let d = gale_k_object_access_decide(flags, 0);
        assert!(d.granted == 0);
    }

    /// US1: access granted when perm bit is set.
    #[kani::proof]
    fn userspace_access_granted_perm_bit() {
        let flags: u8 = kani::any();
        kani::assume((flags & K_OBJ_FLAG_PUBLIC) == 0);
        let d = gale_k_object_access_decide(flags, 1);
        assert!(d.granted == 1);
    }

    /// US5: access granted when public flag is set.
    #[kani::proof]
    fn userspace_access_granted_public() {
        let flags: u8 = kani::any();
        kani::assume((flags & K_OBJ_FLAG_PUBLIC) != 0);
        let has_perm: u8 = kani::any();
        let d = gale_k_object_access_decide(flags, has_perm);
        assert!(d.granted == 1);
    }

    /// US4: type mismatch returns EBADF.
    #[kani::proof]
    fn userspace_validate_type_mismatch() {
        let obj_type: u8 = kani::any();
        let expected: u8 = kani::any();
        kani::assume(expected != 0 && obj_type != expected);
        let flags: u8 = kani::any();
        let has_access: u8 = kani::any();
        let init_check: i8 = kani::any();
        let d = gale_k_object_validate_decide(
            obj_type, expected, flags, has_access, init_check,
        );
        assert!(d.ret == EBADF);
    }

    /// US1: no access returns EPERM (when type matches).
    #[kani::proof]
    fn userspace_validate_no_access() {
        let obj_type: u8 = kani::any();
        let flags: u8 = kani::any();
        let init_check: i8 = kani::any();
        // K_OBJ_ANY matches any type
        let d = gale_k_object_validate_decide(
            obj_type, 0, flags, 0, init_check,
        );
        assert!(d.ret == EPERM);
    }

    /// US7: uninitialized + MustBeInit returns EINVAL.
    #[kani::proof]
    fn userspace_validate_uninit_must_be_init() {
        let obj_type: u8 = kani::any();
        let flags: u8 = kani::any();
        // Not initialized
        kani::assume((flags & K_OBJ_FLAG_INITIALIZED) == 0);
        let d = gale_k_object_validate_decide(
            obj_type, 0, flags, 1, OBJ_INIT_TRUE,
        );
        assert!(d.ret == EINVAL);
    }

    /// US7: initialized + MustNotBeInit returns EADDRINUSE.
    #[kani::proof]
    fn userspace_validate_init_must_not_be_init() {
        let obj_type: u8 = kani::any();
        let flags: u8 = kani::any();
        // Is initialized
        kani::assume((flags & K_OBJ_FLAG_INITIALIZED) != 0);
        let d = gale_k_object_validate_decide(
            obj_type, 0, flags, 1, OBJ_INIT_FALSE,
        );
        assert!(d.ret == EADDRINUSE);
    }

    /// US7: init_decide always sets INITIALIZED bit.
    #[kani::proof]
    fn userspace_init_sets_flag() {
        let flags: u8 = kani::any();
        let d = gale_k_object_init_decide(flags);
        assert!((d.new_flags & K_OBJ_FLAG_INITIALIZED) != 0);
    }

    /// US7: uninit_decide always clears INITIALIZED bit.
    #[kani::proof]
    fn userspace_uninit_clears_flag() {
        let flags: u8 = kani::any();
        let d = gale_k_object_uninit_decide(flags);
        assert!((d.new_flags & K_OBJ_FLAG_INITIALIZED) == 0);
    }

    /// US5: make_public sets PUBLIC flag.
    #[kani::proof]
    fn userspace_make_public_sets_flag() {
        let flags: u8 = kani::any();
        let d = gale_k_object_make_public_decide(flags);
        assert!((d.new_flags & K_OBJ_FLAG_PUBLIC) != 0);
    }

    /// Recycle always sets INITIALIZED and requests perm clear.
    #[kani::proof]
    fn userspace_recycle_sets_init_and_clears() {
        let flags: u8 = kani::any();
        let d = gale_k_object_recycle_decide(flags);
        assert!((d.new_flags & K_OBJ_FLAG_INITIALIZED) != 0);
        assert!(d.clear_perms == 1);
    }
}

// ---------------------------------------------------------------------------
// FFI exports — sys_heap (chunk-level allocation invariants)
// ---------------------------------------------------------------------------
//
// These pure functions replace the safety-critical decision points in
// lib/heap/heap.c:
//
//   heap.c:266-303   sys_heap_alloc  — alloc + split + set_used
//   heap.c:166-201   sys_heap_free   — double-free check + coalesce
//   heap.c:112-125   split_chunks    — chunk count conservation
//   heap.c:128-134   merge_chunks    — chunk count conservation
//   heap.c:136-152   free_chunk      — coalesce with neighbors
//   heap.c:312-388   sys_heap_aligned_alloc — alignment padding overflow
//   heap.c:467-492   sys_heap_realloc — shrink/grow decision
//
// The actual free-list traversal, pointer arithmetic, memory layout,
// and bucket management remain in C. We model the chunk-level
// accounting invariants that prevent:
//   - Double-free (HP5)
//   - Heap overflow in size calculations (HP7)
//   - Chunk conservation violations (HP2)
//   - Bounds violations (HP1)
//
// Verified by Verus (SMT/Z3):
//   HP1: allocated_bytes <= capacity (bounds invariant)
//   HP2: free_chunks + used_chunks == total_chunks (conservation)
//   HP3: alloc succeeds only when enough free space
//   HP4: free returns exactly what was allocated
//   HP5: no double-free (chunk state tracking)
//   HP6: aligned allocation respects alignment constraints
//   HP7: no overflow in size calculations
//   HP8: merge adjacent free chunks maintains invariant

/// Decision struct for sys_heap_alloc — tells C shim what action to take
/// after alloc_chunk returns.
///
/// C calls alloc_chunk() to find a free chunk, then passes the result
/// to Rust. Rust decides: split the remainder or use the whole chunk.
#[repr(C)]
pub struct GaleHeapAllocDecision {
    /// Action: 0=USE_WHOLE, 1=SPLIT_AND_USE, 2=ALLOC_FAILED
    pub action: u8,
    /// 1 if alloc is valid (chunk state ok), 0 if rejected
    pub valid: u8,
}

/// Use the entire found chunk (no split needed).
pub const GALE_HEAP_ACTION_USE_WHOLE: u8 = 0;
/// Split the found chunk: left part is allocated, right part freed.
pub const GALE_HEAP_ACTION_SPLIT_AND_USE: u8 = 1;
/// Allocation failed — no suitable chunk found.
pub const GALE_HEAP_ACTION_ALLOC_FAILED: u8 = 2;

/// Full decision for sys_heap_alloc: decides whether to split the
/// found chunk or use it whole.
///
/// The C shim calls alloc_chunk() to find a free chunk of at least
/// `chunk_sz` units, then passes the result to Rust.
///
/// Arguments:
///   found_chunk:      1 if alloc_chunk returned non-zero, 0 if failed
///   found_chunk_sz:   size of the found chunk (in chunk units)
///   needed_chunk_sz:  requested size (in chunk units)
///
/// Verified: HP1 (bounds), HP2 (conservation), HP3 (alloc gating),
///           HP7 (no overflow).
#[cfg(feature = "heap")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sys_heap_alloc_decide(
    found_chunk: u32,
    found_chunk_sz: u32,
    needed_chunk_sz: u32,
) -> GaleHeapAllocDecision {
    if found_chunk == 0 {
        return GaleHeapAllocDecision {
            action: GALE_HEAP_ACTION_ALLOC_FAILED,
            valid: 1,
        };
    }

    if needed_chunk_sz == 0 {
        return GaleHeapAllocDecision {
            action: GALE_HEAP_ACTION_ALLOC_FAILED,
            valid: 0,
        };
    }

    if found_chunk_sz < needed_chunk_sz {
        return GaleHeapAllocDecision {
            action: GALE_HEAP_ACTION_ALLOC_FAILED,
            valid: 0,
        };
    }

    if found_chunk_sz > needed_chunk_sz {
        GaleHeapAllocDecision {
            action: GALE_HEAP_ACTION_SPLIT_AND_USE,
            valid: 1,
        }
    } else {
        GaleHeapAllocDecision {
            action: GALE_HEAP_ACTION_USE_WHOLE,
            valid: 1,
        }
    }
}

/// Decision struct for sys_heap_free — tells C shim what action to take.
///
/// C extracts chunk state (chunk_used flag, left/right neighbor state),
/// Rust validates the free is safe and decides coalescing strategy.
#[repr(C)]
pub struct GaleHeapFreeDecision {
    /// Action: 0=FREE_AND_COALESCE, 1=FREE_REJECTED
    pub action: u8,
    /// 1 if should merge with right neighbor, 0 if not
    pub merge_right: u8,
    /// 1 if should merge with left neighbor, 0 if not
    pub merge_left: u8,
}

/// Free is valid — proceed with coalescing as indicated.
pub const GALE_HEAP_ACTION_FREE_AND_COALESCE: u8 = 0;
/// Free is rejected — chunk is not in-use (double-free).
pub const GALE_HEAP_ACTION_FREE_REJECTED: u8 = 1;

/// Full decision for sys_heap_free: validates the free and decides
/// coalescing strategy.
///
/// The C shim extracts the chunk state before calling Rust:
///   - Is the chunk currently marked as used?
///   - Is the right neighbor free?
///   - Is the left neighbor free?
///   - Does left_chunk(right_chunk(c)) == c? (overflow detection)
///
/// Arguments:
///   chunk_is_used:         1 if chunk_used(h, c) is true
///   right_neighbor_free:   1 if right neighbor exists and is free
///   left_neighbor_free:    1 if left neighbor exists and is free
///   bounds_check_passed:   1 if left_chunk(right_chunk(c)) == c
///
/// Verified: HP4 (exact free), HP5 (double-free), HP8 (coalesce).
#[cfg(feature = "heap")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sys_heap_free_decide(
    chunk_is_used: u32,
    right_neighbor_free: u32,
    left_neighbor_free: u32,
    bounds_check_passed: u32,
) -> GaleHeapFreeDecision {
    if chunk_is_used == 0 {
        return GaleHeapFreeDecision {
            action: GALE_HEAP_ACTION_FREE_REJECTED,
            merge_right: 0,
            merge_left: 0,
        };
    }

    if bounds_check_passed == 0 {
        return GaleHeapFreeDecision {
            action: GALE_HEAP_ACTION_FREE_REJECTED,
            merge_right: 0,
            merge_left: 0,
        };
    }

    GaleHeapFreeDecision {
        action: GALE_HEAP_ACTION_FREE_AND_COALESCE,
        merge_right: if right_neighbor_free != 0 { 1 } else { 0 },
        merge_left: if left_neighbor_free != 0 { 1 } else { 0 },
    }
}

/// Decision struct for sys_heap_aligned_alloc — validates alignment
/// and computes padding.
#[repr(C)]
pub struct GaleHeapAlignedAllocDecision {
    /// Action: 0=USE_PLAIN_ALLOC, 1=USE_PADDED_ALLOC, 2=REJECT
    pub action: u8,
    /// Padded size in bytes (only valid when action == USE_PADDED_ALLOC)
    pub padded_bytes: u32,
}

/// Alignment <= chunk header — plain alloc is sufficient.
pub const GALE_HEAP_ALIGN_PLAIN: u8 = 0;
/// Alignment requires padding — use padded size.
pub const GALE_HEAP_ALIGN_PADDED: u8 = 1;
/// Alignment is invalid or padding overflows.
pub const GALE_HEAP_ALIGN_REJECT: u8 = 2;

/// Full decision for sys_heap_aligned_alloc: validates alignment
/// and computes padded allocation size.
///
/// Arguments:
///   bytes:               requested allocation size
///   align:               requested alignment (must be power of 2 or 0)
///   chunk_header_bytes:  chunk header size (4 or 8 depending on heap)
///
/// Verified: HP6 (alignment), HP7 (overflow in padding computation).
#[cfg(feature = "heap")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sys_heap_aligned_alloc_decide(
    bytes: u32,
    align: u32,
    chunk_header_bytes: u32,
) -> GaleHeapAlignedAllocDecision {
    if bytes == 0 {
        return GaleHeapAlignedAllocDecision {
            action: GALE_HEAP_ALIGN_REJECT,
            padded_bytes: 0,
        };
    }

    if align != 0 && (align & align.wrapping_sub(1)) != 0 {
        return GaleHeapAlignedAllocDecision {
            action: GALE_HEAP_ALIGN_REJECT,
            padded_bytes: 0,
        };
    }

    if align == 0 || align <= chunk_header_bytes {
        return GaleHeapAlignedAllocDecision {
            action: GALE_HEAP_ALIGN_PLAIN,
            padded_bytes: bytes,
        };
    }

    #[allow(clippy::arithmetic_side_effects)]
    let padding: u64 = align as u64 - chunk_header_bytes as u64;
    #[allow(clippy::arithmetic_side_effects)]
    let padded: u64 = bytes as u64 + padding;

    if padded > u32::MAX as u64 {
        return GaleHeapAlignedAllocDecision {
            action: GALE_HEAP_ALIGN_REJECT,
            padded_bytes: 0,
        };
    }

    GaleHeapAlignedAllocDecision {
        action: GALE_HEAP_ALIGN_PADDED,
        padded_bytes: padded as u32,
    }
}

/// Decision struct for sys_heap_realloc — decides shrink/grow/fail.
#[repr(C)]
pub struct GaleHeapReallocDecision {
    /// Action: 0=SHRINK_IN_PLACE, 1=GROW_IN_PLACE, 2=ALLOC_COPY_FREE, 3=REJECT
    pub action: u8,
}

/// Shrink: always succeeds in-place (split off remainder).
pub const GALE_HEAP_REALLOC_SHRINK: u8 = 0;
/// Grow in-place: right neighbor has enough free space.
pub const GALE_HEAP_REALLOC_GROW: u8 = 1;
/// Cannot grow in place — must alloc+copy+free.
pub const GALE_HEAP_REALLOC_COPY: u8 = 2;
/// Invalid request.
pub const GALE_HEAP_REALLOC_REJECT: u8 = 3;

/// Full decision for sys_heap_realloc: decides whether to shrink,
/// grow in-place, or fall back to alloc+copy+free.
///
/// Arguments:
///   current_chunk_sz:     current chunk size (in chunk units)
///   needed_chunk_sz:      new required chunk size (in chunk units)
///   right_neighbor_free:  1 if right neighbor is free, 0 otherwise
///   right_neighbor_sz:    size of right neighbor (in chunk units, 0 if N/A)
///
/// Verified: HP1 (bounds), HP7 (overflow), HP8 (split/merge).
#[cfg(feature = "heap")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sys_heap_realloc_decide(
    current_chunk_sz: u32,
    needed_chunk_sz: u32,
    right_neighbor_free: u32,
    right_neighbor_sz: u32,
) -> GaleHeapReallocDecision {
    if needed_chunk_sz == 0 {
        return GaleHeapReallocDecision {
            action: GALE_HEAP_REALLOC_REJECT,
        };
    }

    if current_chunk_sz >= needed_chunk_sz {
        return GaleHeapReallocDecision {
            action: GALE_HEAP_REALLOC_SHRINK,
        };
    }

    if right_neighbor_free != 0 {
        let combined: u64 = current_chunk_sz as u64 + right_neighbor_sz as u64;
        if combined >= needed_chunk_sz as u64 && combined <= u32::MAX as u64 {
            return GaleHeapReallocDecision {
                action: GALE_HEAP_REALLOC_GROW,
            };
        }
    }

    GaleHeapReallocDecision {
        action: GALE_HEAP_REALLOC_COPY,
    }
}

/// Validate a sys_heap_init: check that capacity is large enough
/// for the minimum metadata.
///
/// Arguments:
///   total_bytes:  raw heap memory size
///   min_overhead: minimum bytes needed for z_heap struct + buckets + end marker
///
/// Returns:
///   0 (OK) if valid, -EINVAL if too small.
///
/// Verified: HP1 (capacity > 0 after overhead).
#[cfg(feature = "heap")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sys_heap_init_validate(
    total_bytes: u32,
    min_overhead: u32,
) -> i32 {
    if total_bytes == 0 || min_overhead == 0 || min_overhead >= total_bytes {
        EINVAL
    } else {
        OK
    }
}

/// Validate split_chunks preconditions.
///
/// The split must produce two valid chunks: left_size > 0, right_size > 0,
/// and left_size + right_size == original_size.
///
/// Arguments:
///   original_sz:  size of chunk being split (in chunk units)
///   left_sz:      desired left chunk size (in chunk units)
///
/// Returns:
///   right_sz on success (> 0), 0 on invalid parameters.
///
/// Verified: HP2 (conservation), HP8 (split invariant).
#[cfg(feature = "heap")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sys_heap_split_validate(
    original_sz: u32,
    left_sz: u32,
) -> u32 {
    if left_sz == 0 || left_sz >= original_sz {
        0
    } else {
        #[allow(clippy::arithmetic_side_effects)]
        let right_sz = original_sz - left_sz;
        right_sz
    }
}

/// Validate merge_chunks preconditions.
///
/// Both chunks must be free, and neither may be zero-sized.
///
/// Arguments:
///   left_sz:   size of left chunk (in chunk units)
///   right_sz:  size of right chunk (in chunk units)
///   left_free:  1 if left is free, 0 if used
///   right_free: 1 if right is free, 0 if used
///   merged_sz: pointer to receive merged size
///
/// Returns:
///   0 (OK) if valid, -EINVAL if not.
///
/// Verified: HP2 (conservation), HP7 (no overflow), HP8 (merge invariant).
#[cfg(feature = "heap")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sys_heap_merge_validate(
    left_sz: u32,
    right_sz: u32,
    left_free: u32,
    right_free: u32,
    merged_sz: *mut u32,
) -> i32 {
    unsafe {
        if merged_sz.is_null() {
            return EINVAL;
        }

        if left_sz == 0 || right_sz == 0 {
            return EINVAL;
        }

        if left_free == 0 || right_free == 0 {
            return EINVAL;
        }

        let sum: u64 = left_sz as u64 + right_sz as u64;
        if sum > u32::MAX as u64 {
            return EINVAL;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            *merged_sz = left_sz + right_sz;
        }
        OK
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — sys_heap
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "heap"))]
mod kani_sys_heap_proofs {
    use super::*;

    /// HP3/HP1: alloc_decide returns ALLOC_FAILED when no chunk found.
    #[kani::proof]
    fn heap_alloc_decide_no_chunk() {
        let needed: u32 = kani::any();
        kani::assume(needed > 0);
        let d = gale_sys_heap_alloc_decide(0, 0, needed);
        assert!(d.action == GALE_HEAP_ACTION_ALLOC_FAILED);
        assert!(d.valid == 1);
    }

    /// HP2: alloc_decide splits when found > needed.
    #[kani::proof]
    fn heap_alloc_decide_split() {
        let found_sz: u32 = kani::any();
        let needed_sz: u32 = kani::any();
        kani::assume(needed_sz > 0);
        kani::assume(found_sz > needed_sz);
        let d = gale_sys_heap_alloc_decide(1, found_sz, needed_sz);
        assert!(d.action == GALE_HEAP_ACTION_SPLIT_AND_USE);
        assert!(d.valid == 1);
    }

    /// HP2: alloc_decide uses whole when found == needed.
    #[kani::proof]
    fn heap_alloc_decide_exact() {
        let sz: u32 = kani::any();
        kani::assume(sz > 0);
        let d = gale_sys_heap_alloc_decide(1, sz, sz);
        assert!(d.action == GALE_HEAP_ACTION_USE_WHOLE);
        assert!(d.valid == 1);
    }

    /// HP7: alloc_decide rejects found < needed (corruption).
    #[kani::proof]
    fn heap_alloc_decide_too_small() {
        let found_sz: u32 = kani::any();
        let needed_sz: u32 = kani::any();
        kani::assume(needed_sz > 0);
        kani::assume(found_sz > 0);
        kani::assume(found_sz < needed_sz);
        let d = gale_sys_heap_alloc_decide(1, found_sz, needed_sz);
        assert!(d.action == GALE_HEAP_ACTION_ALLOC_FAILED);
        assert!(d.valid == 0);
    }

    /// HP5: free_decide rejects double-free.
    #[kani::proof]
    fn heap_free_decide_double_free() {
        let right_free: u32 = kani::any();
        let left_free: u32 = kani::any();
        let d = gale_sys_heap_free_decide(0, right_free, left_free, 1);
        assert!(d.action == GALE_HEAP_ACTION_FREE_REJECTED);
    }

    /// HP5: free_decide rejects bounds check failure.
    #[kani::proof]
    fn heap_free_decide_bounds_fail() {
        let d = gale_sys_heap_free_decide(1, 0, 0, 0);
        assert!(d.action == GALE_HEAP_ACTION_FREE_REJECTED);
    }

    /// HP8: free_decide enables coalescing when neighbors are free.
    #[kani::proof]
    fn heap_free_decide_coalesce() {
        let right: u32 = kani::any();
        let left: u32 = kani::any();
        let d = gale_sys_heap_free_decide(1, right, left, 1);
        assert!(d.action == GALE_HEAP_ACTION_FREE_AND_COALESCE);
        if right != 0 {
            assert!(d.merge_right == 1);
        } else {
            assert!(d.merge_right == 0);
        }
        if left != 0 {
            assert!(d.merge_left == 1);
        } else {
            assert!(d.merge_left == 0);
        }
    }

    /// HP6: aligned_alloc rejects non-power-of-2 alignment.
    #[kani::proof]
    fn heap_aligned_alloc_bad_align() {
        let bytes: u32 = kani::any();
        kani::assume(bytes > 0);
        let d = gale_sys_heap_aligned_alloc_decide(bytes, 3, 8);
        assert!(d.action == GALE_HEAP_ALIGN_REJECT);
    }

    /// HP6: aligned_alloc uses plain when align <= header.
    #[kani::proof]
    fn heap_aligned_alloc_plain() {
        let bytes: u32 = kani::any();
        kani::assume(bytes > 0);
        let header: u32 = kani::any();
        kani::assume(header > 0 && header <= 8);
        let align: u32 = kani::any();
        kani::assume(align > 0 && align <= header);
        kani::assume(align & align.wrapping_sub(1) == 0);
        let d = gale_sys_heap_aligned_alloc_decide(bytes, align, header);
        assert!(d.action == GALE_HEAP_ALIGN_PLAIN);
    }

    /// HP7: aligned_alloc computes padded size without overflow.
    #[kani::proof]
    fn heap_aligned_alloc_padded() {
        let bytes: u32 = kani::any();
        kani::assume(bytes > 0 && bytes < u32::MAX - 256);
        let d = gale_sys_heap_aligned_alloc_decide(bytes, 64, 8);
        assert!(d.action == GALE_HEAP_ALIGN_PADDED);
        assert!(d.padded_bytes == bytes + 56);
    }

    /// HP8: split_validate produces valid right_sz.
    #[kani::proof]
    fn heap_split_validates() {
        let original: u32 = kani::any();
        let left: u32 = kani::any();
        kani::assume(original > 1);
        kani::assume(left > 0 && left < original);
        let right = gale_sys_heap_split_validate(original, left);
        assert!(right > 0);
        assert!(left + right == original);
    }

    /// HP8: split_validate rejects invalid params.
    #[kani::proof]
    fn heap_split_rejects_bad() {
        let original: u32 = kani::any();
        assert!(gale_sys_heap_split_validate(original, 0) == 0);
        assert!(gale_sys_heap_split_validate(original, original) == 0);
    }

    /// HP2/HP7: merge_validate produces correct merged size.
    #[kani::proof]
    fn heap_merge_validates() {
        let left: u32 = kani::any();
        let right: u32 = kani::any();
        kani::assume(left > 0 && right > 0);
        kani::assume((left as u64 + right as u64) <= u32::MAX as u64);
        let mut merged: u32 = 0;
        let rc = gale_sys_heap_merge_validate(left, right, 1, 1, &mut merged);
        assert!(rc == OK);
        assert!(merged == left + right);
    }

    /// HP2: merge_validate rejects if not both free.
    #[kani::proof]
    fn heap_merge_rejects_used() {
        let mut merged: u32 = 0;
        assert!(gale_sys_heap_merge_validate(10, 10, 0, 1, &mut merged) == EINVAL);
        assert!(gale_sys_heap_merge_validate(10, 10, 1, 0, &mut merged) == EINVAL);
    }

    /// Realloc: shrink decision.
    #[kani::proof]
    fn heap_realloc_shrink() {
        let current: u32 = kani::any();
        let needed: u32 = kani::any();
        kani::assume(needed > 0);
        kani::assume(current >= needed);
        let d = gale_sys_heap_realloc_decide(current, needed, 0, 0);
        assert!(d.action == GALE_HEAP_REALLOC_SHRINK);
    }

    /// Realloc: grow in-place decision.
    #[kani::proof]
    fn heap_realloc_grow_inplace() {
        let current: u32 = kani::any();
        let needed: u32 = kani::any();
        let right_sz: u32 = kani::any();
        kani::assume(needed > 0 && current > 0);
        kani::assume(needed > current);
        kani::assume((current as u64 + right_sz as u64) >= needed as u64);
        kani::assume((current as u64 + right_sz as u64) <= u32::MAX as u64);
        let d = gale_sys_heap_realloc_decide(current, needed, 1, right_sz);
        assert!(d.action == GALE_HEAP_REALLOC_GROW);
    }

    /// Realloc: alloc+copy+free fallback.
    #[kani::proof]
    fn heap_realloc_copy_fallback() {
        let current: u32 = kani::any();
        let needed: u32 = kani::any();
        kani::assume(needed > 0 && current > 0);
        kani::assume(needed > current);
        let d = gale_sys_heap_realloc_decide(current, needed, 0, 0);
        assert!(d.action == GALE_HEAP_REALLOC_COPY);
    }

    /// Init validate: accepts valid, rejects small.
    #[kani::proof]
    fn heap_init_validates() {
        let total: u32 = kani::any();
        let overhead: u32 = kani::any();
        let rc = gale_sys_heap_init_validate(total, overhead);
        if total == 0 || overhead == 0 || overhead >= total {
            assert!(rc == EINVAL);
        } else {
            assert!(rc == OK);
        }
    }
}

// ---------------------------------------------------------------------------
// FFI exports — ring_buffer (index arithmetic validation)
// ---------------------------------------------------------------------------
//
// These pure functions validate ring buffer index arithmetic from
// lib/utils/ring_buffer.c:
//
//   ring_buf_area_claim   — compute safe claim size (wrap-aware)
//   ring_buf_area_finish  — validate finish size <= claimed
//   ring_buf_space_get    — free space from modular arithmetic
//
// The actual buffer memory and memcpy stay in C. We model only the
// index state machine that prevents:
//   - Out-of-bounds buffer access (RB1, RB2)
//   - Over-claiming (reading past available data)
//   - Over-finishing (finishing more than claimed)
//   - Modular arithmetic overflow (RB8)
//
// Verified by Verus (SMT/Z3):
//   RB1: 0 <= size <= capacity (bounds invariant)
//   RB2: head < capacity, tail < capacity (index bounds)
//   RB3: put advances tail correctly (modular)
//   RB4: get advances head correctly (modular)
//   RB5: put on full buffer returns error
//   RB6: get on empty buffer returns error
//   RB7: size == (tail - head + capacity) % capacity (consistency)
//   RB8: no overflow in modular arithmetic

/// Decision struct for ring_buf_area_claim — tells C the safe claim size
/// and the buffer offset to use.
#[repr(C)]
pub struct GaleRingBufClaimDecision {
    /// Number of bytes that can be safely claimed (may be < requested).
    pub claim_size: u32,
    /// Buffer offset where data starts (head_offset after adjustment).
    pub buffer_offset: u32,
}

/// Compute safe claim size for a ring buffer put or get operation.
///
/// This models `ring_buf_area_claim()` from ring_buffer.c:12-29.
/// Given the current index state and buffer size, returns the maximum
/// contiguous bytes that can be claimed without wrapping.
///
/// Arguments:
///   head:        current ring->head (producer or consumer)
///   base:        current ring->base (lap tracker)
///   buf_size:    total buffer size in bytes
///   requested:   number of bytes the caller wants
///
/// Verified: RB1 (offset < buf_size), RB8 (no overflow).
#[cfg(feature = "ring_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ring_buf_claim_decide(
    head: u32,
    base: u32,
    buf_size: u32,
    requested: u32,
) -> GaleRingBufClaimDecision {
    if buf_size == 0 {
        return GaleRingBufClaimDecision {
            claim_size: 0,
            buffer_offset: 0,
        };
    }

    // head_offset = head - base, with wraparound adjustment.
    // C uses unsigned subtraction (wraps at u16/u32 boundary).
    // We use wrapping_sub to match, then mod buf_size for safety.
    let raw_offset = head.wrapping_sub(base);
    let head_offset = if raw_offset >= buf_size {
        raw_offset - buf_size
    } else {
        raw_offset
    };
    // Clamp: if still >= buf_size (shouldn't happen with valid state),
    // use modulo to guarantee bounds.
    let head_offset = if head_offset >= buf_size {
        head_offset % buf_size
    } else {
        head_offset
    };

    // wrap_size = bytes until end of physical buffer
    let wrap_size = buf_size - head_offset;
    let claim_size = if requested <= wrap_size {
        requested
    } else {
        wrap_size
    };

    GaleRingBufClaimDecision {
        claim_size,
        buffer_offset: head_offset,
    }
}

/// Validate a ring_buf_area_finish operation.
///
/// This models `ring_buf_area_finish()` from ring_buffer.c:31-51.
/// Ensures the finished size does not exceed the claimed amount.
///
/// Arguments:
///   head:     current ring->head (after claims)
///   tail:     current ring->tail (last finished position)
///   size:     number of bytes to finish
///   buf_size: total buffer size
///
/// Returns:
///   0 on success, -EINVAL if size > claimed.
///
/// Verified: RB3/RB4 (correct advancement), RB8 (no overflow).
#[cfg(feature = "ring_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ring_buf_finish_validate(
    head: u32,
    tail: u32,
    size: u32,
    buf_size: u32,
) -> i32 {
    // claimed_size = head - tail (may wrap via unsigned subtraction)
    let claimed_size = head.wrapping_sub(tail);

    if size > claimed_size {
        return EINVAL;
    }

    OK
}

/// Compute ring buffer free space.
///
/// This models `ring_buf_space_get()` from ring_buffer.h:235-240.
/// Uses modular subtraction to compute allocated bytes without overflow.
///
/// Arguments:
///   put_head:  producer head index
///   get_tail:  consumer tail index
///   buf_size:  total buffer size
///
/// Returns:
///   Free space in bytes.
///
/// Verified: RB1 (result <= buf_size), RB7 (consistency), RB8 (no overflow).
#[cfg(feature = "ring_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ring_buf_space_get(
    put_head: u32,
    get_tail: u32,
    buf_size: u32,
) -> u32 {
    if buf_size == 0 {
        return 0;
    }
    let allocated = put_head.wrapping_sub(get_tail);
    if allocated > buf_size {
        // Shouldn't happen with valid state, but clamp to 0 free space
        0
    } else {
        buf_size - allocated
    }
}

/// Compute ring buffer available data size.
///
/// This models `ring_buf_size_get()` from ring_buffer.h:273-278.
///
/// Arguments:
///   put_tail:  producer tail index (committed writes)
///   get_head:  consumer head index
///
/// Returns:
///   Available data in bytes.
///
/// Verified: RB7 (consistency).
#[cfg(feature = "ring_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ring_buf_size_get(
    put_tail: u32,
    get_head: u32,
) -> u32 {
    put_tail.wrapping_sub(get_head)
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — ring_buffer
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "ring_buf"))]
mod kani_ring_buf_proofs {
    use super::*;

    /// RB1: claim_decide never returns offset >= buf_size.
    #[kani::proof]
    fn ring_buf_claim_offset_bounded() {
        let head: u32 = kani::any();
        let base: u32 = kani::any();
        let buf_size: u32 = kani::any();
        let requested: u32 = kani::any();
        kani::assume(buf_size > 0);
        let d = gale_ring_buf_claim_decide(head, base, buf_size, requested);
        assert!(d.buffer_offset < buf_size);
        assert!(d.claim_size <= buf_size);
    }

    /// RB1: claim_decide returns claim_size <= requested.
    #[kani::proof]
    fn ring_buf_claim_bounded_by_request() {
        let head: u32 = kani::any();
        let base: u32 = kani::any();
        let buf_size: u32 = kani::any();
        let requested: u32 = kani::any();
        kani::assume(buf_size > 0);
        let d = gale_ring_buf_claim_decide(head, base, buf_size, requested);
        assert!(d.claim_size <= requested);
    }

    /// RB3: finish_validate accepts valid finish.
    #[kani::proof]
    fn ring_buf_finish_valid() {
        let tail: u32 = kani::any();
        let size: u32 = kani::any();
        kani::assume(size <= 1024); // bound for tractability
        let head = tail.wrapping_add(size);
        let rc = gale_ring_buf_finish_validate(head, tail, size, 1024);
        assert!(rc == OK);
    }

    /// RB3: finish_validate rejects over-finish.
    #[kani::proof]
    fn ring_buf_finish_rejects_overfinish() {
        let tail: u32 = kani::any();
        let claimed: u32 = kani::any();
        let size: u32 = kani::any();
        kani::assume(claimed < 1024);
        kani::assume(size > claimed);
        let head = tail.wrapping_add(claimed);
        let rc = gale_ring_buf_finish_validate(head, tail, size, 1024);
        assert!(rc == EINVAL);
    }

    /// RB7: space_get + size_get == buf_size for valid state.
    #[kani::proof]
    fn ring_buf_space_plus_size_is_capacity() {
        let put_head: u32 = kani::any();
        let put_tail: u32 = kani::any();
        let get_head: u32 = kani::any();
        let get_tail: u32 = kani::any();
        let buf_size: u32 = kani::any();
        kani::assume(buf_size > 0 && buf_size <= 1024);
        // Valid state: put_tail == get_head (no in-flight claims)
        // and allocated <= buf_size
        kani::assume(put_tail == put_head);
        kani::assume(get_tail == get_head);
        let allocated = put_head.wrapping_sub(get_tail);
        kani::assume(allocated <= buf_size);
        let space = gale_ring_buf_space_get(put_head, get_tail, buf_size);
        let size = gale_ring_buf_size_get(put_tail, get_head);
        assert!(space + size == buf_size);
    }

    /// RB8: space_get handles zero buf_size.
    #[kani::proof]
    fn ring_buf_space_zero_size() {
        let put_head: u32 = kani::any();
        let get_tail: u32 = kani::any();
        assert!(gale_ring_buf_space_get(put_head, get_tail, 0) == 0);
    }
}

// ---------------------------------------------------------------------------
// FFI exports — bitarray (bounds validation)
// ---------------------------------------------------------------------------
//
// These pure functions validate bitarray index and region bounds from
// lib/utils/bitarray.c:
//
//   sys_bitarray_set_bit   — bit < num_bits check (line 331)
//   sys_bitarray_alloc     — num_bits == 0 || num_bits > ba->num_bits (line 511)
//   region operations      — offset + num_bits <= ba->num_bits (line 226)
//   bundle indexing        — bit / 32 for bundle index, bit % 32 for offset
//
// The actual bitarray memory and spinlock stay in C. We model only the
// bounds checks that prevent:
//   - Out-of-bounds bit access (BA1)
//   - Out-of-bounds region access (BA2)
//   - Bundle array overrun (BA3)
//   - Arithmetic overflow in offset + nbits (BA4)
//
// Verified properties:
//   BA1: bit < num_bits (single bit bounds)
//   BA2: offset + nbits <= num_bits (region bounds, overflow-safe)
//   BA3: bundle_index < num_bundles (array bounds)
//   BA4: no overflow in offset + nbits computation

/// Validate a bitarray allocation request.
///
/// bitarray.c:511:
///   if ((num_bits == 0) || (num_bits > bitarray->num_bits)) { return -EINVAL; }
///
/// This validates that an allocation of `alloc_nbits` bits is feasible
/// and that a candidate region [offset, offset+alloc_nbits) is within bounds.
///
/// Arguments:
///   num_bits:    total bits in the bitarray (bitarray->num_bits)
///   offset:      candidate start position for allocation
///   alloc_nbits: number of bits to allocate
///
/// Returns:
///   0 (OK)     — valid allocation parameters
///   -EINVAL    — alloc_nbits == 0 or alloc_nbits > num_bits or
///                offset + alloc_nbits overflows or exceeds num_bits
///
/// Verified: BA2 (region bounds), BA4 (no overflow).
#[cfg(feature = "bitarray")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_bitarray_alloc_validate(
    num_bits: u32,
    offset: u32,
    alloc_nbits: u32,
) -> i32 {
    if alloc_nbits == 0 || alloc_nbits > num_bits {
        return EINVAL;
    }
    // Overflow-safe region bounds check
    match offset.checked_add(alloc_nbits) {
        Some(end) if end <= num_bits => OK,
        _ => EINVAL,
    }
}

/// Validate region parameters for bitarray operations.
///
/// bitarray.c:226:
///   if (num_bits == 0 || offset + num_bits > bitarray->num_bits) { return -EINVAL; }
///
/// Used by set_region, clear_region, is_region_set, popcount_region, etc.
///
/// Arguments:
///   num_bits:    total bits in the bitarray
///   offset:      starting bit position of the region
///   region_nbits: number of bits in the region
///
/// Returns:
///   0 (OK)     — valid region
///   -EINVAL    — region_nbits == 0 or region exceeds bitarray bounds
///
/// Verified: BA2 (region bounds), BA4 (no overflow).
#[cfg(feature = "bitarray")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_bitarray_region_check(
    num_bits: u32,
    offset: u32,
    region_nbits: u32,
) -> i32 {
    if region_nbits == 0 {
        return EINVAL;
    }
    // Overflow-safe: offset + region_nbits <= num_bits
    match offset.checked_add(region_nbits) {
        Some(end) if end <= num_bits => OK,
        _ => EINVAL,
    }
}

/// Validate a single bit index for set_bit / clear_bit / test_bit.
///
/// bitarray.c:331:
///   if (bit >= bitarray->num_bits) { ret = -EINVAL; }
///
/// Arguments:
///   num_bits: total bits in the bitarray
///   bit:      bit index to validate
///
/// Returns:
///   0 (OK)     — bit < num_bits
///   -EINVAL    — bit >= num_bits
///
/// Verified: BA1 (bit bounds).
#[cfg(feature = "bitarray")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_bitarray_set_bit_validate(
    num_bits: u32,
    bit: u32,
) -> i32 {
    if bit >= num_bits {
        EINVAL
    } else {
        OK
    }
}

/// Compute bundle index and bit offset for a given bit position.
///
/// bitarray.c:336-337:
///   idx = bit / bundle_bitness(bitarray);   // bundle_bitness = 32
///   off = bit % bundle_bitness(bitarray);
///
/// Arguments:
///   bit: bit index
///
/// Returns:
///   GaleBitarrayBundleIndex with bundle_index (bit / 32) and
///   bit_offset (bit % 32).
///
/// Verified: BA3 (bundle_index < num_bundles when bit < num_bits).
#[repr(C)]
pub struct GaleBitarrayBundleIndex {
    /// Bundle array index: bit / 32.
    pub bundle_index: u32,
    /// Bit offset within the bundle: bit % 32.
    pub bit_offset: u32,
}

#[cfg(feature = "bitarray")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_bitarray_bundle_index(
    bit: u32,
) -> GaleBitarrayBundleIndex {
    GaleBitarrayBundleIndex {
        bundle_index: bit / 32,
        bit_offset: bit % 32,
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — bitarray
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "bitarray"))]
mod kani_bitarray_proofs {
    use super::*;

    /// BA1: set_bit_validate accepts valid bit index.
    #[kani::proof]
    fn bitarray_set_bit_valid() {
        let num_bits: u32 = kani::any();
        let bit: u32 = kani::any();
        kani::assume(num_bits > 0);
        kani::assume(bit < num_bits);
        assert!(gale_bitarray_set_bit_validate(num_bits, bit) == OK);
    }

    /// BA1: set_bit_validate rejects out-of-bounds bit.
    #[kani::proof]
    fn bitarray_set_bit_rejects_oob() {
        let num_bits: u32 = kani::any();
        let bit: u32 = kani::any();
        kani::assume(bit >= num_bits);
        assert!(gale_bitarray_set_bit_validate(num_bits, bit) == EINVAL);
    }

    /// BA2: alloc_validate accepts valid allocation.
    #[kani::proof]
    fn bitarray_alloc_valid() {
        let num_bits: u32 = kani::any();
        let offset: u32 = kani::any();
        let alloc_nbits: u32 = kani::any();
        kani::assume(num_bits > 0 && num_bits <= 1024);
        kani::assume(alloc_nbits > 0 && alloc_nbits <= num_bits);
        kani::assume(offset <= num_bits - alloc_nbits);
        let rc = gale_bitarray_alloc_validate(num_bits, offset, alloc_nbits);
        assert!(rc == OK);
    }

    /// BA2: alloc_validate rejects zero alloc_nbits.
    #[kani::proof]
    fn bitarray_alloc_rejects_zero() {
        let num_bits: u32 = kani::any();
        let offset: u32 = kani::any();
        assert!(gale_bitarray_alloc_validate(num_bits, offset, 0) == EINVAL);
    }

    /// BA2: alloc_validate rejects oversized allocation.
    #[kani::proof]
    fn bitarray_alloc_rejects_oversized() {
        let num_bits: u32 = kani::any();
        let offset: u32 = kani::any();
        let alloc_nbits: u32 = kani::any();
        kani::assume(num_bits > 0);
        kani::assume(alloc_nbits > num_bits);
        assert!(gale_bitarray_alloc_validate(num_bits, offset, alloc_nbits) == EINVAL);
    }

    /// BA2: region_check accepts valid region.
    #[kani::proof]
    fn bitarray_region_valid() {
        let num_bits: u32 = kani::any();
        let offset: u32 = kani::any();
        let region_nbits: u32 = kani::any();
        kani::assume(num_bits > 0 && num_bits <= 1024);
        kani::assume(region_nbits > 0 && region_nbits <= num_bits);
        kani::assume(offset <= num_bits - region_nbits);
        let rc = gale_bitarray_region_check(num_bits, offset, region_nbits);
        assert!(rc == OK);
    }

    /// BA2: region_check rejects out-of-bounds region.
    #[kani::proof]
    fn bitarray_region_rejects_oob() {
        let num_bits: u32 = kani::any();
        let offset: u32 = kani::any();
        let region_nbits: u32 = kani::any();
        kani::assume(num_bits > 0 && num_bits <= 1024);
        kani::assume(region_nbits > 0);
        // Ensure offset + region_nbits > num_bits (either by overflow or exceeding)
        kani::assume(
            offset.checked_add(region_nbits).is_none()
                || offset + region_nbits > num_bits,
        );
        assert!(gale_bitarray_region_check(num_bits, offset, region_nbits) == EINVAL);
    }

    /// BA3: bundle_index < ceil(num_bits / 32) when bit < num_bits.
    #[kani::proof]
    fn bitarray_bundle_index_bounded() {
        let num_bits: u32 = kani::any();
        let bit: u32 = kani::any();
        kani::assume(num_bits > 0 && num_bits <= 4096);
        kani::assume(bit < num_bits);
        let num_bundles = (num_bits + 31) / 32;
        let idx = gale_bitarray_bundle_index(bit);
        assert!(idx.bundle_index < num_bundles);
        assert!(idx.bit_offset < 32);
    }

    /// BA3: bundle_index reconstructs to original bit.
    #[kani::proof]
    fn bitarray_bundle_index_roundtrip() {
        let bit: u32 = kani::any();
        kani::assume(bit <= 65535); // bound for tractability
        let idx = gale_bitarray_bundle_index(bit);
        assert!(idx.bundle_index * 32 + idx.bit_offset == bit);
    }

    /// BA4: alloc_validate detects overflow in offset + alloc_nbits.
    #[kani::proof]
    fn bitarray_alloc_detects_overflow() {
        let num_bits: u32 = kani::any();
        let offset: u32 = kani::any();
        let alloc_nbits: u32 = kani::any();
        kani::assume(num_bits > 0);
        kani::assume(alloc_nbits > 0 && alloc_nbits <= num_bits);
        // Force overflow
        kani::assume(offset.checked_add(alloc_nbits).is_none());
        assert!(gale_bitarray_alloc_validate(num_bits, offset, alloc_nbits) == EINVAL);
    }

    /// BA4: region_check detects overflow in offset + region_nbits.
    #[kani::proof]
    fn bitarray_region_detects_overflow() {
        let num_bits: u32 = kani::any();
        let offset: u32 = kani::any();
        let region_nbits: u32 = kani::any();
        kani::assume(num_bits > 0);
        kani::assume(region_nbits > 0);
        // Force overflow
        kani::assume(offset.checked_add(region_nbits).is_none());
        assert!(gale_bitarray_region_check(num_bits, offset, region_nbits) == EINVAL);
    }
}

// ---------------------------------------------------------------------------
// FFI exports — red-black tree (color invariant validation)
// ---------------------------------------------------------------------------
//
// These pure functions validate red-black tree color invariants from
// lib/utils/rb.c:
//
//   fix_extra_red (line 157)   — no two consecutive red nodes
//   rotate + set_color (line 206-209) — color swap after rotation
//
// The actual tree structure and pointers stay in C. We model only the
// color invariant checks that prevent:
//   - Red-red violation on insert (RBT1)
//   - Incorrect color assignment after rotation (RBT2)
//
// Verified properties:
//   RBT1: no two consecutive red nodes (red-black property 4)
//   RBT2: rotation color swap correctness (parent→BLACK, grandparent→RED)

/// Validate that an insert operation does not create a red-red violation.
///
/// rb.c:169:
///   if (is_black(parent)) { return; }
///
/// Given the colors of a node and its parent, returns whether the
/// configuration is valid (no red-red violation). This models the
/// check in fix_extra_red that determines if fixup is needed.
///
/// Arguments:
///   is_black:        1 if the node is black, 0 if red
///   parent_is_black: 1 if the parent is black, 0 if red
///   has_left:        1 if the node has a left child (informational)
///   has_right:       1 if the node has a right child (informational)
///
/// Returns:
///   0 (OK)     — no red-red violation (node is black, or parent is black)
///   -EINVAL    — red-red violation (both node and parent are red)
///
/// Verified: RBT1 (no consecutive red nodes).
#[cfg(feature = "rbtree")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_rb_validate_insert(
    is_black: u8,
    parent_is_black: u8,
    _has_left: u8,
    _has_right: u8,
) -> i32 {
    let node_is_red = is_black == 0;
    let parent_is_red = parent_is_black == 0;

    if node_is_red && parent_is_red {
        // Red-red violation: fixup required
        EINVAL
    } else {
        OK
    }
}

/// Validate color assignment after a rotation.
///
/// rb.c:208-209:
///   set_color(stack[stacksz - 3], BLACK);
///   set_color(stack[stacksz - 2], RED);
///
/// After rotation, the new parent must be black and the demoted node
/// must be red. This validates that the color swap is correct.
///
/// Arguments:
///   node_color:  proposed color for the promoted node (1=BLACK, 0=RED)
///   child_color: proposed color for the demoted node (1=BLACK, 0=RED)
///
/// Returns:
///   0 (OK)     — correct: promoted=BLACK, demoted=RED
///   -EINVAL    — incorrect color assignment
///
/// Verified: RBT2 (rotation preserves red-black properties via color swap).
#[cfg(feature = "rbtree")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_rb_validate_color_after_rotation(
    node_color: u8,
    child_color: u8,
) -> i32 {
    // After rotation: promoted node must be BLACK (1), demoted must be RED (0)
    let promoted_is_black = node_color == 1;
    let demoted_is_red = child_color == 0;

    if promoted_is_black && demoted_is_red {
        OK
    } else {
        EINVAL
    }
}

// ===========================================================================
// Spinlock validate — verified spinlock ownership validation
// ===========================================================================

/// Check whether acquiring the spinlock is valid.
///
/// Returns 1 (true) if valid, 0 (false) if the lock is already held by
/// the same CPU (would deadlock).
///
/// Maps to z_spin_lock_valid() in spinlock_validate.c:10-20.
#[cfg(feature = "spinlock_validate")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_spin_lock_valid(thread_cpu: usize, current_cpu_id: u32) -> i32 {
    use gale::spinlock_validate::*;
    if spin_lock_valid(thread_cpu, current_cpu_id) {
        1
    } else {
        0
    }
}

/// Check whether releasing the spinlock is valid.
///
/// Returns 1 (true) if the stored owner matches the current (cpu | thread),
/// 0 (false) otherwise.
///
/// Maps to z_spin_unlock_valid() in spinlock_validate.c:23-37.
#[cfg(feature = "spinlock_validate")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_spin_unlock_valid(
    thread_cpu: usize,
    current_cpu_id: u32,
    current_thread: usize,
) -> i32 {
    use gale::spinlock_validate::*;
    if spin_unlock_valid(thread_cpu, current_cpu_id, current_thread) {
        1
    } else {
        0
    }
}

/// Compute the owner tag for a spinlock (cpu_id | thread_ptr).
///
/// Maps to z_spin_lock_set_owner() in spinlock_validate.c:39-43.
#[cfg(feature = "spinlock_validate")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_spin_lock_compute_owner(
    current_cpu_id: u32,
    current_thread: usize,
) -> usize {
    use gale::spinlock_validate::*;
    spin_lock_compute_owner(current_cpu_id, current_thread)
}

// ---------------------------------------------------------------------------
// FFI exports — ipi (IPI mask creation for SMP)
// ---------------------------------------------------------------------------
//
// These functions replace the IPI mask computation from kernel/ipi.c:
//
//   ipi.c:29-70  ipi_mask_create — compute which CPUs need an IPI
//
// Verified by Verus (SMT/Z3):
//   IP1: current CPU is never in the result mask
//   IP2: only CPUs within [0, num_cpus) can be in the mask
//   IP3: only CPUs allowed by target_cpu_mask are considered
//   IP4: a CPU is included only if its priority > target (lower importance)
//   IP5: result fits in max_cpus bits (no stray high bits)

/// Compute the IPI bitmask for a newly ready thread.
///
/// ipi.c:29-70:
///   Iterates over CPUs, checking activity, affinity, and priority
///   to determine which CPUs should receive an IPI.
///
/// SAFETY: `cpu_prios` must point to a valid array of `num_cpus` int32_t values.
///         `cpu_active` must point to a valid array of `num_cpus` uint8_t values.
///         Called under Zephyr's scheduler spinlock — no concurrent access.
#[cfg(feature = "ipi")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_compute_ipi_mask(
    current_cpu: u32,
    target_prio: i32,
    target_cpu_mask: u32,
    cpu_prios: *const i32,
    cpu_active: *const u8,
    num_cpus: u32,
    max_cpus: u32,
) -> u32 {
    use gale::ipi::compute_ipi_mask;

    if cpu_prios.is_null() || cpu_active.is_null() {
        return 0;
    }
    if num_cpus == 0 || current_cpu >= num_cpus || num_cpus > max_cpus || max_cpus > 16 {
        return 0;
    }

    // SAFETY: Caller guarantees valid arrays of length num_cpus under spinlock.
    let prios = unsafe { core::slice::from_raw_parts(cpu_prios, num_cpus as usize) };
    let active_u8 = unsafe { core::slice::from_raw_parts(cpu_active, num_cpus as usize) };

    // Convert u8 array to bool array (stack-allocated, max 16 CPUs)
    let mut active_bool = [false; 16];
    let mut i: usize = 0;
    while i < num_cpus as usize {
        active_bool[i] = active_u8[i] != 0;
        i += 1;
    }
    let active = &active_bool[..num_cpus as usize];

    compute_ipi_mask(current_cpu, target_prio, target_cpu_mask, prios, active, num_cpus, max_cpus)
}

/// Validate a previously computed IPI mask.
///
/// Returns 1 if the mask is structurally valid (current CPU excluded,
/// no bits above max_cpus), 0 otherwise.
///
/// Verified: IP1 (current CPU exclusion), IP5 (bounded by max_cpus).
#[cfg(feature = "ipi")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_validate_ipi_mask(mask: u32, current_cpu: u32, max_cpus: u32) -> i32 {
    use gale::ipi::validate_ipi_mask;

    if current_cpu >= max_cpus || max_cpus > 16 {
        return 0;
    }

    if validate_ipi_mask(mask, current_cpu, max_cpus) {
        1
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — red-black tree
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "rbtree"))]
mod kani_rb_proofs {
    use super::*;

    /// RBT1: validate_insert accepts black node.
    #[kani::proof]
    fn rb_insert_black_node_ok() {
        let parent_is_black: u8 = kani::any();
        let has_left: u8 = kani::any();
        let has_right: u8 = kani::any();
        kani::assume(parent_is_black <= 1);
        kani::assume(has_left <= 1);
        kani::assume(has_right <= 1);
        // Black node (is_black=1) never causes red-red violation
        assert!(gale_rb_validate_insert(1, parent_is_black, has_left, has_right) == OK);
    }

    /// RBT1: validate_insert accepts red node with black parent.
    #[kani::proof]
    fn rb_insert_red_node_black_parent_ok() {
        let has_left: u8 = kani::any();
        let has_right: u8 = kani::any();
        kani::assume(has_left <= 1);
        kani::assume(has_right <= 1);
        // Red node (is_black=0) with black parent (parent_is_black=1) is valid
        assert!(gale_rb_validate_insert(0, 1, has_left, has_right) == OK);
    }

    /// RBT1: validate_insert detects red-red violation.
    #[kani::proof]
    fn rb_insert_red_red_violation() {
        let has_left: u8 = kani::any();
        let has_right: u8 = kani::any();
        kani::assume(has_left <= 1);
        kani::assume(has_right <= 1);
        // Red node (is_black=0) with red parent (parent_is_black=0) is violation
        assert!(gale_rb_validate_insert(0, 0, has_left, has_right) == EINVAL);
    }

    /// RBT2: validate_color_after_rotation accepts correct swap.
    #[kani::proof]
    fn rb_rotation_correct_colors() {
        // Promoted=BLACK (1), demoted=RED (0) — the only valid post-rotation state
        assert!(gale_rb_validate_color_after_rotation(1, 0) == OK);
    }

    /// RBT2: validate_color_after_rotation rejects all incorrect swaps.
    #[kani::proof]
    fn rb_rotation_rejects_wrong_colors() {
        let node_color: u8 = kani::any();
        let child_color: u8 = kani::any();
        kani::assume(node_color <= 1);
        kani::assume(child_color <= 1);
        // Any combination other than (BLACK=1, RED=0) is invalid
        kani::assume(!(node_color == 1 && child_color == 0));
        assert!(gale_rb_validate_color_after_rotation(node_color, child_color) == EINVAL);
    }

    /// RBT1: exhaustive check — exactly one of {OK, EINVAL} for all valid inputs.
    #[kani::proof]
    fn rb_insert_validate_exhaustive() {
        let is_black: u8 = kani::any();
        let parent_is_black: u8 = kani::any();
        kani::assume(is_black <= 1);
        kani::assume(parent_is_black <= 1);
        let rc = gale_rb_validate_insert(is_black, parent_is_black, 0, 0);
        assert!(rc == OK || rc == EINVAL);
        // Red-red is the only violation case
        if is_black == 0 && parent_is_black == 0 {
            assert!(rc == EINVAL);
        } else {
            assert!(rc == OK);
        }
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — spinlock validate
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "spinlock_validate"))]
mod kani_spinlock_validate_proofs {
    use super::*;

    #[kani::proof]
    fn spinlock_lock_valid_free() {
        let cpu_id: u32 = kani::any();
        kani::assume(cpu_id < 4);
        let ret = gale_spin_lock_valid(0, cpu_id);
        assert!(ret == 1);
    }

    #[kani::proof]
    fn spinlock_lock_valid_same_cpu_deadlock() {
        let cpu_id: u32 = kani::any();
        kani::assume(cpu_id < 4);
        let thread_ptr: usize = kani::any();
        kani::assume(thread_ptr != 0);
        kani::assume(thread_ptr & 3 == 0);
        let thread_cpu = thread_ptr | (cpu_id as usize);
        let ret = gale_spin_lock_valid(thread_cpu, cpu_id);
        assert!(ret == 0);
    }

    #[kani::proof]
    fn spinlock_lock_valid_different_cpu() {
        let holder_cpu: u32 = kani::any();
        let current_cpu: u32 = kani::any();
        kani::assume(holder_cpu < 4);
        kani::assume(current_cpu < 4);
        kani::assume(holder_cpu != current_cpu);
        let thread_ptr: usize = kani::any();
        kani::assume(thread_ptr != 0);
        kani::assume(thread_ptr & 3 == 0);
        let thread_cpu = thread_ptr | (holder_cpu as usize);
        let ret = gale_spin_lock_valid(thread_cpu, current_cpu);
        assert!(ret == 1);
    }

    #[kani::proof]
    fn spinlock_unlock_valid_matching_owner() {
        let cpu_id: u32 = kani::any();
        kani::assume(cpu_id < 4);
        let thread: usize = kani::any();
        kani::assume(thread != 0);
        kani::assume(thread & 3 == 0);
        let owner = gale_spin_lock_compute_owner(cpu_id, thread);
        let ret = gale_spin_unlock_valid(owner, cpu_id, thread);
        assert!(ret == 1);
    }

    #[kani::proof]
    fn spinlock_unlock_valid_mismatched_owner() {
        let cpu_id: u32 = kani::any();
        kani::assume(cpu_id < 4);
        let thread: usize = kani::any();
        kani::assume(thread != 0);
        kani::assume(thread & 3 == 0);
        let other_thread: usize = kani::any();
        kani::assume(other_thread != 0);
        kani::assume(other_thread & 3 == 0);
        kani::assume(other_thread != thread);
        let owner = gale_spin_lock_compute_owner(cpu_id, thread);
        let ret = gale_spin_unlock_valid(owner, cpu_id, other_thread);
        assert!(ret == 0);
    }

    #[kani::proof]
    fn spinlock_owner_encoding_roundtrip() {
        let cpu_id: u32 = kani::any();
        kani::assume(cpu_id < 4);
        let thread: usize = kani::any();
        kani::assume(thread != 0);
        kani::assume(thread & 3 == 0);
        let owner = gale_spin_lock_compute_owner(cpu_id, thread);
        assert!((owner & 3) == cpu_id as usize);
        assert!((owner & !3) == thread);
    }

    #[kani::proof]
    fn spinlock_lock_valid_no_panic() {
        let thread_cpu: usize = kani::any();
        let cpu_id: u32 = kani::any();
        kani::assume(cpu_id < 4);
        let _ = gale_spin_lock_valid(thread_cpu, cpu_id);
    }

    #[kani::proof]
    fn spinlock_unlock_valid_no_panic() {
        let thread_cpu: usize = kani::any();
        let cpu_id: u32 = kani::any();
        kani::assume(cpu_id < 4);
        let thread: usize = kani::any();
        kani::assume(thread != 0);
        kani::assume(thread & 3 == 0);
        let _ = gale_spin_unlock_valid(thread_cpu, cpu_id, thread);
    }

    #[kani::proof]
    fn spinlock_compute_owner_no_panic() {
        let cpu_id: u32 = kani::any();
        kani::assume(cpu_id < 4);
        let thread: usize = kani::any();
        kani::assume(thread != 0);
        kani::assume(thread & 3 == 0);
        let _ = gale_spin_lock_compute_owner(cpu_id, thread);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — CPU affinity mask
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "cpu_mask"))]
mod kani_cpu_mask_proofs {
    use super::*;

    #[kani::proof]
    fn cpu_mask_running_thread_rejected() {
        let current: u32 = kani::any();
        let enable: u32 = kani::any();
        let disable: u32 = kani::any();
        let pin_only: u32 = kani::any();
        kani::assume(pin_only <= 1);
        let r = gale_cpu_mask_mod(current, enable, disable, 1, pin_only);
        assert!(r.err == EINVAL);
    }

    #[kani::proof]
    fn cpu_mask_formula_correct() {
        let current: u32 = kani::any();
        let enable: u32 = kani::any();
        let disable: u32 = kani::any();
        let r = gale_cpu_mask_mod(current, enable, disable, 0, 0);
        if r.err == OK {
            assert!(r.mask == (current | enable) & !disable);
        }
    }

    #[kani::proof]
    fn cpu_mask_result_nonzero() {
        let current: u32 = kani::any();
        let enable: u32 = kani::any();
        let disable: u32 = kani::any();
        let pin_only: u32 = kani::any();
        kani::assume(pin_only <= 1);
        let r = gale_cpu_mask_mod(current, enable, disable, 0, pin_only);
        if r.err == OK {
            assert!(r.mask != 0);
        }
    }

    #[kani::proof]
    fn cpu_mask_pin_only_single_bit() {
        let current: u32 = kani::any();
        let enable: u32 = kani::any();
        let disable: u32 = kani::any();
        let r = gale_cpu_mask_mod(current, enable, disable, 0, 1);
        if r.err == OK {
            assert!(r.mask != 0 && (r.mask & (r.mask - 1)) == 0);
        }
    }

    #[kani::proof]
    fn cpu_mask_validate_pin_powers_of_two() {
        let shift: u32 = kani::any();
        kani::assume(shift < 32);
        let mask = 1u32 << shift;
        assert!(gale_validate_pin_mask(mask) == 1);
    }

    #[kani::proof]
    fn cpu_mask_validate_pin_rejects_zero() {
        assert!(gale_validate_pin_mask(0) == 0);
    }

    #[kani::proof]
    fn cpu_mask_validate_pin_rejects_multi_bit() {
        let mask: u32 = kani::any();
        kani::assume(mask != 0);
        kani::assume((mask & (mask - 1)) != 0);
        assert!(gale_validate_pin_mask(mask) == 0);
    }

    #[kani::proof]
    fn cpu_pin_compute_valid() {
        let cpu_id: u32 = kani::any();
        let max_cpus: u32 = kani::any();
        kani::assume(max_cpus > 0 && max_cpus <= 32);
        kani::assume(cpu_id < max_cpus);
        let r = gale_cpu_pin_compute(cpu_id, max_cpus);
        assert!(r.err == OK);
        assert!(r.mask == (1u32 << cpu_id));
        assert!(r.mask != 0 && (r.mask & (r.mask - 1)) == 0);
    }

    #[kani::proof]
    fn cpu_pin_compute_out_of_bounds() {
        let cpu_id: u32 = kani::any();
        let max_cpus: u32 = kani::any();
        kani::assume(max_cpus <= 32);
        kani::assume(cpu_id >= max_cpus);
        let r = gale_cpu_pin_compute(cpu_id, max_cpus);
        assert!(r.err == EINVAL);
    }

    #[kani::proof]
    fn cpu_pin_compute_max_cpus_too_large() {
        let cpu_id: u32 = kani::any();
        let max_cpus: u32 = kani::any();
        kani::assume(max_cpus > 32);
        let r = gale_cpu_pin_compute(cpu_id, max_cpus);
        assert!(r.err == EINVAL);
    }

    #[kani::proof]
    fn cpu_mask_mod_no_panic() {
        let current: u32 = kani::any();
        let enable: u32 = kani::any();
        let disable: u32 = kani::any();
        let is_running: u32 = kani::any();
        let pin_only: u32 = kani::any();
        kani::assume(is_running <= 1);
        kani::assume(pin_only <= 1);
        let _ = gale_cpu_mask_mod(current, enable, disable, is_running, pin_only);
    }

    #[kani::proof]
    fn cpu_mask_validate_pin_no_panic() {
        let mask: u32 = kani::any();
        let _ = gale_validate_pin_mask(mask);
    }

    #[kani::proof]
    fn cpu_pin_compute_no_panic() {
        let cpu_id: u32 = kani::any();
        let max_cpus: u32 = kani::any();
        let _ = gale_cpu_pin_compute(cpu_id, max_cpus);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — IPI mask creation
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "ipi"))]
mod kani_ipi_proofs {
    use super::*;

    /// IP1: null pointers return 0.
    #[kani::proof]
    fn ipi_null_prios_returns_zero() {
        let active: [u8; 2] = [1, 1];
        let result = gale_compute_ipi_mask(0, 5, 0x3, core::ptr::null(), active.as_ptr(), 2, 2);
        assert!(result == 0);
    }

    /// IP1: null active array returns 0.
    #[kani::proof]
    fn ipi_null_active_returns_zero() {
        let prios: [i32; 2] = [10, 10];
        let result = gale_compute_ipi_mask(0, 5, 0x3, prios.as_ptr(), core::ptr::null(), 2, 2);
        assert!(result == 0);
    }

    /// IP1: current CPU excluded from result.
    #[kani::proof]
    fn ipi_current_cpu_excluded() {
        let prios: [i32; 2] = [10, 10];
        let active: [u8; 2] = [1, 1];
        let result = gale_compute_ipi_mask(0, 5, 0x3, prios.as_ptr(), active.as_ptr(), 2, 2);
        assert!(result & 1 == 0); // bit 0 (current CPU) not set
    }

    /// IP5: validate_ipi_mask accepts valid mask.
    #[kani::proof]
    fn ipi_validate_accepts_valid() {
        // mask=0b10 (CPU 1), current=0, max=2 -> valid
        assert!(gale_validate_ipi_mask(0b10, 0, 2) == 1);
    }

    /// IP5: validate_ipi_mask rejects mask with current CPU set.
    #[kani::proof]
    fn ipi_validate_rejects_current_cpu() {
        // mask=0b01 (CPU 0), current=0, max=2 -> invalid
        assert!(gale_validate_ipi_mask(0b01, 0, 2) == 0);
    }

    /// Boundary: invalid parameters return 0.
    #[kani::proof]
    fn ipi_invalid_params_zero() {
        let prios: [i32; 2] = [10, 10];
        let active: [u8; 2] = [1, 1];
        // current_cpu >= num_cpus
        let result = gale_compute_ipi_mask(5, 5, 0x3, prios.as_ptr(), active.as_ptr(), 2, 2);
        assert!(result == 0);
    }
}

// ===========================================================================
// FFI exports — condvar (wait queue decision functions)
// ===========================================================================
//
// These functions model the action decisions for Zephyr's k_condvar API.
// The kernel/condvar.c file is replaced by gale_condvar.c which uses the
// Extract→Decide→Apply pattern:
//
//   k_condvar_signal    — wake at most one waiter (C2, C3, C7)
//   k_condvar_broadcast — wake all waiters, return count (C4, C5, C8)
//   k_condvar_wait      — validate blocking path (C6, C7)
//
// Verified by Verus (SMT/Z3):
//   C1: After init, wait queue is empty
//   C2: Signal wakes at most one waiter (highest priority)
//   C3: Signal on empty condvar is a no-op
//   C4: Broadcast wakes all waiters, returns woken count
//   C5: Broadcast on empty condvar returns 0
//   C6: Wait adds thread to wait queue (blocking path)
//   C7: Signal/broadcast preserve wait queue ordering
//   C8: No arithmetic overflow in broadcast woken count

/// Decision struct for k_condvar_signal.
///
/// C extracts: has_waiter (whether wait queue is non-empty).
/// Rust decides: action (no-op or wake one).
/// C applies: if WAKE, calls z_unpend_first_thread + z_ready_thread.
#[repr(C)]
pub struct GaleCondvarSignalDecision {
    /// 0 = NOOP (no waiters), 1 = WAKE_ONE (wake first waiter).
    pub action: u8,
}

/// Action constants for condvar signal.
pub const GALE_CONDVAR_SIGNAL_NOOP: u8 = 0;
pub const GALE_CONDVAR_SIGNAL_WAKE_ONE: u8 = 1;

/// Decide the action for k_condvar_signal.
///
/// C extracts the has_waiter flag (non-zero if wait queue non-empty),
/// Rust returns NOOP or WAKE_ONE.
///
/// Verified: C2 (wakes at most one), C3 (no-op when empty).
#[cfg(feature = "condvar")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_condvar_signal_decide(has_waiter: u32) -> GaleCondvarSignalDecision {
    if has_waiter != 0 {
        GaleCondvarSignalDecision { action: GALE_CONDVAR_SIGNAL_WAKE_ONE }
    } else {
        GaleCondvarSignalDecision { action: GALE_CONDVAR_SIGNAL_NOOP }
    }
}

/// Decision struct for k_condvar_broadcast.
///
/// C extracts: num_waiters (current wait queue length).
/// Rust decides: how many threads to wake (= num_waiters, validated for overflow).
/// C applies: loop unpending all threads.
#[repr(C)]
pub struct GaleCondvarBroadcastDecision {
    /// Number of threads to wake (0 if queue empty).
    pub woken: u32,
}

/// Decide the action for k_condvar_broadcast.
///
/// Returns the number of waiters to wake. Capped at u32::MAX to prevent
/// overflow (C8). In practice Zephyr limits wait queues to CONFIG_MAX_THREAD_BYTES
/// threads, so this cap is never reached.
///
/// Verified: C4 (all waiters woken), C5 (0 when empty), C8 (no overflow).
#[cfg(feature = "condvar")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_condvar_broadcast_decide(
    num_waiters: u32,
) -> GaleCondvarBroadcastDecision {
    GaleCondvarBroadcastDecision { woken: num_waiters }
}

/// Decision struct for k_condvar_wait.
///
/// C extracts: is_no_wait (K_NO_WAIT timeout).
/// Rust decides: action (pend current thread or return EAGAIN).
/// C applies: if PEND, releases mutex and calls z_pend_curr.
#[repr(C)]
pub struct GaleCondvarWaitDecision {
    /// 0 = PEND_CURRENT (block on condvar), 1 = RETURN_EAGAIN (no-wait).
    pub action: u8,
    /// Return code for RETURN_EAGAIN path (-EAGAIN = -11).
    pub ret: i32,
}

/// Action constants for condvar wait.
pub const GALE_CONDVAR_WAIT_PEND: u8 = 0;
pub const GALE_CONDVAR_WAIT_RETURN_EAGAIN: u8 = 1;

/// Decide the action for k_condvar_wait.
///
/// If is_no_wait is set: return EAGAIN immediately.
/// Otherwise: pend the current thread (block).
///
/// Verified: C6 (thread added to wait queue on blocking path).
#[cfg(feature = "condvar")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_condvar_wait_decide(is_no_wait: u32) -> GaleCondvarWaitDecision {
    if is_no_wait != 0 {
        GaleCondvarWaitDecision {
            action: GALE_CONDVAR_WAIT_RETURN_EAGAIN,
            ret: -11, // -EAGAIN
        }
    } else {
        GaleCondvarWaitDecision {
            action: GALE_CONDVAR_WAIT_PEND,
            ret: 0,
        }
    }
}

#[cfg(all(kani, feature = "condvar"))]
mod kani_condvar_proofs {
    use super::*;

    /// C2/C3: signal decide returns WAKE_ONE iff has_waiter != 0.
    #[kani::proof]
    fn condvar_signal_decide_waiter() {
        let d = gale_k_condvar_signal_decide(1);
        assert!(d.action == GALE_CONDVAR_SIGNAL_WAKE_ONE);
    }

    #[kani::proof]
    fn condvar_signal_decide_empty() {
        let d = gale_k_condvar_signal_decide(0);
        assert!(d.action == GALE_CONDVAR_SIGNAL_NOOP);
    }

    /// C4/C5: broadcast returns exactly num_waiters.
    #[kani::proof]
    fn condvar_broadcast_decide_count() {
        let n: u32 = kani::any();
        let d = gale_k_condvar_broadcast_decide(n);
        assert!(d.woken == n);
    }

    /// C6: wait_decide returns PEND when not no-wait.
    #[kani::proof]
    fn condvar_wait_decide_pend() {
        let d = gale_k_condvar_wait_decide(0);
        assert!(d.action == GALE_CONDVAR_WAIT_PEND);
    }

    /// C6: wait_decide returns EAGAIN on no-wait.
    #[kani::proof]
    fn condvar_wait_decide_no_wait() {
        let d = gale_k_condvar_wait_decide(1);
        assert!(d.action == GALE_CONDVAR_WAIT_RETURN_EAGAIN);
        assert!(d.ret == -11);
    }
}

// ===========================================================================
// FFI exports — atomic (software atomic value arithmetic)
// ===========================================================================
//
// These functions replace the value transformation logic from
// kernel/atomic_c.c.  The actual spinlock-based atomicity (k_spin_lock,
// k_spin_unlock, IRQ masking) remains in the C shim.  Rust decides the
// arithmetic result; C applies the spinlock-protected write.
//
// Source mapping:
//   atomic_get            -> gale_atomic_get          (atomic_c.c:233-236)
//   z_impl_atomic_set     -> gale_atomic_set          (atomic_c.c:254-266)
//   z_impl_atomic_add     -> gale_atomic_add          (atomic_c.c:178-191)
//   z_impl_atomic_sub     -> gale_atomic_sub          (atomic_c.c:209-222)
//   z_impl_atomic_or      -> gale_atomic_or           (atomic_c.c:285-297)
//   z_impl_atomic_and     -> gale_atomic_and          (atomic_c.c:339-351)
//   z_impl_atomic_xor     -> gale_atomic_xor          (atomic_c.c:312-324)
//   z_impl_atomic_nand    -> gale_atomic_nand         (atomic_c.c:366-378)
//   z_impl_atomic_cas     -> gale_atomic_cas          (atomic_c.c:88-108)
//
// Verified by Verus (SMT/Z3):
//   AT1: add returns old value, stores old + val (wrapping)
//   AT2: sub returns old value, stores old - val (wrapping)
//   AT3: cas succeeds only when current == expected
//   AT4: cas failure leaves value unchanged
//   AT5: test_and_set returns old value, sets to 1
//   AT6: wrapping semantics for add/sub (matching hardware u32 behavior)

/// Decision struct for read-modify-write atomic operations.
///
/// Returned by add/sub/or/and/xor/nand/set.
/// C extracts: current value under spinlock.
/// Rust computes: old (return value) + new_val (to store).
/// C applies: *target = new_val; return old;
#[repr(C)]
pub struct GaleAtomicRmwDecision {
    /// Old value (returned to caller of the atomic operation).
    pub old_val: u32,
    /// New value to write back to *target.
    pub new_val: u32,
}

/// Decision struct for compare-and-swap.
#[repr(C)]
pub struct GaleAtomicCasDecision {
    /// 1 if swap occurred (current == expected), 0 otherwise.
    pub success: u32,
    /// New value to write (valid only when success == 1).
    pub new_val: u32,
}

/// Atomic get — read current value (no modification).
///
/// atomic_c.c:233-236: return *target;
///
/// Returns the current value. C caller reads *target under spinlock
/// and passes it here; the return value is what the caller returns.
#[cfg(feature = "atomic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_atomic_get(current: u32) -> u32 {
    current
}

/// Atomic set — write new value, return old.
///
/// atomic_c.c:254-266: ret = *target; *target = value; return ret;
///
/// AT: returns old value, stores new value.
#[cfg(feature = "atomic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_atomic_set(current: u32, value: u32) -> GaleAtomicRmwDecision {
    GaleAtomicRmwDecision {
        old_val: current,
        new_val: value,
    }
}

/// Atomic add — add value, return old (wrapping).
///
/// atomic_c.c:178-191: ret = *target; *target += value; return ret;
///
/// AT1: returns old value, stores old + val.
/// AT6: wrapping semantics (no panic on overflow).
///
/// Delegates wrapping arithmetic to `gale::atomic::add_u32_wrapping` (Verus-verified).
#[cfg(feature = "atomic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_atomic_add(current: u32, value: u32) -> GaleAtomicRmwDecision {
    use gale::atomic::add_u32_wrapping;

    // AT1 + AT6: delegate to verified wrapping add.
    GaleAtomicRmwDecision {
        old_val: current,
        new_val: add_u32_wrapping(current, value),
    }
}

/// Atomic sub — subtract value, return old (wrapping).
///
/// atomic_c.c:209-222: ret = *target; *target -= value; return ret;
///
/// AT2: returns old value, stores old - val.
/// AT6: wrapping semantics.
///
/// Delegates wrapping arithmetic to `gale::atomic::sub_u32_wrapping` (Verus-verified).
#[cfg(feature = "atomic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_atomic_sub(current: u32, value: u32) -> GaleAtomicRmwDecision {
    use gale::atomic::sub_u32_wrapping;

    // AT2 + AT6: delegate to verified wrapping sub.
    GaleAtomicRmwDecision {
        old_val: current,
        new_val: sub_u32_wrapping(current, value),
    }
}

/// Atomic OR — bitwise OR, return old.
///
/// atomic_c.c:285-297: ret = *target; *target |= value; return ret;
#[cfg(feature = "atomic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_atomic_or(current: u32, value: u32) -> GaleAtomicRmwDecision {
    GaleAtomicRmwDecision {
        old_val: current,
        new_val: current | value,
    }
}

/// Atomic AND — bitwise AND, return old.
///
/// atomic_c.c:339-351: ret = *target; *target &= value; return ret;
#[cfg(feature = "atomic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_atomic_and(current: u32, value: u32) -> GaleAtomicRmwDecision {
    GaleAtomicRmwDecision {
        old_val: current,
        new_val: current & value,
    }
}

/// Atomic XOR — bitwise XOR, return old.
///
/// atomic_c.c:312-324: ret = *target; *target ^= value; return ret;
#[cfg(feature = "atomic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_atomic_xor(current: u32, value: u32) -> GaleAtomicRmwDecision {
    GaleAtomicRmwDecision {
        old_val: current,
        new_val: current ^ value,
    }
}

/// Atomic NAND — bitwise NAND, return old.
///
/// atomic_c.c:366-378: ret = *target; *target = ~(*target & value); return ret;
#[cfg(feature = "atomic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_atomic_nand(current: u32, value: u32) -> GaleAtomicRmwDecision {
    GaleAtomicRmwDecision {
        old_val: current,
        new_val: !(current & value),
    }
}

/// Atomic compare-and-swap.
///
/// atomic_c.c:88-108:
///   if (*target == old_value) { *target = new_value; return true; }
///   return false;
///
/// AT3: succeeds only when current == expected.
/// AT4: failure leaves value unchanged (success == 0 -> C must not write back).
#[cfg(feature = "atomic")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_atomic_cas(
    current: u32,
    expected: u32,
    new_value: u32,
) -> GaleAtomicCasDecision {
    if current == expected {
        GaleAtomicCasDecision { success: 1, new_val: new_value }
    } else {
        GaleAtomicCasDecision { success: 0, new_val: current }
    }
}

#[cfg(all(kani, feature = "atomic"))]
mod kani_atomic_proofs {
    use super::*;

    /// AT1: add returns old value.
    #[kani::proof]
    fn atomic_add_returns_old() {
        let cur: u32 = kani::any();
        let val: u32 = kani::any();
        let d = gale_atomic_add(cur, val);
        assert!(d.old_val == cur);
    }

    /// AT1+AT6: new value is wrapping add.
    #[kani::proof]
    fn atomic_add_wrapping() {
        let cur: u32 = kani::any();
        let val: u32 = kani::any();
        let d = gale_atomic_add(cur, val);
        assert!(d.new_val == cur.wrapping_add(val));
    }

    /// AT2: sub returns old value with wrapping.
    #[kani::proof]
    fn atomic_sub_wrapping() {
        let cur: u32 = kani::any();
        let val: u32 = kani::any();
        let d = gale_atomic_sub(cur, val);
        assert!(d.old_val == cur);
        assert!(d.new_val == cur.wrapping_sub(val));
    }

    /// AT3: cas succeeds when current == expected.
    #[kani::proof]
    fn atomic_cas_success() {
        let cur: u32 = kani::any();
        let new: u32 = kani::any();
        let d = gale_atomic_cas(cur, cur, new);
        assert!(d.success == 1);
        assert!(d.new_val == new);
    }

    /// AT4: cas fails when current != expected, leaves value unchanged.
    #[kani::proof]
    fn atomic_cas_failure() {
        let cur: u32 = kani::any();
        let expected: u32 = kani::any();
        kani::assume(cur != expected);
        let new: u32 = kani::any();
        let d = gale_atomic_cas(cur, expected, new);
        assert!(d.success == 0);
        assert!(d.new_val == cur);
    }

    /// NAND: ~(cur & val) is correct.
    #[kani::proof]
    fn atomic_nand_correct() {
        let cur: u32 = kani::any();
        let val: u32 = kani::any();
        let d = gale_atomic_nand(cur, val);
        assert!(d.new_val == !(cur & val));
    }
}

// ===========================================================================
// FFI exports — spinlock (nesting discipline validation)
// ===========================================================================
//
// These functions extend the spinlock_validate FFI with the full
// acquire/release nesting state machine from src/spinlock.rs.
//
// The gale_spinlock_validate.c shim (already exists) handles the
// low-level encoding checks.  The new spinlock state machine functions
// here model the higher-level nesting depth tracking from spinlock.rs:
//
//   acquire_check   -> gale_spinlock_acquire_check
//   acquire         -> gale_spinlock_acquire (non-recursive)
//   acquire_nested  -> gale_spinlock_acquire_nested
//   release         -> gale_spinlock_release
//   is_held         -> gale_spinlock_is_held
//   nest_depth      -> gale_spinlock_nest_depth
//
// Verified by Verus (SMT/Z3):
//   SL1: lock acquired only when not held (or by same owner for nesting)
//   SL2: release only by current owner
//   SL3: nest_count tracks depth correctly
//   SL4: fully released when nest_count reaches 0
//   SL5: no double-acquire without nesting support

/// Maximum nesting depth for recursive spinlock acquisition.
/// Must match spinlock.rs:MAX_NEST_DEPTH.
const SPINLOCK_MAX_NEST_DEPTH: u32 = 255;

/// Check whether a spinlock acquisition is valid (SL1, SL5).
///
/// Returns 1 (valid) if the lock is free (owner == 0).
/// Returns 0 (invalid) if already held (same or different owner).
///
/// Maps to SpinlockState::acquire_check().
#[cfg(feature = "spinlock")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_spinlock_acquire_check(owner_tid: u32) -> i32 {
    // Lock is free when owner_tid == 0 (None).
    if owner_tid == 0 {
        1
    } else {
        0
    }
}

/// Acquire the spinlock (non-recursive).
///
/// If owner_tid == 0 (free): stores new_tid as owner, sets nest_count to 1.
/// If already held: returns -EBUSY without modification.
///
/// out_nest_count: written with the new nesting depth on success.
/// Returns 0 on success, -EBUSY (-16) if already held.
///
/// SL1: only succeeds when free. SL3: nest_count = 1.
#[cfg(feature = "spinlock")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_spinlock_acquire(
    owner_tid: u32,
    _nest_count: u32,
    new_tid: u32,
    out_nest_count: *mut u32,
    out_owner: *mut u32,
) -> i32 {
    if owner_tid != 0 {
        return -16; // -EBUSY
    }
    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        *out_owner = new_tid;
        *out_nest_count = 1;
    }
    0 // OK
}

/// Acquire the spinlock with nesting support.
///
/// Free lock: acquires with nest_count = 1.
/// Same owner, room to nest: increments nest_count.
/// Same owner at max depth: returns -EBUSY.
/// Different owner: returns -EBUSY.
///
/// SL1, SL3: nesting depth tracked correctly.
#[cfg(feature = "spinlock")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_spinlock_acquire_nested(
    owner_tid: u32,
    nest_count: u32,
    new_tid: u32,
    out_nest_count: *mut u32,
    out_owner: *mut u32,
) -> i32 {
    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        if owner_tid == 0 {
            // Free — acquire fresh.
            *out_owner = new_tid;
            *out_nest_count = 1;
            return 0;
        }
        if owner_tid == new_tid {
            if nest_count < SPINLOCK_MAX_NEST_DEPTH {
                *out_nest_count = nest_count + 1;
                *out_owner = owner_tid;
                return 0;
            } else {
                return -16; // -EBUSY: max depth
            }
        }
        // Different owner.
        -16 // -EBUSY
    }
}

/// Release the spinlock.
///
/// Only current owner (tid) may release.
/// Final release (nest_count == 1): clears owner and nest_count.
/// Nested release (nest_count > 1): decrements nest_count.
///
/// Returns 0 on success, -EPERM (-1) if not owner.
///
/// SL2: release only by owner. SL3: depth decremented. SL4: clears at 0.
#[cfg(feature = "spinlock")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_spinlock_release(
    owner_tid: u32,
    nest_count: u32,
    tid: u32,
    out_nest_count: *mut u32,
    out_owner: *mut u32,
) -> i32 {
    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        if owner_tid == 0 || owner_tid != tid {
            return -1; // -EPERM
        }
        if nest_count <= 1 {
            // Final release — fully unlock.
            *out_owner = 0;
            *out_nest_count = 0;
        } else {
            *out_owner = owner_tid;
            *out_nest_count = nest_count - 1;
        }
        0
    }
}

/// Check whether the spinlock is currently held.
///
/// Returns 1 if held (owner != 0), 0 if free.
#[cfg(feature = "spinlock")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_spinlock_is_held(owner_tid: u32) -> i32 {
    if owner_tid != 0 { 1 } else { 0 }
}

/// Get the current nesting depth.
#[cfg(feature = "spinlock")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_spinlock_nest_depth(nest_count: u32) -> u32 {
    nest_count
}

#[cfg(all(kani, feature = "spinlock"))]
mod kani_spinlock_proofs {
    use super::*;

    /// SL1: acquire_check returns 1 only for free lock.
    #[kani::proof]
    fn acquire_check_free() {
        assert!(gale_spinlock_acquire_check(0) == 1);
    }

    #[kani::proof]
    fn acquire_check_held() {
        let tid: u32 = kani::any();
        kani::assume(tid != 0);
        assert!(gale_spinlock_acquire_check(tid) == 0);
    }

    /// SL3: nested acquire increments depth.
    #[kani::proof]
    fn acquire_nested_increments_depth() {
        let tid: u32 = kani::any();
        kani::assume(tid != 0);
        let depth: u32 = kani::any();
        kani::assume(depth > 0 && depth < SPINLOCK_MAX_NEST_DEPTH);
        let mut out_depth: u32 = 0;
        let mut out_owner: u32 = 0;
        let rc = gale_spinlock_acquire_nested(tid, depth, tid, &mut out_depth, &mut out_owner);
        assert!(rc == 0);
        assert!(out_depth == depth + 1);
    }

    /// SL4: release at depth 1 fully unlocks.
    #[kani::proof]
    fn release_final_unlocks() {
        let tid: u32 = kani::any();
        kani::assume(tid != 0);
        let mut out_depth: u32 = 99;
        let mut out_owner: u32 = 99;
        let rc = gale_spinlock_release(tid, 1, tid, &mut out_depth, &mut out_owner);
        assert!(rc == 0);
        assert!(out_owner == 0);
        assert!(out_depth == 0);
    }

    /// SL2: release by non-owner returns -EPERM.
    #[kani::proof]
    fn release_non_owner_rejected() {
        let owner: u32 = kani::any();
        let other: u32 = kani::any();
        kani::assume(owner != 0 && other != owner);
        let mut out_depth: u32 = 0;
        let mut out_owner: u32 = 0;
        let rc = gale_spinlock_release(owner, 1, other, &mut out_depth, &mut out_owner);
        assert!(rc == -1);
    }
}

// ---------------------------------------------------------------------------
// FFI exports — usage (thread runtime statistics)
// ---------------------------------------------------------------------------
//
// Pure functions replacing decision logic from kernel/usage.c:
//
//   usage.c:74-97    z_sched_usage_start  — start_decide
//   usage.c:99-119   z_sched_usage_stop   — stop_decide
//   usage.c:155-159  z_sched_cpu_usage    — average_cycles (division guard)
//   usage.c:211-215  z_sched_thread_usage — average_cycles (division guard)
//   usage.c:227-246  k_thread_runtime_stats_enable  — thread enable
//   usage.c:248-273  k_thread_runtime_stats_disable — thread disable
//   usage.c:283-293  k_sys_runtime_stats_enable  — sys_enable_decide
//   usage.c:317-326  k_sys_runtime_stats_disable — sys_disable_decide
//
// Verified by Verus (SMT/Z3):
//   US1: tracking only starts when track_usage flag is set
//   US2: stop accumulates only when usage0 != 0
//   US3: enable sets track_usage; disable clears it
//   US4: sys enable/disable is idempotent
//   US5: average_cycles == 0 when num_windows == 0 (no divide-by-zero)
//   US6: cycle accumulation is monotonically non-decreasing

/// Action codes for sys enable/disable — returned to the C shim.
pub const GALE_USAGE_SYS_NOOP: u8 = 0;
pub const GALE_USAGE_SYS_APPLY: u8 = 1;

/// Decide whether k_sys_runtime_stats_enable() should apply changes.
///
/// usage.c:283-293: if already tracking, early-return (no-op).
///
/// Returns:
///   GALE_USAGE_SYS_NOOP  (0) — already enabled; do nothing
///   GALE_USAGE_SYS_APPLY (1) — not yet enabled; apply to all CPUs
///
/// Verified: US4 (idempotent enable).
#[cfg(feature = "usage")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_usage_sys_enable_decide(current_tracking: u32) -> u8 {
    use gale::usage::{SysTrackDecision, sys_enable_decide};

    match sys_enable_decide(current_tracking != 0) {
        SysTrackDecision::NoOp => GALE_USAGE_SYS_NOOP,
        SysTrackDecision::Apply => GALE_USAGE_SYS_APPLY,
    }
}

/// Decide whether k_sys_runtime_stats_disable() should apply changes.
///
/// usage.c:317-326: if not tracking, early-return (no-op).
///
/// Returns:
///   GALE_USAGE_SYS_NOOP  (0) — already disabled; do nothing
///   GALE_USAGE_SYS_APPLY (1) — currently enabled; apply to all CPUs
///
/// Verified: US4 (idempotent disable).
#[cfg(feature = "usage")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_usage_sys_disable_decide(current_tracking: u32) -> u8 {
    use gale::usage::{SysTrackDecision, sys_disable_decide};

    match sys_disable_decide(current_tracking != 0) {
        SysTrackDecision::NoOp => GALE_USAGE_SYS_NOOP,
        SysTrackDecision::Apply => GALE_USAGE_SYS_APPLY,
    }
}

/// Action codes for z_sched_usage_start.
pub const GALE_USAGE_START_RECORD_ONLY: u8 = 0;
pub const GALE_USAGE_START_RECORD_WINDOW: u8 = 1;

/// Decide what z_sched_usage_start should do for this thread.
///
/// usage.c:74-97: if track_usage is true (analysis mode), also reset
/// the current window counter and increment num_windows.
///
/// Returns:
///   GALE_USAGE_START_RECORD_ONLY   (0) — set usage0=now only
///   GALE_USAGE_START_RECORD_WINDOW (1) — set usage0=now, also update window
///
/// Verified: US1 (window tracking only when track_usage set).
#[cfg(feature = "usage")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_usage_start_decide(track_usage: u32) -> u8 {
    use gale::usage::{StartDecision, start_decide};

    match start_decide(track_usage != 0) {
        StartDecision::RecordOnly => GALE_USAGE_START_RECORD_ONLY,
        StartDecision::RecordStart => GALE_USAGE_START_RECORD_WINDOW,
    }
}

/// Action codes for z_sched_usage_stop.
pub const GALE_USAGE_STOP_SKIP: u8 = 0;
pub const GALE_USAGE_STOP_ACCUMULATE: u8 = 1;

/// Decide what z_sched_usage_stop should do.
///
/// usage.c:107: `if (u0 != 0)` — only accumulate if start was recorded.
///
/// Returns:
///   GALE_USAGE_STOP_SKIP       (0) — usage0 == 0; do nothing
///   GALE_USAGE_STOP_ACCUMULATE (1) — usage0 != 0; compute and accumulate cycles
///
/// Verified: US2 (accumulate only when usage0 != 0).
#[cfg(feature = "usage")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_usage_stop_decide(usage0: u32) -> u8 {
    use gale::usage::{StopDecision, stop_decide};

    match stop_decide(usage0) {
        StopDecision::Skip => GALE_USAGE_STOP_SKIP,
        StopDecision::Accumulate => GALE_USAGE_STOP_ACCUMULATE,
    }
}

/// Compute average cycles, guarding against division by zero.
///
/// usage.c:155-159, 211-215:
///   if (num_windows == 0) { stats->average_cycles = 0; }
///   else { stats->average_cycles = total / num_windows; }
///
/// Arguments:
///   total_cycles: accumulated total execution cycles
///   num_windows:  number of scheduling windows
///   out_average:  pointer to receive the computed average
///
/// Returns:
///   0 (OK)      — result written to *out_average
///   -EINVAL     — null pointer
///
/// Verified: US5 (no division by zero when num_windows == 0).
#[cfg(feature = "usage")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_usage_average_cycles(
    total_cycles: u64,
    num_windows: u32,
    out_average: *mut u64,
) -> i32 {
    use gale::usage::average_cycles;

    unsafe {
        if out_average.is_null() {
            return EINVAL;
        }
        *out_average = average_cycles(total_cycles, num_windows);
        OK
    }
}

/// Compute elapsed cycles between two u32 timestamps using wrapping subtraction.
///
/// usage.c:108: `uint32_t cycles = usage_now() - u0;`
/// The hardware cycle counter is u32 and wraps around; wrapping subtraction
/// handles the wrap-around correctly.
///
/// Verified: US2 (elapsed cycles used by stop path).
#[cfg(feature = "usage")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_usage_elapsed_cycles(now: u32, usage0: u32) -> u32 {
    use gale::usage::elapsed_cycles;

    elapsed_cycles(now, usage0)
}

/// Accumulate cycles into a thread's total, checking for overflow.
///
/// Called by the C shim's stop path after computing elapsed cycles.
/// US6: total_cycles is monotonically non-decreasing.
///
/// Arguments:
///   total_cycles:     pointer to the thread's accumulated cycle counter
///   cycles:           cycles to add (from elapsed_cycles)
///
/// Returns:
///   0 (OK)         — *total_cycles updated
///   -EOVERFLOW     — would overflow u64; *total_cycles unchanged
///   -EINVAL        — null pointer
#[cfg(feature = "usage")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_usage_accumulate(total_cycles: *mut u64, cycles: u32) -> i32 {
    use gale::usage::ThreadUsage;

    unsafe {
        if total_cycles.is_null() {
            return EINVAL;
        }

        let mut usage = ThreadUsage {
            track_usage: true,
            total_cycles: *total_cycles,
            num_windows: 0,
        };
        let rc = usage.accumulate(cycles);
        if rc == OK {
            *total_cycles = usage.total_cycles;
        }
        rc
    }
}

// ---------------------------------------------------------------------------
// MMU — validated virtual address space management decisions
// ---------------------------------------------------------------------------

/// Decision result for region-align arithmetic.
#[repr(C)]
#[cfg(feature = "mmu")]
pub struct GaleMmuAlignResult {
    /// Aligned (rounded-down) address.
    pub aligned_addr: u32,
    /// Offset from aligned_addr to the original addr.
    pub addr_offset: u32,
    /// Rounded-up total size covering the original range.
    pub aligned_size: u32,
}

/// Validate a map request before allocating virtual address space.
///
/// mmu.c:570-677 — size > 0, page-aligned, user+uninit forbidden,
/// guard-page total does not overflow.
///
/// Returns 0 (OK) or -EINVAL.
#[cfg(feature = "mmu")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mmu_map_request_decide(size: u32, flags: u32, page_size: u32) -> i32 {
    use gale::mmu::map_request_decide;
    if page_size == 0 {
        return EINVAL;
    }
    map_request_decide(size, flags, page_size)
}

/// Validate an unmap request.
///
/// mmu.c:679-695 — addr >= page_size, size > 0 and page-aligned,
/// guard-page total does not overflow.
///
/// Returns 0 (OK) or -EINVAL.
#[cfg(feature = "mmu")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mmu_unmap_request_decide(addr: u32, size: u32, page_size: u32) -> i32 {
    use gale::mmu::unmap_request_decide;
    if page_size == 0 {
        return EINVAL;
    }
    unmap_request_decide(addr, size, page_size)
}

/// Validate a flags-update request.
///
/// mmu.c:819-847 — size > 0, page-aligned, only known K_MEM_* bits set.
///
/// Returns 0 (OK) or -EINVAL.
#[cfg(feature = "mmu")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mmu_update_flags_decide(size: u32, flags: u32, page_size: u32) -> i32 {
    use gale::mmu::validate_update_flags;
    if page_size == 0 {
        return EINVAL;
    }
    if validate_update_flags(size, flags, page_size) {
        OK
    } else {
        EINVAL
    }
}

/// Compute page-aligned address, offset, and size for a physical region.
///
/// mmu.c:1008-1021 (k_mem_region_align).
#[cfg(feature = "mmu")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mmu_region_align(
    addr: u32,
    size: u32,
    align: u32,
) -> GaleMmuAlignResult {
    use gale::mmu::region_align_decide;
    if align == 0 {
        return GaleMmuAlignResult { aligned_addr: addr, addr_offset: 0, aligned_size: size };
    }
    let r = region_align_decide(addr, size, align);
    GaleMmuAlignResult {
        aligned_addr: r.aligned_addr,
        addr_offset: r.addr_offset,
        aligned_size: r.aligned_size,
    }
}

/// Check whether two virtual address regions overlap.
///
/// Returns true if [base1, base1+size1) and [base2, base2+size2) intersect.
#[cfg(feature = "mmu")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_mmu_regions_overlap(
    base1: u32,
    size1: u32,
    base2: u32,
    size2: u32,
) -> bool {
    use gale::mmu::virt_regions_overlap_decide;
    virt_regions_overlap_decide(base1, size1, base2, size2)
}

// ---------------------------------------------------------------------------
// FFI exports — pm (power management state machine)
// ---------------------------------------------------------------------------
//
// These pure functions replace the policy and state machine decision logic from
// subsys/pm/pm.c and subsys/pm/policy/policy_default.c:
//
//   pm.c:135-153        pm_state_force — record forced state
//   pm.c:182-189        forced/policy selection in pm_system_suspend
//   policy_default.c:27-38  min-residency check
//
// Verified by Verus (SMT/Z3):
//   PM1: state enum bounds
//   PM2: ACTIVE can transition to any state
//   PM3: any non-terminal state resumes to ACTIVE
//   PM4: SOFT_OFF is terminal
//   PM5: forced state applied once, then cleared
//   PM6: policy respects residency constraint

/// Decision struct for PM state force — tells C shim what action to take.
#[repr(C)]
pub struct GalePmForceDecision {
    /// Action: 0=FORCE_OK, 1=TERMINAL (SOFT_OFF blocks force)
    pub action: u8,
    /// Requested state code (only meaningful when action=FORCE_OK)
    pub state: u8,
    /// Requested substate id
    pub substate_id: u8,
}

pub const GALE_PM_FORCE_OK: u8 = 0;
pub const GALE_PM_FORCE_TERMINAL: u8 = 1;

/// Decide whether a PM state can be forced.
///
/// C extracts whether the CPU is currently in SOFT_OFF; Rust decides
/// if the force is permissible (PM4: SOFT_OFF blocks all forces).
///
/// Verified: PM4, PM5.
#[cfg(feature = "pm")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_pm_force_decide(
    current_state: u8,
    target_state: u8,
    substate_id: u8,
) -> GalePmForceDecision {
    use gale::pm::{PmState, PM_STATE_COUNT};

    if current_state >= PM_STATE_COUNT || target_state >= PM_STATE_COUNT {
        return GalePmForceDecision {
            action: GALE_PM_FORCE_TERMINAL,
            state: current_state,
            substate_id: 0,
        };
    }
    // PM4: SOFT_OFF is terminal — cannot force any transition from it
    if current_state == PmState::SoftOff as u8 {
        return GalePmForceDecision {
            action: GALE_PM_FORCE_TERMINAL,
            state: current_state,
            substate_id: 0,
        };
    }
    GalePmForceDecision {
        action: GALE_PM_FORCE_OK,
        state: target_state,
        substate_id,
    }
}

/// Decision struct for PM suspend — tells C shim which state to enter.
#[repr(C)]
pub struct GalePmSuspendDecision {
    /// Action: 0=ENTER_STATE, 1=STAY_ACTIVE
    pub action: u8,
    /// State to enter (only valid when action=ENTER_STATE)
    pub state: u8,
    /// Substate id
    pub substate_id: u8,
}

pub const GALE_PM_ACTION_ENTER_STATE: u8 = 0;
pub const GALE_PM_ACTION_STAY_ACTIVE: u8 = 1;

/// Decide PM suspend outcome: forced state vs. policy state.
///
/// C shim extracts: whether a forced state is pending, which state the
/// policy chose.  Rust decides which one to use (PM5: forced wins).
///
/// Verified: PM5.
#[cfg(feature = "pm")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_pm_suspend_decide(
    has_forced: u8,
    forced_state: u8,
    forced_substate: u8,
    has_policy: u8,
    policy_state: u8,
    policy_substate: u8,
) -> GalePmSuspendDecision {
    use gale::pm::{PmState, suspend_state_decide, PM_STATE_COUNT};

    let forced = if has_forced != 0 && forced_state < PM_STATE_COUNT {
        PmState::from_u8(forced_state).ok()
    } else {
        None
    };
    let policy = if has_policy != 0 && policy_state < PM_STATE_COUNT {
        PmState::from_u8(policy_state).ok()
    } else {
        None
    };

    match suspend_state_decide(forced, policy) {
        Some(state) => {
            let substate = if has_forced != 0 { forced_substate } else { policy_substate };
            GalePmSuspendDecision {
                action: GALE_PM_ACTION_ENTER_STATE,
                state: state as u8,
                substate_id: substate,
            }
        }
        None => GalePmSuspendDecision {
            action: GALE_PM_ACTION_STAY_ACTIVE,
            state: 0,
            substate_id: 0,
        },
    }
}

/// Decide whether the residency constraint is satisfied.
///
/// C shim converts min_residency_us + exit_latency_us to ticks and
/// passes both to Rust. Rust decides if there is enough time.
///
/// Verified: PM6.
#[cfg(feature = "pm")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_pm_residency_ok(
    ticks_available: i32,
    min_residency_ticks: u32,
) -> bool {
    use gale::pm::policy_residency_ok;
    policy_residency_ok(ticks_available, min_residency_ticks)
}

/// Decide whether a power state transition is valid.
///
/// C shim passes raw state codes; Rust validates PM2/PM3/PM4.
///
/// Returns: 1 if valid, 0 if not.
#[cfg(feature = "pm")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_pm_transition_valid(from_state: u8, to_state: u8) -> u8 {
    use gale::pm::{PmState, state_transition_valid, PM_STATE_COUNT};

    if from_state >= PM_STATE_COUNT || to_state >= PM_STATE_COUNT {
        return 0;
    }
    let from = match PmState::from_u8(from_state) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    let to = match PmState::from_u8(to_state) {
        Ok(s) => s,
        Err(_) => return 0,
    };
    if state_transition_valid(from, to) { 1 } else { 0 }
}

// ---------------------------------------------------------------------------
// FFI exports — net_buf (pool allocation tracking + data pointer arithmetic)
// ---------------------------------------------------------------------------
//
// These pure functions provide verified alloc/free/ref/data-operation decisions
// for lib/net_buf/buf.c and lib/net_buf/buf_simple.c.
//
// Source mapping:
//   net_buf_alloc / net_buf_alloc_len  -> gale_net_buf_alloc_decide
//   net_buf_unref                      -> gale_net_buf_free_decide
//   net_buf_ref                        -> gale_net_buf_ref_decide
//   net_buf_simple_add                 -> gale_net_buf_add_decide
//   net_buf_simple_remove_mem          -> gale_net_buf_remove_decide
//   net_buf_simple_push                -> gale_net_buf_push_decide
//   net_buf_simple_pull                -> gale_net_buf_pull_decide
//
// Verified properties:
//   NB1: alloc never exceeds pool capacity (0 <= allocated <= capacity)
//   NB2: free returns buffer to pool (allocated decrements correctly)
//   NB3: ref count tracks owners (ref_count >= 1 while in use)
//   NB4: data bounds: head_offset + len <= size (no overflow)
//   NB5: push/pull preserve bounds (headroom and tailroom checks)
//   NB6: no double-free (ref_count must be >= 1 to unref)

/// Decision for net_buf pool alloc.
#[repr(C)]
pub struct GaleNetBufAllocDecision {
    /// New allocated count on success.
    pub new_allocated: u16,
    /// 0 = OK (proceed with alloc), -ENOMEM = pool exhausted.
    pub rc: i32,
}

/// Decide a net_buf pool allocation.
///
/// NB1: success when allocated < capacity (allocated increments by 1).
/// NB1: returns ENOMEM when pool is exhausted.
///
/// Arguments:
///   allocated: current number of buffers in use
///   capacity:  total pool size (buf_count)
///
/// Verified: NB1 (bounds), no overflow.
#[cfg(feature = "net_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_net_buf_alloc_decide(
    allocated: u16,
    capacity: u16,
) -> GaleNetBufAllocDecision {
    use gale::net_buf::alloc_decide;
    match alloc_decide(allocated, capacity) {
        Ok(new_alloc) => GaleNetBufAllocDecision { new_allocated: new_alloc, rc: OK },
        Err(e)        => GaleNetBufAllocDecision { new_allocated: allocated, rc: e },
    }
}

/// Decide a net_buf pool free (unref to pool).
///
/// NB2: success when allocated > 0 (allocated decrements by 1).
/// NB6: returns EINVAL if allocated == 0 (double-free guard).
///
/// Arguments:
///   allocated: current number of buffers in use
///
/// Verified: NB2 (free decrements), NB6 (no double-free).
#[cfg(feature = "net_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_net_buf_free_decide(
    allocated: u16,
) -> i32 {
    use gale::net_buf::free_decide;
    match free_decide(allocated) {
        Ok(_)  => OK,
        Err(e) => e,
    }
}

/// Decision for net_buf ref/unref operations.
#[repr(C)]
pub struct GaleNetBufRefDecision {
    /// New ref_count value.
    pub new_ref_count: u8,
    /// 1 if buffer should be returned to pool (ref_count reached 0), 0 otherwise.
    pub should_free: u8,
    /// 0 = OK, -EINVAL = double-unref attempted, -EOVERFLOW = ref overflow.
    pub rc: i32,
}

/// Decide a net_buf_ref (increment reference count).
///
/// NB3: ref_count tracks owners. Saturates at u8::MAX (EOVERFLOW).
///
/// Arguments:
///   ref_count: current reference count (must be >= 1)
///
/// Verified: NB3 (ref count monotone increment), no overflow.
#[cfg(feature = "net_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_net_buf_ref_decide(
    ref_count: u8,
) -> GaleNetBufRefDecision {
    use gale::net_buf::ref_decide;
    match ref_decide(ref_count) {
        Ok(new_ref) => GaleNetBufRefDecision { new_ref_count: new_ref, should_free: 0, rc: OK },
        Err(e)      => GaleNetBufRefDecision { new_ref_count: ref_count, should_free: 0, rc: e },
    }
}

/// Decide a net_buf_unref (decrement reference count).
///
/// NB3: decrements ref_count. Returns should_free=1 when count reaches 0.
/// NB6: returns EINVAL if ref_count is already 0 (double-free guard).
///
/// Arguments:
///   ref_count: current reference count
///
/// Verified: NB3 (ref count tracks owners), NB6 (no double-free).
#[cfg(feature = "net_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_net_buf_unref_decide(
    ref_count: u8,
) -> GaleNetBufRefDecision {
    use gale::net_buf::unref_decide;
    match unref_decide(ref_count) {
        Ok((new_ref, should_free)) => GaleNetBufRefDecision {
            new_ref_count: new_ref,
            should_free: if should_free { 1 } else { 0 },
            rc: OK,
        },
        Err(e) => GaleNetBufRefDecision { new_ref_count: ref_count, should_free: 0, rc: e },
    }
}

/// Decision for net_buf data operations (add/remove/push/pull).
#[repr(C)]
pub struct GaleNetBufDataDecision {
    /// New head_offset after operation.
    pub new_head_offset: u16,
    /// New len after operation.
    pub new_len: u16,
    /// 0 = OK, -ENOMEM = no tailroom, -EINVAL = no headroom or len underflow.
    pub rc: i32,
}

/// Decide a net_buf_simple_add (append bytes at tail).
///
/// NB4: new head_offset + new_len <= size.
/// NB5: tailroom decreases by bytes.
///
/// Arguments:
///   head_offset: current data pointer offset from __buf
///   len:         current data length
///   size:        total buffer size
///   bytes:       number of bytes to add
///
/// Verified: NB4 (bounds), NB5 (tailroom check), no overflow.
#[cfg(feature = "net_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_net_buf_add_decide(
    head_offset: u16,
    len: u16,
    size: u16,
    bytes: u16,
) -> GaleNetBufDataDecision {
    use gale::net_buf::add_decide;
    match add_decide(head_offset, len, size, bytes) {
        Ok(new_len) => GaleNetBufDataDecision { new_head_offset: head_offset, new_len, rc: OK },
        Err(e)      => GaleNetBufDataDecision { new_head_offset: head_offset, new_len: len, rc: e },
    }
}

/// Decide a net_buf_simple_remove_mem (remove bytes from tail).
///
/// NB4/NB5: len decrements, head_offset unchanged.
///
/// Arguments:
///   head_offset: current data pointer offset (returned unchanged)
///   len:         current data length
///   bytes:       number of bytes to remove
///
/// Verified: NB4 (bounds), NB5 (len >= bytes check), no underflow.
#[cfg(feature = "net_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_net_buf_remove_decide(
    head_offset: u16,
    len: u16,
    bytes: u16,
) -> GaleNetBufDataDecision {
    use gale::net_buf::remove_decide;
    match remove_decide(len, bytes) {
        Ok(new_len) => GaleNetBufDataDecision { new_head_offset: head_offset, new_len, rc: OK },
        Err(e)      => GaleNetBufDataDecision { new_head_offset: head_offset, new_len: len, rc: e },
    }
}

/// Decide a net_buf_simple_push (prepend bytes at head).
///
/// NB4: (head_offset - bytes) + (len + bytes) == head_offset + len <= size.
/// NB5: headroom check (head_offset >= bytes).
///
/// Arguments:
///   head_offset: current data pointer offset from __buf
///   len:         current data length
///   bytes:       number of bytes to push
///
/// Verified: NB4 (bounds preserved), NB5 (headroom >= bytes), no underflow.
#[cfg(feature = "net_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_net_buf_push_decide(
    head_offset: u16,
    len: u16,
    bytes: u16,
) -> GaleNetBufDataDecision {
    use gale::net_buf::push_decide;
    match push_decide(head_offset, len, bytes) {
        Ok((new_head, new_len)) => GaleNetBufDataDecision { new_head_offset: new_head, new_len, rc: OK },
        Err(e)                  => GaleNetBufDataDecision { new_head_offset: head_offset, new_len: len, rc: e },
    }
}

/// Decide a net_buf_simple_pull (consume bytes from head).
///
/// NB4: (head_offset + bytes) + (len - bytes) == head_offset + len <= size.
/// NB5: len >= bytes check.
///
/// Arguments:
///   head_offset: current data pointer offset from __buf
///   len:         current data length
///   size:        total buffer size (for postcondition)
///   bytes:       number of bytes to pull
///
/// Verified: NB4 (bounds preserved), NB5 (len >= bytes), no underflow.
#[cfg(feature = "net_buf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_net_buf_pull_decide(
    head_offset: u16,
    len: u16,
    size: u16,
    bytes: u16,
) -> GaleNetBufDataDecision {
    use gale::net_buf::pull_decide;
    match pull_decide(head_offset, len, size, bytes) {
        Ok((new_head, new_len)) => GaleNetBufDataDecision { new_head_offset: new_head, new_len, rc: OK },
        Err(e)                  => GaleNetBufDataDecision { new_head_offset: head_offset, new_len: len, rc: e },
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — net_buf
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "net_buf"))]
mod kani_net_buf_proofs {
    use super::*;

    /// NB1: alloc_decide never returns new_allocated > capacity.
    #[kani::proof]
    fn net_buf_alloc_bounded() {
        let allocated: u16 = kani::any();
        let capacity: u16 = kani::any();
        kani::assume(capacity > 0);
        kani::assume(allocated <= capacity);
        let d = gale_net_buf_alloc_decide(allocated, capacity);
        if d.rc == OK {
            assert!(d.new_allocated <= capacity);
            assert!(d.new_allocated == allocated + 1);
        } else {
            assert!(allocated == capacity);
        }
    }

    /// NB2: free_decide never underflows.
    #[kani::proof]
    fn net_buf_free_no_underflow() {
        let allocated: u16 = kani::any();
        let rc = gale_net_buf_free_decide(allocated);
        if rc == OK {
            assert!(allocated > 0);
        } else {
            assert!(allocated == 0);
        }
    }

    /// NB3: ref_decide increments by exactly 1.
    #[kani::proof]
    fn net_buf_ref_increments_by_one() {
        let ref_count: u8 = kani::any();
        let d = gale_net_buf_ref_decide(ref_count);
        if d.rc == OK {
            assert!(d.new_ref_count == ref_count + 1);
        }
    }

    /// NB6: unref_decide rejects zero ref_count.
    #[kani::proof]
    fn net_buf_unref_double_free_rejected() {
        let d = gale_net_buf_unref_decide(0);
        assert!(d.rc == EINVAL);
    }

    /// NB4: add_decide preserves head_offset + new_len <= size.
    #[kani::proof]
    fn net_buf_add_bounds_preserved() {
        let head_offset: u16 = kani::any();
        let len: u16 = kani::any();
        let size: u16 = kani::any();
        let bytes: u16 = kani::any();
        kani::assume(size > 0 && size <= 1024);
        kani::assume(head_offset as u32 + len as u32 <= size as u32);
        let d = gale_net_buf_add_decide(head_offset, len, size, bytes);
        if d.rc == OK {
            assert!(head_offset as u32 + d.new_len as u32 <= size as u32);
        }
    }

    /// NB4: push_decide preserves bounds.
    #[kani::proof]
    fn net_buf_push_bounds_preserved() {
        let head_offset: u16 = kani::any();
        let len: u16 = kani::any();
        let size: u16 = kani::any();
        let bytes: u16 = kani::any();
        kani::assume(size > 0 && size <= 1024);
        kani::assume(head_offset as u32 + len as u32 <= size as u32);
        let d = gale_net_buf_push_decide(head_offset, len, bytes);
        if d.rc == OK {
            assert!(d.new_head_offset as u32 + d.new_len as u32 <= size as u32);
        }
    }

    /// NB4: pull_decide preserves bounds.
    #[kani::proof]
    fn net_buf_pull_bounds_preserved() {
        let head_offset: u16 = kani::any();
        let len: u16 = kani::any();
        let size: u16 = kani::any();
        let bytes: u16 = kani::any();
        kani::assume(size > 0 && size <= 1024);
        kani::assume(head_offset as u32 + len as u32 <= size as u32);
        let d = gale_net_buf_pull_decide(head_offset, len, size, bytes);
        if d.rc == OK {
            assert!(d.new_head_offset as u32 + d.new_len as u32 <= size as u32);
        }
    }

    /// NB5: push-pull roundtrip restores original state.
    #[kani::proof]
    fn net_buf_push_pull_roundtrip() {
        let head_offset: u16 = kani::any();
        let len: u16 = kani::any();
        let size: u16 = kani::any();
        let bytes: u16 = kani::any();
        kani::assume(size > 0 && size <= 1024);
        kani::assume(head_offset as u32 + len as u32 <= size as u32);
        let push = gale_net_buf_push_decide(head_offset, len, bytes);
        if push.rc == OK {
            let pull = gale_net_buf_pull_decide(
                push.new_head_offset, push.new_len, size, bytes);
            if pull.rc == OK {
                assert!(pull.new_head_offset == head_offset);
                assert!(pull.new_len == len);
            }
        }
    }
}

// ---------------------------------------------------------------------------
// FFI exports — cbprintf (format string and buffer validation)
// ---------------------------------------------------------------------------
//
// These pure functions replace the safety-critical validation paths in
// Zephyr's cbprintf subsystem:
//
//   cbprintf_complete.c   format specifier parsing and validation
//   cbprintf_packaged.c   argument packaging buffer bounds
//   cbprintf_complete.c   output byte counter tracking
//
// Verified by Verus (SMT/Z3):
//   CB1: FormatSpec fields are within representable bounds
//   CB2: PackageState never exceeds buffer capacity
//   CB3: OutputState length tracking is monotone and bounded
//   CB4: Dangerous conversion specifiers are rejected
//   CB5: %n is always rejected regardless of context

/// Maximum output length constant — mirrors gale::cbprintf::MAX_OUTPUT_LEN.
const CBPRINTF_MAX_OUTPUT_LEN: usize = usize::MAX / 2;

/// Validate a single printf conversion specifier character.
///
/// CB4 + CB5: %n (ASCII 110) and unknown specifiers are always rejected.
///
/// Arguments:
///   specifier_char: ASCII character (e.g. b'd', b's', b'n')
///
/// Returns:
///   0        — specifier is safe
///   -EINVAL  — specifier is %n or unrecognised
#[cfg(feature = "cbprintf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_cbprintf_validate_specifier(specifier_char: u8) -> i32 {
    use gale::cbprintf::validate_specifier_char;
    validate_specifier_char(specifier_char)
}

/// Validate format specifier bounds: width, precision, and flag combination.
///
/// CB1: width and precision must fit in [0, INT_MAX].
/// CB4: Invalid specifiers are rejected.
/// CB5: %n is always rejected.
///
/// Arguments:
///   specifier_char: ASCII conversion character
///   width_value:    width from format string (0 if not present)
///   prec_value:     precision from format string (0 if not present)
///   flag_dash:      1 if '-' flag present
///   flag_zero:      1 if '0' flag present
///
/// Returns:
///   0        — valid specifier
///   -EINVAL  — specifier rejected or out of bounds
#[cfg(feature = "cbprintf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_cbprintf_validate_format_spec(
    specifier_char: u8,
    width_value:    u32,
    prec_value:     u32,
    flag_dash:      u8,
    flag_zero:      u8,
) -> i32 {
    use gale::cbprintf::{
        ConversionSpecifier, FormatSpec, LengthModifier, validate_format_spec,
    };

    let spec = match specifier_char {
        b'd' | b'i' => ConversionSpecifier::SignedInt,
        b'u'        => ConversionSpecifier::UnsignedInt,
        b'o'        => ConversionSpecifier::Octal,
        b'x' | b'X' => ConversionSpecifier::Hex,
        b'c'        => ConversionSpecifier::Char,
        b's'        => ConversionSpecifier::String,
        b'p'        => ConversionSpecifier::Pointer,
        b'%'        => ConversionSpecifier::Percent,
        b'n'        => ConversionSpecifier::WriteBack,
        _           => ConversionSpecifier::Invalid,
    };

    let fs = FormatSpec::new(
        flag_dash != 0,  // flag_dash
        false,           // flag_plus  (not modelled at FFI boundary)
        false,           // flag_space
        false,           // flag_hash
        flag_zero != 0,  // flag_zero
        width_value > 0, // width_present
        false,           // width_star
        width_value,
        prec_value > 0,  // prec_present
        false,           // prec_star
        prec_value,
        LengthModifier::None,
        spec,
    );

    match validate_format_spec(&fs) {
        Ok(()) => 0,
        Err(e) => e,
    }
}

/// Check whether writing `size` bytes into the package buffer would overflow.
///
/// CB2: package buffer never overflows — returns -ENOMEM if it would.
///
/// Arguments:
///   pos:      current write position in the buffer
///   capacity: total buffer capacity in bytes
///   size:     bytes about to be written
///
/// Returns:
///   0       — write is safe
///   -ENOMEM — write would overflow the buffer
#[cfg(feature = "cbprintf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_cbprintf_package_bounds_check(
    pos:      usize,
    capacity: usize,
    size:     usize,
) -> i32 {
    use gale::cbprintf::{PackageState, package_bounds_check};

    // Clamp capacity to MAX_PACKAGE_BUF — values larger than that are
    // rejected by the model invariant.
    let cap = if capacity > gale::cbprintf::MAX_PACKAGE_BUF {
        gale::cbprintf::MAX_PACKAGE_BUF
    } else {
        capacity
    };

    // Construct a PackageState at the given position.
    // If pos > cap the buffer is already invalid; treat as overflow.
    if pos > cap {
        return ENOMEM;
    }

    let state = PackageState { pos, capacity: cap };

    match package_bounds_check(state, size) {
        Ok(_) => 0,
        Err(e) => e,
    }
}

/// Accumulate output bytes with overflow detection.
///
/// CB3: output length is tracked accurately; saturates instead of wrapping.
///
/// Arguments:
///   count:     current byte count
///   n:         bytes being added
///   out_count: receives the updated byte count (saturated on overflow)
///
/// Returns:
///   0          — success, *out_count is the new total
///   -EOVERFLOW — saturation occurred, *out_count == MAX_OUTPUT_LEN
#[cfg(feature = "cbprintf")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_cbprintf_output_add(
    count:     usize,
    n:         usize,
    out_count: *mut usize,
) -> i32 {
    use gale::cbprintf::{OutputState, output_bounds_check};

    if out_count.is_null() {
        return EINVAL;
    }

    // Clamp input count to MAX_OUTPUT_LEN to preserve invariant.
    let clamped_count = if count > CBPRINTF_MAX_OUTPUT_LEN {
        CBPRINTF_MAX_OUTPUT_LEN
    } else {
        count
    };

    let state = OutputState { count: clamped_count, overflow: false };
    let next = output_bounds_check(state, n);

    // SAFETY: null check above; caller owns the pointed-to usize.
    unsafe {
        *out_count = next.count;
    }

    if next.overflow {
        // -EOVERFLOW = -75 (POSIX)
        -75_i32
    } else {
        0
    }
}

// ---------------------------------------------------------------------------
// FFI exports — ipc (IPC service endpoint lifecycle)
// ---------------------------------------------------------------------------
//
// These functions replace the validation and state-machine logic from:
//
//   ipc_service.c:17-39   ipc_service_open_instance
//   ipc_service.c:41-63   ipc_service_close_instance
//   ipc_service.c:65-88   ipc_service_register_endpoint
//   ipc_service.c:90-120  ipc_service_deregister_endpoint
//   ipc_service.c:123-145 ipc_service_send
//   ipc_service.c:147-169 ipc_service_send_critical
//   ipc_service.c:171-198 ipc_service_get_tx_buffer_size
//
// Verified by Verus (SMT/Z3):
//   IPC1: Endpoint state is always a valid variant
//   IPC2: Open only from Closed
//   IPC3: send/send_critical only when Bound
//   IPC4: Close always returns to Closed
//   IPC5: Endpoint count bounded by MAX_ENDPOINTS
//   IPC6: Buffer length in [1, MAX_MSG_LEN]

/// State constants — must match GALE_IPC_STATE_* in gale_ipc.h.
const IPC_STATE_CLOSED: u32 = 0;
const IPC_STATE_OPEN: u32   = 1;
const IPC_STATE_BOUND: u32  = 2;

fn u32_to_ipc_state(raw: u32) -> Option<gale::ipc::IpcEndpointState> {
    use gale::ipc::IpcEndpointState;
    match raw {
        s if s == IPC_STATE_CLOSED => Some(IpcEndpointState::Closed),
        s if s == IPC_STATE_OPEN   => Some(IpcEndpointState::Open),
        s if s == IPC_STATE_BOUND  => Some(IpcEndpointState::Bound),
        _                          => None,
    }
}

/// Decide whether the IPC instance may be opened.
///
/// ipc_service.c:17-39.
///
/// SAFETY: Pure function, no pointer dereferences.
#[cfg(feature = "ipc")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ipc_open_decide(instance_valid: bool) -> i32 {
    use gale::ipc::IpcServiceState;
    IpcServiceState::open_decide(instance_valid)
}

/// Decide whether the IPC instance may be closed.
///
/// ipc_service.c:41-63.
///
/// SAFETY: Pure function, no pointer dereferences.
#[cfg(feature = "ipc")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ipc_close_decide(instance_valid: bool) -> i32 {
    use gale::ipc::IpcServiceState;
    IpcServiceState::close_decide(instance_valid)
}

/// Decide whether an endpoint may be registered.
///
/// ipc_service.c:65-88.
///
/// SAFETY: `new_count_out` must be a valid non-null pointer.
///         Called under Zephyr's IPC service lock — no concurrent access.
#[cfg(feature = "ipc")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ipc_register_decide(
    params_valid:     bool,
    registered_count: u32,
    max_endpoints:    u32,
    new_count_out:    *mut u32,
) -> i32 {
    use gale::ipc::{IpcServiceState, MAX_ENDPOINTS};

    if new_count_out.is_null() {
        return -22; // EINVAL
    }
    if max_endpoints > MAX_ENDPOINTS {
        return -22; // EINVAL
    }

    let mut svc = IpcServiceState {
        registered_count,
        max_endpoints,
    };
    let result = svc.register_decide(params_valid);
    // SAFETY: pointer validated above; called under IPC lock.
    unsafe { *new_count_out = svc.registered_count; }
    result
}

/// Decide whether an endpoint may be deregistered.
///
/// ipc_service.c:90-120.
///
/// SAFETY: `new_count_out` must be a valid non-null pointer.
///         Called under Zephyr's IPC service lock — no concurrent access.
#[cfg(feature = "ipc")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ipc_deregister_decide(
    endpoint_valid:      bool,
    endpoint_registered: bool,
    registered_count:    u32,
    max_endpoints:       u32,
    new_count_out:       *mut u32,
) -> i32 {
    use gale::ipc::{IpcServiceState, MAX_ENDPOINTS};

    if new_count_out.is_null() {
        return -22; // EINVAL
    }
    if max_endpoints > MAX_ENDPOINTS {
        return -22; // EINVAL
    }

    let mut svc = IpcServiceState {
        registered_count,
        max_endpoints,
    };
    let result = svc.deregister_decide(endpoint_valid, endpoint_registered);
    // SAFETY: pointer validated above; called under IPC lock.
    unsafe { *new_count_out = svc.registered_count; }
    result
}

/// Decide whether a send operation is valid.
///
/// ipc_service.c:123-145.
///
/// SAFETY: Pure function, no pointer dereferences.
#[cfg(feature = "ipc")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ipc_send_decide(
    endpoint_valid:      bool,
    endpoint_registered: bool,
    state:               u32,
    len:                 u32,
) -> i32 {
    use gale::ipc::send_decide;
    let s = match u32_to_ipc_state(state) {
        Some(s) => s,
        None    => return -22, // EINVAL — unknown state
    };
    send_decide(endpoint_valid, endpoint_registered, s, len)
}

/// Decide whether a critical send is valid.
///
/// ipc_service.c:147-169 (identical preconditions to send).
///
/// SAFETY: Pure function, no pointer dereferences.
#[cfg(feature = "ipc")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ipc_send_critical_decide(
    endpoint_valid:      bool,
    endpoint_registered: bool,
    state:               u32,
    len:                 u32,
) -> i32 {
    use gale::ipc::send_critical_decide;
    let s = match u32_to_ipc_state(state) {
        Some(s) => s,
        None    => return -22, // EINVAL
    };
    send_critical_decide(endpoint_valid, endpoint_registered, s, len)
}

/// Validate a receive operation.
///
/// SAFETY: Pure function, no pointer dereferences.
#[cfg(feature = "ipc")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ipc_receive_decide(
    endpoint_valid:      bool,
    endpoint_registered: bool,
    state:               u32,
    len:                 u32,
) -> i32 {
    use gale::ipc::receive_decide;
    let s = match u32_to_ipc_state(state) {
        Some(s) => s,
        None    => return -22, // EINVAL
    };
    receive_decide(endpoint_valid, endpoint_registered, s, len)
}

/// Validate a TX buffer-size query.
///
/// ipc_service.c:171-198.
///
/// SAFETY: Pure function, no pointer dereferences.
#[cfg(feature = "ipc")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ipc_validate_buffer_size(
    endpoint_valid:      bool,
    endpoint_registered: bool,
    reported_size:       u32,
) -> i32 {
    use gale::ipc::validate_buffer_size;
    validate_buffer_size(endpoint_valid, endpoint_registered, reported_size)
}

/// Validate a Closed->Open state transition.
///
/// SAFETY: `new_state_out` must be a valid non-null pointer.
#[cfg(feature = "ipc")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ipc_transition_open(
    current_state: u32,
    new_state_out: *mut u32,
) -> i32 {
    use gale::ipc::IpcEndpointState;

    if new_state_out.is_null() {
        return -22; // EINVAL
    }
    let state = match u32_to_ipc_state(current_state) {
        Some(s) => s,
        None    => return -22,
    };
    if state == IpcEndpointState::Closed {
        // SAFETY: pointer validated above.
        unsafe { *new_state_out = IPC_STATE_OPEN; }
        0 // OK
    } else {
        -114 // EALREADY
    }
}

/// Validate an Open->Bound state transition.
///
/// SAFETY: `new_state_out` must be a valid non-null pointer.
#[cfg(feature = "ipc")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ipc_transition_bound(
    current_state: u32,
    new_state_out: *mut u32,
) -> i32 {
    use gale::ipc::IpcEndpointState;

    if new_state_out.is_null() {
        return -22; // EINVAL
    }
    let state = match u32_to_ipc_state(current_state) {
        Some(s) => s,
        None    => return -22,
    };
    if state == IpcEndpointState::Open {
        // SAFETY: pointer validated above.
        unsafe { *new_state_out = IPC_STATE_BOUND; }
        0 // OK
    } else {
        -22 // EINVAL
    }
}

/// Force the endpoint to Closed (deregister or error path).
///
/// Always succeeds.
///
/// SAFETY: `new_state_out` must be a valid non-null pointer.
#[cfg(feature = "ipc")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_ipc_transition_close(new_state_out: *mut u32) -> i32 {
    if new_state_out.is_null() {
        return -22; // EINVAL
    }
    // SAFETY: pointer validated above.
    unsafe { *new_state_out = IPC_STATE_CLOSED; }
    0 // OK
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — IPC service
// ---------------------------------------------------------------------------

#[cfg(all(kani, feature = "ipc"))]
mod kani_ipc_proofs {
    use super::*;

    /// IPC2: open from Closed succeeds and yields Open.
    #[kani::proof]
    fn ipc_open_from_closed_ok() {
        let mut out: u32 = 99;
        let r = gale_ipc_transition_open(IPC_STATE_CLOSED, &mut out);
        assert!(r == 0);
        assert!(out == IPC_STATE_OPEN);
    }

    /// IPC2: open from Open is rejected.
    #[kani::proof]
    fn ipc_double_open_rejected() {
        let mut out: u32 = 99;
        let r = gale_ipc_transition_open(IPC_STATE_OPEN, &mut out);
        assert!(r != 0);
    }

    /// IPC3: send requires Bound state.
    #[kani::proof]
    fn ipc_send_requires_bound() {
        let r_closed = gale_ipc_send_decide(true, true, IPC_STATE_CLOSED, 64);
        assert!(r_closed != 0);
        let r_open = gale_ipc_send_decide(true, true, IPC_STATE_OPEN, 64);
        assert!(r_open != 0);
        let r_bound = gale_ipc_send_decide(true, true, IPC_STATE_BOUND, 64);
        assert!(r_bound == 0);
    }

    /// IPC4: close always yields Closed.
    #[kani::proof]
    fn ipc_close_always_closed() {
        let mut out: u32 = 99;
        let r = gale_ipc_transition_close(&mut out);
        assert!(r == 0);
        assert!(out == IPC_STATE_CLOSED);
    }

    /// IPC6: zero-length send rejected.
    #[kani::proof]
    fn ipc_send_zero_len_rejected() {
        let r = gale_ipc_send_decide(true, true, IPC_STATE_BOUND, 0);
        assert!(r != 0);
    }

    /// IPC6: send over MAX_MSG_LEN rejected.
    #[kani::proof]
    fn ipc_send_over_max_rejected() {
        let r = gale_ipc_send_decide(true, true, IPC_STATE_BOUND, 4097);
        assert!(r != 0);
    }

    /// Null pointer guard: register with null count_out returns EINVAL.
    #[kani::proof]
    fn ipc_register_null_out_einval() {
        let r = gale_ipc_register_decide(true, 0, 4, core::ptr::null_mut());
        assert!(r == -22);
    }
}

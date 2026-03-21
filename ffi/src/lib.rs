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

#![cfg_attr(not(any(test, kani)), no_std)]
// FFI boundary crate — unsafe is inherent (no_mangle, raw pointers).
// The verified pure logic lives in the `gale` crate which denies unsafe.

pub mod coarse;

use gale::error::{
    EAGAIN, EBUSY, ECANCELED, EDEADLK, EINVAL, ENOMEM, ENOMSG, EOVERFLOW, EPERM, EPIPE,
    ETIMEDOUT, OK,
};

// ---------------------------------------------------------------------------
// FFI exports — pure count arithmetic
// ---------------------------------------------------------------------------

/// Validate semaphore init parameters.
///
/// sem.c:48-50:
///   CHECKIF(limit == 0U || initial_count > limit) { return -EINVAL; }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_count_give(count: u32, limit: u32) -> u32 {
    if count != limit {
        // Verified: count < limit <= u32::MAX, no overflow possible.
        #[allow(clippy::arithmetic_side_effects)]
        let new_count = count + 1;
        new_count
    } else {
        count
    }
}

/// Attempt to decrement count for take.
///
/// sem.c:143-144:
///   if (likely(sem->count > 0U)) { sem->count--; ret = 0; }
///
/// SAFETY: `count` must point to a valid `unsigned int` (Zephyr's
/// sem->count).  Called under Zephyr's spinlock — no concurrent access.
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_count_take(count: *mut u32) -> i32 {
    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        if count.is_null() {
            return EINVAL;
        }
        if *count > 0 {
            // Verified: count > 0, so count-1 >= 0, no underflow.
            #[allow(clippy::arithmetic_side_effects)]
            {
                *count -= 1;
            }
            OK
        } else {
            EBUSY
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
/// Verified: P3 (count capped at limit), P9 (no overflow).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_sem_give_decide(
    count: u32,
    limit: u32,
    has_waiter: u32,
) -> GaleSemGiveDecision {
    if has_waiter != 0 {
        GaleSemGiveDecision {
            action: GALE_SEM_ACTION_WAKE,
            new_count: count,
        }
    } else {
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
/// Verified: P5 (decrement), P6 (-EBUSY), P9 (no underflow).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_sem_take_decide(
    count: u32,
    is_no_wait: u32,
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
            ret: 0,
            new_count: 0,
            action: GALE_SEM_ACTION_PEND,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_mutex_lock_validate(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
    new_lock_count: *mut u32,
) -> i32 {
    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        if new_lock_count.is_null() {
            return EINVAL;
        }

        if lock_count == 0 || owner_is_null != 0 {
            // Mutex unlocked — acquire (M3).
            *new_lock_count = 1;
            OK
        } else if owner_is_current != 0 {
            // Reentrant lock — same owner (M4, M10).
            match lock_count.checked_add(1) {
                Some(n) => {
                    *new_lock_count = n;
                    OK
                }
                None => {
                    // Overflow would violate M10.
                    EINVAL
                }
            }
        } else {
            // Different owner — contended (M5).
            EBUSY
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_mutex_unlock_validate(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
    new_lock_count: *mut u32,
) -> i32 {
    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        if new_lock_count.is_null() {
            return EINVAL;
        }

        // M6a: not locked
        if owner_is_null != 0 {
            return EINVAL;
        }

        // M6b: not owner
        if owner_is_current == 0 {
            return EPERM;
        }

        // M7: reentrant release (lock_count > 1)
        if lock_count > 1 {
            // Verified: lock_count > 1, so lock_count - 1 >= 1, no underflow.
            #[allow(clippy::arithmetic_side_effects)]
            {
                *new_lock_count = lock_count - 1;
            }
            GALE_MUTEX_RELEASED
        } else {
            // Fully unlocked — caller handles waiter transfer.
            *new_lock_count = 0;
            GALE_MUTEX_UNLOCKED
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
/// Verified: M3 (acquire), M4 (reentrant), M5 (contended), M10 (no overflow).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mutex_lock_decide(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
    is_no_wait: u32,
) -> GaleMutexLockDecision {
    if owner_is_null != 0 {
        // Mutex unlocked — acquire (M3).
        GaleMutexLockDecision {
            ret: OK,
            action: GALE_MUTEX_ACTION_ACQUIRED,
            new_lock_count: 1,
        }
    } else if owner_is_current != 0 {
        // Reentrant lock — same owner (M4, M10).
        match lock_count.checked_add(1) {
            Some(n) => GaleMutexLockDecision {
                ret: OK,
                action: GALE_MUTEX_ACTION_ACQUIRED,
                new_lock_count: n,
            },
            None => {
                // Overflow would violate M10.
                GaleMutexLockDecision {
                    ret: EINVAL,
                    action: GALE_MUTEX_ACTION_BUSY,
                    new_lock_count: lock_count,
                }
            }
        }
    } else if is_no_wait != 0 {
        // Different owner, no-wait — return busy (M5).
        GaleMutexLockDecision {
            ret: EBUSY,
            action: GALE_MUTEX_ACTION_BUSY,
            new_lock_count: lock_count,
        }
    } else {
        // Different owner, willing to wait — pend (M5).
        GaleMutexLockDecision {
            ret: 0,
            action: GALE_MUTEX_ACTION_PEND,
            new_lock_count: lock_count,
        }
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
/// Verified: M6a (EINVAL), M6b (EPERM), M7 (reentrant), M10 (no underflow).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mutex_unlock_decide(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
) -> GaleMutexUnlockDecision {
    // M6a: not locked
    if owner_is_null != 0 {
        return GaleMutexUnlockDecision {
            ret: EINVAL,
            action: GALE_MUTEX_UNLOCK_ERROR,
            new_lock_count: 0,
        };
    }

    // M6b: not owner
    if owner_is_current == 0 {
        return GaleMutexUnlockDecision {
            ret: EPERM,
            action: GALE_MUTEX_UNLOCK_ERROR,
            new_lock_count: lock_count,
        };
    }

    // M7: reentrant release (lock_count > 1)
    if lock_count > 1 {
        // Verified: lock_count > 1, so lock_count - 1 >= 1, no underflow.
        #[allow(clippy::arithmetic_side_effects)]
        let new_count = lock_count - 1;
        GaleMutexUnlockDecision {
            ret: OK,
            action: GALE_MUTEX_UNLOCK_RELEASED,
            new_lock_count: new_count,
        }
    } else {
        // Fully unlocked — caller handles waiter transfer.
        GaleMutexUnlockDecision {
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_msgq_put(
    write_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    new_write_idx: *mut u32,
    new_used: *mut u32,
) -> i32 {
    unsafe {
        if new_write_idx.is_null() || new_used.is_null() || max_msgs == 0 {
            return EINVAL;
        }

        if used_msgs >= max_msgs {
            return ENOMSG;
        }

        // Advance write index with wrap.
        #[allow(clippy::arithmetic_side_effects)]
        let next = if write_idx + 1 < max_msgs {
            write_idx + 1
        } else {
            0
        };
        *new_write_idx = next;

        // Increment used count.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_used = used_msgs + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_msgq_put_front(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    new_read_idx: *mut u32,
    new_used: *mut u32,
) -> i32 {
    unsafe {
        if new_read_idx.is_null() || new_used.is_null() || max_msgs == 0 {
            return EINVAL;
        }

        if used_msgs >= max_msgs {
            return ENOMSG;
        }

        // Retreat read index with wrap.
        #[allow(clippy::arithmetic_side_effects)]
        let prev = if read_idx == 0 {
            max_msgs - 1
        } else {
            read_idx - 1
        };
        *new_read_idx = prev;

        // Increment used count.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_used = used_msgs + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_msgq_get(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    new_read_idx: *mut u32,
    new_used: *mut u32,
) -> i32 {
    unsafe {
        if new_read_idx.is_null() || new_used.is_null() || max_msgs == 0 {
            return EINVAL;
        }

        if used_msgs == 0 {
            return ENOMSG;
        }

        // Advance read index with wrap.
        #[allow(clippy::arithmetic_side_effects)]
        let next = if read_idx + 1 < max_msgs {
            read_idx + 1
        } else {
            0
        };
        *new_read_idx = next;

        // Decrement used count.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_used = used_msgs - 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_msgq_peek_at(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    idx: u32,
    slot_idx: *mut u32,
) -> i32 {
    unsafe {
        if slot_idx.is_null() || max_msgs == 0 {
            return EINVAL;
        }

        if idx >= used_msgs {
            return ENOMSG;
        }

        // Compute (read_idx + idx) % max_msgs.
        // Both values < max_msgs, so sum < 2 * max_msgs.
        #[allow(clippy::arithmetic_side_effects)]
        let sum = read_idx + idx;
        if sum < max_msgs {
            *slot_idx = sum;
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            {
                *slot_idx = sum - max_msgs;
            }
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_msgq_put_decide(
    write_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    has_waiter: u32,
    is_no_wait: u32,
) -> GaleMsgqPutDecision {
    if used_msgs < max_msgs {
        if has_waiter != 0 {
            GaleMsgqPutDecision {
                ret: OK,
                action: GALE_MSGQ_ACTION_WAKE_READER,
                new_write_idx: write_idx,
                new_used: used_msgs,
            }
        } else {
            #[allow(clippy::arithmetic_side_effects)]
            let next = if write_idx + 1 < max_msgs {
                write_idx + 1
            } else {
                0
            };
            #[allow(clippy::arithmetic_side_effects)]
            let new_used = used_msgs + 1;
            GaleMsgqPutDecision {
                ret: OK,
                action: GALE_MSGQ_ACTION_PUT_OK,
                new_write_idx: next,
                new_used,
            }
        }
    } else if is_no_wait != 0 {
        GaleMsgqPutDecision {
            ret: ENOMSG,
            action: GALE_MSGQ_ACTION_RETURN_FULL,
            new_write_idx: write_idx,
            new_used: used_msgs,
        }
    } else {
        GaleMsgqPutDecision {
            ret: 0,
            action: GALE_MSGQ_ACTION_PUT_PEND,
            new_write_idx: write_idx,
            new_used: used_msgs,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_msgq_get_decide(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    has_waiter: u32,
    is_no_wait: u32,
) -> GaleMsgqGetDecision {
    if used_msgs > 0 {
        #[allow(clippy::arithmetic_side_effects)]
        let next = if read_idx + 1 < max_msgs {
            read_idx + 1
        } else {
            0
        };
        #[allow(clippy::arithmetic_side_effects)]
        let new_used = used_msgs - 1;

        if has_waiter != 0 {
            GaleMsgqGetDecision {
                ret: OK,
                action: GALE_MSGQ_ACTION_WAKE_WRITER,
                new_read_idx: next,
                new_used,
            }
        } else {
            GaleMsgqGetDecision {
                ret: OK,
                action: GALE_MSGQ_ACTION_GET_OK,
                new_read_idx: next,
                new_used,
            }
        }
    } else if is_no_wait != 0 {
        GaleMsgqGetDecision {
            ret: ENOMSG,
            action: GALE_MSGQ_ACTION_RETURN_EMPTY,
            new_read_idx: read_idx,
            new_used: 0,
        }
    } else {
        GaleMsgqGetDecision {
            ret: 0,
            action: GALE_MSGQ_ACTION_GET_PEND,
            new_read_idx: read_idx,
            new_used: 0,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_pipe_write_check(
    used: u32,
    size: u32,
    flags: u8,
    request_len: u32,
    actual_len: *mut u32,
    new_used: *mut u32,
) -> i32 {
    unsafe {
        if actual_len.is_null() || new_used.is_null() || size == 0 {
            return EINVAL;
        }

        if (flags & PIPE_FLAG_RESET) != 0 {
            return ECANCELED;
        }
        if (flags & PIPE_FLAG_OPEN) == 0 {
            return EPIPE;
        }
        if request_len == 0 {
            return ENOMSG;
        }

        if used >= size {
            return EAGAIN;
        }

        #[allow(clippy::arithmetic_side_effects)]
        let free = size - used;
        let n = if request_len <= free { request_len } else { free };
        *actual_len = n;
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_used = used + n;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_pipe_read_check(
    used: u32,
    flags: u8,
    request_len: u32,
    actual_len: *mut u32,
    new_used: *mut u32,
) -> i32 {
    unsafe {
        if actual_len.is_null() || new_used.is_null() {
            return EINVAL;
        }

        if (flags & PIPE_FLAG_RESET) != 0 {
            return ECANCELED;
        }
        if request_len == 0 {
            return ENOMSG;
        }
        if used == 0 {
            if (flags & PIPE_FLAG_OPEN) == 0 {
                return EPIPE;
            }
            return EAGAIN;
        }

        let n = if request_len <= used { request_len } else { used };
        *actual_len = n;
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_used = used - n;
        }
        OK
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
/// Verified: PP3-PP5, PP9-PP10
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_pipe_write_decide(
    used: u32,
    size: u32,
    flags: u8,
    request_len: u32,
    has_reader: u32,
) -> GalePipeWriteDecision {
    if (flags & PIPE_FLAG_RESET) != 0 {
        return GalePipeWriteDecision {
            ret: ECANCELED,
            action: GALE_PIPE_ACTION_WRITE_ERROR,
            actual_bytes: 0,
            new_used: used,
        };
    }
    if (flags & PIPE_FLAG_OPEN) == 0 {
        return GalePipeWriteDecision {
            ret: EPIPE,
            action: GALE_PIPE_ACTION_WRITE_ERROR,
            actual_bytes: 0,
            new_used: used,
        };
    }
    if used == 0 && has_reader != 0 {
        let actual = if size > 0 && request_len > 0 {
            if request_len <= size { request_len } else { size }
        } else {
            0
        };
        return GalePipeWriteDecision {
            ret: OK,
            action: GALE_PIPE_ACTION_WAKE_READER,
            actual_bytes: actual,
            new_used: 0,
        };
    }
    if size == 0 || used >= size {
        return GalePipeWriteDecision {
            ret: OK,
            action: GALE_PIPE_ACTION_WRITE_PEND,
            actual_bytes: 0,
            new_used: used,
        };
    }
    #[allow(clippy::arithmetic_side_effects)]
    let free = size - used;
    let n = if request_len <= free { request_len } else { free };
    #[allow(clippy::arithmetic_side_effects)]
    let nu = used + n;
    GalePipeWriteDecision {
        ret: OK,
        action: GALE_PIPE_ACTION_WRITE_OK,
        actual_bytes: n,
        new_used: nu,
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
/// Verified: PP3-PP6, PP9-PP10
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_pipe_read_decide(
    used: u32,
    size: u32,
    flags: u8,
    request_len: u32,
    has_writer: u32,
) -> GalePipeReadDecision {
    if (flags & PIPE_FLAG_RESET) != 0 {
        return GalePipeReadDecision {
            ret: ECANCELED,
            action: GALE_PIPE_ACTION_READ_ERROR,
            actual_bytes: 0,
            new_used: used,
        };
    }
    if used >= size && has_writer != 0 {
        // Buffer full (or zero-size pipe): wake writers so they can
        // copy data directly to us.  For zero-size pipes size==0 and
        // used==0, so this path is the only way to trigger the writer.
        let n = if request_len <= used { request_len } else { used };
        #[allow(clippy::arithmetic_side_effects)]
        let nu = used - n;
        return GalePipeReadDecision {
            ret: OK,
            action: GALE_PIPE_ACTION_WAKE_WRITER,
            actual_bytes: n,
            new_used: nu,
        };
    }
    if used > 0 {
        let n = if request_len <= used { request_len } else { used };
        #[allow(clippy::arithmetic_side_effects)]
        let nu = used - n;
        return GalePipeReadDecision {
            ret: OK,
            action: GALE_PIPE_ACTION_READ_OK,
            actual_bytes: n,
            new_used: nu,
        };
    }
    if (flags & PIPE_FLAG_OPEN) == 0 {
        return GalePipeReadDecision {
            ret: EPIPE,
            action: GALE_PIPE_ACTION_READ_ERROR,
            actual_bytes: 0,
            new_used: 0,
        };
    }
    GalePipeReadDecision {
        ret: OK,
        action: GALE_PIPE_ACTION_READ_PEND,
        actual_bytes: 0,
        new_used: 0,
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_stack_push_validate(
    count: u32,
    capacity: u32,
    new_count: *mut u32,
) -> i32 {
    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        if count >= capacity {
            return ENOMEM;
        }

        // Verified: count < capacity <= u32::MAX, no overflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_count = count + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_stack_pop_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        if count == 0 {
            return EBUSY;
        }

        // Verified: count > 0, no underflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_count = count - 1;
        }
        OK
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
/// Verified: SK1 (bounds), SK3 (increment), SK4 (-ENOMEM).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_stack_push_decide(
    count: u32,
    capacity: u32,
    has_waiter: u32,
) -> GaleStackPushDecision {
    if has_waiter != 0 {
        // Waiter exists: give data directly to waiting thread (count unchanged)
        GaleStackPushDecision {
            ret: OK,
            new_count: count,
            action: GALE_STACK_PUSH_WAKE,
        }
    } else if count < capacity {
        // Space available: store data, increment count
        #[allow(clippy::arithmetic_side_effects)]
        let new_count = count + 1;
        GaleStackPushDecision {
            ret: OK,
            new_count,
            action: GALE_STACK_PUSH_STORE,
        }
    } else {
        // Full: reject
        GaleStackPushDecision {
            ret: ENOMEM,
            new_count: count,
            action: GALE_STACK_PUSH_FULL,
        }
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
/// Verified: SK1 (bounds), SK5 (decrement), SK6 (-EBUSY).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_stack_pop_decide(
    count: u32,
    is_no_wait: u32,
) -> GaleStackPopDecision {
    if count > 0 {
        #[allow(clippy::arithmetic_side_effects)]
        let new_count = count - 1;
        GaleStackPopDecision {
            ret: OK,
            new_count,
            action: GALE_STACK_POP_OK,
        }
    } else if is_no_wait != 0 {
        GaleStackPopDecision {
            ret: EBUSY,
            new_count: 0,
            action: GALE_STACK_POP_OK,
        }
    } else {
        GaleStackPopDecision {
            ret: 0,
            new_count: 0,
            action: GALE_STACK_POP_PEND,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_timer_expire(
    status: u32,
    new_status: *mut u32,
) -> i32 {
    unsafe {
        if new_status.is_null() {
            return EINVAL;
        }

        if status == u32::MAX {
            return EOVERFLOW;
        }

        // Verified: status < u32::MAX, no overflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_status = status + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_timer_status_get(
    status: u32,
    new_status: *mut u32,
) -> u32 {
    unsafe {
        if !new_status.is_null() {
            *new_status = 0;
        }
        status
    }
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
/// Verified: TM5 (increment), TM8 (no overflow — saturates at u32::MAX).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_timer_expire_decide(
    status: u32,
    period: u32,
) -> GaleTimerExpireDecision {
    let new_status = if status < u32::MAX {
        #[allow(clippy::arithmetic_side_effects)]
        { status + 1 }
    } else {
        // Saturate at u32::MAX — no overflow.
        status
    };
    GaleTimerExpireDecision {
        new_status,
        is_periodic: if period > 0 { 1 } else { 0 },
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_mem_slab_alloc_validate(
    num_used: u32,
    num_blocks: u32,
    new_num_used: *mut u32,
) -> i32 {
    unsafe {
        if new_num_used.is_null() {
            return EINVAL;
        }

        if num_used >= num_blocks {
            return ENOMEM;
        }

        // Verified: num_used < num_blocks <= u32::MAX, no overflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_num_used = num_used + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_mem_slab_free_validate(
    num_used: u32,
    new_num_used: *mut u32,
) -> i32 {
    unsafe {
        if new_num_used.is_null() {
            return EINVAL;
        }

        if num_used == 0 {
            return EINVAL;
        }

        // Verified: num_used > 0, no underflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_num_used = num_used - 1;
        }
        OK
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
/// Verified: MS4 (increment), MS5 (-ENOMEM), MS1 (bounds).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mem_slab_alloc_decide(
    num_used: u32,
    num_blocks: u32,
    is_no_wait: u32,
) -> GaleMemSlabAllocDecision {
    if num_used < num_blocks {
        #[allow(clippy::arithmetic_side_effects)]
        let new_num_used = num_used + 1;
        GaleMemSlabAllocDecision {
            ret: OK,
            new_num_used,
            action: GALE_MEM_SLAB_ACTION_ALLOC_OK,
        }
    } else if is_no_wait != 0 {
        GaleMemSlabAllocDecision {
            ret: ENOMEM,
            new_num_used: num_used,
            action: GALE_MEM_SLAB_ACTION_RETURN_NOMEM,
        }
    } else {
        GaleMemSlabAllocDecision {
            ret: 0,
            new_num_used: num_used,
            action: GALE_MEM_SLAB_ACTION_PEND_CURRENT,
        }
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
/// Verified: MS6 (decrement), MS1 (bounds).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_mem_slab_free_decide(
    num_used: u32,
    has_waiter: u32,
) -> GaleMemSlabFreeDecision {
    if has_waiter != 0 {
        // Don't decrement — the block goes directly to the woken thread,
        // so the allocation count stays the same.
        GaleMemSlabFreeDecision {
            new_num_used: num_used,
            action: GALE_MEM_SLAB_ACTION_WAKE_THREAD,
        }
    } else if num_used > 0 {
        #[allow(clippy::arithmetic_side_effects)]
        let new_num_used = num_used - 1;
        GaleMemSlabFreeDecision {
            new_num_used,
            action: GALE_MEM_SLAB_ACTION_FREE_OK,
        }
    } else {
        // All blocks already free — should not happen with valid usage.
        // Return unchanged count with FREE_OK action (no-op).
        GaleMemSlabFreeDecision {
            new_num_used: 0,
            action: GALE_MEM_SLAB_ACTION_FREE_OK,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_event_post(
    events: u32,
    new_events: u32,
    result: *mut u32,
) -> i32 {
    unsafe {
        if result.is_null() {
            return EINVAL;
        }

        *result = events | new_events;
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_event_clear(
    events: u32,
    clear_bits: u32,
    result: *mut u32,
) -> i32 {
    unsafe {
        if result.is_null() {
            return EINVAL;
        }

        *result = events & !clear_bits;
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_event_set_masked(
    events: u32,
    new_bits: u32,
    mask: u32,
    result: *mut u32,
) -> i32 {
    unsafe {
        if result.is_null() {
            return EINVAL;
        }

        *result = (events & !mask) | (new_bits & mask);
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_event_wait_check_any(
    events: u32,
    desired: u32,
) -> i32 {
    if (events & desired) != 0 {
        1
    } else {
        0
    }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_event_wait_check_all(
    events: u32,
    desired: u32,
) -> i32 {
    if (events & desired) == desired {
        1
    } else {
        0
    }
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
/// Verified: EV4 — set_masked computes (current & ~mask) | (new & mask)
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_event_post_decide(
    current_events: u32,
    new_events: u32,
    mask: u32,
) -> GaleEventPostDecision {
    GaleEventPostDecision {
        new_events: (current_events & !mask) | (new_events & mask),
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
/// Verified: EV5 (any-bit match), EV6 (all-bits match)
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_event_wait_decide(
    current_events: u32,
    desired: u32,
    wait_type: u8,
    is_no_wait: u32,
) -> GaleEventWaitDecision {
    let matched = current_events & desired;

    let condition_met = if wait_type == GALE_EVENT_WAIT_ALL {
        // ALL: every desired bit must be present
        (current_events & desired) == desired
    } else {
        // ANY: at least one desired bit present
        matched != 0
    };

    if condition_met {
        GaleEventWaitDecision {
            ret: 0,
            matched_events: matched,
            action: GALE_EVENT_ACTION_MATCHED,
        }
    } else if is_no_wait != 0 {
        GaleEventWaitDecision {
            ret: 0,
            matched_events: 0,
            action: GALE_EVENT_ACTION_TIMEOUT,
        }
    } else {
        GaleEventWaitDecision {
            ret: 0,
            matched_events: 0,
            action: GALE_EVENT_ACTION_PEND,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_fifo_put_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        if count >= u32::MAX - 1 {
            return EOVERFLOW;
        }

        // Verified: count < u32::MAX - 1, no overflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_count = count + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_fifo_get_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        if count == 0 {
            return EAGAIN;
        }

        // Verified: count > 0, no underflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_count = count - 1;
        }
        OK
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
/// Verified: FI1 (no overflow), FI2 (increment via PUT_OK path).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_fifo_put_decide(
    _count: u32,
    has_waiter: u32,
) -> GaleFifoPutDecision {
    if has_waiter != 0 {
        GaleFifoPutDecision {
            action: GALE_FIFO_PUT_WAKE,
        }
    } else {
        GaleFifoPutDecision {
            action: GALE_FIFO_PUT_OK,
        }
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
/// Verified: FI3 (no underflow), FI4 (decrement via GET_OK path).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_fifo_get_decide(
    count: u32,
    is_no_wait: u32,
) -> GaleFifoGetDecision {
    if count > 0 {
        GaleFifoGetDecision {
            ret: OK,
            action: GALE_FIFO_GET_OK,
        }
    } else if is_no_wait != 0 {
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_lifo_put_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        if count >= u32::MAX - 1 {
            return EOVERFLOW;
        }

        // Verified: count < u32::MAX - 1, no overflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_count = count + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_lifo_get_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        if count == 0 {
            return EAGAIN;
        }

        // Verified: count > 0, no underflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_count = count - 1;
        }
        OK
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
/// Verified: LI1 (no overflow), LI2 (increment via PUT_OK path).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_lifo_put_decide(
    _count: u32,
    has_waiter: u32,
) -> GaleLifoPutDecision {
    if has_waiter != 0 {
        GaleLifoPutDecision {
            action: GALE_LIFO_PUT_WAKE,
        }
    } else {
        GaleLifoPutDecision {
            action: GALE_LIFO_PUT_OK,
        }
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
/// Verified: LI3 (no underflow), LI4 (decrement via GET_OK path).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_lifo_get_decide(
    count: u32,
    is_no_wait: u32,
) -> GaleLifoGetDecision {
    if count > 0 {
        GaleLifoGetDecision {
            ret: OK,
            action: GALE_LIFO_GET_OK,
        }
    } else if is_no_wait != 0 {
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_queue_append_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        if count >= u32::MAX - 1 {
            return EOVERFLOW;
        }

        // Verified: count < u32::MAX - 1, no overflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_count = count + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_queue_prepend_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        if count >= u32::MAX - 1 {
            return EOVERFLOW;
        }

        // Verified: count < u32::MAX - 1, no overflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_count = count + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_queue_get_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

        if count == 0 {
            return EAGAIN;
        }

        // Verified: count > 0, no underflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_count = count - 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_mbox_validate_send(size: u32) -> i32 {
    if size == 0 {
        EINVAL
    } else {
        OK
    }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_mbox_match_check(send_id: u32, recv_id: u32) -> i32 {
    if send_id == 0 || recv_id == 0 || send_id == recv_id {
        1
    } else {
        0
    }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_mbox_data_exchange(tx_size: u32, rx_buf_size: u32) -> u32 {
    if tx_size < rx_buf_size {
        tx_size
    } else {
        rx_buf_size
    }
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
/// Verified: QU1/QU2 (append), QU3/QU4 (prepend) — state transition only.
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_queue_insert_decide(
    has_waiter: u32,
) -> GaleQueueInsertDecision {
    if has_waiter != 0 {
        GaleQueueInsertDecision {
            action: GALE_QUEUE_ACTION_WAKE,
        }
    } else {
        GaleQueueInsertDecision {
            action: GALE_QUEUE_ACTION_INSERT,
        }
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
/// Verified: QU5/QU6 — state transition only.
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_queue_get_decide(
    has_data: u32,
    is_no_wait: u32,
) -> GaleQueueGetDecision {
    if has_data != 0 {
        GaleQueueGetDecision {
            action: GALE_QUEUE_ACTION_DEQUEUE,
        }
    } else if is_no_wait != 0 {
        GaleQueueGetDecision {
            action: GALE_QUEUE_ACTION_RETURN_NULL,
        }
    } else {
        GaleQueueGetDecision {
            action: GALE_QUEUE_ACTION_PEND,
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeout_add_decide(
    current_tick: u64,
    duration: u64,
) -> GaleTimeoutAddDecision {
    if current_tick >= K_FOREVER_TICKS {
        return GaleTimeoutAddDecision {
            ret: EINVAL,
            deadline: 0,
        };
    }

    if duration >= K_FOREVER_TICKS - current_tick {
        return GaleTimeoutAddDecision {
            ret: EINVAL,
            deadline: 0,
        };
    }

    #[allow(clippy::arithmetic_side_effects)]
    let dl = current_tick + duration;
    GaleTimeoutAddDecision {
        ret: OK,
        deadline: dl,
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeout_abort_decide(
    is_linked: u32,
) -> GaleTimeoutAbortDecision {
    if is_linked != 0 {
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeout_announce_decide(
    current_tick: u64,
    ticks: u64,
    deadline: u64,
    active: u32,
) -> GaleTimeoutAnnounceDecision {
    if ticks >= K_FOREVER_TICKS - current_tick {
        return GaleTimeoutAnnounceDecision {
            ret: EINVAL,
            new_tick: 0,
            fired: 0,
        };
    }

    #[allow(clippy::arithmetic_side_effects)]
    let advanced = current_tick + ticks;

    let fired = if active != 0
        && deadline != K_FOREVER_TICKS
        && deadline <= advanced
    {
        1u32
    } else {
        0u32
    };

    GaleTimeoutAnnounceDecision {
        ret: OK,
        new_tick: advanced,
        fired,
    }
}

// ---- Legacy API (kept for backward compatibility) ----

/// Schedule a timeout: compute absolute deadline from current tick + duration.
///
/// Delegates to gale_timeout_add_decide.
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeout_abort(active: u32) -> i32 {
    gale_timeout_abort_decide(active).ret
}

/// Advance tick and check if a timeout has expired.
///
/// Delegates to gale_timeout_announce_decide.
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_poll_check_sem(
    event_type: u32,
    sem_count: u32,
) -> i32 {
    const TYPE_SEM_AVAILABLE: u32 = 1;
    if event_type == TYPE_SEM_AVAILABLE && sem_count > 0 {
        1
    } else {
        0
    }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_poll_signal_raise(
    signaled: *mut u32,
    result: *mut i32,
    result_val: i32,
) -> i32 {
    unsafe {
        if signaled.is_null() || result.is_null() {
            return EINVAL;
        }

        *result = result_val;
        *signaled = 1;
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_poll_signal_raise_decide(
    signaled: u32,
    result_val: i32,
    has_poll_event: u32,
) -> GalePollSignalRaiseDecision {
    // Regardless of prior signaled state, raise always sets signaled=1
    // and stores the result value.
    let _ = signaled;
    if has_poll_event != 0 {
        GalePollSignalRaiseDecision {
            new_signaled: 1,
            new_result: result_val,
            action: GALE_POLL_ACTION_SIGNAL_EVENT,
        }
    } else {
        GalePollSignalRaiseDecision {
            new_signaled: 1,
            new_result: result_val,
            action: GALE_POLL_ACTION_NO_EVENT,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_futex_wait_check(val: u32, expected: u32) -> i32 {
    if val == expected {
        OK
    } else {
        EAGAIN
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_futex_wake(
    num_waiters: u32,
    wake_all: u32,
    woken: *mut u32,
    remaining: *mut u32,
) -> i32 {
    unsafe {
        if woken.is_null() || remaining.is_null() {
            return EINVAL;
        }

        if num_waiters == 0 {
            *woken = 0;
            *remaining = 0;
        } else if wake_all != 0 {
            *woken = num_waiters;
            *remaining = 0;
        } else {
            *woken = 1;
            #[allow(clippy::arithmetic_side_effects)]
            {
                *remaining = num_waiters - 1;
            }
        }
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
/// Verified: FX1 (block when val == expected), FX2 (EAGAIN on mismatch).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_futex_wait_decide(
    val: u32,
    expected: u32,
    is_no_wait: u32,
) -> GaleFutexWaitDecision {
    if val != expected {
        GaleFutexWaitDecision {
            action: GALE_FUTEX_ACTION_RETURN_EAGAIN,
            ret: EAGAIN,
        }
    } else if is_no_wait != 0 {
        // Value matches but caller specified K_NO_WAIT — cannot block
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
/// Verified: FX3 (wake count correct), FX4 (wake_all wakes all), FX5 (single wake).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_futex_wake_decide(
    num_waiters: u32,
    wake_all: u32,
) -> GaleFutexWakeDecision {
    if wake_all != 0 {
        GaleFutexWakeDecision {
            wake_limit: num_waiters,
        }
    } else if num_waiters > 0 {
        GaleFutexWakeDecision {
            wake_limit: 1,
        }
    } else {
        GaleFutexWakeDecision {
            wake_limit: 0,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeslice_reset(
    slice_max_ticks: u32,
    new_ticks: *mut u32,
) -> i32 {
    unsafe {
        if new_ticks.is_null() {
            return EINVAL;
        }

        *new_ticks = slice_max_ticks;
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_timeslice_tick(
    slice_ticks: u32,
    new_ticks: *mut u32,
    expired: *mut u32,
) -> i32 {
    unsafe {
        if new_ticks.is_null() || expired.is_null() {
            return EINVAL;
        }

        if slice_ticks > 0 {
            #[allow(clippy::arithmetic_side_effects)]
            let decremented = slice_ticks - 1;
            *new_ticks = decremented;
            *expired = if decremented == 0 { 1 } else { 0 };
        } else {
            *new_ticks = 0;
            *expired = 1;
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_timeslice_tick_decide(
    ticks_remaining: u32,
    slice_ticks: u32,
    is_cooperative: u32,
) -> GaleTimesliceTickDecision {
    // No time slicing configured for this thread
    if slice_ticks == 0 {
        return GaleTimesliceTickDecision {
            action: GALE_TIMESLICE_ACTION_NO_YIELD,
            new_ticks: ticks_remaining,
        };
    }

    // Cooperative threads never yield to timeslicing
    if is_cooperative != 0 {
        return GaleTimesliceTickDecision {
            action: GALE_TIMESLICE_ACTION_NO_YIELD,
            new_ticks: ticks_remaining,
        };
    }

    // Slice expired — yield and reset
    if ticks_remaining == 0 {
        GaleTimesliceTickDecision {
            action: GALE_TIMESLICE_ACTION_YIELD,
            new_ticks: slice_ticks,
        }
    } else {
        GaleTimesliceTickDecision {
            action: GALE_TIMESLICE_ACTION_NO_YIELD,
            new_ticks: ticks_remaining,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_kheap_alloc_validate(
    allocated_bytes: u32,
    capacity: u32,
    bytes: u32,
    new_allocated: *mut u32,
) -> i32 {
    unsafe {
        if new_allocated.is_null() || bytes == 0 {
            return EINVAL;
        }

        #[allow(clippy::arithmetic_side_effects)]
        let remaining = capacity - allocated_bytes.min(capacity);
        if bytes <= remaining {
            #[allow(clippy::arithmetic_side_effects)]
            {
                *new_allocated = allocated_bytes + bytes;
            }
            OK
        } else {
            ENOMEM
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_kheap_free_validate(
    allocated_bytes: u32,
    bytes: u32,
    new_allocated: *mut u32,
) -> i32 {
    unsafe {
        if new_allocated.is_null() || bytes == 0 {
            return EINVAL;
        }

        if bytes <= allocated_bytes {
            #[allow(clippy::arithmetic_side_effects)]
            {
                *new_allocated = allocated_bytes - bytes;
            }
            OK
        } else {
            EINVAL
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

const MAX_THREADS: u32 = 256;
const MAX_PRIORITY: u32 = 32;

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
#[unsafe(no_mangle)]
pub extern "C" fn gale_thread_create_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

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
#[unsafe(no_mangle)]
pub extern "C" fn gale_thread_exit_validate(
    count: u32,
    new_count: *mut u32,
) -> i32 {
    unsafe {
        if new_count.is_null() {
            return EINVAL;
        }

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
#[unsafe(no_mangle)]
pub extern "C" fn gale_thread_priority_validate(priority: u32) -> i32 {
    if priority < MAX_PRIORITY {
        OK
    } else {
        EINVAL
    }
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
/// Verified: TH1 (priority range), TH3 (stack_size > 0), TH6 (no overflow).
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_thread_create_decide(
    stack_size: u32,
    priority: u32,
    _options: u32,
    active_count: u32,
) -> GaleThreadCreateDecision {
    // TH3: stack must have nonzero, minimum size
    if stack_size < MIN_STACK_SIZE {
        return GaleThreadCreateDecision {
            action: GALE_THREAD_ACTION_REJECT,
            ret: EINVAL,
        };
    }

    // TH1: priority must be in valid range
    if priority >= MAX_PRIORITY {
        return GaleThreadCreateDecision {
            action: GALE_THREAD_ACTION_REJECT,
            ret: EINVAL,
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_work_submit_decide(
    flags: u8,
    is_queued: u8,
    is_running: u8,
) -> GaleWorkSubmitDecision {
    // If canceling, reject the submission
    if (flags & WORK_FLAG_CANCELING) != 0 {
        return GaleWorkSubmitDecision {
            action: GALE_WORK_SUBMIT_REJECT,
            new_flags: flags,
            ret: EBUSY,
        };
    }

    // If already queued, no-op
    if is_queued != 0 {
        return GaleWorkSubmitDecision {
            action: GALE_WORK_SUBMIT_ALREADY,
            new_flags: flags,
            ret: 0,
        };
    }

    // Not queued -- set QUEUED flag
    #[allow(clippy::arithmetic_side_effects)]
    let new_flags = flags | WORK_FLAG_QUEUED;

    if is_running != 0 {
        // Was running -- re-queue to the same queue
        GaleWorkSubmitDecision {
            action: GALE_WORK_SUBMIT_REQUEUE,
            new_flags,
            ret: 2,
        }
    } else {
        // Idle -- newly queued
        GaleWorkSubmitDecision {
            action: GALE_WORK_SUBMIT_QUEUE,
            new_flags,
            ret: 1,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_work_cancel_decide(
    flags: u8,
    is_queued: u8,
    is_running: u8,
) -> GaleWorkCancelDecision {
    let _ = is_running; // used implicitly via flags

    // Step 1: If not already canceling, clear QUEUED
    let dequeued = (flags & WORK_FLAG_CANCELING) == 0 && is_queued != 0;

    #[allow(clippy::arithmetic_side_effects)]
    let mut f = if dequeued {
        flags & !WORK_FLAG_QUEUED
    } else {
        flags
    };

    // Step 2: Check busy status after dequeue
    #[allow(clippy::arithmetic_side_effects)]
    let busy = f & WORK_BUSY_MASK;

    // Step 3: If still busy, set CANCELING
    if busy != 0 {
        #[allow(clippy::arithmetic_side_effects)]
        {
            f |= WORK_FLAG_CANCELING;
        }
    }

    let action = if busy == 0 {
        GALE_WORK_CANCEL_IDLE
    } else if dequeued {
        GALE_WORK_CANCEL_DEQUEUE
    } else {
        GALE_WORK_CANCEL_CANCELING
    };

    GaleWorkCancelDecision {
        action,
        new_flags: f,
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_k_fatal_decide(
    reason: u32,
    is_isr: u32,
    test_mode: u32,
) -> GaleFatalDecision {
    // Validate reason code
    if reason > 4 {
        return GaleFatalDecision {
            action: GALE_FATAL_ACTION_HALT,
            ret: EINVAL,
        };
    }

    let action = if test_mode != 0 {
        // Test mode — more permissive
        if is_isr != 0 {
            if reason == 2 {
                // STACK_CHECK_FAIL — abort even in ISR
                GALE_FATAL_ACTION_ABORT_THREAD
            } else {
                GALE_FATAL_ACTION_IGNORE
            }
        } else {
            GALE_FATAL_ACTION_ABORT_THREAD
        }
    } else {
        // Production mode
        if reason == 4 {
            // KERNEL_PANIC — always halt
            GALE_FATAL_ACTION_HALT
        } else if reason == 2 {
            // STACK_CHECK_FAIL — always abort thread
            GALE_FATAL_ACTION_ABORT_THREAD
        } else if is_isr != 0 {
            // ISR context — halt
            GALE_FATAL_ACTION_HALT
        } else {
            // Thread context — abort
            GALE_FATAL_ACTION_ABORT_THREAD
        }
    };

    GaleFatalDecision { action, ret: 0 }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_mempool_alloc_validate(
    allocated: u32,
    capacity: u32,
    new_allocated: *mut u32,
) -> i32 {
    unsafe {
        if new_allocated.is_null() {
            return EINVAL;
        }

        if allocated >= capacity {
            return ENOMEM;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_allocated = allocated + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_mempool_free_validate(
    allocated: u32,
    new_allocated: *mut u32,
) -> i32 {
    unsafe {
        if new_allocated.is_null() {
            return EINVAL;
        }

        if allocated == 0 {
            return EINVAL;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_allocated = allocated - 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_dynamic_alloc_validate(
    active: u32,
    max_threads: u32,
    new_active: *mut u32,
) -> i32 {
    unsafe {
        if new_active.is_null() {
            return EINVAL;
        }

        if active >= max_threads {
            return ENOMEM;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_active = active + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_dynamic_free_validate(
    active: u32,
    new_active: *mut u32,
) -> i32 {
    unsafe {
        if new_active.is_null() {
            return EINVAL;
        }

        if active == 0 {
            return EINVAL;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_active = active - 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_smp_start_cpu_validate(
    active_cpus: u32,
    max_cpus: u32,
    new_active: *mut u32,
) -> i32 {
    unsafe {
        if new_active.is_null() {
            return EINVAL;
        }

        if active_cpus >= max_cpus {
            return EBUSY;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_active = active_cpus + 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_smp_stop_cpu_validate(
    active_cpus: u32,
    new_active: *mut u32,
) -> i32 {
    unsafe {
        if new_active.is_null() {
            return EINVAL;
        }

        if active_cpus <= 1 {
            return EINVAL;
        }

        #[allow(clippy::arithmetic_side_effects)]
        {
            *new_active = active_cpus - 1;
        }
        OK
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_dynamic_alloc_decide(
    active: u32,
    max_threads: u32,
) -> GaleDynamicAllocDecision {
    if active >= max_threads {
        GaleDynamicAllocDecision {
            action: GALE_DYNAMIC_ACTION_POOL_FULL,
            new_active: active,
        }
    } else {
        #[allow(clippy::arithmetic_side_effects)]
        let new_active = active + 1;
        GaleDynamicAllocDecision {
            action: GALE_DYNAMIC_ACTION_ALLOC_OK,
            new_active,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_dynamic_free_decide(
    active: u32,
) -> GaleDynamicFreeDecision {
    if active == 0 {
        GaleDynamicFreeDecision {
            action: GALE_DYNAMIC_ACTION_UNDERFLOW,
            new_active: 0,
        }
    } else {
        #[allow(clippy::arithmetic_side_effects)]
        let new_active = active - 1;
        GaleDynamicFreeDecision {
            action: GALE_DYNAMIC_ACTION_FREE_OK,
            new_active,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_smp_start_cpu_decide(
    active_cpus: u32,
    max_cpus: u32,
) -> GaleSmpStartDecision {
    if active_cpus >= max_cpus {
        GaleSmpStartDecision {
            action: GALE_SMP_ACTION_ALL_ACTIVE,
            new_active: active_cpus,
        }
    } else {
        #[allow(clippy::arithmetic_side_effects)]
        let new_active = active_cpus + 1;
        GaleSmpStartDecision {
            action: GALE_SMP_ACTION_START_OK,
            new_active,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_smp_stop_cpu_decide(
    active_cpus: u32,
) -> GaleSmpStopDecision {
    if active_cpus <= 1 {
        GaleSmpStopDecision {
            action: GALE_SMP_ACTION_LAST_CPU,
            new_active: active_cpus,
        }
    } else {
        #[allow(clippy::arithmetic_side_effects)]
        let new_active = active_cpus - 1;
        GaleSmpStopDecision {
            action: GALE_SMP_ACTION_STOP_OK,
            new_active,
        }
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
#[unsafe(no_mangle)]
pub extern "C" fn gale_sched_should_preempt(
    current_is_cooperative: u32,
    candidate_is_metairq: u32,
    swap_ok: u32,
) -> i32 {
    if swap_ok != 0 {
        return 1;
    }
    if current_is_cooperative != 0 && candidate_is_metairq == 0 {
        return 0;
    }
    1
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

#[cfg(kani)]
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

#[cfg(kani)]
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

#[cfg(kani)]
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

#[cfg(kani)]
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

#[cfg(kani)]
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

#[cfg(kani)]
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

#[cfg(kani)]
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

#[cfg(kani)]
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

#[cfg(kani)]
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
        kani::assume((current & desired) == 0);
        let d = gale_k_event_wait_decide(current, desired, wait_type, 1);
        assert!(d.action == GALE_EVENT_ACTION_TIMEOUT);
        assert!(d.matched_events == 0);
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — fifo
// ---------------------------------------------------------------------------

#[cfg(kani)]
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

#[cfg(kani)]
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

#[cfg(kani)]
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

#[cfg(kani)]
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

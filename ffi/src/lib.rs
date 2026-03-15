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

#![cfg_attr(not(any(test, kani)), no_std)]
// FFI boundary crate — unsafe is inherent (no_mangle, raw pointers).
// The verified pure logic lives in the `gale` crate which denies unsafe.

pub mod coarse;

use gale::error::{EAGAIN, EBUSY, ECANCELED, EINVAL, ENOMEM, ENOMSG, EOVERFLOW, EPERM, EPIPE, OK};

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

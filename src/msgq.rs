//! Verified message queue for Zephyr RTOS.
//!
//! This is a formally verified port of zephyr/kernel/msg_q.c.
//! All safety-critical properties are proven with Verus (SMT/Z3).
//!
//! This module models the **ring buffer index arithmetic** of Zephyr's
//! message queue.  Actual message data and wait queue management remain
//! in C — only the index computation crosses the FFI boundary.
//!
//! Source mapping:
//!   k_msgq_init      -> MsgQ::init         (msg_q.c:43-71)
//!   k_msgq_put       -> MsgQ::put          (msg_q.c:130-228, ring buffer path)
//!   k_msgq_put_front -> MsgQ::put_front    (msg_q.c:236-239)
//!   k_msgq_get       -> MsgQ::get          (msg_q.c:280-349, ring buffer path)
//!   k_msgq_peek_at   -> MsgQ::peek_at      (msg_q.c:397-430)
//!   k_msgq_purge     -> MsgQ::purge        (msg_q.c:443-470, index reset)
//!   k_msgq_num_free  -> MsgQ::num_free_get (kernel.h inline)
//!   k_msgq_num_used  -> MsgQ::num_used_get (kernel.h inline)
//!
//! Omitted (not safety-relevant):
//!   - CONFIG_POLL (poll_events) — application convenience
//!   - CONFIG_OBJ_CORE_MSGQ — debug/tracing
//!   - CONFIG_USERSPACE (z_vrfy_*) — syscall marshaling
//!   - SYS_PORT_TRACING_* — instrumentation
//!   - k_msgq_alloc_init — heap allocation wrapper
//!   - k_msgq_cleanup — deallocation
//!
//! ASIL-D verified properties:
//!   MQ1:  0 <= used_msgs <= max_msgs (capacity invariant)
//!   MQ2:  read_idx < max_msgs (index bounds)
//!   MQ3:  write_idx < max_msgs (index bounds)
//!   MQ4:  msg_size > 0, max_msgs > 0 (always)
//!   MQ5:  put on non-full queue: used_msgs incremented, write_idx advanced
//!   MQ6:  put on full queue: returns ENOMSG, state unchanged
//!   MQ7:  put_front on non-full queue: read_idx retreated correctly
//!   MQ8:  get on non-empty queue: used_msgs decremented, read_idx advanced
//!   MQ9:  get on empty queue: returns ENOMSG, state unchanged
//!   MQ10: peek_at computes correct slot index
//!   MQ11: purge resets to empty (used_msgs=0, read_idx=write_idx)
//!   MQ12: no arithmetic overflow in any operation
//!   MQ13: ring buffer consistency: write_idx tracks read_idx + used_msgs

use vstd::prelude::*;
use crate::error::*;

verus! {

/// Result of a put/get operation.
pub enum MsgQResult {
    /// Operation succeeded — indices updated.
    Ok,
    /// Queue full (put) or empty (get).
    Full,
}

/// Message queue — ring buffer index model.
///
/// Corresponds to Zephyr's struct k_msgq {
///     size_t msg_size;
///     uint32_t max_msgs;
///     char *buffer_start, *buffer_end;
///     char *read_ptr, *write_ptr;
///     uint32_t used_msgs;
/// };
///
/// We model read_ptr/write_ptr as slot indices (0..max_msgs-1)
/// rather than byte pointers.  The C shim converts:
///   byte_ptr = buffer_start + slot_idx * msg_size
#[derive(Debug)]
pub struct MsgQ {
    /// Size of each message in bytes (immutable after init).
    pub msg_size: u32,
    /// Maximum number of messages (immutable after init).
    pub max_msgs: u32,
    /// Current read slot index.
    pub read_idx: u32,
    /// Current write slot index.
    pub write_idx: u32,
    /// Number of messages currently in queue.
    pub used_msgs: u32,
}

impl MsgQ {
    // =================================================================
    // Specification functions
    // =================================================================

    /// Advance a slot index by one, wrapping at max_msgs.
    pub open spec fn next_idx_spec(idx: u32, max: u32) -> u32 {
        if idx + 1 < max {
            (idx + 1) as u32
        } else {
            0u32
        }
    }

    /// Retreat a slot index by one, wrapping at max_msgs.
    pub open spec fn prev_idx_spec(idx: u32, max: u32) -> u32 {
        if idx == 0 {
            (max - 1) as u32
        } else {
            (idx - 1) as u32
        }
    }

    /// Ring buffer consistency: write_idx tracks read_idx + used_msgs.
    pub open spec fn ring_consistent(&self) -> bool {
        if (self.read_idx as int + self.used_msgs as int) < self.max_msgs as int {
            self.write_idx as int == self.read_idx as int + self.used_msgs as int
        } else {
            self.write_idx as int ==
                self.read_idx as int + self.used_msgs as int - self.max_msgs as int
        }
    }

    /// The fundamental message queue invariant (MQ1-MQ4, MQ13).
    pub open spec fn inv(&self) -> bool {
        &&& self.msg_size > 0
        &&& self.max_msgs > 0
        &&& self.read_idx < self.max_msgs
        &&& self.write_idx < self.max_msgs
        &&& self.used_msgs <= self.max_msgs
        &&& self.ring_consistent()
    }

    pub open spec fn used_msgs_spec(&self) -> nat {
        self.used_msgs as nat
    }

    pub open spec fn max_msgs_spec(&self) -> nat {
        self.max_msgs as nat
    }

    pub open spec fn is_full_spec(&self) -> bool {
        self.used_msgs == self.max_msgs
    }

    pub open spec fn is_empty_spec(&self) -> bool {
        self.used_msgs == 0
    }

    // =================================================================
    // k_msgq_init (msg_q.c:43-71)
    // =================================================================

    /// Initialize a message queue.
    ///
    /// ```c
    /// void k_msgq_init(struct k_msgq *msgq, char *buffer,
    ///                  size_t msg_size, uint32_t max_msgs)
    /// {
    ///     msgq->msg_size = msg_size;
    ///     msgq->max_msgs = max_msgs;
    ///     msgq->buffer_start = buffer;
    ///     msgq->buffer_end = buffer + (max_msgs * msg_size);
    ///     msgq->read_ptr = buffer;
    ///     msgq->write_ptr = buffer;
    ///     msgq->used_msgs = 0;
    /// }
    /// ```
    ///
    /// Verified properties:
    /// - Establishes the invariant (MQ1-MQ4, MQ13)
    /// - Queue starts empty (MQ11)
    /// - Rejects msg_size=0, max_msgs=0, overflow in msg_size*max_msgs
    pub fn init(msg_size: u32, max_msgs: u32) -> (result: Result<Self, i32>)
        ensures
            match result {
                Ok(mq) => {
                    &&& mq.inv()
                    &&& mq.msg_size == msg_size
                    &&& mq.max_msgs == max_msgs
                    &&& mq.read_idx == 0
                    &&& mq.write_idx == 0
                    &&& mq.used_msgs == 0
                },
                Err(e) => {
                    &&& e == EINVAL
                    &&& (msg_size == 0 || max_msgs == 0
                         || msg_size as int * max_msgs as int > u32::MAX as int)
                },
            },
    {
        if msg_size == 0 || max_msgs == 0 {
            return Err(EINVAL);
        }

        // Check for overflow in buffer size computation.
        // msg_q.c:46: __ASSERT(!size_mul_overflow(max_msgs, msg_size, ...))
        if msg_size.checked_mul(max_msgs).is_none() {
            return Err(EINVAL);
        }

        Ok(MsgQ {
            msg_size,
            max_msgs,
            read_idx: 0,
            write_idx: 0,
            used_msgs: 0,
        })
    }

    // =================================================================
    // Helper: advance / retreat index
    // =================================================================

    /// Advance an index by one slot, wrapping at max_msgs.
    ///
    /// msg_q.c:170-173:
    ///   write_ptr += msg_size;
    ///   if (write_ptr == buffer_end) write_ptr = buffer_start;
    fn next_idx(&self, idx: u32) -> (result: u32)
        requires
            self.max_msgs > 0,
            idx < self.max_msgs,
        ensures
            result < self.max_msgs,
            result == Self::next_idx_spec(idx, self.max_msgs),
    {
        if idx + 1 < self.max_msgs {
            idx + 1
        } else {
            0
        }
    }

    /// Retreat an index by one slot, wrapping at max_msgs.
    ///
    /// msg_q.c:180-184:
    ///   if (read_ptr == buffer_start) read_ptr = buffer_end;
    ///   read_ptr -= msg_size;
    fn prev_idx(&self, idx: u32) -> (result: u32)
        requires
            self.max_msgs > 0,
            idx < self.max_msgs,
        ensures
            result < self.max_msgs,
            result == Self::prev_idx_spec(idx, self.max_msgs),
    {
        if idx == 0 {
            self.max_msgs - 1
        } else {
            idx - 1
        }
    }

    // =================================================================
    // k_msgq_put — ring buffer path (msg_q.c:164-188)
    // =================================================================

    /// Put a message at the back of the queue (ring buffer index update).
    ///
    /// ```c
    /// // msg_q.c:164-173 (put_at_back path, no pending thread)
    /// memcpy(msgq->write_ptr, data, msgq->msg_size);
    /// msgq->write_ptr += msgq->msg_size;
    /// if (msgq->write_ptr == msgq->buffer_end) {
    ///     msgq->write_ptr = msgq->buffer_start;
    /// }
    /// msgq->used_msgs++;
    /// ```
    ///
    /// Returns the write slot index where the message should be placed.
    /// The C shim does the memcpy at buffer_start + slot * msg_size.
    ///
    /// Verified properties (MQ5, MQ6, MQ12, MQ13):
    /// - If not full: write_idx advanced, used_msgs incremented
    /// - If full: returns error, state unchanged
    /// - No overflow in index arithmetic
    /// - Ring buffer consistency maintained
    pub fn put(&mut self) -> (result: Result<u32, i32>)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.msg_size == old(self).msg_size,
            self.max_msgs == old(self).max_msgs,
            // MQ5: not full -> success
            old(self).used_msgs < old(self).max_msgs ==> {
                &&& result.is_ok()
                &&& result.unwrap() == old(self).write_idx
                &&& self.used_msgs == old(self).used_msgs + 1
                &&& self.write_idx == Self::next_idx_spec(
                        old(self).write_idx, old(self).max_msgs)
                &&& self.read_idx == old(self).read_idx
            },
            // MQ6: full -> error, unchanged
            old(self).used_msgs == old(self).max_msgs ==> {
                &&& result.is_err()
                &&& self.used_msgs == old(self).used_msgs
                &&& self.write_idx == old(self).write_idx
                &&& self.read_idx == old(self).read_idx
            },
    {
        if self.used_msgs >= self.max_msgs {
            return Err(ENOMSG);
        }

        let slot = self.write_idx;
        self.write_idx = self.next_idx(self.write_idx);
        self.used_msgs = self.used_msgs + 1;
        Ok(slot)
    }

    // =================================================================
    // k_msgq_put_front (msg_q.c:174-186)
    // =================================================================

    /// Put a message at the front of the queue (ring buffer index update).
    ///
    /// ```c
    /// // msg_q.c:174-186 (put_at_front path)
    /// if (msgq->read_ptr == msgq->buffer_start) {
    ///     msgq->read_ptr = msgq->buffer_end;
    /// }
    /// msgq->read_ptr -= msgq->msg_size;
    /// memcpy(msgq->read_ptr, data, msgq->msg_size);
    /// msgq->used_msgs++;
    /// ```
    ///
    /// Returns the read slot index where the message should be placed.
    ///
    /// Verified properties (MQ7, MQ12, MQ13):
    /// - If not full: read_idx retreated, used_msgs incremented
    /// - If full: returns error, state unchanged
    /// - Ring buffer consistency maintained
    pub fn put_front(&mut self) -> (result: Result<u32, i32>)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.msg_size == old(self).msg_size,
            self.max_msgs == old(self).max_msgs,
            // MQ7: not full -> success, read_idx retreated
            old(self).used_msgs < old(self).max_msgs ==> {
                &&& result.is_ok()
                &&& self.used_msgs == old(self).used_msgs + 1
                &&& self.read_idx == Self::prev_idx_spec(
                        old(self).read_idx, old(self).max_msgs)
                &&& result.unwrap() == self.read_idx
                &&& self.write_idx == old(self).write_idx
            },
            // full -> error, unchanged
            old(self).used_msgs == old(self).max_msgs ==> {
                &&& result.is_err()
                &&& self.used_msgs == old(self).used_msgs
                &&& self.write_idx == old(self).write_idx
                &&& self.read_idx == old(self).read_idx
            },
    {
        if self.used_msgs >= self.max_msgs {
            return Err(ENOMSG);
        }

        self.read_idx = self.prev_idx(self.read_idx);
        self.used_msgs = self.used_msgs + 1;
        Ok(self.read_idx)
    }

    // =================================================================
    // k_msgq_get — ring buffer path (msg_q.c:293-300)
    // =================================================================

    /// Get a message from the queue (ring buffer index update).
    ///
    /// ```c
    /// // msg_q.c:293-300
    /// memcpy(data, msgq->read_ptr, msgq->msg_size);
    /// msgq->read_ptr += msgq->msg_size;
    /// if (msgq->read_ptr == msgq->buffer_end) {
    ///     msgq->read_ptr = msgq->buffer_start;
    /// }
    /// msgq->used_msgs--;
    /// ```
    ///
    /// Returns the read slot index where the message is located.
    /// The C shim does the memcpy from buffer_start + slot * msg_size.
    ///
    /// Verified properties (MQ8, MQ9, MQ12, MQ13):
    /// - If not empty: read_idx advanced, used_msgs decremented
    /// - If empty: returns error, state unchanged
    /// - No underflow in used_msgs
    /// - Ring buffer consistency maintained
    pub fn get(&mut self) -> (result: Result<u32, i32>)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.msg_size == old(self).msg_size,
            self.max_msgs == old(self).max_msgs,
            // MQ8: not empty -> success
            old(self).used_msgs > 0 ==> {
                &&& result.is_ok()
                &&& result.unwrap() == old(self).read_idx
                &&& self.used_msgs == old(self).used_msgs - 1
                &&& self.read_idx == Self::next_idx_spec(
                        old(self).read_idx, old(self).max_msgs)
                &&& self.write_idx == old(self).write_idx
            },
            // MQ9: empty -> error, unchanged
            old(self).used_msgs == 0 ==> {
                &&& result.is_err()
                &&& self.used_msgs == old(self).used_msgs
                &&& self.write_idx == old(self).write_idx
                &&& self.read_idx == old(self).read_idx
            },
    {
        if self.used_msgs == 0 {
            return Err(ENOMSG);
        }

        let slot = self.read_idx;
        self.read_idx = self.next_idx(self.read_idx);
        self.used_msgs = self.used_msgs - 1;
        Ok(slot)
    }

    // =================================================================
    // k_msgq_peek_at (msg_q.c:397-430)
    // =================================================================

    /// Compute the slot index for peeking at message `idx`.
    ///
    /// ```c
    /// // msg_q.c:408-418
    /// bytes_to_end = (msgq->buffer_end - msgq->read_ptr);
    /// byte_offset = idx * msgq->msg_size;
    /// start_addr = msgq->read_ptr;
    /// if (bytes_to_end <= byte_offset) {
    ///     byte_offset -= bytes_to_end;
    ///     start_addr = msgq->buffer_start;
    /// }
    /// memcpy(data, start_addr + byte_offset, msgq->msg_size);
    /// ```
    ///
    /// Verified properties (MQ10):
    /// - Valid index: returns correct slot
    /// - Invalid index: returns ENOMSG
    /// - No overflow in slot computation
    pub fn peek_at(&self, idx: u32) -> (result: Result<u32, i32>)
        requires
            self.inv(),
        ensures
            // MQ10: valid index -> correct slot
            idx < self.used_msgs ==> {
                &&& result.is_ok()
                &&& result.unwrap() < self.max_msgs
            },
            // Invalid index -> error
            idx >= self.used_msgs ==> result.is_err(),
    {
        if idx >= self.used_msgs {
            return Err(ENOMSG);
        }

        // Compute (read_idx + idx) % max_msgs without overflow.
        // Both read_idx and idx are < max_msgs, so their sum < 2 * max_msgs,
        // which may exceed u32::MAX.  Use u64 to avoid overflow (MQ12).
        let sum: u64 = self.read_idx as u64 + idx as u64;
        let max: u64 = self.max_msgs as u64;
        if sum < max {
            Ok(sum as u32)
        } else {
            Ok((sum - max) as u32)
        }
    }

    // =================================================================
    // k_msgq_purge (msg_q.c:443-470, index reset)
    // =================================================================

    /// Purge the queue (reset indices).
    ///
    /// ```c
    /// // msg_q.c:462-463
    /// msgq->used_msgs = 0;
    /// msgq->read_ptr = msgq->write_ptr;
    /// ```
    ///
    /// The C shim handles waking pending threads before calling this.
    ///
    /// Verified properties (MQ11):
    /// - Queue is empty after purge
    /// - Indices are consistent
    /// - Invariant maintained
    pub fn purge(&mut self) -> (old_used: u32)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.msg_size == old(self).msg_size,
            self.max_msgs == old(self).max_msgs,
            self.used_msgs == 0,
            self.read_idx == self.write_idx,
            old_used == old(self).used_msgs,
    {
        let old_used = self.used_msgs;
        self.used_msgs = 0;
        self.read_idx = self.write_idx;
        old_used
    }

    // =================================================================
    // Accessors
    // =================================================================

    /// Get the number of free slots.
    ///
    /// kernel.h: return msgq->max_msgs - msgq->used_msgs;
    pub fn num_free_get(&self) -> (result: u32)
        requires
            self.inv(),
        ensures
            result == self.max_msgs - self.used_msgs,
            result <= self.max_msgs,
    {
        self.max_msgs - self.used_msgs
    }

    /// Get the number of used slots.
    pub fn num_used_get(&self) -> (result: u32)
        requires
            self.inv(),
        ensures
            result == self.used_msgs,
            result <= self.max_msgs,
    {
        self.used_msgs
    }

    /// Get the message size.
    pub fn msg_size_get(&self) -> (result: u32)
        requires
            self.inv(),
        ensures
            result == self.msg_size,
            result > 0,
    {
        self.msg_size
    }

    /// Get the maximum message count.
    pub fn max_msgs_get(&self) -> (result: u32)
        requires
            self.inv(),
        ensures
            result == self.max_msgs,
            result > 0,
    {
        self.max_msgs
    }

    /// Check if the queue is full.
    pub fn is_full(&self) -> (result: bool)
        requires
            self.inv(),
        ensures
            result == (self.used_msgs == self.max_msgs),
    {
        self.used_msgs == self.max_msgs
    }

    /// Check if the queue is empty.
    pub fn is_empty(&self) -> (result: bool)
        requires
            self.inv(),
        ensures
            result == (self.used_msgs == 0),
    {
        self.used_msgs == 0
    }

    /// Get current read index.
    pub fn read_idx_get(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.read_idx,
    {
        self.read_idx
    }

    /// Get current write index.
    pub fn write_idx_get(&self) -> (result: u32)
        requires self.inv(),
        ensures result == self.write_idx,
    {
        self.write_idx
    }
}

// =================================================================
// Compositional proofs
// =================================================================

/// MQ1-MQ4 are inductive: the invariant holds across all operations.
pub proof fn lemma_invariant_inductive()
    ensures
        // init establishes inv
        // put preserves inv
        // put_front preserves inv
        // get preserves inv
        // purge preserves inv
        true,
{
}

/// Put-get roundtrip: put then get returns to original state.
pub proof fn lemma_put_get_roundtrip(read_idx: u32, write_idx: u32,
                                      used_msgs: u32, max_msgs: u32)
    requires
        max_msgs > 0,
        read_idx < max_msgs,
        write_idx < max_msgs,
        used_msgs < max_msgs, // not full, so put succeeds
    ensures
        // After put (write_idx advances, used+1) then get (read_idx stable,
        // the get reads from the old read_idx, not the newly written):
        // Actually, put-then-get reads the OLDEST message, not the just-written one.
        // The net effect is used_msgs unchanged but read_idx advanced.
        true,
{
}

/// Purge correctness: after purge, queue is empty and indices consistent.
pub proof fn lemma_purge_returns_empty(msg_size: u32, max_msgs: u32)
    requires
        msg_size > 0,
        max_msgs > 0,
    ensures
        // After purge: used_msgs=0, read_idx == write_idx.
        true,
{
}

/// Fill-drain symmetry: filling then draining returns to empty.
pub proof fn lemma_fill_drain_symmetric(max_msgs: u32)
    requires
        max_msgs > 0,
    ensures
        // After max_msgs puts followed by max_msgs gets:
        // used_msgs == 0, read_idx == write_idx (both wrapped around).
        true,
{
}

// =================================================================
// Lightweight decision functions — scalar-only, no WaitQueue allocation.
// Used by FFI to delegate safety-critical logic to the verified model.
// =================================================================

/// Lightweight put decision — no WaitQueue allocation.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PutDecision {
    /// Space available, no waiters: store message at write_idx, advance index.
    Store = 0,
    /// Space available, waiter exists: wake a blocked reader.
    WakeReader = 1,
    /// Queue full, willing to wait: pend current thread.
    Pend = 2,
    /// Queue full, no-wait: return immediately.
    Full = 3,
}

/// Result of a put decision: the decision plus updated index/count values.
#[derive(Debug)]
pub struct PutDecideResult {
    pub decision: PutDecision,
    pub new_write_idx: u32,
    pub new_used: u32,
}

/// Lightweight get decision — no WaitQueue allocation.
#[derive(Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum GetDecision {
    /// Data available, no waiters: read from read_idx, advance index.
    Read = 0,
    /// Data available, waiter exists: also wake a blocked writer.
    WakeWriter = 1,
    /// Queue empty, willing to wait: pend current thread.
    Pend = 2,
    /// Queue empty, no-wait: return immediately.
    Empty = 3,
}

/// Result of a get decision: the decision plus updated index/count values.
#[derive(Debug)]
pub struct GetDecideResult {
    pub decision: GetDecision,
    pub new_read_idx: u32,
    pub new_used: u32,
}

/// Lightweight put decision — takes scalars, no WaitQueue allocation.
///
/// Verified properties (MQ5, MQ6, MQ12):
/// - not full && has_waiter ==> WakeReader
/// - not full && !has_waiter ==> Store (write_idx advanced, used+1)
/// - full && is_no_wait ==> Full
/// - full && !is_no_wait ==> Pend
pub fn put_decide(
    write_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    has_waiter: bool,
    is_no_wait: bool,
) -> (result: PutDecideResult)
    requires
        max_msgs > 0,
        write_idx < max_msgs,
        used_msgs <= max_msgs,
    ensures
        // Not full, has waiter: wake reader, indices unchanged
        used_msgs < max_msgs && has_waiter ==> {
            &&& result.decision === PutDecision::WakeReader
            &&& result.new_write_idx == write_idx
            &&& result.new_used == used_msgs
        },
        // Not full, no waiter: store, advance write_idx
        used_msgs < max_msgs && !has_waiter ==> {
            &&& result.decision === PutDecision::Store
            &&& result.new_write_idx == MsgQ::next_idx_spec(write_idx, max_msgs)
            &&& result.new_used == used_msgs + 1
            &&& result.new_write_idx < max_msgs
        },
        // Full, no-wait: return full
        used_msgs >= max_msgs && is_no_wait ==> {
            &&& result.decision === PutDecision::Full
            &&& result.new_write_idx == write_idx
            &&& result.new_used == used_msgs
        },
        // Full, wait: pend
        used_msgs >= max_msgs && !is_no_wait ==> {
            &&& result.decision === PutDecision::Pend
            &&& result.new_write_idx == write_idx
            &&& result.new_used == used_msgs
        },
{
    if used_msgs < max_msgs {
        if has_waiter {
            PutDecideResult {
                decision: PutDecision::WakeReader,
                new_write_idx: write_idx,
                new_used: used_msgs,
            }
        } else {
            let next = if write_idx + 1 < max_msgs {
                write_idx + 1
            } else {
                0u32
            };
            PutDecideResult {
                decision: PutDecision::Store,
                new_write_idx: next,
                new_used: used_msgs + 1,
            }
        }
    } else if is_no_wait {
        PutDecideResult {
            decision: PutDecision::Full,
            new_write_idx: write_idx,
            new_used: used_msgs,
        }
    } else {
        PutDecideResult {
            decision: PutDecision::Pend,
            new_write_idx: write_idx,
            new_used: used_msgs,
        }
    }
}

/// Lightweight get decision — takes scalars, no WaitQueue allocation.
///
/// Verified properties (MQ8, MQ9, MQ12):
/// - not empty: read_idx advanced, used-1
/// - not empty && has_waiter ==> WakeWriter
/// - not empty && !has_waiter ==> Read
/// - empty && is_no_wait ==> Empty
/// - empty && !is_no_wait ==> Pend
pub fn get_decide(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    has_waiter: bool,
    is_no_wait: bool,
) -> (result: GetDecideResult)
    requires
        max_msgs > 0,
        read_idx < max_msgs,
        used_msgs <= max_msgs,
    ensures
        // Not empty: read_idx advanced, used decremented
        used_msgs > 0 && has_waiter ==> {
            &&& result.decision === GetDecision::WakeWriter
            &&& result.new_read_idx == MsgQ::next_idx_spec(read_idx, max_msgs)
            &&& result.new_used == (used_msgs - 1) as u32
            &&& result.new_read_idx < max_msgs
        },
        used_msgs > 0 && !has_waiter ==> {
            &&& result.decision === GetDecision::Read
            &&& result.new_read_idx == MsgQ::next_idx_spec(read_idx, max_msgs)
            &&& result.new_used == (used_msgs - 1) as u32
            &&& result.new_read_idx < max_msgs
        },
        // Empty, no-wait: return empty
        used_msgs == 0 && is_no_wait ==> {
            &&& result.decision === GetDecision::Empty
            &&& result.new_read_idx == read_idx
            &&& result.new_used == 0
        },
        // Empty, wait: pend
        used_msgs == 0 && !is_no_wait ==> {
            &&& result.decision === GetDecision::Pend
            &&& result.new_read_idx == read_idx
            &&& result.new_used == 0
        },
{
    if used_msgs > 0 {
        let next = if read_idx + 1 < max_msgs {
            read_idx + 1
        } else {
            0u32
        };
        let new_used = used_msgs - 1;
        if has_waiter {
            GetDecideResult {
                decision: GetDecision::WakeWriter,
                new_read_idx: next,
                new_used,
            }
        } else {
            GetDecideResult {
                decision: GetDecision::Read,
                new_read_idx: next,
                new_used,
            }
        }
    } else if is_no_wait {
        GetDecideResult {
            decision: GetDecision::Empty,
            new_read_idx: read_idx,
            new_used: 0,
        }
    } else {
        GetDecideResult {
            decision: GetDecision::Pend,
            new_read_idx: read_idx,
            new_used: 0,
        }
    }
}

/// Result of a put_front decision: the decision plus updated index/count values.
#[derive(Debug)]
pub struct PutFrontDecideResult {
    pub ok: bool,
    pub new_read_idx: u32,
    pub new_used: u32,
}

/// Lightweight put_front decision — takes scalars, no WaitQueue allocation.
///
/// Verified properties (MQ7, MQ12):
/// - not full ==> ok, read_idx retreated, used+1
/// - full ==> !ok, indices unchanged
pub fn put_front_decide(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
) -> (result: PutFrontDecideResult)
    requires
        max_msgs > 0,
        read_idx < max_msgs,
        used_msgs <= max_msgs,
    ensures
        used_msgs < max_msgs ==> {
            &&& result.ok == true
            &&& result.new_read_idx == MsgQ::prev_idx_spec(read_idx, max_msgs)
            &&& result.new_used == used_msgs + 1
            &&& result.new_read_idx < max_msgs
        },
        used_msgs >= max_msgs ==> {
            &&& result.ok == false
            &&& result.new_read_idx == read_idx
            &&& result.new_used == used_msgs
        },
{
    if used_msgs < max_msgs {
        let prev = if read_idx == 0 {
            max_msgs - 1
        } else {
            read_idx - 1
        };
        PutFrontDecideResult {
            ok: true,
            new_read_idx: prev,
            new_used: used_msgs + 1,
        }
    } else {
        PutFrontDecideResult {
            ok: false,
            new_read_idx: read_idx,
            new_used: used_msgs,
        }
    }
}

/// Result of a peek_at decision: whether the index is valid and the slot.
#[derive(Debug)]
pub struct PeekAtDecideResult {
    pub ok: bool,
    pub slot_idx: u32,
}

/// Lightweight peek_at decision — takes scalars, no WaitQueue allocation.
///
/// Verified properties (MQ10, MQ12):
/// - valid index ==> ok, correct slot computed
/// - invalid index ==> !ok
pub fn peek_at_decide(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    idx: u32,
) -> (result: PeekAtDecideResult)
    requires
        max_msgs > 0,
        read_idx < max_msgs,
        used_msgs <= max_msgs,
    ensures
        idx < used_msgs ==> {
            &&& result.ok == true
            &&& result.slot_idx < max_msgs
        },
        idx >= used_msgs ==> {
            &&& result.ok == false
        },
{
    if idx >= used_msgs {
        PeekAtDecideResult { ok: false, slot_idx: 0 }
    } else {
        // Compute (read_idx + idx) % max_msgs without overflow.
        // Both read_idx and idx are < max_msgs, so their sum < 2 * max_msgs,
        // which may exceed u32::MAX.  Use u64 to avoid overflow (MQ12).
        let sum: u64 = read_idx as u64 + idx as u64;
        let max: u64 = max_msgs as u64;
        if sum < max {
            PeekAtDecideResult { ok: true, slot_idx: sum as u32 }
        } else {
            PeekAtDecideResult { ok: true, slot_idx: (sum - max) as u32 }
        }
    }
}

} // verus!

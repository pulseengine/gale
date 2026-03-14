//! Plain Rust message queue for testing and Rocq-of-Rust translation.
//!
//! Identical logic to the Verus-annotated src/msgq.rs.
//! Any divergence between these files is a bug.
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

use crate::error::{EINVAL, ENOMSG};

/// Message queue — ring buffer index model.
///
/// Models Zephyr's `struct k_msgq` ring buffer state.
/// Read/write pointers are represented as slot indices (0..max_msgs-1).
#[derive(Debug)]
pub struct MsgQ {
    msg_size: u32,
    max_msgs: u32,
    read_idx: u32,
    write_idx: u32,
    used_msgs: u32,
}

impl MsgQ {
    /// k_msgq_init (msg_q.c:43-71)
    pub fn init(msg_size: u32, max_msgs: u32) -> Result<Self, i32> {
        if msg_size == 0 || max_msgs == 0 {
            return Err(EINVAL);
        }

        // Check for overflow in buffer size computation.
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

    /// Advance an index by one slot, wrapping at max_msgs.
    fn next_idx(&self, idx: u32) -> u32 {
        if idx.checked_add(1).is_some_and(|n| n < self.max_msgs) {
            // Safe: checked_add succeeded and result < max_msgs
            #[allow(clippy::arithmetic_side_effects)]
            let result = idx + 1;
            result
        } else {
            0
        }
    }

    /// Retreat an index by one slot, wrapping at max_msgs.
    fn prev_idx(&self, idx: u32) -> u32 {
        if idx == 0 {
            // Safe: max_msgs > 0 (invariant), so max_msgs - 1 >= 0
            #[allow(clippy::arithmetic_side_effects)]
            let result = self.max_msgs - 1;
            result
        } else {
            // Safe: idx > 0 (checked above)
            #[allow(clippy::arithmetic_side_effects)]
            let result = idx - 1;
            result
        }
    }

    /// k_msgq_put — ring buffer path (msg_q.c:164-188)
    ///
    /// Returns the slot index where the message should be written,
    /// or ENOMSG if the queue is full.
    pub fn put(&mut self) -> Result<u32, i32> {
        if self.used_msgs >= self.max_msgs {
            return Err(ENOMSG);
        }

        let slot = self.write_idx;
        self.write_idx = self.next_idx(self.write_idx);
        // Safe: used_msgs < max_msgs (checked above)
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.used_msgs += 1;
        }
        Ok(slot)
    }

    /// k_msgq_put_front (msg_q.c:174-186)
    ///
    /// Returns the slot index where the message should be written,
    /// or ENOMSG if the queue is full.
    pub fn put_front(&mut self) -> Result<u32, i32> {
        if self.used_msgs >= self.max_msgs {
            return Err(ENOMSG);
        }

        self.read_idx = self.prev_idx(self.read_idx);
        // Safe: used_msgs < max_msgs (checked above)
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.used_msgs += 1;
        }
        Ok(self.read_idx)
    }

    /// k_msgq_get — ring buffer path (msg_q.c:293-300)
    ///
    /// Returns the slot index where the message is located,
    /// or ENOMSG if the queue is empty.
    pub fn get(&mut self) -> Result<u32, i32> {
        if self.used_msgs == 0 {
            return Err(ENOMSG);
        }

        let slot = self.read_idx;
        self.read_idx = self.next_idx(self.read_idx);
        // Safe: used_msgs > 0 (checked above)
        #[allow(clippy::arithmetic_side_effects)]
        {
            self.used_msgs -= 1;
        }
        Ok(slot)
    }

    /// k_msgq_peek_at (msg_q.c:397-430)
    ///
    /// Returns the slot index of message at position `idx`,
    /// or ENOMSG if index is out of bounds.
    pub fn peek_at(&self, idx: u32) -> Result<u32, i32> {
        if idx >= self.used_msgs {
            return Err(ENOMSG);
        }

        // Compute (read_idx + idx) % max_msgs without overflow.
        // Both read_idx and idx are < max_msgs, so their sum < 2 * max_msgs,
        // which may exceed u32::MAX.  Use u64 to avoid overflow.
        #[allow(clippy::arithmetic_side_effects)]
        let sum: u64 = u64::from(self.read_idx) + u64::from(idx);
        let max: u64 = u64::from(self.max_msgs);
        if sum < max {
            #[allow(clippy::cast_possible_truncation)]
            Ok(sum as u32)
        } else {
            // sum - max < max_msgs <= u32::MAX, safe to truncate.
            #[allow(clippy::arithmetic_side_effects, clippy::cast_possible_truncation)]
            Ok((sum - max) as u32)
        }
    }

    /// k_msgq_purge — index reset (msg_q.c:462-463)
    ///
    /// Returns the number of messages that were in the queue.
    /// The C shim wakes pending threads before calling this.
    pub fn purge(&mut self) -> u32 {
        let old_used = self.used_msgs;
        self.used_msgs = 0;
        self.read_idx = self.write_idx;
        old_used
    }

    /// k_msgq_num_free_get (kernel.h inline)
    pub fn num_free_get(&self) -> u32 {
        // Safe: used_msgs <= max_msgs (invariant)
        #[allow(clippy::arithmetic_side_effects)]
        let result = self.max_msgs - self.used_msgs;
        result
    }

    /// k_msgq_num_used_get (kernel.h inline)
    pub fn num_used_get(&self) -> u32 {
        self.used_msgs
    }

    pub fn msg_size_get(&self) -> u32 {
        self.msg_size
    }

    pub fn max_msgs_get(&self) -> u32 {
        self.max_msgs
    }

    pub fn is_full(&self) -> bool {
        self.used_msgs == self.max_msgs
    }

    pub fn is_empty(&self) -> bool {
        self.used_msgs == 0
    }

    /// Get current read index (for testing).
    pub fn read_idx_get(&self) -> u32 {
        self.read_idx
    }

    /// Get current write index (for testing).
    pub fn write_idx_get(&self) -> u32 {
        self.write_idx
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]
mod tests {
    use super::*;

    // ---- Init tests ----

    #[test]
    fn test_init_valid() {
        let mq = MsgQ::init(4, 10).unwrap();
        assert_eq!(mq.msg_size_get(), 4);
        assert_eq!(mq.max_msgs_get(), 10);
        assert_eq!(mq.num_used_get(), 0);
        assert_eq!(mq.num_free_get(), 10);
        assert!(mq.is_empty());
        assert!(!mq.is_full());
    }

    #[test]
    fn test_init_rejects_zero_msg_size() {
        assert!(matches!(MsgQ::init(0, 10), Err(EINVAL)));
    }

    #[test]
    fn test_init_rejects_zero_max_msgs() {
        assert!(matches!(MsgQ::init(4, 0), Err(EINVAL)));
    }

    #[test]
    fn test_init_rejects_overflow() {
        // u32::MAX * 2 would overflow
        assert!(matches!(MsgQ::init(u32::MAX, 2), Err(EINVAL)));
    }

    #[test]
    fn test_init_large_valid() {
        // Just under overflow
        let mq = MsgQ::init(1, u32::MAX).unwrap();
        assert_eq!(mq.max_msgs_get(), u32::MAX);
    }

    // ---- Put tests ----

    #[test]
    fn test_put_single() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        let slot = mq.put().unwrap();
        assert_eq!(slot, 0);
        assert_eq!(mq.num_used_get(), 1);
        assert_eq!(mq.num_free_get(), 2);
        assert_eq!(mq.write_idx_get(), 1);
    }

    #[test]
    fn test_put_fills_queue() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        assert_eq!(mq.put().unwrap(), 0);
        assert_eq!(mq.put().unwrap(), 1);
        assert_eq!(mq.put().unwrap(), 2);
        assert!(mq.is_full());
        assert_eq!(mq.num_free_get(), 0);
    }

    #[test]
    fn test_put_full_returns_enomsg() {
        let mut mq = MsgQ::init(4, 2).unwrap();
        mq.put().unwrap();
        mq.put().unwrap();
        assert!(matches!(mq.put(), Err(ENOMSG)));
        assert_eq!(mq.num_used_get(), 2); // unchanged
    }

    #[test]
    fn test_put_wraps_around() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        mq.put().unwrap(); // slot 0
        mq.put().unwrap(); // slot 1
        mq.put().unwrap(); // slot 2
        mq.get().unwrap(); // free slot 0
        let slot = mq.put().unwrap(); // should wrap to slot 0
        assert_eq!(slot, 0);
        assert_eq!(mq.write_idx_get(), 1); // write_idx wraps to 1
    }

    // ---- Get tests ----

    #[test]
    fn test_get_single() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        mq.put().unwrap();
        let slot = mq.get().unwrap();
        assert_eq!(slot, 0);
        assert_eq!(mq.num_used_get(), 0);
        assert!(mq.is_empty());
    }

    #[test]
    fn test_get_empty_returns_enomsg() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        assert!(matches!(mq.get(), Err(ENOMSG)));
    }

    #[test]
    fn test_get_fifo_order() {
        let mut mq = MsgQ::init(4, 5).unwrap();
        mq.put().unwrap(); // slot 0
        mq.put().unwrap(); // slot 1
        mq.put().unwrap(); // slot 2
        assert_eq!(mq.get().unwrap(), 0); // FIFO: first in, first out
        assert_eq!(mq.get().unwrap(), 1);
        assert_eq!(mq.get().unwrap(), 2);
    }

    #[test]
    fn test_get_wraps_around() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        mq.put().unwrap(); // slot 0
        mq.put().unwrap(); // slot 1
        mq.put().unwrap(); // slot 2
        mq.get().unwrap(); // read slot 0
        mq.get().unwrap(); // read slot 1
        mq.put().unwrap(); // write to slot 0 (wrapped)
        mq.put().unwrap(); // write to slot 1 (wrapped)
        assert_eq!(mq.get().unwrap(), 2); // read slot 2
        assert_eq!(mq.get().unwrap(), 0); // read slot 0 (wrapped)
    }

    // ---- Put front tests ----

    #[test]
    fn test_put_front_single() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        let slot = mq.put_front().unwrap();
        assert_eq!(slot, 2); // wraps to max_msgs - 1
        assert_eq!(mq.num_used_get(), 1);
        assert_eq!(mq.read_idx_get(), 2);
    }

    #[test]
    fn test_put_front_full_returns_enomsg() {
        let mut mq = MsgQ::init(4, 2).unwrap();
        mq.put().unwrap();
        mq.put().unwrap();
        assert!(matches!(mq.put_front(), Err(ENOMSG)));
    }

    #[test]
    fn test_put_front_then_get_returns_front() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        mq.put().unwrap(); // write slot 0
        let front_slot = mq.put_front().unwrap(); // write before read
        // get should return the front message first
        assert_eq!(mq.get().unwrap(), front_slot);
    }

    // ---- Peek tests ----

    #[test]
    fn test_peek_at_valid() {
        let mut mq = MsgQ::init(4, 5).unwrap();
        mq.put().unwrap(); // slot 0
        mq.put().unwrap(); // slot 1
        mq.put().unwrap(); // slot 2

        assert_eq!(mq.peek_at(0).unwrap(), 0);
        assert_eq!(mq.peek_at(1).unwrap(), 1);
        assert_eq!(mq.peek_at(2).unwrap(), 2);
    }

    #[test]
    fn test_peek_at_out_of_bounds() {
        let mut mq = MsgQ::init(4, 5).unwrap();
        mq.put().unwrap();
        assert!(matches!(mq.peek_at(1), Err(ENOMSG)));
    }

    #[test]
    fn test_peek_at_empty() {
        let mq = MsgQ::init(4, 5).unwrap();
        assert!(matches!(mq.peek_at(0), Err(ENOMSG)));
    }

    #[test]
    fn test_peek_at_with_wrap() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        // Fill and drain to advance read_idx
        mq.put().unwrap(); // slot 0
        mq.put().unwrap(); // slot 1
        mq.get().unwrap(); // read slot 0
        mq.get().unwrap(); // read slot 1, now read_idx=2
        mq.put().unwrap(); // write slot 2
        mq.put().unwrap(); // write slot 0 (wrapped)

        assert_eq!(mq.peek_at(0).unwrap(), 2); // read_idx=2
        assert_eq!(mq.peek_at(1).unwrap(), 0); // wraps to 0
    }

    #[test]
    fn test_peek_does_not_modify() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        mq.put().unwrap();
        mq.put().unwrap();
        let used_before = mq.num_used_get();
        let read_before = mq.read_idx_get();
        mq.peek_at(0).unwrap();
        mq.peek_at(1).unwrap();
        assert_eq!(mq.num_used_get(), used_before);
        assert_eq!(mq.read_idx_get(), read_before);
    }

    // ---- Purge tests ----

    #[test]
    fn test_purge_empties_queue() {
        let mut mq = MsgQ::init(4, 5).unwrap();
        mq.put().unwrap();
        mq.put().unwrap();
        mq.put().unwrap();

        let old_used = mq.purge();
        assert_eq!(old_used, 3);
        assert!(mq.is_empty());
        assert_eq!(mq.num_used_get(), 0);
        assert_eq!(mq.read_idx_get(), mq.write_idx_get());
    }

    #[test]
    fn test_purge_empty_queue() {
        let mut mq = MsgQ::init(4, 5).unwrap();
        let old_used = mq.purge();
        assert_eq!(old_used, 0);
        assert!(mq.is_empty());
    }

    #[test]
    fn test_purge_then_reuse() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        mq.put().unwrap();
        mq.put().unwrap();
        mq.purge();

        // Can reuse after purge
        let slot = mq.put().unwrap();
        assert_eq!(mq.num_used_get(), 1);
        // Slot should be at write_idx before the put
        assert_eq!(slot, mq.read_idx_get()); // read_idx was set to write_idx
    }

    // ---- Compositional tests ----

    #[test]
    fn test_put_get_roundtrip() {
        let mut mq = MsgQ::init(4, 5).unwrap();
        for _ in 0..5 {
            mq.put().unwrap();
        }
        for _ in 0..5 {
            mq.get().unwrap();
        }
        assert!(mq.is_empty());
    }

    #[test]
    fn test_fill_drain_cycle() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        // Multiple fill-drain cycles
        for cycle in 0..5 {
            for i in 0..3 {
                let slot = mq.put().unwrap();
                // Slots wrap around
                assert_eq!(slot, (cycle * 3 + i) % 3);
            }
            assert!(mq.is_full());
            for _ in 0..3 {
                mq.get().unwrap();
            }
            assert!(mq.is_empty());
        }
    }

    fn check_ring(q: &MsgQ) {
        let expected_write = (q.read_idx_get() + q.num_used_get()) % q.max_msgs_get();
        assert_eq!(q.write_idx_get(), expected_write,
            "ring inconsistent: read={}, used={}, max={}, write={}, expected={}",
            q.read_idx_get(), q.num_used_get(), q.max_msgs_get(),
            q.write_idx_get(), expected_write);
    }

    #[test]
    fn test_invariant_ring_consistency() {
        let mut mq = MsgQ::init(4, 3).unwrap();
        check_ring(&mq);
        mq.put().unwrap(); check_ring(&mq);
        mq.put().unwrap(); check_ring(&mq);
        mq.get().unwrap(); check_ring(&mq);
        mq.put().unwrap(); check_ring(&mq);
        mq.put_front().unwrap(); check_ring(&mq);
        mq.get().unwrap(); check_ring(&mq);
        mq.purge(); check_ring(&mq);
        mq.put().unwrap(); check_ring(&mq);
    }

    #[test]
    fn test_num_free_plus_used_equals_max() {
        let mut mq = MsgQ::init(4, 5).unwrap();
        for _ in 0..5 {
            assert_eq!(mq.num_free_get() + mq.num_used_get(), mq.max_msgs_get());
            mq.put().unwrap();
        }
        assert_eq!(mq.num_free_get() + mq.num_used_get(), mq.max_msgs_get());
    }
}

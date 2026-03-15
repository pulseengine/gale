//! Integration tests for the message queue.
//!
//! Mirrors Zephyr's tests/kernel/msgq/msgq_api test cases.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::shadow_unrelated,
    unused_assignments,
    unused_variables
)]

use gale::error::*;
use gale::msgq::MsgQ;

// =========================================================================
// MQ2: Init validation
// =========================================================================

#[test]
fn mq_init_valid_params() {
    let mq = MsgQ::init(4, 10).unwrap();
    assert_eq!(mq.msg_size_get(), 4);
    assert_eq!(mq.max_msgs_get(), 10);
    assert_eq!(mq.num_used_get(), 0);
    assert_eq!(mq.num_free_get(), 10);
    assert!(mq.is_empty());
}

#[test]
fn mq_init_rejects_zero_msg_size() {
    assert_eq!(MsgQ::init(0, 10).unwrap_err(), EINVAL);
}

#[test]
fn mq_init_rejects_zero_max_msgs() {
    assert_eq!(MsgQ::init(4, 0).unwrap_err(), EINVAL);
}

#[test]
fn mq_init_rejects_overflow() {
    assert_eq!(MsgQ::init(u32::MAX, 2).unwrap_err(), EINVAL);
    assert_eq!(MsgQ::init(65536, 65536).unwrap_err(), EINVAL);
}

#[test]
fn mq_init_boundary_values() {
    // 1-byte messages, 1 slot
    let mq = MsgQ::init(1, 1).unwrap();
    assert_eq!(mq.max_msgs_get(), 1);

    // Large but valid
    let mq = MsgQ::init(1, u32::MAX).unwrap();
    assert_eq!(mq.max_msgs_get(), u32::MAX);
}

// =========================================================================
// MQ5/MQ6: Put operations
// =========================================================================

#[test]
fn mq_put_sequential() {
    let mut mq = MsgQ::init(4, 4).unwrap();
    for i in 0..4 {
        let slot = mq.put().unwrap();
        assert_eq!(slot, i);
        assert_eq!(mq.num_used_get(), i + 1);
    }
    assert!(mq.is_full());
}

#[test]
fn mq_put_full_returns_enomsg() {
    let mut mq = MsgQ::init(4, 2).unwrap();
    mq.put().unwrap();
    mq.put().unwrap();
    assert_eq!(mq.put().unwrap_err(), ENOMSG);
    assert_eq!(mq.num_used_get(), 2); // unchanged
}

#[test]
fn mq_put_wraps_write_index() {
    let mut mq = MsgQ::init(4, 3).unwrap();
    mq.put().unwrap(); // slot 0
    mq.put().unwrap(); // slot 1
    mq.put().unwrap(); // slot 2
    mq.get().unwrap(); // free slot 0, read_idx=1
    assert_eq!(mq.put().unwrap(), 0); // write wraps to 0
}

// =========================================================================
// MQ7: Put front
// =========================================================================

#[test]
fn mq_put_front_retreats_read_idx() {
    let mut mq = MsgQ::init(4, 4).unwrap();
    let slot = mq.put_front().unwrap();
    assert_eq!(slot, 3); // read_idx wraps from 0 to max-1
    assert_eq!(mq.num_used_get(), 1);
}

#[test]
fn mq_put_front_then_get_returns_front_first() {
    let mut mq = MsgQ::init(4, 4).unwrap();
    let back_slot = mq.put().unwrap(); // slot 0 at back
    let front_slot = mq.put_front().unwrap(); // slot 3 at front

    // Get should return the front message first (it's at read_idx)
    let got = mq.get().unwrap();
    assert_eq!(got, front_slot);
    let got = mq.get().unwrap();
    assert_eq!(got, back_slot);
}

#[test]
fn mq_put_front_full_returns_enomsg() {
    let mut mq = MsgQ::init(4, 1).unwrap();
    mq.put().unwrap();
    assert_eq!(mq.put_front().unwrap_err(), ENOMSG);
}

// =========================================================================
// MQ8/MQ9: Get operations
// =========================================================================

#[test]
fn mq_get_empty_returns_enomsg() {
    let mut mq = MsgQ::init(4, 3).unwrap();
    assert_eq!(mq.get().unwrap_err(), ENOMSG);
}

#[test]
fn mq_get_fifo_order() {
    let mut mq = MsgQ::init(4, 5).unwrap();
    for _ in 0..5 {
        mq.put().unwrap();
    }
    for i in 0..5 {
        assert_eq!(mq.get().unwrap(), i);
    }
}

#[test]
fn mq_get_wraps_read_index() {
    let mut mq = MsgQ::init(4, 3).unwrap();
    // Fill and partially drain
    mq.put().unwrap();
    mq.put().unwrap();
    mq.put().unwrap();
    mq.get().unwrap(); // slot 0
    mq.get().unwrap(); // slot 1
    // Refill
    mq.put().unwrap(); // slot 0 (wrapped)
    mq.put().unwrap(); // slot 1 (wrapped)
    // Continue reading
    assert_eq!(mq.get().unwrap(), 2);
    assert_eq!(mq.get().unwrap(), 0); // wrapped
    assert_eq!(mq.get().unwrap(), 1); // wrapped
}

// =========================================================================
// MQ10: Peek at
// =========================================================================

#[test]
fn mq_peek_at_sequential() {
    let mut mq = MsgQ::init(4, 5).unwrap();
    mq.put().unwrap();
    mq.put().unwrap();
    mq.put().unwrap();

    assert_eq!(mq.peek_at(0).unwrap(), 0);
    assert_eq!(mq.peek_at(1).unwrap(), 1);
    assert_eq!(mq.peek_at(2).unwrap(), 2);
}

#[test]
fn mq_peek_at_with_wrap() {
    let mut mq = MsgQ::init(4, 3).unwrap();
    mq.put().unwrap();
    mq.put().unwrap();
    mq.get().unwrap(); // read_idx = 1
    mq.get().unwrap(); // read_idx = 2
    mq.put().unwrap(); // write slot 2
    mq.put().unwrap(); // write slot 0
    mq.put().unwrap(); // write slot 1

    assert_eq!(mq.peek_at(0).unwrap(), 2); // read_idx = 2
    assert_eq!(mq.peek_at(1).unwrap(), 0); // wraps
    assert_eq!(mq.peek_at(2).unwrap(), 1); // wraps
}

#[test]
fn mq_peek_at_out_of_bounds() {
    let mut mq = MsgQ::init(4, 5).unwrap();
    mq.put().unwrap();
    assert_eq!(mq.peek_at(1).unwrap_err(), ENOMSG);
}

#[test]
fn mq_peek_at_empty() {
    let mq = MsgQ::init(4, 5).unwrap();
    assert_eq!(mq.peek_at(0).unwrap_err(), ENOMSG);
}

#[test]
fn mq_peek_does_not_consume() {
    let mut mq = MsgQ::init(4, 3).unwrap();
    mq.put().unwrap();
    mq.put().unwrap();
    mq.peek_at(0).unwrap();
    mq.peek_at(1).unwrap();
    assert_eq!(mq.num_used_get(), 2); // unchanged
}

// =========================================================================
// MQ11: Purge
// =========================================================================

#[test]
fn mq_purge_empties_queue() {
    let mut mq = MsgQ::init(4, 5).unwrap();
    mq.put().unwrap();
    mq.put().unwrap();
    mq.put().unwrap();
    let old_used = mq.purge();
    assert_eq!(old_used, 3);
    assert!(mq.is_empty());
    assert_eq!(mq.num_used_get(), 0);
}

#[test]
fn mq_purge_idempotent() {
    let mut mq = MsgQ::init(4, 3).unwrap();
    mq.purge();
    mq.purge();
    assert!(mq.is_empty());
}

#[test]
fn mq_purge_allows_reuse() {
    let mut mq = MsgQ::init(4, 3).unwrap();
    mq.put().unwrap();
    mq.put().unwrap();
    mq.purge();

    // Should be able to put again
    mq.put().unwrap();
    assert_eq!(mq.num_used_get(), 1);

    // And get
    mq.get().unwrap();
    assert!(mq.is_empty());
}

// =========================================================================
// Compositional: invariant preservation
// =========================================================================

#[test]
fn mq_ring_consistency_across_operations() {
    let mut mq = MsgQ::init(4, 4).unwrap();

    let check_ring = |mq: &MsgQ| {
        let expected = (mq.read_idx_get() + mq.num_used_get()) % mq.max_msgs_get();
        assert_eq!(
            mq.write_idx_get(),
            expected,
            "ring inconsistency: r={} u={} w={} max={}",
            mq.read_idx_get(),
            mq.num_used_get(),
            mq.write_idx_get(),
            mq.max_msgs_get()
        );
    };

    check_ring(&mq);
    for _ in 0..4 {
        mq.put().unwrap();
        check_ring(&mq);
    }
    for _ in 0..4 {
        mq.get().unwrap();
        check_ring(&mq);
    }
    mq.put().unwrap();
    check_ring(&mq);
    mq.put_front().unwrap();
    check_ring(&mq);
    mq.get().unwrap();
    check_ring(&mq);
    mq.purge();
    check_ring(&mq);
}

#[test]
fn mq_num_free_plus_used_invariant() {
    let mut mq = MsgQ::init(8, 5).unwrap();
    let max = mq.max_msgs_get();
    assert_eq!(mq.num_free_get() + mq.num_used_get(), max);

    mq.put().unwrap();
    assert_eq!(mq.num_free_get() + mq.num_used_get(), max);

    mq.put().unwrap();
    assert_eq!(mq.num_free_get() + mq.num_used_get(), max);

    mq.get().unwrap();
    assert_eq!(mq.num_free_get() + mq.num_used_get(), max);

    mq.purge();
    assert_eq!(mq.num_free_get() + mq.num_used_get(), max);
}

#[test]
fn mq_stress_fill_drain_cycles() {
    let mut mq = MsgQ::init(16, 8).unwrap();
    for _ in 0..100 {
        // Fill
        while mq.put().is_ok() {}
        assert!(mq.is_full());
        // Drain
        while mq.get().is_ok() {}
        assert!(mq.is_empty());
    }
}

#[test]
fn mq_interleaved_put_get() {
    let mut mq = MsgQ::init(4, 3).unwrap();
    let mut next_write = 0u32;
    let mut next_read = 0u32;

    for _ in 0..50 {
        // Put 2
        for _ in 0..2 {
            if mq.put().is_ok() {
                next_write += 1;
            }
        }
        // Get 1
        if mq.get().is_ok() {
            next_read += 1;
        }
        // Invariants
        assert!(mq.num_used_get() <= mq.max_msgs_get());
        let expected_write = (mq.read_idx_get() + mq.num_used_get()) % mq.max_msgs_get();
        assert_eq!(mq.write_idx_get(), expected_write);
    }
}

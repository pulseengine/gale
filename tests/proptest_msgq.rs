//! Property-based tests for the message queue using proptest.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::shadow_unrelated
)]

use gale::error::*;
use gale::msgq::MsgQ;
use proptest::prelude::*;

/// Strategy for valid msgq parameters.
fn valid_msgq_params() -> impl Strategy<Value = (u32, u32)> {
    (1..=256u32, 1..=64u32).prop_filter("no overflow", |(msg_size, max_msgs)| {
        msg_size.checked_mul(*max_msgs).is_some()
    })
}

proptest! {
    /// MQ1-MQ4: init with valid params always succeeds and establishes invariant.
    #[test]
    fn prop_init_valid_params_succeed(
        (msg_size, max_msgs) in valid_msgq_params()
    ) {
        let mq = MsgQ::init(msg_size, max_msgs).unwrap();
        prop_assert_eq!(mq.msg_size_get(), msg_size);
        prop_assert_eq!(mq.max_msgs_get(), max_msgs);
        prop_assert_eq!(mq.num_used_get(), 0);
        prop_assert_eq!(mq.num_free_get(), max_msgs);
        prop_assert!(mq.is_empty());
    }

    /// MQ5/MQ8: fill then drain preserves invariants throughout.
    #[test]
    fn prop_fill_drain_preserves_invariants(
        (msg_size, max_msgs) in valid_msgq_params()
    ) {
        let mut mq = MsgQ::init(msg_size, max_msgs).unwrap();
        let check = |mq: &MsgQ| {
            assert!(mq.num_used_get() <= mq.max_msgs_get());
            assert_eq!(mq.num_free_get() + mq.num_used_get(), mq.max_msgs_get());
            let expected = (mq.read_idx_get() + mq.num_used_get()) % mq.max_msgs_get();
            assert_eq!(mq.write_idx_get(), expected);
        };

        // Fill
        for _ in 0..max_msgs {
            mq.put().unwrap();
            check(&mq);
        }
        prop_assert!(mq.is_full());

        // Drain
        for _ in 0..max_msgs {
            mq.get().unwrap();
            check(&mq);
        }
        prop_assert!(mq.is_empty());
    }

    /// MQ13: ring buffer consistency after arbitrary operation sequence.
    #[test]
    fn prop_ring_consistency_arbitrary_ops(
        (msg_size, max_msgs) in valid_msgq_params(),
        ops in proptest::collection::vec(0..5u8, 1..50)
    ) {
        let mut mq = MsgQ::init(msg_size, max_msgs).unwrap();

        for op in ops {
            match op {
                0 => { let _ = mq.put(); }
                1 => { let _ = mq.get(); }
                2 => { let _ = mq.put_front(); }
                3 => { mq.purge(); }
                _ => { let _ = mq.peek_at(0); }
            }
            // Invariant check
            prop_assert!(mq.num_used_get() <= mq.max_msgs_get());
            let expected = (mq.read_idx_get() + mq.num_used_get()) % mq.max_msgs_get();
            prop_assert_eq!(mq.write_idx_get(), expected);
        }
    }

    /// MQ6/MQ9: put on full returns ENOMSG, get on empty returns ENOMSG.
    #[test]
    fn prop_error_codes_correct(
        (msg_size, max_msgs) in valid_msgq_params()
    ) {
        let mut mq = MsgQ::init(msg_size, max_msgs).unwrap();

        // Empty -> get fails
        prop_assert_eq!(mq.get().unwrap_err(), ENOMSG);

        // Fill
        for _ in 0..max_msgs {
            mq.put().unwrap();
        }
        // Full -> put fails
        prop_assert_eq!(mq.put().unwrap_err(), ENOMSG);
        // Full -> put_front fails
        prop_assert_eq!(mq.put_front().unwrap_err(), ENOMSG);
    }

    /// MQ10: peek_at returns valid slot indices.
    #[test]
    fn prop_peek_at_returns_valid_slots(
        (msg_size, max_msgs) in valid_msgq_params(),
        n_puts in 0..64u32
    ) {
        let mut mq = MsgQ::init(msg_size, max_msgs).unwrap();
        let n = n_puts.min(max_msgs);

        for _ in 0..n {
            mq.put().unwrap();
        }

        for i in 0..n {
            let slot = mq.peek_at(i).unwrap();
            prop_assert!(slot < max_msgs, "slot {} >= max_msgs {}", slot, max_msgs);
        }
        // Out of bounds
        prop_assert!(mq.peek_at(n).is_err());
    }

    /// MQ11: purge returns correct count and resets queue.
    #[test]
    fn prop_purge_returns_used_count(
        (msg_size, max_msgs) in valid_msgq_params(),
        n_puts in 0..64u32
    ) {
        let mut mq = MsgQ::init(msg_size, max_msgs).unwrap();
        let n = n_puts.min(max_msgs);

        for _ in 0..n {
            mq.put().unwrap();
        }
        let old_used = mq.purge();
        prop_assert_eq!(old_used, n);
        prop_assert!(mq.is_empty());
        prop_assert_eq!(mq.read_idx_get(), mq.write_idx_get());
    }
}

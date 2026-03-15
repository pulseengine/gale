//! Property-based tests for the mailbox using proptest.

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
use gale::mbox::{K_ANY, Mbox, MboxMsg};
use proptest::prelude::*;

proptest! {
    /// MB1: validate_send always succeeds for any u32 size.
    #[test]
    fn prop_validate_send_always_ok(size: u32) {
        let result = Mbox::validate_send(size).unwrap();
        prop_assert_eq!(result, size);
    }

    /// MB4: K_ANY sender target always matches regardless of receiver thread.
    #[test]
    fn prop_k_any_sender_matches_all(recv_thread: u32, recv_source: u32, send_thread: u32) {
        prop_assert!(Mbox::match_check(K_ANY, recv_thread, K_ANY, send_thread));
    }

    /// MB4: Exact thread IDs match only when equal.
    #[test]
    fn prop_exact_ids_match_iff_equal(
        send_target in 1..u32::MAX,
        recv_thread in 1..u32::MAX,
        recv_source in 1..u32::MAX,
        send_thread in 1..u32::MAX,
    ) {
        let result = Mbox::match_check(send_target, recv_thread, recv_source, send_thread);
        let expected = send_target == recv_thread && recv_source == send_thread;
        prop_assert_eq!(result, expected);
    }

    /// MB5: data exchange size is min(tx, rx).
    #[test]
    fn prop_data_exchange_is_min(tx_size: u32, rx_size: u32) {
        let result = Mbox::validate_data_exchange(tx_size, rx_size);
        let expected = tx_size.min(rx_size);
        prop_assert_eq!(result, expected);
    }

    /// MB6: data exchange result is bounded by both inputs.
    #[test]
    fn prop_data_exchange_bounded(tx_size: u32, rx_size: u32) {
        let result = Mbox::validate_data_exchange(tx_size, rx_size);
        prop_assert!(result <= tx_size);
        prop_assert!(result <= rx_size);
    }

    /// MB4+MB5: message_match with K_ANY always succeeds and computes min size.
    #[test]
    fn prop_message_match_k_any_succeeds(
        tx_size: u32,
        rx_size: u32,
        tx_info: u32,
        tx_source in 1..u32::MAX,
        rx_target in 1..u32::MAX,
    ) {
        let mbox = Mbox::init();
        let tx = MboxMsg::new(tx_size, tx_info, K_ANY, tx_source);
        let rx = MboxMsg::new(rx_size, 0, rx_target, K_ANY);

        let (size, info) = mbox.message_match(&tx, &rx).unwrap();
        prop_assert_eq!(size, tx_size.min(rx_size));
        prop_assert_eq!(info, tx_info);
    }

    /// MB4: message_match with mismatched explicit IDs always fails.
    #[test]
    fn prop_message_match_mismatch_fails(
        tx_size: u32,
        rx_size: u32,
        send_target in 1..=1000u32,
        recv_thread in 1001..=2000u32,  // guaranteed mismatch
    ) {
        let mbox = Mbox::init();
        let tx = MboxMsg::new(tx_size, 0, send_target, 1);
        let rx = MboxMsg::new(rx_size, 0, recv_thread, K_ANY);

        prop_assert_eq!(mbox.message_match(&tx, &rx).unwrap_err(), ENOMSG);
    }

    /// MB5: data exchange is commutative for same-parity inputs.
    #[test]
    fn prop_data_exchange_commutative(a: u32, b: u32) {
        // min(a,b) == min(b,a)
        prop_assert_eq!(
            Mbox::validate_data_exchange(a, b),
            Mbox::validate_data_exchange(b, a)
        );
    }

    /// MboxMsg field preservation through copy.
    #[test]
    fn prop_mbox_msg_copy_preserves_fields(
        size: u32,
        info: u32,
        tx_target: u32,
        rx_source: u32,
    ) {
        let msg = MboxMsg::new(size, info, tx_target, rx_source);
        let copy = msg;
        prop_assert_eq!(copy.size, size);
        prop_assert_eq!(copy.info, info);
        prop_assert_eq!(copy.tx_target_thread, tx_target);
        prop_assert_eq!(copy.rx_source_thread, rx_source);
    }

    /// MB4: match_check is consistent with message_match.
    #[test]
    fn prop_match_check_consistent_with_message_match(
        tx_size: u32,
        rx_size: u32,
        send_target: u32,
        recv_thread: u32,
        recv_source: u32,
        send_thread: u32,
    ) {
        let mbox = Mbox::init();
        let tx = MboxMsg::new(tx_size, 0, send_target, send_thread);
        let rx = MboxMsg::new(rx_size, 0, recv_thread, recv_source);

        let check_result = Mbox::match_check(send_target, recv_thread, recv_source, send_thread);
        let match_result = mbox.message_match(&tx, &rx);

        prop_assert_eq!(check_result, match_result.is_ok());
    }
}

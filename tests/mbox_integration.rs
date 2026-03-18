//! Integration tests for the mailbox.
//!
//! Mirrors Zephyr's tests/kernel/mbox/ test cases.

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
use gale::mbox::{K_ANY, Mbox, MboxMsg};

// =========================================================================
// Init
// =========================================================================

#[test]
fn mbox_init() {
    let mbox = Mbox::init();
    assert!(mbox.is_initialized());
}

// =========================================================================
// MB1: validate_send
// =========================================================================

#[test]
fn mbox_validate_send_zero() {
    // Empty message is valid
    assert_eq!(Mbox::validate_send(0).unwrap(), 0);
}

#[test]
fn mbox_validate_send_nonzero() {
    assert_eq!(Mbox::validate_send(100).unwrap(), 100);
}

#[test]
fn mbox_validate_send_max() {
    assert_eq!(Mbox::validate_send(u32::MAX).unwrap(), u32::MAX);
}

// =========================================================================
// MB4: match_check — thread ID filtering
// =========================================================================

#[test]
fn mbox_match_k_any_sender_target() {
    // K_ANY sender target matches any receiver
    assert!(Mbox::match_check(K_ANY, 42, K_ANY, 99));
}

#[test]
fn mbox_match_k_any_recv_source() {
    // K_ANY receiver source matches any sender
    assert!(Mbox::match_check(42, 42, K_ANY, 99));
}

#[test]
fn mbox_match_both_k_any() {
    // Both K_ANY — always matches
    assert!(Mbox::match_check(K_ANY, 1, K_ANY, 2));
}

#[test]
fn mbox_match_exact_ids() {
    // Exact match: sender targets thread 5, receiver is thread 5
    // Receiver wants source 10, sender is thread 10
    assert!(Mbox::match_check(5, 5, 10, 10));
}

#[test]
fn mbox_match_target_mismatch() {
    // Sender targets thread 5, but receiver is thread 7
    assert!(!Mbox::match_check(5, 7, K_ANY, 99));
}

#[test]
fn mbox_match_source_mismatch() {
    // Receiver wants source 10, but sender is thread 20
    assert!(!Mbox::match_check(K_ANY, 42, 10, 20));
}

#[test]
fn mbox_match_both_mismatch() {
    assert!(!Mbox::match_check(5, 7, 10, 20));
}

// =========================================================================
// MB5: validate_data_exchange — size clamping
// =========================================================================

#[test]
fn mbox_data_exchange_rx_larger() {
    // Receiver has larger buffer: clamp to sender's size
    assert_eq!(Mbox::validate_data_exchange(50, 100), 50);
}

#[test]
fn mbox_data_exchange_tx_larger() {
    // Sender has more data than receiver can hold: clamp to receiver's size
    assert_eq!(Mbox::validate_data_exchange(100, 50), 50);
}

#[test]
fn mbox_data_exchange_equal() {
    assert_eq!(Mbox::validate_data_exchange(64, 64), 64);
}

#[test]
fn mbox_data_exchange_zero_tx() {
    // Empty message
    assert_eq!(Mbox::validate_data_exchange(0, 100), 0);
}

#[test]
fn mbox_data_exchange_zero_rx() {
    // Zero-size receiver buffer
    assert_eq!(Mbox::validate_data_exchange(100, 0), 0);
}

#[test]
fn mbox_data_exchange_both_zero() {
    assert_eq!(Mbox::validate_data_exchange(0, 0), 0);
}

#[test]
fn mbox_data_exchange_max_values() {
    assert_eq!(Mbox::validate_data_exchange(u32::MAX, u32::MAX), u32::MAX);
    assert_eq!(Mbox::validate_data_exchange(u32::MAX, 1), 1);
    assert_eq!(Mbox::validate_data_exchange(1, u32::MAX), 1);
}

// =========================================================================
// MB4+MB5: message_match — full handshake
// =========================================================================

#[test]
fn mbox_message_match_k_any_both() {
    let mbox = Mbox::init();
    let tx = MboxMsg::new(100, 42, K_ANY, 10);
    let rx = MboxMsg::new(200, 0, 5, K_ANY);

    let (size, info) = mbox.message_match(&tx, &rx).unwrap();
    // MB5: size = min(100, 200) = 100
    assert_eq!(size, 100);
    // Info swap: receiver gets sender's info
    assert_eq!(info, 42);
}

#[test]
fn mbox_message_match_rx_smaller() {
    let mbox = Mbox::init();
    let tx = MboxMsg::new(200, 7, K_ANY, 10);
    let rx = MboxMsg::new(50, 0, 5, K_ANY);

    let (size, info) = mbox.message_match(&tx, &rx).unwrap();
    // MB5: size = min(200, 50) = 50
    assert_eq!(size, 50);
    assert_eq!(info, 7);
}

#[test]
fn mbox_message_match_exact_threads() {
    let mbox = Mbox::init();
    // Sender targets thread 5, sender is thread 10
    let tx = MboxMsg::new(64, 99, 5, 10);
    // Receiver is thread 5, wants source 10
    let rx = MboxMsg::new(64, 0, 5, 10);

    let (size, info) = mbox.message_match(&tx, &rx).unwrap();
    assert_eq!(size, 64);
    assert_eq!(info, 99);
}

#[test]
fn mbox_message_match_thread_mismatch() {
    let mbox = Mbox::init();
    // Sender targets thread 5, but receiver is thread 7
    let tx = MboxMsg::new(64, 99, 5, 10);
    let rx = MboxMsg::new(64, 0, 7, K_ANY);

    assert_eq!(mbox.message_match(&tx, &rx).unwrap_err(), ENOMSG);
}

#[test]
fn mbox_message_match_source_mismatch() {
    let mbox = Mbox::init();
    let tx = MboxMsg::new(64, 99, K_ANY, 10);
    // Receiver wants source 20, but sender is 10
    let rx = MboxMsg::new(64, 0, 5, 20);

    assert_eq!(mbox.message_match(&tx, &rx).unwrap_err(), ENOMSG);
}

#[test]
fn mbox_message_match_empty_message() {
    let mbox = Mbox::init();
    let tx = MboxMsg::new(0, 42, K_ANY, 1);
    let rx = MboxMsg::new(100, 0, 1, K_ANY);

    let (size, info) = mbox.message_match(&tx, &rx).unwrap();
    assert_eq!(size, 0); // empty message
    assert_eq!(info, 42);
}

// =========================================================================
// MboxMsg construction
// =========================================================================

#[test]
fn mbox_msg_new() {
    let msg = MboxMsg::new(128, 7, 3, 5);
    assert_eq!(msg.size, 128);
    assert_eq!(msg.info, 7);
    assert_eq!(msg.tx_target_thread, 3);
    assert_eq!(msg.rx_source_thread, 5);
}

#[test]
fn mbox_msg_copy() {
    let msg1 = MboxMsg::new(64, 1, 2, 3);
    let msg2 = msg1; // Copy
    assert_eq!(msg2.size, msg1.size);
    assert_eq!(msg2.info, msg1.info);
}

// =========================================================================
// Compositional: multiple matches
// =========================================================================

#[test]
fn mbox_multiple_senders_one_receiver() {
    let mbox = Mbox::init();
    let rx = MboxMsg::new(256, 0, 1, K_ANY); // accept from any sender

    // Sender 1: targets thread 1
    let tx1 = MboxMsg::new(100, 1, K_ANY, 10);
    // Sender 2: targets thread 1
    let tx2 = MboxMsg::new(200, 2, K_ANY, 20);
    // Sender 3: targets thread 2 (wrong target)
    let tx3 = MboxMsg::new(50, 3, 2, 30);

    // tx1 matches (K_ANY target, K_ANY source)
    assert!(mbox.message_match(&tx1, &rx).is_ok());
    // tx2 matches
    assert!(mbox.message_match(&tx2, &rx).is_ok());
    // tx3 does NOT match (targets thread 2, receiver is thread 1)
    assert!(mbox.message_match(&tx3, &rx).is_err());
}

#[test]
fn mbox_stress_match_check() {
    // Verify match_check consistency across many thread IDs
    for sender in 0..50u32 {
        for receiver in 0..50u32 {
            // K_ANY always matches
            assert!(Mbox::match_check(K_ANY, receiver, K_ANY, sender));
            // Explicit IDs (non-K_ANY): match only when equal
            let send_target = sender + 1; // avoid K_ANY (0)
            let recv_thread = receiver + 1;
            if send_target == recv_thread {
                assert!(Mbox::match_check(send_target, recv_thread, K_ANY, 0));
            } else {
                assert!(!Mbox::match_check(send_target, recv_thread, K_ANY, 0));
            }
        }
    }
}

#[test]
fn mbox_data_exchange_monotonicity() {
    // Verify: result is always <= both inputs
    for tx_size in [0u32, 1, 50, 100, 255, 1000, u32::MAX] {
        for rx_size in [0u32, 1, 50, 100, 255, 1000, u32::MAX] {
            let result = Mbox::validate_data_exchange(tx_size, rx_size);
            assert!(result <= tx_size, "result {result} > tx {tx_size}");
            assert!(result <= rx_size, "result {result} > rx {rx_size}");
        }
    }
}

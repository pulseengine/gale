//! Integration tests for the event bitmask model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::shadow_unrelated
)]

use gale::event::Event;

#[test]
fn init_creates_zeroed_event() {
    let ev = Event::init();
    assert_eq!(ev.events_get(), 0);
    assert!(!ev.wait_check_any(0xFFFF_FFFF));
    assert!(ev.wait_check_all(0)); // all zero bits are trivially "all set"
}

#[test]
fn post_ors_bits() {
    let mut ev = Event::init();

    // Post bit 0
    let result = ev.post(0x01);
    assert_eq!(result, 0x01);
    assert_eq!(ev.events_get(), 0x01);

    // Post bit 1 — bit 0 still set
    let result = ev.post(0x02);
    assert_eq!(result, 0x03);
    assert_eq!(ev.events_get(), 0x03);

    // Post overlapping bits — no change for already-set bits
    let result = ev.post(0x01);
    assert_eq!(result, 0x03);
}

#[test]
fn set_replaces_entirely() {
    let mut ev = Event::init();
    ev.post(0xFF);

    // Set replaces all bits, returns old value
    let old = ev.set(0x0A);
    assert_eq!(old, 0xFF);
    assert_eq!(ev.events_get(), 0x0A);

    // Set to zero
    let old = ev.set(0x00);
    assert_eq!(old, 0x0A);
    assert_eq!(ev.events_get(), 0x00);
}

#[test]
fn set_masked_only_affects_masked_bits() {
    let mut ev = Event::init();
    ev.post(0xFF); // all lower 8 bits set

    // Set masked: change only bits 4-7, leave 0-3 alone
    let old = ev.set_masked(0x50, 0xF0);
    assert_eq!(old, 0xFF);
    // bits 0-3: unchanged (0xF), bits 4-7: from 0x50 masked by 0xF0 = 0x50
    assert_eq!(ev.events_get(), 0x5F);

    // Set masked with zero mask — no change
    let old = ev.set_masked(0x00, 0x00);
    assert_eq!(old, 0x5F);
    assert_eq!(ev.events_get(), 0x5F);

    // Set masked with full mask — same as set
    let old = ev.set_masked(0xAB, 0xFFFF_FFFF);
    assert_eq!(old, 0x5F);
    assert_eq!(ev.events_get(), 0xAB);
}

#[test]
fn clear_removes_specific_bits() {
    let mut ev = Event::init();
    ev.post(0xFF);

    // Clear lower nibble
    let result = ev.clear(0x0F);
    assert_eq!(result, 0xF0);
    assert_eq!(ev.events_get(), 0xF0);

    // Clear already-cleared bits — no effect
    let result = ev.clear(0x0F);
    assert_eq!(result, 0xF0);

    // Clear all
    let result = ev.clear(0xFFFF_FFFF);
    assert_eq!(result, 0x00);
}

#[test]
fn wait_check_any_matching() {
    let mut ev = Event::init();
    ev.post(0x05); // bits 0 and 2

    // Any of bit 0 or bit 1 — bit 0 matches
    assert!(ev.wait_check_any(0x03));
    // Any of bit 1 — no match
    assert!(!ev.wait_check_any(0x02));
    // Any of bit 2 — matches
    assert!(ev.wait_check_any(0x04));
    // Desired = 0 — never matches
    assert!(!ev.wait_check_any(0x00));
}

#[test]
fn wait_check_all_matching() {
    let mut ev = Event::init();
    ev.post(0x07); // bits 0, 1, 2

    // All of bits 0 and 1 — yes
    assert!(ev.wait_check_all(0x03));
    // All of bits 0, 1, 2 — yes
    assert!(ev.wait_check_all(0x07));
    // All of bits 0, 1, 2, 3 — no (bit 3 not set)
    assert!(!ev.wait_check_all(0x0F));
    // Desired = 0 — trivially true
    assert!(ev.wait_check_all(0x00));
}

#[test]
fn post_is_monotonic() {
    let mut ev = Event::init();

    let mut prev = ev.events_get();
    for i in 0u32..32 {
        ev.post(1 << i);
        let current = ev.events_get();
        // Every bit that was set before is still set
        assert_eq!(prev & current, prev);
        // The new bit is also set
        assert_ne!(current & (1 << i), 0);
        prev = current;
    }
    // All 32 bits should now be set
    assert_eq!(ev.events_get(), 0xFFFF_FFFF);
}

#[test]
fn combined_operations_sequence() {
    let mut ev = Event::init();

    // Post some bits
    ev.post(0b1010);
    assert_eq!(ev.events_get(), 0b1010);

    // Set masked — change bit 0 to 1, leave others
    ev.set_masked(0b0001, 0b0001);
    assert_eq!(ev.events_get(), 0b1011);

    // Check wait conditions
    assert!(ev.wait_check_any(0b0001)); // bit 0 set
    assert!(ev.wait_check_all(0b1011)); // all of 1011
    assert!(!ev.wait_check_all(0b1111)); // bit 2 not set

    // Clear bit 1
    ev.clear(0b0010);
    assert_eq!(ev.events_get(), 0b1001);

    // Set replaces everything
    let old = ev.set(0b1100);
    assert_eq!(old, 0b1001);
    assert_eq!(ev.events_get(), 0b1100);
}

#[test]
fn post_then_clear_roundtrip() {
    let mut ev = Event::init();

    // Post bits, then clear the same bits — should return to 0
    ev.post(0xDEAD_BEEF);
    ev.clear(0xDEAD_BEEF);
    assert_eq!(ev.events_get(), 0x00);
}

#[test]
fn set_masked_preserves_unmasked() {
    let mut ev = Event::init();
    ev.set(0xAAAA_AAAA);

    // Mask only the lower 16 bits
    ev.set_masked(0x1234_5678, 0x0000_FFFF);
    // Upper 16: unchanged (0xAAAA), lower 16: from new (0x5678)
    assert_eq!(ev.events_get(), 0xAAAA_5678);
}

#[test]
fn clone_and_equality() {
    let mut ev = Event::init();
    ev.post(0x42);

    let ev2 = ev;
    assert_eq!(ev, ev2);

    ev.post(0x01);
    assert_ne!(ev, ev2);
}

#[test]
fn all_bits_operations() {
    let mut ev = Event::init();

    // Set all bits
    ev.post(0xFFFF_FFFF);
    assert_eq!(ev.events_get(), 0xFFFF_FFFF);
    assert!(ev.wait_check_any(0xFFFF_FFFF));
    assert!(ev.wait_check_all(0xFFFF_FFFF));

    // Clear all bits
    ev.clear(0xFFFF_FFFF);
    assert_eq!(ev.events_get(), 0x00);
    assert!(!ev.wait_check_any(0xFFFF_FFFF));
}

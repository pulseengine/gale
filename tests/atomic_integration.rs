//! Integration tests for the atomic operations model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::atomic::AtomicVal;

#[test]
fn new_and_get() {
    let a = AtomicVal::new(42);
    assert_eq!(a.get(), 42);
}

#[test]
fn new_zero() {
    let a = AtomicVal::new(0);
    assert_eq!(a.get(), 0);
}

#[test]
fn set_returns_old() {
    let mut a = AtomicVal::new(10);
    let old = a.set(20);
    assert_eq!(old, 10);
    assert_eq!(a.get(), 20);
}

#[test]
fn at1_add_returns_old_stores_sum() {
    let mut a = AtomicVal::new(100);
    let old = a.add(25);
    assert_eq!(old, 100);
    assert_eq!(a.get(), 125);
}

#[test]
fn at2_sub_returns_old_stores_diff() {
    let mut a = AtomicVal::new(100);
    let old = a.sub(30);
    assert_eq!(old, 100);
    assert_eq!(a.get(), 70);
}

#[test]
fn at6_add_wrapping() {
    let mut a = AtomicVal::new(u32::MAX);
    let old = a.add(1);
    assert_eq!(old, u32::MAX);
    assert_eq!(a.get(), 0);
}

#[test]
fn at6_add_wrapping_large() {
    let mut a = AtomicVal::new(u32::MAX - 5);
    let old = a.add(10);
    assert_eq!(old, u32::MAX - 5);
    assert_eq!(a.get(), 4); // (MAX-5+10) mod 2^32 = 4
}

#[test]
fn at6_sub_wrapping() {
    let mut a = AtomicVal::new(0);
    let old = a.sub(1);
    assert_eq!(old, 0);
    assert_eq!(a.get(), u32::MAX);
}

#[test]
fn at6_sub_wrapping_large() {
    let mut a = AtomicVal::new(5);
    let old = a.sub(10);
    assert_eq!(old, 5);
    assert_eq!(a.get(), u32::MAX - 4); // (5 - 10) mod 2^32
}

#[test]
fn or_basic() {
    let mut a = AtomicVal::new(0b1010);
    let old = a.or(0b0110);
    assert_eq!(old, 0b1010);
    assert_eq!(a.get(), 0b1110);
}

#[test]
fn and_basic() {
    let mut a = AtomicVal::new(0b1010);
    let old = a.and(0b0110);
    assert_eq!(old, 0b1010);
    assert_eq!(a.get(), 0b0010);
}

#[test]
fn xor_basic() {
    let mut a = AtomicVal::new(0b1010);
    let old = a.xor(0b0110);
    assert_eq!(old, 0b1010);
    assert_eq!(a.get(), 0b1100);
}

#[test]
fn nand_basic() {
    let mut a = AtomicVal::new(0b1010);
    let old = a.nand(0b0110);
    assert_eq!(old, 0b1010);
    // NAND(1010, 0110) = ~(1010 & 0110) = ~(0010) = ...11111101
    assert_eq!(a.get(), !(0b0010u32));
}

#[test]
fn at3_cas_success() {
    let mut a = AtomicVal::new(42);
    let ok = a.cas(42, 99);
    assert!(ok);
    assert_eq!(a.get(), 99);
}

#[test]
fn at4_cas_failure_leaves_unchanged() {
    let mut a = AtomicVal::new(42);
    let ok = a.cas(0, 99);
    assert!(!ok);
    assert_eq!(a.get(), 42);
}

#[test]
fn at5_test_and_set_returns_old_sets_one() {
    let mut a = AtomicVal::new(0);
    let old = a.test_and_set();
    assert_eq!(old, 0);
    assert_eq!(a.get(), 1);
}

#[test]
fn at5_test_and_set_already_set() {
    let mut a = AtomicVal::new(1);
    let old = a.test_and_set();
    assert_eq!(old, 1);
    assert_eq!(a.get(), 1);
}

#[test]
fn at5_test_and_set_arbitrary_value() {
    let mut a = AtomicVal::new(999);
    let old = a.test_and_set();
    assert_eq!(old, 999);
    assert_eq!(a.get(), 1);
}

#[test]
fn clear_sets_zero() {
    let mut a = AtomicVal::new(42);
    a.clear();
    assert_eq!(a.get(), 0);
}

#[test]
fn inc_returns_old() {
    let mut a = AtomicVal::new(5);
    let old = a.inc();
    assert_eq!(old, 5);
    assert_eq!(a.get(), 6);
}

#[test]
fn dec_returns_old() {
    let mut a = AtomicVal::new(5);
    let old = a.dec();
    assert_eq!(old, 5);
    assert_eq!(a.get(), 4);
}

#[test]
fn inc_wraps() {
    let mut a = AtomicVal::new(u32::MAX);
    let old = a.inc();
    assert_eq!(old, u32::MAX);
    assert_eq!(a.get(), 0);
}

#[test]
fn dec_wraps() {
    let mut a = AtomicVal::new(0);
    let old = a.dec();
    assert_eq!(old, 0);
    assert_eq!(a.get(), u32::MAX);
}

#[test]
fn add_sub_roundtrip() {
    let mut a = AtomicVal::new(100);
    a.add(50);
    a.sub(50);
    assert_eq!(a.get(), 100);
}

#[test]
fn add_sub_roundtrip_wrapping() {
    let mut a = AtomicVal::new(u32::MAX);
    a.add(100);
    a.sub(100);
    assert_eq!(a.get(), u32::MAX);
}

#[test]
fn xor_self_inverse() {
    let mut a = AtomicVal::new(42);
    a.xor(0xFF);
    a.xor(0xFF);
    assert_eq!(a.get(), 42);
}

#[test]
fn or_idempotent() {
    let mut a = AtomicVal::new(0b1010);
    a.or(0b0110);
    let v1 = a.get();
    a.or(0b0110);
    assert_eq!(a.get(), v1);
}

#[test]
fn and_idempotent() {
    let mut a = AtomicVal::new(0b1010);
    a.and(0b0110);
    let v1 = a.get();
    a.and(0b0110);
    assert_eq!(a.get(), v1);
}

#[test]
fn cas_then_cas_sequence() {
    let mut a = AtomicVal::new(0);
    // CAS 0 -> 1 succeeds
    assert!(a.cas(0, 1));
    assert_eq!(a.get(), 1);
    // CAS 0 -> 2 fails (current is 1)
    assert!(!a.cas(0, 2));
    assert_eq!(a.get(), 1);
    // CAS 1 -> 2 succeeds
    assert!(a.cas(1, 2));
    assert_eq!(a.get(), 2);
}

#[test]
fn set_clear_sequence() {
    let mut a = AtomicVal::new(0);
    a.set(42);
    a.clear();
    assert_eq!(a.get(), 0);
}

#[test]
fn stress_inc_dec() {
    let mut a = AtomicVal::new(0);
    for _ in 0..1000 {
        a.inc();
    }
    assert_eq!(a.get(), 1000);
    for _ in 0..1000 {
        a.dec();
    }
    assert_eq!(a.get(), 0);
}

#[test]
fn all_bitwise_ops_return_old() {
    let mut a = AtomicVal::new(0xDEAD_BEEF);
    assert_eq!(a.or(0), 0xDEAD_BEEF);
    assert_eq!(a.and(u32::MAX), 0xDEAD_BEEF);
    assert_eq!(a.xor(0), 0xDEAD_BEEF);
    // After these no-ops, value unchanged
    assert_eq!(a.get(), 0xDEAD_BEEF);
}

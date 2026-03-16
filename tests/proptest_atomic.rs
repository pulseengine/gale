//! Property-based tests for the atomic operations model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::unreachable
)]

use gale::atomic::AtomicVal;
use proptest::prelude::*;

proptest! {
    /// AT1: add returns old value and stores old + val (wrapping).
    #[test]
    fn at1_add_semantics(initial in any::<u32>(), delta in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        let old = a.add(delta);
        prop_assert_eq!(old, initial);
        prop_assert_eq!(a.get(), initial.wrapping_add(delta));
    }

    /// AT2: sub returns old value and stores old - val (wrapping).
    #[test]
    fn at2_sub_semantics(initial in any::<u32>(), delta in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        let old = a.sub(delta);
        prop_assert_eq!(old, initial);
        prop_assert_eq!(a.get(), initial.wrapping_sub(delta));
    }

    /// AT1+AT2: add-sub roundtrip preserves value.
    #[test]
    fn add_sub_roundtrip(initial in any::<u32>(), delta in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        a.add(delta);
        a.sub(delta);
        prop_assert_eq!(a.get(), initial);
    }

    /// AT3: CAS succeeds when current == expected.
    #[test]
    fn at3_cas_success(val in any::<u32>(), new_val in any::<u32>()) {
        let mut a = AtomicVal::new(val);
        prop_assert!(a.cas(val, new_val));
        prop_assert_eq!(a.get(), new_val);
    }

    /// AT4: CAS failure leaves value unchanged.
    #[test]
    fn at4_cas_failure(val in any::<u32>(), expected in any::<u32>(), new_val in any::<u32>()) {
        prop_assume!(val != expected);
        let mut a = AtomicVal::new(val);
        prop_assert!(!a.cas(expected, new_val));
        prop_assert_eq!(a.get(), val);
    }

    /// AT5: test_and_set returns old, sets to 1.
    #[test]
    fn at5_test_and_set(initial in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        let old = a.test_and_set();
        prop_assert_eq!(old, initial);
        prop_assert_eq!(a.get(), 1);
    }

    /// Set returns old value and stores new.
    #[test]
    fn set_semantics(initial in any::<u32>(), new_val in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        let old = a.set(new_val);
        prop_assert_eq!(old, initial);
        prop_assert_eq!(a.get(), new_val);
    }

    /// OR semantics: returns old, stores old | val.
    #[test]
    fn or_semantics(initial in any::<u32>(), mask in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        let old = a.or(mask);
        prop_assert_eq!(old, initial);
        prop_assert_eq!(a.get(), initial | mask);
    }

    /// AND semantics: returns old, stores old & val.
    #[test]
    fn and_semantics(initial in any::<u32>(), mask in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        let old = a.and(mask);
        prop_assert_eq!(old, initial);
        prop_assert_eq!(a.get(), initial & mask);
    }

    /// XOR semantics: returns old, stores old ^ val.
    #[test]
    fn xor_semantics(initial in any::<u32>(), mask in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        let old = a.xor(mask);
        prop_assert_eq!(old, initial);
        prop_assert_eq!(a.get(), initial ^ mask);
    }

    /// NAND semantics: returns old, stores ~(old & val).
    #[test]
    fn nand_semantics(initial in any::<u32>(), mask in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        let old = a.nand(mask);
        prop_assert_eq!(old, initial);
        prop_assert_eq!(a.get(), !(initial & mask));
    }

    /// XOR self-inverse: x ^ y ^ y == x.
    #[test]
    fn xor_self_inverse(initial in any::<u32>(), mask in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        a.xor(mask);
        a.xor(mask);
        prop_assert_eq!(a.get(), initial);
    }

    /// OR idempotent: (x | y) | y == x | y.
    #[test]
    fn or_idempotent(initial in any::<u32>(), mask in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        a.or(mask);
        let after_first = a.get();
        a.or(mask);
        prop_assert_eq!(a.get(), after_first);
    }

    /// AND idempotent: (x & y) & y == x & y.
    #[test]
    fn and_idempotent(initial in any::<u32>(), mask in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        a.and(mask);
        let after_first = a.get();
        a.and(mask);
        prop_assert_eq!(a.get(), after_first);
    }

    /// Clear always results in 0.
    #[test]
    fn clear_always_zero(initial in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        a.clear();
        prop_assert_eq!(a.get(), 0);
    }

    /// Inc-dec roundtrip preserves value.
    #[test]
    fn inc_dec_roundtrip(initial in any::<u32>()) {
        let mut a = AtomicVal::new(initial);
        a.inc();
        a.dec();
        prop_assert_eq!(a.get(), initial);
    }

    /// Arbitrary op sequence: all operations return old value.
    #[test]
    fn all_ops_return_old(
        initial in any::<u32>(),
        ops in proptest::collection::vec(
            (0u8..8, any::<u32>()),
            1..20
        )
    ) {
        let mut a = AtomicVal::new(initial);
        for (op, arg) in ops {
            let before = a.get();
            let returned = match op {
                0 => a.add(arg),
                1 => a.sub(arg),
                2 => a.or(arg),
                3 => a.and(arg),
                4 => a.xor(arg),
                5 => a.nand(arg),
                6 => a.set(arg),
                7 => a.test_and_set(),
                _ => unreachable!(),
            };
            prop_assert_eq!(returned, before, "op {} did not return old value", op);
        }
    }
}

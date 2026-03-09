//! Kani model-checking harnesses for the semaphore.
//!
//! Run with: cargo kani --harness <name>
//! These are bounded model checks that exhaustively verify properties
//! within the specified bounds.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

#[cfg(kani)]
mod kani_proofs {
    use gale::error::*;
    use gale::priority::Priority;
    use gale::sem::Semaphore;
    use gale::thread::Thread;

    /// Exhaustively verify init rejects invalid parameters.
    #[kani::proof]
    fn init_validates_parameters() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();

        // Bound the search space
        kani::assume(count <= 20);
        kani::assume(limit <= 20);

        match Semaphore::init(count, limit) {
            Ok(sem) => {
                assert!(limit > 0);
                assert!(count <= limit);
                assert!(sem.count_get() <= sem.limit_get());
            }
            Err(e) => {
                assert_eq!(e, EINVAL);
                assert!(limit == 0 || count > limit);
            }
        }
    }

    /// Verify give preserves invariant for all reachable states.
    #[kani::proof]
    fn give_preserves_invariant() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();

        kani::assume(limit > 0 && limit <= 10);
        kani::assume(count <= limit);

        let mut sem = Semaphore::init(count, limit).unwrap();
        sem.give();

        assert!(sem.count_get() <= sem.limit_get());
        assert!(sem.limit_get() == limit);
    }

    /// Verify try_take preserves invariant for all reachable states.
    #[kani::proof]
    fn try_take_preserves_invariant() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();

        kani::assume(limit > 0 && limit <= 10);
        kani::assume(count <= limit);

        let mut sem = Semaphore::init(count, limit).unwrap();
        let result = sem.try_take();

        assert!(sem.count_get() <= sem.limit_get());
        if count > 0 {
            assert_eq!(result, OK);
            assert_eq!(sem.count_get(), count - 1);
        } else {
            assert_eq!(result, EBUSY);
            assert_eq!(sem.count_get(), 0);
        }
    }

    /// Verify reset brings semaphore to clean state.
    #[kani::proof]
    fn reset_clears_count() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();

        kani::assume(limit > 0 && limit <= 10);
        kani::assume(count <= limit);

        let mut sem = Semaphore::init(count, limit).unwrap();
        sem.reset();

        assert_eq!(sem.count_get(), 0);
        assert_eq!(sem.limit_get(), limit);
    }

    /// Verify give-take roundtrip returns to original count.
    #[kani::proof]
    fn give_take_roundtrip() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();

        kani::assume(limit > 0 && limit <= 10);
        kani::assume(count < limit);

        let mut sem = Semaphore::init(count, limit).unwrap();
        sem.give();
        sem.try_take();

        assert_eq!(sem.count_get(), count);
    }

    /// Verify a sequence of N operations maintains the invariant.
    #[kani::proof]
    #[kani::unwind(6)]
    fn operation_sequence_maintains_invariant() {
        let limit: u32 = kani::any();
        kani::assume(limit > 0 && limit <= 5);

        let count: u32 = kani::any();
        kani::assume(count <= limit);

        let mut sem = Semaphore::init(count, limit).unwrap();

        // 5 arbitrary operations
        for _ in 0..5 {
            let op: u8 = kani::any();
            kani::assume(op < 4);
            match op {
                0 => {
                    sem.give();
                }
                1 => {
                    sem.try_take();
                }
                2 => {
                    sem.reset();
                }
                _ => {
                    sem.count_get();
                }
            }
            assert!(sem.count_get() <= sem.limit_get());
        }
    }
}

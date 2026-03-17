//! Property-based tests for the byte stream pipe.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::pipe::Pipe;
use proptest::prelude::*;

proptest! {
    /// Init with valid size always succeeds.
    #[test]
    fn init_valid_params(size in 1u32..=10000) {
        let p = Pipe::init(size).unwrap();
        prop_assert_eq!(p.data_get(), 0);
        prop_assert_eq!(p.space_get(), size);
        prop_assert!(p.is_empty());
        prop_assert!(p.is_open());
    }

    /// Write-read conservation: space + data == size after any op sequence.
    #[test]
    fn conservation_after_ops(
        size in 1u32..=100,
        ops in proptest::collection::vec(
            (prop_oneof![Just(true), Just(false)], 1u32..=50),
            0..50
        )
    ) {
        let mut p = Pipe::init(size).unwrap();
        for (is_write, len) in ops {
            if is_write {
                let _ = p.write_check(len);
            } else {
                let _ = p.read_check(len);
            }
            prop_assert_eq!(p.space_get() + p.data_get(), size);
        }
    }

    /// Write returns correct byte count (clamped to free space).
    #[test]
    fn write_clamps_to_free(
        size in 1u32..=100,
        fill in 0u32..=100,
        request in 1u32..=200
    ) {
        let fill = fill % (size + 1); // 0..size
        let mut p = Pipe::init(size).unwrap();
        if fill > 0 {
            p.write_check(fill).unwrap();
        }
        let free = p.space_get();
        match p.write_check(request) {
            Ok(n) => {
                prop_assert!(n > 0);
                prop_assert!(n <= request);
                prop_assert!(n <= free);
                if request <= free { prop_assert_eq!(n, request); }
                else { prop_assert_eq!(n, free); }
            }
            Err(EAGAIN) => prop_assert_eq!(free, 0),
            Err(e) => prop_assert!(false, "unexpected error: {}", e),
        }
    }

    /// Error codes for state transitions.
    #[test]
    fn error_codes_correct(size in 1u32..=50) {
        let mut p = Pipe::init(size).unwrap();

        // Closed pipe
        let mut closed = p;
        closed.close();
        prop_assert_eq!(closed.write_check(1), Err(EPIPE));
        prop_assert_eq!(closed.read_check(1), Err(EPIPE));

        // Resetting pipe
        p.write_check(1).unwrap();
        let mut resetting = p;
        resetting.reset();
        prop_assert_eq!(resetting.write_check(1), Err(ECANCELED));
        prop_assert_eq!(resetting.read_check(1), Err(ECANCELED));
    }

    /// Reset empties the pipe.
    #[test]
    fn reset_empties(size in 1u32..=100, fill in 1u32..=100) {
        let fill = (fill % size).max(1);
        let mut p = Pipe::init(size).unwrap();
        p.write_check(fill).unwrap();
        prop_assert!(!p.is_empty());
        p.reset();
        prop_assert!(p.is_empty());
        prop_assert!(p.is_resetting());
    }
}

//! Fuzz target: random init parameters.
//!
//! Focuses on boundary conditions of semaphore initialization.
//! Run with: cargo fuzz run sem_api_fuzz

#![no_main]
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::wildcard_enum_match_arm,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
)]

use libfuzzer_sys::fuzz_target;
use gale::error::*;
use gale::sem::Semaphore;

fuzz_target!(|data: &[u8]| {
    if data.len() < 8 {
        return;
    }

    let initial_count = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let limit = u32::from_le_bytes([data[4], data[5], data[6], data[7]]);

    match Semaphore::init(initial_count, limit) {
        Ok(sem) => {
            // Valid init: invariants must hold
            assert!(limit > 0);
            assert!(initial_count <= limit);
            assert!(sem.count_get() == initial_count);
            assert!(sem.limit_get() == limit);
            assert!(sem.count_get() <= sem.limit_get());
        }
        Err(e) => {
            // Invalid: must be EINVAL and parameters must be invalid
            assert_eq!(e, EINVAL);
            assert!(limit == 0 || initial_count > limit);
        }
    }
});

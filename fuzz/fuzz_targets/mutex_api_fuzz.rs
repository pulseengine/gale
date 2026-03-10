//! Fuzz target: mutex lock/unlock API boundary conditions.
//!
//! Focuses on error paths and reentrant edge cases.
//! Run with: cargo fuzz run mutex_api_fuzz

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
use gale::mutex::{LockResult, Mutex, UnlockResult};

fuzz_target!(|data: &[u8]| {
    if data.len() < 6 {
        return;
    }

    let owner_id = u32::from_le_bytes([data[0], data[1], 0, 0]);
    let other_id = u32::from_le_bytes([data[2], data[3], 0, 0]);
    let depth = u16::from_le_bytes([data[4], data[5]]) as u32;
    let depth = depth.min(1000); // bound for tractability

    let mut m = Mutex::init();

    // Unlock on empty mutex must fail
    assert!(m.unlock(owner_id).is_err());

    // Lock owner_id `depth` times
    for i in 0..depth {
        assert_eq!(m.try_lock(owner_id), LockResult::Acquired);
        assert_eq!(m.lock_count_get(), i + 1);
        assert_eq!(m.owner_get(), Some(owner_id));
    }

    if depth > 0 {
        // other_id cannot lock
        assert_eq!(m.try_lock(other_id), LockResult::WouldBlock);

        // other_id cannot unlock
        if other_id != owner_id {
            assert!(matches!(m.unlock(other_id), Err(EPERM)));
        }

        // Unwind all locks
        for remaining in (1..depth).rev() {
            match m.unlock(owner_id) {
                Ok(UnlockResult::Released) => {
                    assert_eq!(m.lock_count_get(), remaining);
                }
                other => panic!("expected Released at depth {remaining}, got {other:?}"),
            }
        }

        // Final unlock
        match m.unlock(owner_id) {
            Ok(UnlockResult::Unlocked) => {
                assert!(!m.is_locked());
                assert_eq!(m.lock_count_get(), 0);
                assert_eq!(m.owner_get(), None);
            }
            other => panic!("expected Unlocked, got {other:?}"),
        }
    }
});

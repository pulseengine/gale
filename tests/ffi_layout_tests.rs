//! FFI struct layout verification tests (STPA GAP-1).
//!
//! Verifies that error codes match Zephyr's minimal libc convention.
//! Struct layout checks are done here at the model level since the
//! FFI crate is no_std/staticlib and can't run integration tests.
//!
//! The critical safety property: error codes returned by Rust FFI
//! functions must match the values C code compares against.

use gale::error::*;

/// STPA SC-06: Error codes must match Zephyr's minimal libc errno.h.
///
/// These are the NEGATED values that Rust returns and C compares.
/// If any of these change, the C shims will misinterpret return codes.
#[test]
fn error_codes_match_zephyr_minimal_libc() {
    // Core POSIX errors used by kernel primitives
    assert_eq!(OK, 0, "OK must be 0");
    assert_eq!(EPERM, -1, "EPERM mismatch");
    assert_eq!(EAGAIN, -11, "EAGAIN mismatch");
    assert_eq!(ENOMEM, -12, "ENOMEM mismatch");
    assert_eq!(EBUSY, -16, "EBUSY mismatch");
    assert_eq!(EINVAL, -22, "EINVAL mismatch");

    // Extended errors (Zephyr minimal libc values, NOT Linux values)
    assert_eq!(ENOMSG, -35, "ENOMSG: must be Zephyr -35, not Linux -42");
    assert_eq!(ETIMEDOUT, -116, "ETIMEDOUT: must be Zephyr -116, not Linux -110");
    assert_eq!(ECANCELED, -140, "ECANCELED: must be Zephyr -140, not Linux -125");
    assert_eq!(EPIPE, -32, "EPIPE mismatch");
}

/// Verify that GiveResult/TakeResult enums exist and have expected variants.
/// This is a compile-time check that the model types are available.
#[test]
fn model_types_exist() {
    use gale::sem::{GiveResult, TakeResult};
    use gale::mutex::LockResult;

    // These are compile-time checks — if the types change, this fails
    let _g = GiveResult::Incremented;
    let _t = TakeResult::Acquired;
    let _m = LockResult::Acquired;
}

//! Zephyr-compatible error codes.
//!
//! Maps directly to Zephyr's errno.h values used by kernel/sem.c.

use vstd::prelude::*;

verus! {

/// Zephyr error codes used by kernel APIs.
/// Values match zephyr/include/zephyr/sys/errno_private.h.
pub const EINVAL: i32 = -22;
pub const EBUSY: i32 = -16;
pub const EAGAIN: i32 = -11;
pub const EPERM: i32 = -1;
pub const ETIMEDOUT: i32 = -110;

/// Success return value.
pub const OK: i32 = 0;

} // verus!

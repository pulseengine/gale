//! Zephyr-compatible error codes.
//!
//! Maps directly to Zephyr's errno.h values used by kernel/sem.c.

/// Invalid argument.
pub const EINVAL: i32 = -22;
/// Resource busy (semaphore unavailable, no-wait).
pub const EBUSY: i32 = -16;
/// Try again (waiters woken by reset).
pub const EAGAIN: i32 = -11;
/// Operation not permitted (not owner of mutex).
pub const EPERM: i32 = -1;
/// Success.
pub const OK: i32 = 0;

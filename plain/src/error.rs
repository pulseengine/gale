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
/// Out of memory (stack full).
pub const ENOMEM: i32 = -12;
/// Broken pipe (pipe closed).
pub const EPIPE: i32 = -32;
/// No message of desired type (message queue empty/full).
pub const ENOMSG: i32 = -42;
/// Timed out.
pub const ETIMEDOUT: i32 = -110;
/// Operation canceled (pipe resetting).
pub const ECANCELED: i32 = -125;
/// Success.
pub const OK: i32 = 0;

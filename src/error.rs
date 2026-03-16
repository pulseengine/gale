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
pub const ENOMEM: i32 = -12;
pub const EPIPE: i32 = -32;
pub const ENOMSG: i32 = -42;
pub const ETIMEDOUT: i32 = -110;
pub const ECANCELED: i32 = -125;
/// No space left on device / no free slots.
pub const ENOSPC: i32 = -28;
/// No such entry / not found.
pub const ENOENT: i32 = -2;
/// Value too large (arithmetic overflow).
pub const EOVERFLOW: i32 = -75;

/// Success return value.
pub const OK: i32 = 0;

} // verus!

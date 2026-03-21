//! Zephyr-compatible error codes.
//!
//! Maps directly to Zephyr's errno.h values used by kernel/sem.c.
/// Zephyr error codes used by kernel APIs.
/// Values match zephyr/include/zephyr/sys/errno_private.h.
pub const EINVAL: i32 = -22;
pub const EBUSY: i32 = -16;
pub const EAGAIN: i32 = -11;
pub const EPERM: i32 = -1;
pub const ENOMEM: i32 = -12;
pub const EPIPE: i32 = -32;
pub const ENOMSG: i32 = -35;
pub const ETIMEDOUT: i32 = -116;
pub const ECANCELED: i32 = -140;
/// No space left on device / no free slots.
pub const ENOSPC: i32 = -28;
/// No such entry / not found.
pub const ENOENT: i32 = -2;
/// Value too large (arithmetic overflow).
pub const EOVERFLOW: i32 = -139;
/// Bad file descriptor / object not found or type mismatch.
pub const EBADF: i32 = -9;
/// Address already in use / object already initialized.
pub const EADDRINUSE: i32 = -112;
/// Resource deadlock would occur.
pub const EDEADLK: i32 = -45;
/// Success return value.
pub const OK: i32 = 0;

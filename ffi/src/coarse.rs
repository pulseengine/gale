//! Coarsened FFI: `#[repr(C)]` state structs instead of individual scalar args.
//!
//! This module provides a v2 API that passes entire state structs through the
//! FFI boundary.  Benefits:
//!   - Fewer function parameters → fewer ABI mistakes
//!   - State struct is self-documenting (named fields)
//!   - Mutations happen in-place → no separate output pointers
//!
//! The existing v1 scalar API in `lib.rs` is untouched.  C shims can migrate
//! to these functions incrementally.
//!
//! Scope (proof of concept): semaphore, stack, pipe.

use gale::error::{EAGAIN, EBUSY, ECANCELED, EINVAL, ENOMEM, ENOMSG, EPIPE, OK};

// ---------------------------------------------------------------------------
// State structs
// ---------------------------------------------------------------------------

/// Semaphore state passed across the FFI boundary.
///
/// Maps to the count/limit fields of `struct k_sem`.
#[repr(C)]
pub struct GaleSemState {
    pub count: u32,
    pub limit: u32,
}

/// Stack state passed across the FFI boundary.
///
/// Maps to the count/capacity derived from pointer differences in
/// `struct k_stack` (count = next - base, capacity = top - base).
#[repr(C)]
pub struct GaleStackState {
    pub count: u32,
    pub capacity: u32,
}

/// Pipe state passed across the FFI boundary.
///
/// Maps to the used byte count, buffer capacity, and state flags of
/// `struct k_pipe`.
#[repr(C)]
pub struct GalePipeState {
    pub used: u32,
    pub size: u32,
    pub flags: u8,
}

// Flag constants must match lib.rs (and Zephyr's internal values).
const PIPE_FLAG_OPEN: u8 = 1;
const PIPE_FLAG_RESET: u8 = 2;

// ---------------------------------------------------------------------------
// Semaphore v2
// ---------------------------------------------------------------------------

/// Validate semaphore init parameters (v2, struct-based).
///
/// Checks that `state->limit > 0` and `state->count <= state->limit`.
///
/// Returns:
///   0 (OK)    — valid
///   -EINVAL   — null pointer, limit == 0, or count > limit
#[cfg(feature = "sem")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_validate_v2(state: *const GaleSemState) -> i32 {
    unsafe {
        if state.is_null() {
            return EINVAL;
        }
        let s = &*state;
        if s.limit == 0 || s.count > s.limit {
            EINVAL
        } else {
            OK
        }
    }
}

/// Give (signal) a semaphore: increment count up to limit (v2).
///
/// On success, `state->count` is updated in place.
///
/// Returns:
///   0 (OK)    — count incremented (or already at limit, saturated)
///   -EINVAL   — null pointer
#[cfg(feature = "sem")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_give_v2(state: *mut GaleSemState) -> i32 {
    unsafe {
        if state.is_null() {
            return EINVAL;
        }
        let s = &mut *state;
        // U-1 (STPA): `count < limit`, not `count != limit`. A caller with
        // corrupt state where count > limit (e.g., count=u32::MAX) would
        // pass `!=` and then overflow on `count + 1`. The `<` form
        // matches gale::sem::give_decide's precondition and invariant P1
        // (0 <= count <= limit).
        if s.count < s.limit {
            #[allow(clippy::arithmetic_side_effects)]
            {
                s.count += 1;
            }
        }
        OK
    }
}

/// Take (acquire) a semaphore: decrement count if > 0 (v2).
///
/// On success, `state->count` is decremented in place.
///
/// Returns:
///   0 (OK)    — count decremented
///   -EBUSY    — count is 0, nothing taken
///   -EINVAL   — null pointer
#[cfg(feature = "sem")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_take_v2(state: *mut GaleSemState) -> i32 {
    unsafe {
        if state.is_null() {
            return EINVAL;
        }
        let s = &mut *state;
        if s.count > 0 {
            // Verified: count > 0, no underflow.
            #[allow(clippy::arithmetic_side_effects)]
            {
                s.count -= 1;
            }
            OK
        } else {
            EBUSY
        }
    }
}

// ---------------------------------------------------------------------------
// Stack v2
// ---------------------------------------------------------------------------

/// Validate stack init parameters (v2, struct-based).
///
/// Returns:
///   0 (OK)    — valid capacity
///   -EINVAL   — null pointer or capacity == 0
#[cfg(feature = "stack")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_stack_init_validate_v2(state: *const GaleStackState) -> i32 {
    unsafe {
        if state.is_null() {
            return EINVAL;
        }
        let s = &*state;
        if s.capacity == 0 {
            EINVAL
        } else {
            OK
        }
    }
}

/// Push onto stack: increment count if below capacity (v2).
///
/// On success, `state->count` is incremented in place.  Caller stores
/// data at `stack->next` before calling, then advances `next`.
///
/// Returns:
///   0 (OK)    — space available, count incremented
///   -ENOMEM   — stack full
///   -EINVAL   — null pointer
#[cfg(feature = "stack")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_stack_push_v2(state: *mut GaleStackState) -> i32 {
    unsafe {
        if state.is_null() {
            return EINVAL;
        }
        let s = &mut *state;
        if s.count >= s.capacity {
            return ENOMEM;
        }
        // Verified: count < capacity <= u32::MAX, no overflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            s.count += 1;
        }
        OK
    }
}

/// Pop from stack: decrement count if > 0 (v2).
///
/// On success, `state->count` is decremented in place.  Caller reads
/// data after decrementing `next`.
///
/// Returns:
///   0 (OK)    — data available, count decremented
///   -EBUSY    — stack empty
///   -EINVAL   — null pointer
#[cfg(feature = "stack")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_stack_pop_v2(state: *mut GaleStackState) -> i32 {
    unsafe {
        if state.is_null() {
            return EINVAL;
        }
        let s = &mut *state;
        if s.count == 0 {
            return EBUSY;
        }
        // Verified: count > 0, no underflow.
        #[allow(clippy::arithmetic_side_effects)]
        {
            s.count -= 1;
        }
        OK
    }
}

// ---------------------------------------------------------------------------
// Pipe v2
// ---------------------------------------------------------------------------

/// Validate a pipe write and compute how many bytes can be written (v2).
///
/// On success, `state->used` is updated to the new byte count.
/// `*actual_len` receives the number of bytes that can actually be written
/// (may be less than `request_len` if the pipe is nearly full).
///
/// Returns:
///   0 (OK)       — write is valid, state->used updated
///   -EPIPE       — pipe closed
///   -ECANCELED   — pipe resetting
///   -EAGAIN      — pipe full
///   -ENOMSG      — zero-length request
///   -EINVAL      — null pointer or zero-capacity pipe
#[cfg(feature = "pipe")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_pipe_write_v2(
    state: *mut GalePipeState,
    request_len: u32,
    actual_len: *mut u32,
) -> i32 {
    unsafe {
        if state.is_null() || actual_len.is_null() {
            return EINVAL;
        }
        let s = &mut *state;
        if s.size == 0 {
            return EINVAL;
        }
        if (s.flags & PIPE_FLAG_RESET) != 0 {
            return ECANCELED;
        }
        if (s.flags & PIPE_FLAG_OPEN) == 0 {
            return EPIPE;
        }
        if request_len == 0 {
            return ENOMSG;
        }
        if s.used >= s.size {
            return EAGAIN;
        }

        #[allow(clippy::arithmetic_side_effects)]
        let free = s.size - s.used;
        let n = if request_len <= free {
            request_len
        } else {
            free
        };
        *actual_len = n;
        #[allow(clippy::arithmetic_side_effects)]
        {
            s.used += n;
        }
        OK
    }
}

/// Validate a pipe read and compute how many bytes can be read (v2).
///
/// On success, `state->used` is updated to the new byte count.
/// `*actual_len` receives the number of bytes that can actually be read
/// (may be less than `request_len` if the pipe has fewer bytes).
///
/// Returns:
///   0 (OK)       — read is valid, state->used updated
///   -EPIPE       — pipe closed and empty
///   -ECANCELED   — pipe resetting
///   -EAGAIN      — pipe empty (but open)
///   -ENOMSG      — zero-length request
///   -EINVAL      — null pointer
#[cfg(feature = "pipe")]
#[unsafe(no_mangle)]
pub extern "C" fn gale_pipe_read_v2(
    state: *mut GalePipeState,
    request_len: u32,
    actual_len: *mut u32,
) -> i32 {
    unsafe {
        if state.is_null() || actual_len.is_null() {
            return EINVAL;
        }
        let s = &mut *state;

        if (s.flags & PIPE_FLAG_RESET) != 0 {
            return ECANCELED;
        }
        if request_len == 0 {
            return ENOMSG;
        }
        if s.used == 0 {
            if (s.flags & PIPE_FLAG_OPEN) == 0 {
                return EPIPE;
            }
            return EAGAIN;
        }

        let n = if request_len <= s.used {
            request_len
        } else {
            s.used
        };
        *actual_len = n;
        #[allow(clippy::arithmetic_side_effects)]
        {
            s.used -= n;
        }
        OK
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use core::ptr;

    // -- Semaphore v2 -------------------------------------------------------

    #[test]
    fn sem_validate_ok() {
        let s = GaleSemState { count: 3, limit: 5 };
        assert_eq!(gale_sem_validate_v2(&s), OK);
    }

    #[test]
    fn sem_validate_zero_limit() {
        let s = GaleSemState { count: 0, limit: 0 };
        assert_eq!(gale_sem_validate_v2(&s), EINVAL);
    }

    #[test]
    fn sem_validate_count_exceeds_limit() {
        let s = GaleSemState { count: 6, limit: 5 };
        assert_eq!(gale_sem_validate_v2(&s), EINVAL);
    }

    #[test]
    fn sem_validate_null() {
        assert_eq!(gale_sem_validate_v2(ptr::null()), EINVAL);
    }

    #[test]
    fn sem_give_increments() {
        let mut s = GaleSemState { count: 2, limit: 5 };
        assert_eq!(gale_sem_give_v2(&mut s), OK);
        assert_eq!(s.count, 3);
    }

    #[test]
    fn sem_give_saturates_at_limit() {
        let mut s = GaleSemState { count: 5, limit: 5 };
        assert_eq!(gale_sem_give_v2(&mut s), OK);
        assert_eq!(s.count, 5);
    }

    #[test]
    fn sem_take_decrements() {
        let mut s = GaleSemState { count: 3, limit: 5 };
        assert_eq!(gale_sem_take_v2(&mut s), OK);
        assert_eq!(s.count, 2);
    }

    #[test]
    fn sem_take_empty_returns_ebusy() {
        let mut s = GaleSemState { count: 0, limit: 5 };
        assert_eq!(gale_sem_take_v2(&mut s), EBUSY);
        assert_eq!(s.count, 0);
    }

    // -- Stack v2 -----------------------------------------------------------

    #[test]
    fn stack_init_validate_ok() {
        let s = GaleStackState {
            count: 0,
            capacity: 10,
        };
        assert_eq!(gale_stack_init_validate_v2(&s), OK);
    }

    #[test]
    fn stack_init_validate_zero_capacity() {
        let s = GaleStackState {
            count: 0,
            capacity: 0,
        };
        assert_eq!(gale_stack_init_validate_v2(&s), EINVAL);
    }

    #[test]
    fn stack_push_increments() {
        let mut s = GaleStackState {
            count: 3,
            capacity: 10,
        };
        assert_eq!(gale_stack_push_v2(&mut s), OK);
        assert_eq!(s.count, 4);
    }

    #[test]
    fn stack_push_full_returns_enomem() {
        let mut s = GaleStackState {
            count: 10,
            capacity: 10,
        };
        assert_eq!(gale_stack_push_v2(&mut s), ENOMEM);
        assert_eq!(s.count, 10);
    }

    #[test]
    fn stack_pop_decrements() {
        let mut s = GaleStackState {
            count: 5,
            capacity: 10,
        };
        assert_eq!(gale_stack_pop_v2(&mut s), OK);
        assert_eq!(s.count, 4);
    }

    #[test]
    fn stack_pop_empty_returns_ebusy() {
        let mut s = GaleStackState {
            count: 0,
            capacity: 10,
        };
        assert_eq!(gale_stack_pop_v2(&mut s), EBUSY);
        assert_eq!(s.count, 0);
    }

    // -- Pipe v2 ------------------------------------------------------------

    #[test]
    fn pipe_write_ok() {
        let mut s = GalePipeState {
            used: 4,
            size: 16,
            flags: PIPE_FLAG_OPEN,
        };
        let mut actual: u32 = 0;
        assert_eq!(gale_pipe_write_v2(&mut s, 5, &mut actual), OK);
        assert_eq!(actual, 5);
        assert_eq!(s.used, 9);
    }

    #[test]
    fn pipe_write_clamps_to_free() {
        let mut s = GalePipeState {
            used: 14,
            size: 16,
            flags: PIPE_FLAG_OPEN,
        };
        let mut actual: u32 = 0;
        assert_eq!(gale_pipe_write_v2(&mut s, 10, &mut actual), OK);
        assert_eq!(actual, 2);
        assert_eq!(s.used, 16);
    }

    #[test]
    fn pipe_write_full_returns_eagain() {
        let mut s = GalePipeState {
            used: 16,
            size: 16,
            flags: PIPE_FLAG_OPEN,
        };
        let mut actual: u32 = 0;
        assert_eq!(gale_pipe_write_v2(&mut s, 1, &mut actual), EAGAIN);
    }

    #[test]
    fn pipe_write_closed_returns_epipe() {
        let mut s = GalePipeState {
            used: 0,
            size: 16,
            flags: 0,
        };
        let mut actual: u32 = 0;
        assert_eq!(gale_pipe_write_v2(&mut s, 1, &mut actual), EPIPE);
    }

    #[test]
    fn pipe_write_resetting_returns_ecanceled() {
        let mut s = GalePipeState {
            used: 0,
            size: 16,
            flags: PIPE_FLAG_OPEN | PIPE_FLAG_RESET,
        };
        let mut actual: u32 = 0;
        assert_eq!(gale_pipe_write_v2(&mut s, 1, &mut actual), ECANCELED);
    }

    #[test]
    fn pipe_read_ok() {
        let mut s = GalePipeState {
            used: 10,
            size: 16,
            flags: PIPE_FLAG_OPEN,
        };
        let mut actual: u32 = 0;
        assert_eq!(gale_pipe_read_v2(&mut s, 5, &mut actual), OK);
        assert_eq!(actual, 5);
        assert_eq!(s.used, 5);
    }

    #[test]
    fn pipe_read_clamps_to_used() {
        let mut s = GalePipeState {
            used: 3,
            size: 16,
            flags: PIPE_FLAG_OPEN,
        };
        let mut actual: u32 = 0;
        assert_eq!(gale_pipe_read_v2(&mut s, 10, &mut actual), OK);
        assert_eq!(actual, 3);
        assert_eq!(s.used, 0);
    }

    #[test]
    fn pipe_read_empty_open_returns_eagain() {
        let mut s = GalePipeState {
            used: 0,
            size: 16,
            flags: PIPE_FLAG_OPEN,
        };
        let mut actual: u32 = 0;
        assert_eq!(gale_pipe_read_v2(&mut s, 1, &mut actual), EAGAIN);
    }

    #[test]
    fn pipe_read_empty_closed_returns_epipe() {
        let mut s = GalePipeState {
            used: 0,
            size: 16,
            flags: 0,
        };
        let mut actual: u32 = 0;
        assert_eq!(gale_pipe_read_v2(&mut s, 1, &mut actual), EPIPE);
    }

    #[test]
    fn pipe_null_pointers() {
        let mut s = GalePipeState {
            used: 0,
            size: 16,
            flags: PIPE_FLAG_OPEN,
        };
        assert_eq!(
            gale_pipe_write_v2(ptr::null_mut(), 1, &mut 0u32 as *mut u32),
            EINVAL
        );
        assert_eq!(gale_pipe_write_v2(&mut s, 1, ptr::null_mut()), EINVAL);
        assert_eq!(
            gale_pipe_read_v2(ptr::null_mut(), 1, &mut 0u32 as *mut u32),
            EINVAL
        );
        assert_eq!(gale_pipe_read_v2(&mut s, 1, ptr::null_mut()), EINVAL);
    }
}

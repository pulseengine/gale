//! Phase 1 C FFI: verified kernel primitives for Zephyr.
//!
//! ## Semaphore (gale_sem_count_*)
//!
//! Pure functions replacing count arithmetic from kernel/sem.c:
//!   sem.c:48-50   CHECKIF(limit == 0U || initial_count > limit)
//!   sem.c:110     sem->count += (sem->count != sem->limit) ? 1U : 0U;
//!   sem.c:143-144 if (likely(sem->count > 0U)) { sem->count--; }
//!
//! Verified: P1-P3, P5-P6, P9 (count bounds, no overflow/underflow).
//!
//! ## Mutex (gale_mutex_*_validate)
//!
//! Pure functions replacing state machine validation from kernel/mutex.c:
//!   mutex.c:121-129  lock_count/owner checks + lock_count++
//!   mutex.c:238-268  owner checks + lock_count--
//!
//! Verified: M3-M7, M10 (ownership, reentrancy, no overflow/underflow).

#![cfg_attr(not(any(test, kani)), no_std)]
// FFI boundary crate — unsafe is inherent (no_mangle, raw pointers).
// The verified pure logic lives in the `gale` crate which denies unsafe.

use gale::error::{EBUSY, EINVAL, OK};

// ---------------------------------------------------------------------------
// FFI exports — pure count arithmetic
// ---------------------------------------------------------------------------

/// Validate semaphore init parameters.
///
/// sem.c:48-50:
///   CHECKIF(limit == 0U || initial_count > limit) { return -EINVAL; }
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_count_init(initial_count: u32, limit: u32) -> i32 {
    if limit == 0 || initial_count > limit {
        EINVAL
    } else {
        OK
    }
}

/// Compute new count after give with no waiters.
///
/// sem.c:110:
///   sem->count += (sem->count != sem->limit) ? 1U : 0U;
///
/// Safe: count < limit <= u32::MAX when count != limit, so count+1
/// cannot overflow.  Verified by Verus lemma_give_saturation.
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_count_give(count: u32, limit: u32) -> u32 {
    if count != limit {
        // Verified: count < limit <= u32::MAX, no overflow possible.
        #[allow(clippy::arithmetic_side_effects)]
        let new_count = count + 1;
        new_count
    } else {
        count
    }
}

/// Attempt to decrement count for take.
///
/// sem.c:143-144:
///   if (likely(sem->count > 0U)) { sem->count--; ret = 0; }
///
/// SAFETY: `count` must point to a valid `unsigned int` (Zephyr's
/// sem->count).  Called under Zephyr's spinlock — no concurrent access.
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_count_take(count: *mut u32) -> i32 {
    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        if count.is_null() {
            return EINVAL;
        }
        if *count > 0 {
            // Verified: count > 0, so count-1 >= 0, no underflow.
            #[allow(clippy::arithmetic_side_effects)]
            {
                *count -= 1;
            }
            OK
        } else {
            EBUSY
        }
    }
}

// ---------------------------------------------------------------------------
// Mutex FFI exports — state machine validation
// ---------------------------------------------------------------------------
//
// These pure functions replace the safety-critical state machine checks
// and arithmetic from kernel/mutex.c:
//
//   mutex.c:121-129  lock_count/owner checks + lock_count++
//   mutex.c:238-268  owner checks + lock_count--
//
// All other mutex logic (wait queue, scheduling, priority inheritance,
// tracing) remains native Zephyr C in gale_mutex.c.
//
// Verified by Verus (SMT/Z3):
//   M3:  lock unlocked -> lock_count = 1
//   M4:  reentrant lock -> lock_count + 1
//   M5:  contended -> -EBUSY
//   M6a: unlock no owner -> -EINVAL
//   M6b: unlock wrong owner -> -EPERM
//   M7:  reentrant unlock -> lock_count - 1
//   M10: no overflow/underflow in lock_count

use gale::error::EPERM;

/// Validate a mutex lock attempt.
///
/// mutex.c:121-129:
///   if ((lock_count == 0) || (owner == _current)) {
///       lock_count++;
///       owner = _current;
///   }
///
/// Arguments:
///   lock_count:       current mutex->lock_count
///   owner_is_null:    1 if mutex->owner == NULL, 0 otherwise
///   owner_is_current: 1 if mutex->owner == _current, 0 otherwise
///   new_lock_count:   pointer to receive the new lock_count value
///
/// Returns:
///   0 (OK)    — lock acquired, *new_lock_count set, caller sets owner
///   -EBUSY    — contended (different owner holds it)
#[unsafe(no_mangle)]
pub extern "C" fn gale_mutex_lock_validate(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
    new_lock_count: *mut u32,
) -> i32 {
    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        if new_lock_count.is_null() {
            return EINVAL;
        }

        if lock_count == 0 || owner_is_null != 0 {
            // Mutex unlocked — acquire (M3).
            *new_lock_count = 1;
            OK
        } else if owner_is_current != 0 {
            // Reentrant lock — same owner (M4, M10).
            match lock_count.checked_add(1) {
                Some(n) => {
                    *new_lock_count = n;
                    OK
                }
                None => {
                    // Overflow would violate M10.
                    EINVAL
                }
            }
        } else {
            // Different owner — contended (M5).
            EBUSY
        }
    }
}

/// Return code: mutex still held (reentrant unlock, lock_count decremented).
pub const GALE_MUTEX_RELEASED: i32 = 1;
/// Return code: mutex fully unlocked (caller should check waiters).
pub const GALE_MUTEX_UNLOCKED: i32 = 0;

/// Validate a mutex unlock attempt.
///
/// mutex.c:238-268:
///   CHECKIF(owner == NULL)    -> -EINVAL
///   CHECKIF(owner != current) -> -EPERM
///   if (lock_count > 1)       -> lock_count--; return 0;
///   else                      -> fully unlock, handle waiters
///
/// Arguments:
///   lock_count:       current mutex->lock_count
///   owner_is_null:    1 if mutex->owner == NULL, 0 otherwise
///   owner_is_current: 1 if mutex->owner == _current, 0 otherwise
///   new_lock_count:   pointer to receive the new lock_count value
///
/// Returns:
///   1 (GALE_MUTEX_RELEASED) — still held, *new_lock_count decremented
///   0 (GALE_MUTEX_UNLOCKED) — fully unlocked, *new_lock_count = 0,
///                             caller should check waiters
///   -EINVAL                 — not locked (no owner)
///   -EPERM                  — not the owner
#[unsafe(no_mangle)]
pub extern "C" fn gale_mutex_unlock_validate(
    lock_count: u32,
    owner_is_null: u32,
    owner_is_current: u32,
    new_lock_count: *mut u32,
) -> i32 {
    // SAFETY: Zephyr guarantees valid pointer under spinlock.
    unsafe {
        if new_lock_count.is_null() {
            return EINVAL;
        }

        // M6a: not locked
        if owner_is_null != 0 {
            return EINVAL;
        }

        // M6b: not owner
        if owner_is_current == 0 {
            return EPERM;
        }

        // M7: reentrant release (lock_count > 1)
        if lock_count > 1 {
            // Verified: lock_count > 1, so lock_count - 1 >= 1, no underflow.
            #[allow(clippy::arithmetic_side_effects)]
            {
                *new_lock_count = lock_count - 1;
            }
            GALE_MUTEX_RELEASED
        } else {
            // Fully unlocked — caller handles waiter transfer.
            *new_lock_count = 0;
            GALE_MUTEX_UNLOCKED
        }
    }
}

// Panic handler for no_std
#[cfg(not(any(test, kani)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — semaphore
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_sem_proofs {
    use super::*;

    /// P1/P2: init rejects limit==0 and initial_count > limit,
    /// accepts all valid combinations.
    #[kani::proof]
    fn sem_init_validates_all_params() {
        let initial: u32 = kani::any();
        let limit: u32 = kani::any();
        let ret = gale_sem_count_init(initial, limit);
        if limit == 0 || initial > limit {
            assert!(ret == EINVAL);
        } else {
            assert!(ret == OK);
        }
    }

    /// P3/P9: give never overflows and saturates at limit.
    #[kani::proof]
    fn sem_give_no_overflow() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();
        // Pre: valid semaphore state
        kani::assume(limit > 0);
        kani::assume(count <= limit);

        let new_count = gale_sem_count_give(count, limit);

        // Post: result in bounds
        assert!(new_count <= limit);
        // Post: correct arithmetic
        if count < limit {
            assert!(new_count == count + 1);
        } else {
            assert!(new_count == count);
        }
    }

    /// P5/P6/P9: take never underflows and returns correct status.
    #[kani::proof]
    fn sem_take_no_underflow() {
        let mut count: u32 = kani::any();
        let original = count;

        let ret = gale_sem_count_take(&mut count);

        if original > 0 {
            assert!(ret == OK);
            assert!(count == original - 1);
        } else {
            assert!(ret == EBUSY);
            assert!(count == 0);
        }
    }

    /// Null pointer returns EINVAL.
    #[kani::proof]
    fn sem_take_null_returns_einval() {
        let ret = gale_sem_count_take(core::ptr::null_mut());
        assert!(ret == EINVAL);
    }

    /// Give-take roundtrip: giving then taking returns to original count.
    #[kani::proof]
    fn sem_give_take_roundtrip() {
        let count: u32 = kani::any();
        let limit: u32 = kani::any();
        kani::assume(limit > 0);
        kani::assume(count < limit); // below limit so give increments

        let mut after_give = gale_sem_count_give(count, limit);
        assert!(after_give == count + 1);

        let ret = gale_sem_count_take(&mut after_give);
        assert!(ret == OK);
        assert!(after_give == count);
    }

    /// Repeated gives saturate at limit.
    #[kani::proof]
    #[kani::unwind(4)]
    fn sem_repeated_give_saturates() {
        let limit: u32 = kani::any();
        kani::assume(limit > 0 && limit <= 8); // bound for tractability
        let mut count = limit; // start at limit

        // 3 gives should all saturate
        let mut i: u32 = 0;
        while i < 3 {
            count = gale_sem_count_give(count, limit);
            assert!(count == limit);
            i += 1;
        }
    }
}

// ---------------------------------------------------------------------------
// Kani bounded model checking — mutex
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_mutex_proofs {
    use super::*;

    /// M3: lock when unlocked sets lock_count = 1.
    #[kani::proof]
    fn mutex_lock_unlocked() {
        let mut new_lc: u32 = 0;
        let ret = gale_mutex_lock_validate(0, 1, 0, &mut new_lc);
        assert!(ret == OK);
        assert!(new_lc == 1);
    }

    /// M4/M10: reentrant lock increments without overflow.
    #[kani::proof]
    fn mutex_lock_reentrant_no_overflow() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0 && lock_count < u32::MAX);

        let mut new_lc: u32 = 0;
        let ret = gale_mutex_lock_validate(lock_count, 0, 1, &mut new_lc);
        assert!(ret == OK);
        assert!(new_lc == lock_count + 1);
    }

    /// M4/M10: reentrant lock at u32::MAX returns error (overflow protection).
    #[kani::proof]
    fn mutex_lock_reentrant_overflow_protection() {
        let mut new_lc: u32 = 0;
        let ret = gale_mutex_lock_validate(u32::MAX, 0, 1, &mut new_lc);
        assert!(ret == EINVAL);
    }

    /// M5: lock by different owner returns -EBUSY.
    #[kani::proof]
    fn mutex_lock_contended() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0);

        let mut new_lc: u32 = 0;
        let ret = gale_mutex_lock_validate(lock_count, 0, 0, &mut new_lc);
        assert!(ret == EBUSY);
    }

    /// M6a: unlock when not locked returns -EINVAL.
    #[kani::proof]
    fn mutex_unlock_not_locked() {
        let mut new_lc: u32 = 0;
        let ret = gale_mutex_unlock_validate(0, 1, 0, &mut new_lc);
        assert!(ret == EINVAL);
    }

    /// M6b: unlock by wrong owner returns -EPERM.
    #[kani::proof]
    fn mutex_unlock_not_owner() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0);

        let mut new_lc: u32 = 0;
        let ret = gale_mutex_unlock_validate(lock_count, 0, 0, &mut new_lc);
        assert!(ret == EPERM);
    }

    /// M7: reentrant unlock decrements correctly.
    #[kani::proof]
    fn mutex_unlock_reentrant() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 1);

        let mut new_lc: u32 = 0;
        let ret = gale_mutex_unlock_validate(lock_count, 0, 1, &mut new_lc);
        assert!(ret == GALE_MUTEX_RELEASED);
        assert!(new_lc == lock_count - 1);
    }

    /// M9: final unlock returns UNLOCKED.
    #[kani::proof]
    fn mutex_unlock_final() {
        let mut new_lc: u32 = 0;
        let ret = gale_mutex_unlock_validate(1, 0, 1, &mut new_lc);
        assert!(ret == GALE_MUTEX_UNLOCKED);
        assert!(new_lc == 0);
    }

    /// Lock-unlock roundtrip: lock then unlock returns to lock_count = 0.
    #[kani::proof]
    fn mutex_lock_unlock_roundtrip() {
        let mut new_lc: u32 = 0;
        // Lock (unlocked mutex)
        let ret = gale_mutex_lock_validate(0, 1, 0, &mut new_lc);
        assert!(ret == OK);
        assert!(new_lc == 1);

        // Unlock
        let ret = gale_mutex_unlock_validate(new_lc, 0, 1, &mut new_lc);
        assert!(ret == GALE_MUTEX_UNLOCKED);
        assert!(new_lc == 0);
    }

    /// Reentrant lock-unlock roundtrip preserves lock_count.
    #[kani::proof]
    fn mutex_reentrant_roundtrip() {
        let lock_count: u32 = kani::any();
        kani::assume(lock_count > 0 && lock_count < u32::MAX);

        let mut new_lc: u32 = 0;
        // Reentrant lock
        let ret = gale_mutex_lock_validate(lock_count, 0, 1, &mut new_lc);
        assert!(ret == OK);
        assert!(new_lc == lock_count + 1);

        // Reentrant unlock
        let ret = gale_mutex_unlock_validate(new_lc, 0, 1, &mut new_lc);
        assert!(ret == GALE_MUTEX_RELEASED);
        assert!(new_lc == lock_count);
    }

    /// Null pointer to lock_validate returns EINVAL.
    #[kani::proof]
    fn mutex_lock_null_returns_einval() {
        let ret = gale_mutex_lock_validate(0, 1, 0, core::ptr::null_mut());
        assert!(ret == EINVAL);
    }

    /// Null pointer to unlock_validate returns EINVAL.
    #[kani::proof]
    fn mutex_unlock_null_returns_einval() {
        let ret = gale_mutex_unlock_validate(1, 0, 1, core::ptr::null_mut());
        assert!(ret == EINVAL);
    }
}

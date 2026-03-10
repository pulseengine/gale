//! Phase 1 C FFI: verified count arithmetic for Zephyr's k_sem.
//!
//! These pure functions replace exactly three lines from kernel/sem.c:
//!
//!   sem.c:48-50   CHECKIF(limit == 0U || initial_count > limit)
//!   sem.c:110     sem->count += (sem->count != sem->limit) ? 1U : 0U;
//!   sem.c:143-144 if (likely(sem->count > 0U)) { sem->count--; }
//!
//! All other semaphore logic (wait queue, scheduling, tracing, poll)
//! remains native Zephyr C in gale_sem.c.
//!
//! Verified by Verus (SMT/Z3) + Rocq proofs:
//!   P1: 0 <= count <= limit      (gale_sem_count_init)
//!   P2: limit > 0                (gale_sem_count_init)
//!   P3: give: count+1 capped     (gale_sem_count_give)
//!   P5: take: count-1 when >0    (gale_sem_count_take)
//!   P6: take: -EBUSY when ==0    (gale_sem_count_take)
//!   P9: no overflow/underflow    (all three)

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

// Panic handler for no_std
#[cfg(not(any(test, kani)))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// ---------------------------------------------------------------------------
// Kani bounded model checking
// ---------------------------------------------------------------------------

#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// P1/P2: init rejects limit==0 and initial_count > limit,
    /// accepts all valid combinations.
    #[kani::proof]
    fn init_validates_all_params() {
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
    fn give_no_overflow() {
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
    fn take_no_underflow() {
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
    fn take_null_returns_einval() {
        let ret = gale_sem_count_take(core::ptr::null_mut());
        assert!(ret == EINVAL);
    }

    /// Give-take roundtrip: giving then taking returns to original count.
    #[kani::proof]
    fn give_take_roundtrip() {
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
    fn repeated_give_saturates() {
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

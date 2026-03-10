//! C FFI bindings for the Gale verified kernel semaphore.
//!
//! These functions expose the formally verified semaphore logic to Zephyr's
//! C kernel.  The C shim (gale_sem.c) calls these from the z_impl_k_sem_*
//! functions, handling Zephyr-specific concerns (spinlocks, scheduling,
//! tracing, poll events) on the C side.
//!
//! Architecture:
//!   Zephyr test suite (C)
//!     → z_impl_k_sem_give (gale_sem.c — handles spinlock + scheduling)
//!       → gale_sem_give (this file — verified count logic)
//!         → gale::sem::Semaphore::give (plain/src/sem.rs — proven correct)

#![no_std]
#![deny(unsafe_code)]
#![allow(clippy::not_unsafe_ptr_arg_deref)]

use core::ptr;
use gale::error::{EINVAL, OK};
use gale::priority::Priority;
use gale::sem::{GiveResult, Semaphore};
use gale::thread::Thread;

/// Opaque semaphore handle for the C side.
/// Wraps a pool index into the static semaphore table.
#[repr(C)]
pub struct GaleSem {
    pool_index: u32,
}

/// Result of a give operation, communicated to C.
#[repr(C)]
pub struct GaleGiveResult {
    /// 0 = incremented, 1 = woke thread, 2 = saturated
    pub kind: u32,
    /// If kind == 1 (woke thread): the thread ID that was woken.
    pub woken_thread_id: u32,
    /// If kind == 1: the thread's priority value.
    pub woken_thread_priority: u32,
}

// ---------------------------------------------------------------------------
// Static semaphore pool
// ---------------------------------------------------------------------------

/// Maximum semaphores (overridable via GALE_MAX_SEMS env at build time).
const MAX_SEMS: usize = 32;

struct SemSlot {
    in_use: bool,
    sem: Option<Semaphore>,
}

/// Global semaphore pool.  Access is safe because Zephyr holds its spinlock
/// around all semaphore operations (single-threaded access guaranteed by
/// the kernel's locking protocol).
static mut SEM_POOL: [SemSlot; MAX_SEMS] = {
    const EMPTY: SemSlot = SemSlot {
        in_use: false,
        sem: None,
    };
    [EMPTY; MAX_SEMS]
};

fn alloc_slot() -> Option<usize> {
    // SAFETY: called under Zephyr's spinlock — no concurrent access.
    #[allow(unsafe_code)]
    unsafe {
        for (i, slot) in SEM_POOL.iter_mut().enumerate() {
            if !slot.in_use {
                slot.in_use = true;
                return Some(i);
            }
        }
    }
    None
}

fn get_sem(handle: *const GaleSem) -> Option<&'static mut Semaphore> {
    #[allow(unsafe_code)]
    unsafe {
        if handle.is_null() {
            return None;
        }
        let idx = (*handle).pool_index as usize;
        if idx >= MAX_SEMS {
            return None;
        }
        SEM_POOL[idx].sem.as_mut()
    }
}

// ---------------------------------------------------------------------------
// FFI exports
// ---------------------------------------------------------------------------

/// Initialize a semaphore.  Returns 0 on success, -EINVAL on invalid params,
/// -ENOMEM (-12) if the pool is exhausted.
///
/// The C shim stores the returned handle in the k_sem struct.
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_init(
    handle: *mut GaleSem,
    initial_count: u32,
    limit: u32,
) -> i32 {
    if handle.is_null() {
        return EINVAL;
    }

    let slot_idx = match alloc_slot() {
        Some(i) => i,
        None => return -12, // ENOMEM
    };

    let sem = match Semaphore::init(initial_count, limit) {
        Ok(s) => s,
        Err(e) => {
            // Free the slot we just allocated
            #[allow(unsafe_code)]
            unsafe {
                SEM_POOL[slot_idx].in_use = false;
            }
            return e;
        }
    };

    #[allow(unsafe_code)]
    unsafe {
        SEM_POOL[slot_idx].sem = Some(sem);
        (*handle).pool_index = slot_idx as u32;
    }

    OK
}

/// Give (signal) a semaphore.  Returns the result in the output struct.
///
/// The C shim is responsible for:
/// - Acquiring/releasing the spinlock
/// - Calling z_ready_thread + z_reschedule if kind == 1 (woke thread)
/// - Calling handle_poll_events if kind == 0 (incremented)
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_give(handle: *const GaleSem, result: *mut GaleGiveResult) -> i32 {
    let sem = match get_sem(handle) {
        Some(s) => s,
        None => return EINVAL,
    };

    let give_result = sem.give();

    #[allow(unsafe_code)]
    unsafe {
        match give_result {
            GiveResult::Incremented => {
                (*result).kind = 0;
                (*result).woken_thread_id = 0;
                (*result).woken_thread_priority = 0;
            }
            GiveResult::WokeThread(thread) => {
                (*result).kind = 1;
                (*result).woken_thread_id = thread.id;
                (*result).woken_thread_priority = thread.priority.get();
            }
            GiveResult::Saturated => {
                (*result).kind = 2;
                (*result).woken_thread_id = 0;
                (*result).woken_thread_priority = 0;
            }
        }
    }

    OK
}

/// Try to take (acquire) a semaphore without blocking.
/// Returns 0 (OK) if acquired, -EBUSY if count is zero.
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_try_take(handle: *const GaleSem) -> i32 {
    match get_sem(handle) {
        Some(sem) => sem.try_take(),
        None => EINVAL,
    }
}

/// Enqueue a thread as a waiter on the semaphore (blocking take path).
/// Called by the C shim when count == 0 and the thread should block.
/// Returns true (1) if the thread was enqueued, false (0) if the wait
/// queue is full.
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_pend_thread(
    handle: *const GaleSem,
    thread_id: u32,
    priority: u32,
) -> i32 {
    let sem = match get_sem(handle) {
        Some(s) => s,
        None => return 0,
    };

    let prio = match Priority::new(priority) {
        Ok(p) => p,
        Err(_) => return 0,
    };

    let mut thread = Thread::new(thread_id, prio);
    thread.dispatch();

    if sem.take_blocking(thread) {
        1 // acquired immediately (shouldn't happen if C side checked count)
    } else {
        2 // thread was enqueued
    }
}

/// Reset the semaphore: count → 0, wake all waiters.
/// Returns the number of waiters that were woken.
///
/// The C shim is responsible for setting return values on the woken
/// Zephyr threads and calling z_ready_thread for each.
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_reset(handle: *const GaleSem) -> u32 {
    match get_sem(handle) {
        Some(sem) => sem.reset() as u32,
        None => 0,
    }
}

/// Get the current count.
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_count_get(handle: *const GaleSem) -> u32 {
    match get_sem(handle) {
        Some(sem) => sem.count_get(),
        None => 0,
    }
}

/// Get the limit.
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_limit_get(handle: *const GaleSem) -> u32 {
    match get_sem(handle) {
        Some(sem) => sem.limit_get(),
        None => 0,
    }
}

/// Free a semaphore slot (called when k_sem is destroyed).
#[unsafe(no_mangle)]
pub extern "C" fn gale_sem_free(handle: *const GaleSem) {
    #[allow(unsafe_code)]
    unsafe {
        if handle.is_null() {
            return;
        }
        let idx = (*handle).pool_index as usize;
        if idx < MAX_SEMS {
            SEM_POOL[idx].sem = None;
            SEM_POOL[idx].in_use = false;
        }
    }
}

// Panic handler for no_std
#[cfg(not(test))]
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

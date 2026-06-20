//! gale-kiln — the gale kernel as a WebAssembly **Component** (`world host`).
//!
//! Implements `gale:kernel/{sem,msgq,mutex,event}` (see `../../wit/gale.wit`)
//! directly over the verified `gale::*` decision functions — the same
//! machine-checked logic (Verus/Rocq/Lean + Kani) that ships in the Zephyr
//! drop-in and the wasm-cross-LTO modules. This is the **no-C-FFI host side**
//! (Phase 2 / SWREQ-KILN-002): an application component imports `gale:kernel`
//! and these decisions resolve to direct Rust calls into `gale`.
//!
//! Enum mapping uses the crate's stable `as i32` discriminants (the same
//! convention the browser demo uses); declaration order in the WIT matches the
//! gale `Decision` enums, so the mapping is a straight discriminant copy.
#![allow(warnings)]

wit_bindgen::generate!({
    world: "host",
    path: "../../wit",
});

use exports::gale::kernel::event::Guest as EventGuest;
use exports::gale::kernel::msgq::{GetDecision, Guest as MsgqGuest, PutDecision};
use exports::gale::kernel::mutex::{Guest as MutexGuest, LockDecision, UnlockDecision};
use exports::gale::kernel::sem::{GiveDecision, Guest as SemGuest, TakeDecision};
use exports::gale::kernel::event::WaitDecision;

struct Component;

impl SemGuest for Component {
    fn give(count: u32, limit: u32, has_waiter: bool) -> GiveDecision {
        match gale::sem::give_decide(count, limit, has_waiter) as i32 {
            0 => GiveDecision::Wake,
            1 => GiveDecision::Increment,
            _ => GiveDecision::Saturated,
        }
    }
    fn take(count: u32, is_no_wait: bool) -> TakeDecision {
        match gale::sem::take_decide(count, is_no_wait) as i32 {
            0 => TakeDecision::Acquired,
            1 => TakeDecision::WouldBlock,
            _ => TakeDecision::Pend,
        }
    }
}

impl MsgqGuest for Component {
    fn put(write_idx: u32, used: u32, max: u32, has_waiter: bool, is_no_wait: bool) -> PutDecision {
        match gale::msgq::put_decide(write_idx, used, max, has_waiter, is_no_wait).decision as i32 {
            0 => PutDecision::Store,
            1 => PutDecision::WakeReader,
            2 => PutDecision::Pend,
            _ => PutDecision::Full,
        }
    }
    fn get(read_idx: u32, used: u32, max: u32, has_waiter: bool, is_no_wait: bool) -> GetDecision {
        match gale::msgq::get_decide(read_idx, used, max, has_waiter, is_no_wait).decision as i32 {
            0 => GetDecision::Read,
            1 => GetDecision::WakeWriter,
            2 => GetDecision::Pend,
            _ => GetDecision::Empty,
        }
    }
}

impl MutexGuest for Component {
    fn lock(lock_count: u32, owner_is_null: bool, owner_is_current: bool, is_no_wait: bool) -> LockDecision {
        match gale::mutex::lock_decide(lock_count, owner_is_null, owner_is_current, is_no_wait) as i32 {
            0 => LockDecision::Acquire,
            1 => LockDecision::Reentrant,
            2 => LockDecision::Pend,
            3 => LockDecision::Busy,
            _ => LockDecision::Overflow,
        }
    }
    fn unlock(lock_count: u32, owner_is_null: bool, owner_is_current: bool) -> UnlockDecision {
        match gale::mutex::unlock_decide(lock_count, owner_is_null, owner_is_current) as i32 {
            0 => UnlockDecision::NotLocked,
            1 => UnlockDecision::NotOwner,
            2 => UnlockDecision::Released,
            _ => UnlockDecision::FullyUnlocked,
        }
    }
}

impl EventGuest for Component {
    fn post(current_events: u32, new_events: u32, mask: u32) -> u32 {
        gale::event::post_decide(current_events, new_events, mask)
    }
    fn wait(current_events: u32, desired: u32, wait_all: bool, is_no_wait: bool) -> WaitDecision {
        // gale wait_type: 0 = ANY, 1 = ALL
        let wait_type: u8 = if wait_all { 1 } else { 0 };
        match gale::event::wait_decide(current_events, desired, wait_type, is_no_wait).decision as i32 {
            0 => WaitDecision::Matched,
            1 => WaitDecision::Pend,
            _ => WaitDecision::Timeout,
        }
    }
}

export!(Component);

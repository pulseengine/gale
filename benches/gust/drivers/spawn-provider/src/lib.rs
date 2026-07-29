//! gust:os `spawn` provider (world `spawn-provider`, wit-os/gust-os.wit) — Task 6
//! Step 5. STATELESS by construction: it owns no task table and includes no
//! executor. `start`/`poll` marshal onto the node's single executor instance
//! through the `gust:sched/tasks` import (exported by exec-provider, which holds
//! the one `Tasks`), so the handle this provider returns IS the executor's handle
//! and means the same task to `timer.sleep` and `exec.state`.
//!
//! It previously `#[path]`-included `plain/src/executor.rs` and kept its own
//! `static mut TASKS`. Composed, that was a second scheduler: `spawn.start`'s
//! handle indexed a table nothing else could see. The include is gone rather than
//! shared — a provider must import every capability it does not own.
#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}
use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator]
static A: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "spawn-provider", path: "../wit-os", generate_all });
use crate::gust::sched::tasks;
use exports::gust::os::spawn::Guest;

/// `tasks::state`'s encoding (= `gust:os/exec.state`'s). `spawn.poll` reports a
/// different one, documented in the WIT, so the mapping is explicit here.
const ST_FREE: u32 = 0;
const ST_DONE: u32 = 2;
const INVALID: u32 = 0xFFFF_FFFF;

struct P;
impl Guest for P {
    /// Register task `entry` and make it immediately runnable. This WIT ABI
    /// carries no priority/deadline (unlike exec-provider's richer C-ABI probe
    /// surface), so v1 admits at a fixed neutral priority and `wake`s it right
    /// away — `spawn` semantics are "ready now", not tickless-deadline-driven
    /// (that half of the executor is exercised by gust_exec_probe instead).
    /// `admit`/`wake` perform the ENTIRE decision, now in the owning instance;
    /// `entry` is not otherwise interpreted here (v1 has no per-entry dispatch
    /// table — the owner's `poll-task` seam is the dispatch point). The returned
    /// handle is returned UNCHANGED: no mapping table, so nothing here can
    /// desynchronise from the executor's own numbering.
    fn start(entry: u32) -> u32 {
        let _ = entry;
        let h = tasks::admit(0);
        tasks::wake(h);
        h
    }

    /// Poll task `handle`: drives one full `poll-round` (cooperative, so any
    /// poll call drains every currently-ready task, not only `handle`) and
    /// reports `handle`'s resulting state as the WIT-documented code: `0` =
    /// pending, `1` = done, `0xFFFF_FFFF` = invalid handle. The state query
    /// comes first so an invalid handle is rejected without driving a round,
    /// as before.
    fn poll(handle: u32) -> u32 {
        match tasks::state(handle) {
            ST_FREE | INVALID => INVALID,
            _ => {
                tasks::poll_round();
                match tasks::state(handle) {
                    ST_DONE => 1,
                    ST_FREE | INVALID => INVALID,
                    _ => 0,
                }
            }
        }
    }
}
export!(P);

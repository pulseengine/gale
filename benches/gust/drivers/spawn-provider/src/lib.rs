//! gust:os `spawn` provider (world `spawn-provider`, wit-os/gust-os.wit) — Task 6
//! Step 5. Backs `start`/`poll` with the SAME verified async executor Task 6's
//! exec-provider dissolves, not a hand-written placeholder: `plain/src/executor.rs`
//! (verus-strip's output of the Verus+Kani-proven src/executor.rs) is included
//! verbatim below, exactly as in drivers/exec-provider/src/lib.rs. `start`/`poll`
//! keep the byte-identical `func(u32) -> u32` WIT ABI; only marshalling lives here.
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
use exports::gust::os::spawn::Guest;

#[path = "../../../../../plain/src/executor.rs"]
mod executor;
use executor::{TaskState, Tasks, MAX_TASKS};

/// Resolve the executor's trusted `extern "C" poll_task` seam INSIDE the module by
/// forwarding it to the WIT-typed `gust:os/taskdisp.poll-task` import. This is the
/// "WIT-typed task-dispatch seam" RESULTS.md deferred: with it, no raw
/// `env::poll_task` core import survives, so `wasm-tools component new` accepts the
/// module and the ts-node composes like step-1/2. The contract is unchanged
/// (dispatch task `id` once; 1 = completed) — only the import's TYPE moved from raw
/// C-ABI to WIT.
#[no_mangle]
pub extern "C" fn poll_task(id: u32) -> u32 {
    crate::gust::os::taskdisp::poll_task(id)
}

// Lazily-initialized executor state. NOT `Option<Tasks>`: the niche-encoded
// `None` discriminant is one initialized byte inside an otherwise-zero struct,
// which wasm-ld splits across the .data end / .bss tail — exactly the
// straddling-static geometry synth's --shadow-stack-size shrink refuses
// (VCR-MEM-001/#678) when this module is meld-fused into the ts-node. A
// MaybeUninit table + separate flag is all-zero at init, so the whole table
// lands in .bss and the node's data segment stays clean.
static mut TASKS_INIT: u32 = 0;
static mut TASKS: core::mem::MaybeUninit<Tasks> = core::mem::MaybeUninit::uninit();

#[allow(static_mut_refs)]
unsafe fn tasks() -> &'static mut Tasks {
    if TASKS_INIT == 0 {
        TASKS.write(Tasks::new());
        TASKS_INIT = 1;
    }
    TASKS.assume_init_mut()
}

struct P;
impl Guest for P {
    /// Register task `entry` and make it immediately runnable. This WIT ABI
    /// carries no priority/deadline (unlike exec-provider's richer C-ABI probe
    /// surface), so v1 admits at a fixed neutral priority and `wake`s it right
    /// away — `spawn` semantics are "ready now", not tickless-deadline-driven
    /// (that half of the executor is exercised by gust_exec_probe instead).
    /// `admit`/`wake` perform the ENTIRE decision; `entry` is not otherwise
    /// interpreted here (v1 has no per-entry dispatch table — `poll_task(h)`,
    /// inside the included `executor` module, is the dispatch point).
    fn start(entry: u32) -> u32 {
        let _ = entry;
        let t = unsafe { tasks() };
        let h = t.admit(0);
        if h < MAX_TASKS as u32 {
            t.wake(h);
        }
        h
    }

    /// Poll task `handle`: drives one full `poll_round` (cooperative, so any
    /// poll call drains every currently-ready task, not only `handle`) and
    /// reports `handle`'s resulting state as the WIT-documented code: `0` =
    /// pending, `1` = done, `0xFFFF_FFFF` = invalid handle.
    fn poll(handle: u32) -> u32 {
        let t = unsafe { tasks() };
        if handle >= MAX_TASKS as u32 {
            return 0xFFFF_FFFF;
        }
        t.poll_round();
        match t.state[handle as usize] {
            TaskState::Done => 1,
            TaskState::Pending => 0,
            TaskState::Free => 0xFFFF_FFFF,
        }
    }
}
export!(P);

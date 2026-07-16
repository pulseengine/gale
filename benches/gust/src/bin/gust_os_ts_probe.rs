//! gust-os-ts-probe — LOCAL qemu-semihosting liveness probe of the gust:os v0.4.0
//! STEP-3 node (drivers/os-node/os-ts-cm3.o): an app importing gust:os {time,
//! spawn} — spawn is the first EXECUTOR-BACKED capability crossing the syscall
//! seam (start/poll marshal onto the Verus+Kani-proven executor inside
//! spawn-provider) — wac-plugged with a time provider + a spawn provider,
//! meld-fused + dissolved to one bounded-SRAM object exporting `run` and importing
//! only `read32` + the trusted `poll-task` dispatch seam (gust:os/taskdisp; this
//! node has no mmio WRITE path, so unlike the tl node `write32` is not imported —
//! the stub below is kept for TCB-shape parity and goes dead at link). Proves the
//! executor-backed compose is functionally live end-to-end: app calls
//! spawn.start(0) -> h, polls h (bounded, <= 4), and returns the LAST poll result
//! — 1 (= done), because the executor's first `poll_round` dispatches the task
//! through `poll-task`, which this probe (the trusted layer) answers with 1 =
//! completed, same contract as gust_exec_probe's poll_task. We provide the TCB
//! atoms, call `run` (via an r11=0 trampoline — the object is synth
//! --native-pointer-abi, so r11 is the pinned wasm linmem base, same convention as
//! gust_control.rs), and check the return code.
#![no_std]
#![no_main]
use core::ptr::addr_of_mut;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// The mmio TCB atoms. read32 backs gust:os/time's timer CNT read (constant tick is
// fine for liveness — time math is pure). write32 is NOT imported by this node
// (spawn-provider does no mmio); kept as a no-op for tl-probe TCB-shape parity.
#[no_mangle]
pub extern "C" fn read32(_addr: u32) -> u32 { 1000 }

#[no_mangle]
pub extern "C" fn write32(_addr: u32, _val: u32) {}

// The trusted task-dispatch seam (gust:os/taskdisp.poll-task — the executor's
// `poll_task` FFI, WIT-typed so spawn-provider componentizes; the dissolved symbol
// keeps the WIT field name, dash included). Dispatch task `_id`'s pending poll
// once; this probe's stub completes every task on its first dispatch (returns 1 =
// done), the same stub contract gust_exec_probe uses — so the app's FIRST poll
// must observe Done. Count the dispatches to assert the seam was actually crossed.
static mut DISPATCHES: u32 = 0;

#[export_name = "poll-task"]
pub extern "C" fn poll_task(_id: u32) -> u32 {
    unsafe { *addr_of_mut!(DISPATCHES) += 1 };
    1
}

// The dissolved ts-node was compiled with synth --native-pointer-abi, which pins
// r11 as the wasm linmem base (0 — the shared-memory arena's absolute addresses are
// used directly). The executor's Tasks table lives in that arena, so every
// state-touching function relies on r11 == 0 at entry and never restores it itself
// (same convention as gust_control.rs / gust_os_tl_probe.rs). Callers don't get
// r11 == 0 for free, so wrap the raw `run` export in a 4-instruction r11=0
// trampoline, same pattern as gust_control.rs.
core::arch::global_asm!(
    ".section .text.run_ts",
    ".global run_ts",
    ".thumb_func",
    "run_ts:",
    "    push  {{r11, lr}}",
    "    mov.w r11, #0",
    "    bl    run",
    "    pop   {{r11, pc}}",
);

extern "C" {
    fn run_ts() -> u32;
}

#[entry]
fn main() -> ! {
    let r = unsafe { run_ts() };
    let d = unsafe { *addr_of_mut!(DISPATCHES) };
    if r == 1 && d >= 1 {
        hprintln!("gust-os-ts-probe OK: poll==1");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!(
            "gust-os-ts-probe FAIL: run()={:#x} dispatches={} (want run()==1 — last poll Done — and >=1 poll-task dispatch)",
            r, d
        );
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}

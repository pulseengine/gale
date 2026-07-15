//! gust:os **executor node** — Task 6 of the gust-async-executor plan
//! (docs/superpowers/plans/2026-07-15-gust-async-executor.md). Dissolves the
//! verified async executor (src/executor.rs: Verus-proven no-lost-wakeups,
//! fair/work-conserving pick_next, bounded+terminating poll_round, tickless
//! next_deadline/expire; Kani-cross-checked) to a SINGLE native Cortex-M3
//! object — no wac plug, no meld fuse, so not synth#739-blocked.
//!
//! ## Provenance (the load-bearing part of this file)
//!
//! `plain/src/executor.rs` is verus-strip's output of `src/executor.rs` — the
//! exact plain-executable mirror `cargo kani` already builds against
//! (`tools/verus-strip/tests/gate.rs` enforces the two stay convergent). This
//! module includes that file VERBATIM via `#[path]` — not a hand-retyped copy —
//! so the wasm this crate compiles to, and the object synth dissolves it to,
//! runs the identical proven `Tasks::{new, admit, wake, is_ready, pick_next,
//! next_deadline, expire, consume, poll_round}` state machine. Everything below
//! this include is scalar arg/return marshalling ONLY: no scheduling decision
//! (which task runs next, when something becomes ready, when a round ends) is
//! re-implemented here. The one field write below (`deadline[h] = deadline`)
//! is not a decision either — `admit()`'s own contract doesn't take a deadline
//! parameter (the WIT/C-ABI surface needs one for the probe's due-now case),
//! so this shim sets that plain data field directly through `Tasks`'s public
//! fields, the same way `new()` initializes them.
#![no_std]

#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

#[path = "../../../../../plain/src/executor.rs"]
mod executor;

use executor::{TaskState, Tasks, MAX_TASKS};

/// Footgun guard: `src/executor.rs`'s `by(bit_vector)` lemmas
/// (`lemma_set_bit_bounded`, `lemma_zero_when_no_low_bits_and_bounded`) and
/// `Tasks::ready_popcount`'s 8-term sum both hardcode the task-table width to
/// 8, not `MAX_TASKS` symbolically — Verus's `by(bit_vector)` blocks and the
/// popcount unrolling need concrete bit positions, so they were written
/// against the literal 8. If `MAX_TASKS` ever changes, those proofs and this
/// popcount go silently out of sync with the actual table size. Placed here
/// (rather than in `src/executor.rs`) so it fails loudly for any crate that
/// builds against the plain mirror without requiring a `src/executor.rs`
/// edit + plain/ regen + Verus/strip-gate re-run just to add a compile-time
/// check.
const _: () = assert!(
    MAX_TASKS == 8,
    "executor bit_vector lemmas + ready_popcount hardcode width 8"
);

/// One instance's worth of scheduler state. A dissolved component (like a wasm
/// component instance) owns its state for its lifetime — this is the executor
/// node's only static data, and it is exactly `size_of::<Tasks>()`, which is
/// what makes `exec-cm3.o`'s `.bss` small and bounded (a fixed MAX_TASKS=8
/// table), not proportional to anything unbounded.
static mut TASKS: Option<Tasks> = None;

/// Lazily construct `Tasks` via its own verified `new()` (never hand-inlined),
/// on first use. Single-threaded (no preemption in this v1 static single-
/// partition scope — see REQ-OS-EXEC-001), so a bare `static mut` singleton
/// is the standard no_std embedded pattern (same shape cortex-m-rt itself
/// uses for peripheral singletons).
#[allow(static_mut_refs)]
unsafe fn tasks() -> &'static mut Tasks {
    if TASKS.is_none() {
        TASKS = Some(Tasks::new());
    }
    match &mut TASKS {
        Some(t) => t,
        None => unreachable!(),
    }
}

/// Admit a task at `prio` (lower = higher priority, per `Priority` convention)
/// with wake-by deadline `deadline_lo`/`deadline_hi` (the 64-bit deadline's
/// halves; tick units; `u64::MAX` = no timer). Returns the handle, or
/// `0xFFFF_FFFF` if the table (MAX_TASKS=8) is full — exactly `Tasks::admit`'s
/// contract. `deadline` has no admit() parameter (see the module doc above),
/// so it is set via the public field after a successful admit; admit() itself
/// performs the ENTIRE decision (which slot, Pending vs Free, ready-bit clear).
///
/// ABI note (see `exec_poll_round`'s doc for the synth#518 root cause): kept
/// as three plain u32 params — not `(prio: u32, deadline: u64)` — so both
/// this wasm export and the qemu probe's native caller use the SAME simple
/// sequential-register convention (r0, r1, r2), with no dependence on
/// synth's ARM backend correctly reproducing AAPCS's 64-bit register-pair
/// alignment padding for a mixed u32-then-u64 parameter list (empirically it
/// silently does NOT: an earlier `(u32, u64) -> u32` version of this function
/// dissolved without a synth warning, but the resulting object read garbage
/// for `deadline`, which the qemu probe caught as a liveness FAIL — the
/// deadline never compared `<=now` inside `expire`, so the admitted tasks
/// never became ready and the round drained nothing).
#[no_mangle]
pub extern "C" fn exec_admit(prio: u32, deadline_lo: u32, deadline_hi: u32) -> u32 {
    let deadline = (u64::from(deadline_hi) << 32) | u64::from(deadline_lo);
    let t = unsafe { tasks() };
    let h = t.admit(prio);
    if h < MAX_TASKS as u32 {
        t.deadline[h as usize] = deadline;
    }
    h
}

/// Drive one scheduler round at time `now`: fire the tickless alarm
/// (`expire`, marking any Pending task whose deadline has passed ready), then
/// drain every ready task exactly once (`poll_round`) via the trusted
/// `poll_task` FFI seam (declared inside the included `executor` module,
/// resolved at dissolve-link by the qemu probe's own `poll_task` export).
///
/// ABI note (synth#518 workaround): a wasm export that both takes a 64-bit
/// param AND (post loom-inline) contains a call hits a synth codegen gap
/// ("an i64/f64 param in a frame-backing function ... is not yet lowered"),
/// which silently DROPPED this function from the dissolved object (verified
/// empirically: `synth compile` emitted "skipping function 'exec_poll_round'"
/// and produced only 4/5 functions). Splitting `now` into its lo/hi u32 halves
/// at the wasm-export boundary sidesteps the bug: no exported function has an
/// i64 param, so the buggy lowering path is never entered. This is pure
/// marshalling (identical to AAPCS's own u64-in-register-pair convention —
/// r0=lo, r1=hi — which is exactly how the qemu probe's `extern "C" fn
/// exec_poll_round(now: u64)` call already places its argument), NOT a
/// scheduling-logic change: `now` is reconstructed by a shift+or before it
/// ever reaches `expire`/`poll_round`, which run unmodified.
#[no_mangle]
pub extern "C" fn exec_poll_round(now_lo: u32, now_hi: u32) {
    let now = (u64::from(now_hi) << 32) | u64::from(now_lo);
    let t = unsafe { tasks() };
    t.expire(now);
    t.poll_round();
}

/// Task `h`'s state as a small integer ABI: `0`=Free, `1`=Pending, `2`=Done,
/// `0xFFFF_FFFF`=out-of-range handle. A direct, total mapping of
/// `Tasks::state[h]` — no scheduling decision.
#[no_mangle]
pub extern "C" fn exec_state(h: u32) -> u32 {
    let t = unsafe { tasks() };
    if h >= MAX_TASKS as u32 {
        return 0xFFFF_FFFF;
    }
    match t.state[h as usize] {
        TaskState::Free => 0,
        TaskState::Pending => 1,
        TaskState::Done => 2,
    }
}


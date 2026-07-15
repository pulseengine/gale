
// ===========================================================================
// Trusted FFI seam (Task 5) — the intersection boundary
// ===========================================================================
//
// `poll_task` is NOT verified: it dispatches into the app's async task body
// (possibly meld-dissolved, possibly hand-written) for one poll. This is the
// one place the verified scheduler core hands control to unverified code, so
// it is declared outside the verification macro's block below — it never
// becomes a proof obligation. The only caller is `Tasks::dispatch_one`
// (`#[verifier::external_body]`, below), which is itself only reachable
// through the fully verified `Tasks::poll_round` loop.
//
// Edition 2024 requires `unsafe extern` blocks and an `unsafe { }` at the
// call site; both are used here even though the Verus checker itself invokes
// `rustc --edition=2021` (`unsafe extern` parses under both editions, so one
// source serves both toolchains).
//
// Crate-wide `unsafe_code = "deny"` (Cargo.toml `[lints.rust]`, an ASIL-D
// safety-critical policy) is deliberately overridden here with a single,
// narrowly-scoped `#[allow(unsafe_code)]` — the intersection boundary is
// the ONE place in this crate an FFI call is unavoidable. Everything else
// in the crate (including the rest of this file) stays under the deny.

//! gust async executor (v1) — a verified fixed-priority + tickless-deadline
//! scheduler core over a static task table. Scalar-only (no async/closures in the
//! verified core); task bodies run through the trusted `poll_task` seam (Task 5).
//! Builds on `crate::priority::Priority`. Single-component dissolve (not meld-fused),
//! so it is not blocked on synth#739.
#[allow(unsafe_code)]
unsafe extern "C" {
    /// Poll task `id` once. Returns 0 if still pending, 1 if it completed.
    pub fn poll_task(id: u32) -> u32;
}
pub const MAX_TASKS: usize = 8;
#[derive(PartialEq, Eq)]
pub enum TaskState {
    Free,
    Pending,
    Done,
}
pub struct Tasks {
    pub state: [TaskState; MAX_TASKS],
    pub prio: [u32; MAX_TASKS],
    pub deadline: [u64; MAX_TASKS],
    pub ready: u32,
}
impl Tasks {
    pub fn new() -> Tasks {
        let r = Tasks {
            state: [
                TaskState::Free,
                TaskState::Free,
                TaskState::Free,
                TaskState::Free,
                TaskState::Free,
                TaskState::Free,
                TaskState::Free,
                TaskState::Free,
            ],
            prio: [0; MAX_TASKS],
            deadline: [u64::MAX; MAX_TASKS],
            ready: 0u32,
        };
        r
    }
    /// Admit a fresh task into the first `Free` slot, marking it `Pending`
    /// with `ready` bit clear. Returns the slot handle, or `0xFFFF_FFFF`
    /// if the table is full.
    pub fn admit(&mut self, prio: u32) -> u32 {
        let mut i: usize = 0;
        while i < MAX_TASKS {
            if matches!(self.state[i], TaskState::Free) {
                let old_ready = self.ready;
                self.state[i] = TaskState::Pending;
                self.prio[i] = prio;
                self.ready = self.ready & !(1u32 << (i as u32));
                return i as u32;
            }
            i += 1;
        }
        0xFFFF_FFFFu32
    }
    /// THE no-lost-wakeups primitive: waking a Pending task sets its ready bit and it
    /// stays set until `consume`. Waking a non-Pending (or out-of-range) handle is a
    /// no-op (inv-safe).
    pub fn wake(&mut self, h: u32) {
        if h < MAX_TASKS as u32 && matches!(self.state[h as usize], TaskState::Pending) {
            let old_ready = self.ready;
            self.ready = self.ready | (1u32 << h);
        }
    }
    pub fn is_ready(&self, h: u32) -> bool {
        ((self.ready >> h) & 1u32) == 1u32
    }
    /// The highest-priority ready task's handle (lowest `prio` value wins;
    /// among equal priorities the lowest index — i.e. the first found by the
    /// scan — wins). Work-conserving: if anything is ready, some ready task
    /// is returned. Fair: the returned task has no ready rival with strictly
    /// lower `prio` (strictly higher priority). Returns `MAX_TASKS as u32`
    /// (an always-invalid handle) when nothing is ready.
    pub fn pick_next(&self) -> u32 {
        let mut best: u32 = MAX_TASKS as u32;
        let mut i: u32 = 0;
        while i < MAX_TASKS as u32 {
            if ((self.ready >> i) & 1u32) == 1u32 {
                if best == MAX_TASKS as u32
                    || self.prio[i as usize] < self.prio[best as usize]
                {
                    best = i;
                }
            }
            i += 1;
        }
        best
    }
    /// Minimum `deadline` over Pending tasks, or `u64::MAX` if none are Pending —
    /// the value the outer layer/HW timer arms a one-shot alarm for (tickless: no
    /// periodic tick, just "wake me at this instant").
    pub fn next_deadline(&self) -> u64 {
        let mut d: u64 = u64::MAX;
        let mut i: usize = 0;
        while i < MAX_TASKS {
            if matches!(self.state[i], TaskState::Pending) && self.deadline[i] < d {
                d = self.deadline[i];
            }
            i += 1;
        }
        d
    }
    /// Tickless expiry: on the one-shot alarm firing at `now`, mark every Pending
    /// task whose deadline has passed as ready. No periodic tick — this runs only
    /// when `now >= next_deadline()`. Reuses the `wake`/`consume` set-bit lemmas:
    /// setting bit `i` never disturbs any other bit, so ready bits established in
    /// earlier loop iterations survive later ones.
    pub fn expire(&mut self, now: u64) {
        let mut i: usize = 0;
        while i < MAX_TASKS {
            if matches!(self.state[i], TaskState::Pending) && self.deadline[i] <= now {
                let old_ready = self.ready;
                self.ready = self.ready | (1u32 << (i as u32));
            }
            i += 1;
        }
    }
    /// Clear task `h`'s ready bit — called as the task is about to be polled.
    pub fn consume(&mut self, h: u32) {
        let old_ready = self.ready;
        self.ready = self.ready & !(1u32 << h);
        let new_ready = self.ready;
    }
    /// The trusted FFI seam, wrapped to the minimum trusted surface: dispatch
    /// task `h`'s pending poll once via `poll_task`, and record `Done` if it
    /// completed. `#[verifier::external_body]` means Verus takes this
    /// function's `ensures` on faith (the body is not checked) — kept
    /// deliberately weak (only a frame condition: `ready` untouched, every
    /// OTHER slot's `state` untouched) so `poll_round`'s termination and
    /// `ready == 0` proof never has to trust *what* `poll_task` decided, only
    /// that dispatching can't resurrect readiness or clobber other slots.
    /// `poll_round` (the only caller) re-derives `inv()` itself afterwards
    /// from these two facts plus `consume`'s already-established `inv()` —
    /// i.e. the trusted annotation carries as little weight as possible.
    #[allow(unsafe_code)]
    fn dispatch_one(&mut self, h: u32) {
        let done = unsafe { poll_task(h) };
        if done == 1u32 {
            self.state[h as usize] = TaskState::Done;
        }
    }
    /// One scheduler round: drain every currently-ready task, each polled
    /// exactly once via the trusted `poll_task` seam, until nothing is ready.
    ///
    /// Terminates because `consume` clears exactly the picked task's ready
    /// bit each iteration and `dispatch_one` never sets any ready bit (its
    /// `ensures` says `ready` is untouched) — `ready_popcount()` strictly
    /// decreases every iteration (`lemma_popcount_decreases`), so the loop
    /// is bounded by the entry popcount (<= `MAX_TASKS`). It exits only when
    /// `pick_next` finds nothing ready, at which point `ready == 0u32`
    /// (`lemma_zero_when_no_low_bits_and_bounded`, using the loop-maintained
    /// `ready < 2^MAX_TASKS` bound established via `consume`'s new frame
    /// fact). This is bounded-poll: at most `popcount(ready) <= MAX_TASKS`
    /// dispatches, each ready task consumed exactly once.
    pub fn poll_round(&mut self) {
        loop {
            let h = self.pick_next();
            if h == MAX_TASKS as u32 {
                return;
            }
            self.consume(h);
            self.dispatch_one(h);
        }
    }
}
/// Kani cross-check: `pick_next` (Verus-proven above via SMT/Z3) against an
/// independent brute-force scan, under Kani's bounded model checker (a
/// different solver/engine entirely — SAT-based CBMC). This is not a
/// duplicate of the Verus proof; it is a second, independent tool checking
/// the SAME shipped executable code path (after `verus-strip` removes the
/// ghost-only `requires`/`ensures`/`invariant`/`decreases` clauses,
/// `pick_next`'s body is plain executable Rust — Kani calls that exact
/// function, not a hand-copied mirror, so there is no risk of a spec/impl
/// drift between what Verus verified and what Kani exercises).
#[cfg(kani)]
mod exec_kani {
    use super::*;
    /// An arbitrary well-formed `Tasks`: `prio` and `ready` are unconstrained
    /// (the two fields `pick_next` actually reads); `state` is fixed to
    /// all-`Pending` so `inv()` holds trivially regardless of `ready`
    /// (`inv()` only constrains slots whose ready bit IS set).
    fn arbitrary_tasks() -> Tasks {
        let prio: [u32; MAX_TASKS] = kani::any();
        let ready: u32 = kani::any();
        Tasks {
            state: [
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
            ],
            prio,
            deadline: [0u64; MAX_TASKS],
            ready,
        }
    }
    #[kani::proof]
    #[kani::unwind(9)]
    fn pick_next_is_min_prio_ready() {
        let t = arbitrary_tasks();
        let h = t.pick_next();
        if h == MAX_TASKS as u32 {
            for j in 0..MAX_TASKS as u32 {
                assert!(! t.is_ready(j));
            }
        } else {
            assert!(t.is_ready(h));
            for j in 0..MAX_TASKS as u32 {
                if t.is_ready(j) {
                    assert!(t.prio[h as usize] <= t.prio[j as usize]);
                }
            }
        }
    }
    /// Same construction as `arbitrary_tasks`, additionally constrained to
    /// `ready < 2^MAX_TASKS` — the bound `poll_round` requires (see its
    /// "Additive (Task 5)" note: `inv()` alone doesn't rule out garbage bits
    /// >= MAX_TASKS, which real mutators never set but which an unconstrained
    /// `kani::any()` u32 certainly can). Kani can't call the spec-only
    /// `inv()` (stripped from the plain/executable code this harness runs
    /// against), so the bound is asserted directly here instead, mirroring
    /// what `poll_round`'s `requires` demands of any real caller.
    fn arbitrary_tasks_bounded() -> Tasks {
        let prio: [u32; MAX_TASKS] = kani::any();
        let ready: u32 = kani::any();
        kani::assume(ready < 256u32);
        Tasks {
            state: [
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
                TaskState::Pending,
            ],
            prio,
            deadline: [0u64; MAX_TASKS],
            ready,
        }
    }
    /// Hamming weight of `x`'s low `MAX_TASKS` bits — the plain-executable
    /// (non-spec) counterpart to `Tasks::ready_popcount`, used only to state
    /// the Kani harness's bound.
    fn popcount(x: u32) -> u32 {
        let mut n = 0u32;
        let mut i = 0u32;
        while i < MAX_TASKS as u32 {
            if (x >> i) & 1u32 == 1u32 {
                n += 1;
            }
            i += 1;
        }
        n
    }
    /// Exec-only mirror of `poll_round`'s loop, driving the SAME verified,
    /// shipped `pick_next`/`consume` (post-`verus-strip`, plain executable
    /// Rust — no hand-copied duplicate of the scheduling logic). The only
    /// thing it substitutes is the trusted FFI dispatch: Kani cannot link
    /// against the real `poll_task` extern (no implementation exists), so a
    /// `kani::any()` bool stands in for its result, applied via the exact
    /// same Pending -> Done transition on exactly the consumed slot that
    /// `dispatch_one` performs — nothing else. Counts iterations for the
    /// bounded-poll check.
    fn poll_round_counted(t: &mut Tasks) -> u32 {
        let mut calls: u32 = 0;
        loop {
            let h = t.pick_next();
            if h == MAX_TASKS as u32 {
                break;
            }
            t.consume(h);
            calls += 1;
            let done: bool = kani::any();
            if done {
                t.state[h as usize] = TaskState::Done;
            }
        }
        calls
    }
    /// `poll_round` drains all ready tasks (`ready == 0` afterwards) and is
    /// bounded: at most one dispatch per task that was ready at entry.
    #[kani::proof]
    #[kani::unwind(9)]
    fn poll_round_drains_and_bounds() {
        let mut t = arbitrary_tasks_bounded();
        let before = popcount(t.ready);
        let calls = poll_round_counted(&mut t);
        assert!(t.ready == 0);
        assert!(calls <= before);
    }
}

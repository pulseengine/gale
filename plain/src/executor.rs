//! gust async executor (v1) — a verified fixed-priority + tickless-deadline
//! scheduler core over a static task table. Scalar-only (no async/closures in the
//! verified core); task bodies run through the trusted `poll_task` seam (Task 5).
//! Builds on `crate::priority::Priority`. Single-component dissolve (not meld-fused),
//! so it is not blocked on synth#739.
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
}

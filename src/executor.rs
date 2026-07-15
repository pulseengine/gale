//! gust async executor (v1) — a verified fixed-priority + tickless-deadline
//! scheduler core over a static task table. Scalar-only (no async/closures in the
//! verified core); task bodies run through the trusted `poll_task` seam (Task 5).
//! Builds on `crate::priority::Priority`. Single-component dissolve (not meld-fused),
//! so it is not blocked on synth#739.
use vstd::prelude::*;

verus! {

pub const MAX_TASKS: usize = 8;

#[derive(PartialEq, Eq)]
pub enum TaskState { Free, Pending, Done }

pub struct Tasks {
    pub state: [TaskState; MAX_TASKS],
    pub prio:  [u32; MAX_TASKS],       // lower = higher priority (Priority convention)
    pub deadline: [u64; MAX_TASKS],    // wake-by tick; u64::MAX = no timer
    pub ready: u32,                    // bit i set => task i wants to run
}

impl Tasks {
    /// Representation invariant: `ready` bits only ever set for Pending slots,
    /// and only within [0, MAX_TASKS). This is the anchor every proof rests on.
    pub open spec fn inv(&self) -> bool {
        forall|i: int| 0 <= i < MAX_TASKS ==>
            (#[trigger] self.ready_bit(i)) ==> self.state[i as int] === TaskState::Pending
    }

    /// Ghost: is bit i of `ready` set?
    pub open spec fn ready_bit(&self, i: int) -> bool {
        0 <= i < MAX_TASKS && (self.ready >> (i as u32)) & 1u32 == 1u32
    }

    pub fn new() -> (r: Tasks)
        ensures r.inv(), r.ready == 0u32,
    {
        let r = Tasks {
            state: [
                TaskState::Free, TaskState::Free, TaskState::Free, TaskState::Free,
                TaskState::Free, TaskState::Free, TaskState::Free, TaskState::Free,
            ],
            prio: [0; MAX_TASKS],
            deadline: [u64::MAX; MAX_TASKS],
            ready: 0u32,
        };
        assert forall|i: int| 0 <= i < MAX_TASKS implies
            (#[trigger] r.ready_bit(i)) ==> r.state[i as int] === TaskState::Pending
        by {
            if r.ready_bit(i) {
                lemma_zero_shr_bit(i as u32);
                assert(false);
            }
        }
        r
    }

    /// Admit a fresh task into the first `Free` slot, marking it `Pending`
    /// with `ready` bit clear. Returns the slot handle, or `0xFFFF_FFFF`
    /// if the table is full.
    pub fn admit(&mut self, prio: u32) -> (h: u32)
        requires old(self).inv(),
        ensures self.inv(),
            // a fresh handle is Pending and not yet ready
            h < MAX_TASKS as u32 ==>
                self.state[h as int] === TaskState::Pending && !self.ready_bit(h as int),
    {
        let mut i: usize = 0;
        while i < MAX_TASKS
            invariant
                self.inv(), 0 <= i <= MAX_TASKS,
                // Loop bodies are verified against the invariant list alone — the
                // function's `requires old(self).inv()` isn't implicitly carried in,
                // so it must be restated here to be usable inside the loop.
                old(self).inv(),
                // nothing is written until a Free slot is found (and then we return
                // immediately), so self stays tied to the entry state throughout scan.
                self.ready == old(self).ready,
                forall|j: int| 0 <= j < MAX_TASKS ==>
                    (#[trigger] self.state[j as int]) === old(self).state[j as int],
            decreases MAX_TASKS - i,
        {
            if matches!(self.state[i], TaskState::Free) {
                let old_ready = self.ready;
                self.state[i] = TaskState::Pending;
                self.prio[i] = prio;
                // Clear (a fresh admit must not appear ready), keeping inv trivially:
                // the invariant only constrains bits that ARE set.
                self.ready = self.ready & !(1u32 << (i as u32));
                proof {
                    lemma_clear_bit_self(old_ready, i as u32);
                    assert forall|j: int| 0 <= j < MAX_TASKS implies
                        (#[trigger] self.ready_bit(j)) ==> self.state[j as int] === TaskState::Pending
                    by {
                        if self.ready_bit(j) {
                            if j == i as int {
                                // self.ready_bit(i) is false (lemma_clear_bit_self above);
                                // contradicts the branch assumption.
                                assert(false);
                            } else {
                                lemma_clear_bit_other(old_ready, i as u32, j as u32);
                                // self.ready_bit(j) == old(self).ready_bit(j) (old_ready ==
                                // old(self).ready, by the loop invariant); force the trigger
                                // term so old(self).inv() fires at index j.
                                assert(old(self).ready_bit(j));
                                lemma_inv_ready_implies_pending(*old(self), j);
                                // state[j] unchanged (only index i was written this iteration,
                                // and self.state[j] === old(self).state[j] by loop invariant).
                                assert(self.state[j as int] === old(self).state[j as int]);
                            }
                        }
                    }
                }
                return i as u32;
            }
            i += 1;
        }
        0xFFFF_FFFFu32
    }

    /// THE no-lost-wakeups primitive: waking a Pending task sets its ready bit and it
    /// stays set until `consume`. Waking a non-Pending (or out-of-range) handle is a
    /// no-op (inv-safe).
    pub fn wake(&mut self, h: u32)
        requires old(self).inv(),
        ensures self.inv(),
            (h < MAX_TASKS as u32 && self.state[h as int] === TaskState::Pending)
                ==> self.ready_bit(h as int),
            // no other task's readiness changes
            forall|j: int| 0 <= j < MAX_TASKS && j != h as int ==>
                self.ready_bit(j) == old(self).ready_bit(j),
    {
        // When the guard is false, postconditions hold trivially: self ==
        // old(self), so ready_bit is pointwise unchanged and inv is preserved
        // unmodified — no `else` branch needed (an empty documentation-only
        // else strips down to a `clippy::needless_else` lint in plain Rust).
        if h < MAX_TASKS as u32 && matches!(self.state[h as usize], TaskState::Pending) {
            let old_ready = self.ready;
            self.ready = self.ready | (1u32 << h);
            proof {
                lemma_set_bit_self(old_ready, h);
                assert forall|j: int| 0 <= j < MAX_TASKS && j != h as int implies
                    self.ready_bit(j) == old(self).ready_bit(j)
                by {
                    lemma_set_bit_other(old_ready, h, j as u32);
                }
                assert forall|i: int| 0 <= i < MAX_TASKS implies
                    (#[trigger] self.ready_bit(i)) ==> self.state[i as int] === TaskState::Pending
                by {
                    if self.ready_bit(i) {
                        if i == h as int {
                            // state[h] is Pending — that's exactly the branch guard above.
                        } else {
                            lemma_set_bit_other(old_ready, h, i as u32);
                            // self.ready_bit(i) == old(self).ready_bit(i); old(self).inv() closes it
                            // (state array untouched in this branch).
                        }
                    }
                }
            }
        }
    }

    pub fn is_ready(&self, h: u32) -> (b: bool)
        requires self.inv(), h < MAX_TASKS as u32,
        ensures b == self.ready_bit(h as int),
    {
        ((self.ready >> h) & 1u32) == 1u32
    }

    /// The highest-priority ready task's handle (lowest `prio` value wins;
    /// among equal priorities the lowest index — i.e. the first found by the
    /// scan — wins). Work-conserving: if anything is ready, some ready task
    /// is returned. Fair: the returned task has no ready rival with strictly
    /// lower `prio` (strictly higher priority). Returns `MAX_TASKS as u32`
    /// (an always-invalid handle) when nothing is ready.
    pub fn pick_next(&self) -> (h: u32)
        requires self.inv(),
        ensures
            // either nothing is ready...
            (h == MAX_TASKS as u32 && forall|i: int| 0 <= i < MAX_TASKS ==> !self.ready_bit(i))
            // ...or h is a valid ready task and no ready task outranks it
            // (lower prio value == higher priority).
            || (h < MAX_TASKS as u32 && self.ready_bit(h as int)
                && forall|j: int| 0 <= j < MAX_TASKS && self.ready_bit(j) ==>
                       self.prio[h as int] <= self.prio[j]),
    {
        let mut best: u32 = MAX_TASKS as u32;
        let mut i: u32 = 0;
        while i < MAX_TASKS as u32
            invariant
                self.inv(), 0 <= i <= MAX_TASKS as u32,
                // Strengthened vs. the naive "best == MAX_TASKS as u32" disjunct:
                // that alone doesn't tell Verus anything about ready bits already
                // scanned, so the inductive step (proving a freshly-found best is
                // truly minimal) can't get off the ground. Spell out "nothing ready
                // in [0, i) yet" for the not-found-anything-so-far case too.
                (best == MAX_TASKS as u32 && forall|j: int| 0 <= j < i ==> !self.ready_bit(j))
                    || (best < i && self.ready_bit(best as int)
                        && forall|j: int| 0 <= j < i && self.ready_bit(j) ==>
                               self.prio[best as int] <= self.prio[j]),
            decreases MAX_TASKS as u32 - i,
        {
            // Exec array indexing uses `as usize` (spec-context comparisons
            // above use `as int`) — `int` is ghost-only and does not exist
            // at runtime.
            if ((self.ready >> i) & 1u32) == 1u32 {
                if best == MAX_TASKS as u32 || self.prio[i as usize] < self.prio[best as usize] {
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
    pub fn next_deadline(&self) -> (d: u64)
        requires self.inv(),
        ensures
            forall|i: int| 0 <= i < MAX_TASKS && self.state[i as int] === TaskState::Pending
                ==> d <= self.deadline[i as int],
    {
        let mut d: u64 = u64::MAX;
        let mut i: usize = 0;
        while i < MAX_TASKS
            invariant
                0 <= i <= MAX_TASKS,
                forall|k: int| 0 <= k < i && self.state[k as int] === TaskState::Pending
                    ==> d <= self.deadline[k as int],
            decreases MAX_TASKS - i,
        {
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
    pub fn expire(&mut self, now: u64)
        requires old(self).inv(),
        ensures self.inv(),
            forall|i: int| 0 <= i < MAX_TASKS
                && self.state[i as int] === TaskState::Pending && self.deadline[i as int] <= now
                ==> self.ready_bit(i),
    {
        let mut i: usize = 0;
        while i < MAX_TASKS
            invariant
                self.inv(),
                0 <= i <= MAX_TASKS,
                forall|k: int| 0 <= k < i
                    && self.state[k as int] === TaskState::Pending && self.deadline[k as int] <= now
                    ==> self.ready_bit(k),
            decreases MAX_TASKS - i,
        {
            if matches!(self.state[i], TaskState::Pending) && self.deadline[i] <= now {
                let old_ready = self.ready;
                proof {
                    // Snapshot facts about old_ready while self.ready still equals it
                    // (i.e. before this iteration's write), so the loop invariants —
                    // which talk about "self" at loop-top — apply directly.
                    assert forall|k: int| 0 <= k < i
                        && self.state[k as int] === TaskState::Pending && self.deadline[k as int] <= now implies
                        ((old_ready >> (k as u32)) & 1u32) == 1u32
                    by {
                        assert(self.ready_bit(k));
                    }
                    assert forall|kk: int| 0 <= kk < MAX_TASKS
                        && ((old_ready >> (kk as u32)) & 1u32) == 1u32 implies
                        self.state[kk as int] === TaskState::Pending
                    by {
                        assert(self.ready_bit(kk));
                        lemma_inv_ready_implies_pending(*self, kk);
                    }
                }
                self.ready = self.ready | (1u32 << (i as u32));
                proof {
                    lemma_set_bit_self(old_ready, i as u32);
                    // Progress invariant carries forward to i+1: bits [0, i) already
                    // satisfying the expiry condition (snapshotted above from
                    // old_ready) are untouched by setting bit i; bit i itself is now
                    // set exactly when this branch's guard held.
                    assert forall|k: int| 0 <= k < i
                        && self.state[k as int] === TaskState::Pending && self.deadline[k as int] <= now implies
                        self.ready_bit(k)
                    by {
                        lemma_set_bit_other(old_ready, i as u32, k as u32);
                    }
                    assert forall|kk: int| 0 <= kk < MAX_TASKS implies
                        (#[trigger] self.ready_bit(kk)) ==> self.state[kk as int] === TaskState::Pending
                    by {
                        if self.ready_bit(kk) {
                            if kk == i as int {
                                // branch guard establishes state[i] === Pending.
                            } else {
                                lemma_set_bit_other(old_ready, i as u32, kk as u32);
                                // self.ready_bit(kk) == (old_ready bit kk); the
                                // pre-mutation snapshot above closes state[kk] ===
                                // Pending from that.
                            }
                        }
                    }
                }
            }
            i += 1;
        }
    }

    /// Clear task `h`'s ready bit — called as the task is about to be polled.
    pub fn consume(&mut self, h: u32)
        requires old(self).inv(), h < MAX_TASKS as u32,
        ensures self.inv(), !self.ready_bit(h as int),
            forall|j: int| 0 <= j < MAX_TASKS && j != h as int ==>
                self.ready_bit(j) == old(self).ready_bit(j),
    {
        let old_ready = self.ready;
        self.ready = self.ready & !(1u32 << h);
        proof {
            lemma_clear_bit_self(old_ready, h);
            assert forall|j: int| 0 <= j < MAX_TASKS && j != h as int implies
                self.ready_bit(j) == old(self).ready_bit(j)
            by {
                lemma_clear_bit_other(old_ready, h, j as u32);
            }
            assert forall|i: int| 0 <= i < MAX_TASKS implies
                (#[trigger] self.ready_bit(i)) ==> self.state[i as int] === TaskState::Pending
            by {
                if self.ready_bit(i) {
                    if i == h as int {
                        // self.ready_bit(h) is false (lemma_clear_bit_self above);
                        // contradicts the branch assumption.
                        assert(false);
                    } else {
                        lemma_clear_bit_other(old_ready, h, i as u32);
                    }
                }
            }
        }
    }
}

/// Shifting zero right by any amount and masking the low bit is always zero.
/// Pure bit-vector fact, used to discharge `new()`'s empty-`ready` invariant.
proof fn lemma_zero_shr_bit(k: u32)
    ensures (0u32 >> k) & 1u32 == 0u32,
{
    assert((0u32 >> k) & 1u32 == 0u32) by (bit_vector);
}

/// Setting bit `h` and then reading bit `h` back always yields 1.
proof fn lemma_set_bit_self(x: u32, h: u32)
    requires h < 32,
    ensures ((x | (1u32 << h)) >> h) & 1u32 == 1u32,
{
    assert(((x | (1u32 << h)) >> h) & 1u32 == 1u32) by (bit_vector)
        requires h < 32;
}

/// Setting bit `h` never disturbs any other bit `j`.
proof fn lemma_set_bit_other(x: u32, h: u32, j: u32)
    requires h < 32, j < 32, h != j,
    ensures ((x | (1u32 << h)) >> j) & 1u32 == (x >> j) & 1u32,
{
    assert(((x | (1u32 << h)) >> j) & 1u32 == (x >> j) & 1u32) by (bit_vector)
        requires h < 32, j < 32, h != j;
}

/// Clearing bit `h` and then reading bit `h` back always yields 0.
proof fn lemma_clear_bit_self(x: u32, h: u32)
    requires h < 32,
    ensures ((x & !(1u32 << h)) >> h) & 1u32 == 0u32,
{
    assert(((x & !(1u32 << h)) >> h) & 1u32 == 0u32) by (bit_vector)
        requires h < 32;
}

/// Clearing bit `h` never disturbs any other bit `j`.
proof fn lemma_clear_bit_other(x: u32, h: u32, j: u32)
    requires h < 32, j < 32, h != j,
    ensures ((x & !(1u32 << h)) >> j) & 1u32 == (x >> j) & 1u32,
{
    assert(((x & !(1u32 << h)) >> j) & 1u32 == (x >> j) & 1u32) by (bit_vector)
        requires h < 32, j < 32, h != j;
}

/// Point-wise instantiation of `Tasks::inv()`: if `t` is well-formed and slot `j`
/// is ready, slot `j` is Pending. A standalone lemma call gives Verus a fresh,
/// unambiguous instantiation site (avoids relying on `inv()`'s internal forall
/// auto-firing through an `old(self)` snapshot at an arbitrary proof point).
proof fn lemma_inv_ready_implies_pending(t: Tasks, j: int)
    requires t.inv(), 0 <= j < MAX_TASKS, t.ready_bit(j),
    ensures t.state[j] === TaskState::Pending,
{
}

/// no-lost-wakeups: if `h` is Pending and ready in `t0`, then consuming any OTHER
/// handle `other` (clearing `other`'s ready bit) leaves `h`'s readiness untouched.
/// This is the ghost-level model of `consume(other)` applied to `t0.ready`.
pub proof fn lemma_no_lost_wakeup(t0: Tasks, h: u32, other: u32)
    requires
        t0.inv(), h < MAX_TASKS as u32, other < MAX_TASKS as u32, other != h,
        t0.state[h as int] === TaskState::Pending, t0.ready_bit(h as int),
    ensures
        // t1 == t0 after consume(other): only the ready bitmask changes, by clearing
        // bit `other`. h's readiness — and thus h's slot in inv() — survives.
        ({
            let t1_ready = t0.ready & !(1u32 << other);
            (t1_ready >> h) & 1u32 == 1u32
        }),
{
    lemma_clear_bit_other(t0.ready, other, h);
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
                TaskState::Pending, TaskState::Pending, TaskState::Pending, TaskState::Pending,
                TaskState::Pending, TaskState::Pending, TaskState::Pending, TaskState::Pending,
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
            // brute force: nothing is ready
            for j in 0..MAX_TASKS as u32 {
                assert!(!t.is_ready(j));
            }
        } else {
            assert!(t.is_ready(h));
            // brute force: no ready j has strictly higher priority (strictly
            // lower prio value) than h.
            for j in 0..MAX_TASKS as u32 {
                if t.is_ready(j) {
                    assert!(t.prio[h as usize] <= t.prio[j as usize]);
                }
            }
        }
    }
}

} // verus!

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
        } else {
            // Postconditions hold trivially: self == old(self), so ready_bit is
            // pointwise unchanged and inv is preserved unmodified.
        }
    }

    pub fn is_ready(&self, h: u32) -> (b: bool)
        requires self.inv(), h < MAX_TASKS as u32,
        ensures b == self.ready_bit(h as int),
    {
        ((self.ready >> h) & 1u32) == 1u32
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

} // verus!

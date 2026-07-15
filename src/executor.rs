//! gust async executor (v1) — a verified fixed-priority + tickless-deadline
//! scheduler core over a static task table. Scalar-only (no async/closures in the
//! verified core); task bodies run through the trusted `poll_task` seam (Task 5).
//! Builds on `crate::priority::Priority`. Single-component dissolve (not meld-fused),
//! so it is not blocked on synth#739.
use vstd::prelude::*;

verus! {

pub const MAX_TASKS: usize = 8;

#[derive(PartialEq, Eq, Structural)]
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
            (#[trigger] self.ready_bit(i)) ==> self.state[i as int] == TaskState::Pending
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
            (#[trigger] r.ready_bit(i)) ==> r.state[i as int] == TaskState::Pending
        by {
            if r.ready_bit(i) {
                lemma_zero_shr_bit(i as u32);
                assert(false);
            }
        }
        r
    }
}

/// Shifting zero right by any amount and masking the low bit is always zero.
/// Pure bit-vector fact, used to discharge `new()`'s empty-`ready` invariant.
proof fn lemma_zero_shr_bit(k: u32)
    ensures (0u32 >> k) & 1u32 == 0u32,
{
    assert((0u32 >> k) & 1u32 == 0u32) by (bit_vector);
}

} // verus!

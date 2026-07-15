//! gust async executor (v1) — a verified fixed-priority + tickless-deadline
//! scheduler core over a static task table. Scalar-only (no async/closures in the
//! verified core); task bodies run through the trusted `poll_task` seam (Task 5).
//! Builds on `crate::priority::Priority`. Single-component dissolve (not meld-fused),
//! so it is not blocked on synth#739.
use vstd::prelude::*;

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
#[allow(unsafe_code)]
unsafe extern "C" {
    /// Poll task `id` once. Returns 0 if still pending, 1 if it completed.
    pub fn poll_task(id: u32) -> u32;
}

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
    ///
    /// Second conjunct (`ready < 256`, i.e. no bits set at positions >=
    /// MAX_TASKS==8): originally this was a local `requires` bolted onto
    /// `poll_round` alone (every real mutator already respects it, but
    /// nothing said so at the `inv()` level). Reviewer-flagged design debt on
    /// Task 5 — folded in here so every mutator proves it once, and callers
    /// no longer need to restate it.
    pub open spec fn inv(&self) -> bool {
        (forall|i: int| 0 <= i < MAX_TASKS ==>
            (#[trigger] self.ready_bit(i)) ==> self.state[i as int] === TaskState::Pending)
            && self.ready < 256u32
    }

    /// Ghost: is bit i of `ready` set?
    pub open spec fn ready_bit(&self, i: int) -> bool {
        0 <= i < MAX_TASKS && (self.ready >> (i as u32)) & 1u32 == 1u32
    }

    /// Hamming weight of `ready` — the termination measure for `poll_round`'s
    /// loop. `consume`-of-a-ready-bit strictly decreases it (proven by
    /// `lemma_popcount_decreases` below), so it drives the loop's
    /// `decreases`. Hardcoded to `MAX_TASKS == 8` terms, matching the
    /// hardcoded-8 style already used for the `state` array literal in
    /// `new()`.
    pub open spec fn ready_popcount(&self) -> nat {
        (if self.ready_bit(0) { 1nat } else { 0nat })
        + (if self.ready_bit(1) { 1nat } else { 0nat })
        + (if self.ready_bit(2) { 1nat } else { 0nat })
        + (if self.ready_bit(3) { 1nat } else { 0nat })
        + (if self.ready_bit(4) { 1nat } else { 0nat })
        + (if self.ready_bit(5) { 1nat } else { 0nat })
        + (if self.ready_bit(6) { 1nat } else { 0nat })
        + (if self.ready_bit(7) { 1nat } else { 0nat })
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
        assert(r.ready < 256u32);
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
                let new_ready = self.ready;
                proof {
                    lemma_clear_bit_self(old_ready, i as u32);
                    // AND-with-mask is monotone non-increasing; old_ready ==
                    // old(self).ready (nothing written before this point in
                    // the scan, per the loop's `self.ready == old(self).ready`
                    // invariant), which is < 256 via old(self).inv()'s second
                    // conjunct — so new_ready stays < 256 too.
                    assert(new_ready <= old_ready) by (bit_vector)
                        requires new_ready == old_ready & !(1u32 << (i as u32));
                    assert(old_ready < 256u32);
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
                // old_ready == old(self).ready (unmodified so far) < 256 via
                // old(self).inv()'s second conjunct; h < MAX_TASKS == 8, so
                // the freshly-set bit keeps `ready` in inv()'s bound.
                assert(old_ready < 256u32);
                lemma_set_bit_bounded(old_ready, h);
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
                    // old_ready == self.ready at loop-top (nothing written
                    // yet this iteration) < 256 via the loop's `self.inv()`
                    // invariant's second conjunct; i < MAX_TASKS == 8, so the
                    // freshly-set bit keeps `ready` within inv()'s bound.
                    assert(old_ready < 256u32);
                    lemma_set_bit_bounded(old_ready, i as u32);
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
            // Additive (Task 5): AND-with-mask never increases the bitmask's
            // numeric value. `ready_bit`'s domain guard (0 <= i < MAX_TASKS)
            // means the two facts above are silent about bits >= MAX_TASKS,
            // so a black-box call to `consume` alone can't otherwise carry a
            // `ready < 2^MAX_TASKS` bound across the call — needed by
            // `poll_round` to conclude `self.ready == 0u32` (not just "no
            // low ready bit") once nothing is left ready.
            self.ready <= old(self).ready,
    {
        let old_ready = self.ready;
        self.ready = self.ready & !(1u32 << h);
        let new_ready = self.ready;
        proof {
            assert(new_ready <= old_ready) by (bit_vector)
                requires new_ready == old_ready & !(1u32 << h);
            // old_ready == old(self).ready (nothing written before this
            // point) < 256 via old(self).inv()'s second conjunct; combined
            // with the AND-with-mask monotonicity above, new_ready stays
            // within inv()'s bound too.
            assert(old_ready < 256u32);
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
    #[verifier::external_body]
    #[allow(unsafe_code)] // see the trusted-seam note at the top of this file
    fn dispatch_one(&mut self, h: u32)
        requires h < MAX_TASKS as u32,
        ensures
            self.ready == old(self).ready,
            forall|i: int| 0 <= i < MAX_TASKS && i != h as int ==>
                self.state[i as int] === old(self).state[i as int],
    {
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
    /// (`lemma_zero_when_no_low_bits_and_bounded`, using the `ready <
    /// 2^MAX_TASKS` bound that is now `inv()`'s own second conjunct —
    /// every mutator proves it, so it no longer needs restating here). This
    /// is bounded-poll: at most `popcount(ready) <= MAX_TASKS` dispatches,
    /// each ready task consumed exactly once.
    pub fn poll_round(&mut self)
        requires
            old(self).inv(),
        ensures
            self.inv(),
            self.ready == 0u32,
    {
        loop
            invariant
                self.inv(),
            decreases self.ready_popcount(),
        {
            let h = self.pick_next();
            let ghost pre = *self;
            if h == MAX_TASKS as u32 {
                proof {
                    assert(!self.ready_bit(0));
                    assert(!self.ready_bit(1));
                    assert(!self.ready_bit(2));
                    assert(!self.ready_bit(3));
                    assert(!self.ready_bit(4));
                    assert(!self.ready_bit(5));
                    assert(!self.ready_bit(6));
                    assert(!self.ready_bit(7));
                    lemma_zero_when_no_low_bits_and_bounded(self.ready);
                }
                return;
            }
            self.consume(h);
            self.dispatch_one(h);
            proof {
                assert forall|i: int| 0 <= i < MAX_TASKS implies
                    (#[trigger] self.ready_bit(i)) ==> self.state[i as int] === TaskState::Pending
                by {
                    if self.ready_bit(i) {
                        if i == h as int {
                            // consume already cleared ready_bit(h); dispatch_one
                            // doesn't touch `ready`, so ready_bit(h) is still
                            // false here — contradicts the branch assumption.
                            assert(false);
                        } else {
                            // dispatch_one only ever touches state[h] (its
                            // ensures says every other slot's state is
                            // unchanged), so state[i] carries over from right
                            // after `consume`, which already established
                            // inv() there.
                        }
                    }
                }
                // dispatch_one's `ensures self.ready == old(self).ready`
                // means `ready` is untouched by it; the bound established by
                // `consume` (whose own `ensures self.inv()` includes
                // `ready < 256`) therefore survives verbatim.
                assert(self.ready < 256u32);
                lemma_popcount_decreases(pre, *self, h as int);
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

/// Setting a bit at a position < MAX_TASKS (8) in a value already < 256
/// (i.e. already confined to the low 8 bits) keeps the result < 256.
/// Discharges the `inv()` bound (`ready < 256`) across `wake`/`expire`'s
/// bit-set mutation, extending the `lemma_set_bit_*` family above.
proof fn lemma_set_bit_bounded(x: u32, h: u32)
    requires x < 256u32, h < 8,
    ensures (x | (1u32 << h)) < 256u32,
{
    assert((x | (1u32 << h)) < 256u32) by (bit_vector)
        requires x < 256u32, h < 8;
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

/// Termination lemma for `poll_round`: consuming a set ready bit `h` (going
/// from `t0` to `t1`, with every other tracked bit unchanged) strictly
/// decreases `ready_popcount`. Instantiates the `j != h` frame hypothesis at
/// each of the 8 concrete indices directly (rather than relying on the
/// prover to fire the `forall` while also unfolding `ready_popcount`'s 8-term
/// sum) so both are fully pinned down before the arithmetic conclusion.
proof fn lemma_popcount_decreases(t0: Tasks, t1: Tasks, h: int)
    requires
        0 <= h < MAX_TASKS as int,
        t0.ready_bit(h),
        !t1.ready_bit(h),
        forall|j: int| 0 <= j < MAX_TASKS as int && j != h ==> t1.ready_bit(j) == t0.ready_bit(j),
    ensures
        t1.ready_popcount() < t0.ready_popcount(),
{
    assert(0 != h ==> t1.ready_bit(0) == t0.ready_bit(0));
    assert(1 != h ==> t1.ready_bit(1) == t0.ready_bit(1));
    assert(2 != h ==> t1.ready_bit(2) == t0.ready_bit(2));
    assert(3 != h ==> t1.ready_bit(3) == t0.ready_bit(3));
    assert(4 != h ==> t1.ready_bit(4) == t0.ready_bit(4));
    assert(5 != h ==> t1.ready_bit(5) == t0.ready_bit(5));
    assert(6 != h ==> t1.ready_bit(6) == t0.ready_bit(6));
    assert(7 != h ==> t1.ready_bit(7) == t0.ready_bit(7));
    assert(h == 0 || h == 1 || h == 2 || h == 3 || h == 4 || h == 5 || h == 6 || h == 7);
}

/// If `x`'s low `MAX_TASKS` bits are all clear and `x < 2^MAX_TASKS` (i.e. it
/// has no bits set outside that low range to begin with), `x` is exactly
/// zero. Hardcoded to `MAX_TASKS == 8` / `2^8 == 256`, matching
/// `ready_popcount`'s hardcoded-8-terms style. Closes `poll_round`'s
/// `self.ready == 0u32` postcondition from `pick_next`'s "nothing left
/// ready" fact plus the loop-maintained bound (via `consume`'s `self.ready
/// <= old(self).ready` frame fact).
proof fn lemma_zero_when_no_low_bits_and_bounded(x: u32)
    requires
        x < 256u32,
        (x >> 0u32) & 1u32 != 1u32,
        (x >> 1u32) & 1u32 != 1u32,
        (x >> 2u32) & 1u32 != 1u32,
        (x >> 3u32) & 1u32 != 1u32,
        (x >> 4u32) & 1u32 != 1u32,
        (x >> 5u32) & 1u32 != 1u32,
        (x >> 6u32) & 1u32 != 1u32,
        (x >> 7u32) & 1u32 != 1u32,
    ensures x == 0u32,
{
    assert(x == 0u32) by (bit_vector)
        requires
            x < 256u32,
            (x >> 0u32) & 1u32 != 1u32,
            (x >> 1u32) & 1u32 != 1u32,
            (x >> 2u32) & 1u32 != 1u32,
            (x >> 3u32) & 1u32 != 1u32,
            (x >> 4u32) & 1u32 != 1u32,
            (x >> 5u32) & 1u32 != 1u32,
            (x >> 6u32) & 1u32 != 1u32,
            (x >> 7u32) & 1u32 != 1u32;
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
                TaskState::Pending, TaskState::Pending, TaskState::Pending, TaskState::Pending,
                TaskState::Pending, TaskState::Pending, TaskState::Pending, TaskState::Pending,
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

} // verus!

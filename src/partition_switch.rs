//! gust partition-switch FSM (safety line v0.6.0) — the verified OUTER
//! scheduler policy core for two-level time partitioning (ARINC-653-style
//! major frame). A static major frame of `MAX_WINDOWS` windows exactly covers
//! `[0, frame_len)` with no gap and no overlap (temporal isolation); at each
//! window-end boundary the switch FSM preempts NON-MASKABLY (Running →
//! SaveCtx has no disable path — the wdg-thin "cannot-un-start" construction
//! applied to preemption), then walks SaveCtx → ProgramRegions → Resume →
//! Running one-way, so the memory-protection region swap for the incoming
//! partition strictly precedes its resume. The timer tick and the context
//! save/region-program/resume are trusted seams (`ctx_save` / `region_swap` /
//! `ctx_resume`); the POLICY — boundary detection, phase ordering, window
//! sequencing — is what is verified. [REQ-OS-SWITCH-001]
use vstd::prelude::*;

// ===========================================================================
// Trusted FFI seams — the intersection boundary
// ===========================================================================
//
// `ctx_save` / `region_swap` / `ctx_resume` are NOT verified: they touch real
// hardware (CPU register file, MPU region registers). They are declared
// outside the verification macro's block below, so they never become proof
// obligations. Each has exactly one caller — the matching
// `#[verifier::external_body]` thin wrapper below — and those wrappers are
// only reachable through the fully verified `Switcher::run_switch`, whose
// code order plus the FSM's `swapped` invariant proves region programming
// strictly precedes resume. At integration, `region_swap` is wired to the
// isolation core's partition programmer; this module builds only against the
// contract, not that code.
//
// Edition 2024 requires `unsafe extern` blocks and an `unsafe { }` at the
// call site; both are used here even though the Verus checker itself invokes
// `rustc --edition=2021` (`unsafe extern` parses under both editions, so one
// source serves both toolchains).
//
// Crate-wide `unsafe_code = "deny"` (Cargo.toml `[lints.rust]`, an ASIL-D
// safety-critical policy) is deliberately overridden here with a single,
// narrowly-scoped `#[allow(unsafe_code)]` — the trusted seam is the ONE
// place in this module an FFI call is unavoidable.
#[allow(unsafe_code)]
unsafe extern "C" {
    /// Save the outgoing partition's execution context. Returns a status word.
    pub fn ctx_save(part: u32) -> u32;
    /// Program the memory-protection regions for the incoming partition.
    /// Returns a status word.
    pub fn region_swap(part: u32) -> u32;
    /// Resume the incoming partition's saved execution context. Returns a
    /// status word.
    pub fn ctx_resume(part: u32) -> u32;
}

verus! {

/// Number of windows in the static major frame. Fixed at build time — the
/// frame is a static table, never resized.
pub const MAX_WINDOWS: usize = 4;

/// Where the switch is in its preemption sequence. `Running` is the steady
/// state (a partition executes); the other three phases form the one-way
/// switch pipeline. There is deliberately NO transition that leaves the
/// pipeline early and NO input that suppresses entering it at a window
/// boundary — that absence IS the non-maskability property (cf. wdg-thin's
/// missing `stop`).
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SwPhase {
    /// A partition is executing inside its window.
    Running,
    /// Window boundary hit: the outgoing partition's context is being saved.
    SaveCtx,
    /// Memory-protection regions are being programmed for the incoming
    /// partition. Strictly after SaveCtx, strictly before Resume.
    ProgramRegions,
    /// The incoming partition is being resumed. Reachable ONLY through
    /// ProgramRegions (proven: `inv`'s `swapped` conjunct + `k2`).
    Resume,
}

/// The static major frame: `MAX_WINDOWS` windows, each a (partition,
/// offset, budget) triple, jointly covering `[0, frame_len)` exactly.
#[derive(Clone, Copy)]
pub struct MajorFrame {
    /// Which partition owns each window.
    pub partition_id: [u32; MAX_WINDOWS],
    /// Start tick of each window on the major-frame timeline.
    pub offset: [u32; MAX_WINDOWS],
    /// Length (in ticks) of each window. Always > 0 under `frame_inv`.
    pub budget: [u32; MAX_WINDOWS],
    /// Total major-frame length: the exclusive end of the last window.
    pub frame_len: u32,
}

impl MajorFrame {
    /// Ghost: exclusive end of window `i` on the major-frame timeline, in
    /// `int` arithmetic (spec-side sums cannot overflow).
    pub open spec fn win_end(&self, i: int) -> int {
        self.offset[i] as int + self.budget[i] as int
    }

    /// Frame invariant — THE temporal-isolation core: the windows are
    /// contiguous and exactly cover `[0, frame_len)`:
    ///   - `window[0].offset == 0`,
    ///   - `window[i+1].offset == window[i].offset + window[i].budget`,
    ///   - the last window ends exactly at `frame_len`,
    ///   - every budget > 0 (no zero-width window).
    /// Coverage-without-overlap follows (proven: `current_window`,
    /// `lemma_window_disjoint`, `k3`). Stated over the 4 concrete indices
    /// (hardcoded-4, matching executor.rs's hardcoded-8 style) so every
    /// downstream proof is plain linear arithmetic.
    pub open spec fn frame_inv(&self) -> bool {
        self.offset[0] == 0u32
        && self.budget[0] > 0u32
        && self.budget[1] > 0u32
        && self.budget[2] > 0u32
        && self.budget[3] > 0u32
        && self.win_end(0) == self.offset[1] as int
        && self.win_end(1) == self.offset[2] as int
        && self.win_end(2) == self.offset[3] as int
        && self.win_end(3) == self.frame_len as int
    }

    /// Ghost: does tick `t` fall inside window `i`?
    pub open spec fn in_window(&self, i: int, t: int) -> bool {
        0 <= i < MAX_WINDOWS as int
        && self.offset[i] as int <= t < self.win_end(i)
    }

    /// Exec validator: true IFF `frame_inv` holds. The integration seam calls
    /// this once on the static frame table before constructing a `Switcher`
    /// — after that, every proof rides on the established invariant. All
    /// sums are computed in u64 so the check itself can never overflow, even
    /// on a hostile table.
    pub fn check(&self) -> (b: bool)
        ensures b == self.frame_inv(),
    {
        self.offset[0] == 0
            && self.budget[0] > 0
            && self.budget[1] > 0
            && self.budget[2] > 0
            && self.budget[3] > 0
            && (self.offset[0] as u64) + (self.budget[0] as u64) == (self.offset[1] as u64)
            && (self.offset[1] as u64) + (self.budget[1] as u64) == (self.offset[2] as u64)
            && (self.offset[2] as u64) + (self.budget[2] as u64) == (self.offset[3] as u64)
            && (self.offset[3] as u64) + (self.budget[3] as u64) == (self.frame_len as u64)
    }

    /// The unique window containing tick `t`. Containment AND uniqueness are
    /// both ensured — for every `t < frame_len` there is exactly one window
    /// (coverage-without-overlap, the temporal-isolation core). Straight-line
    /// (no loop): with 4 windows the scan is three ordered comparisons
    /// against the window start offsets.
    pub fn current_window(&self, t: u32) -> (w: u32)
        requires
            self.frame_inv(),
            t < self.frame_len,
        ensures
            w < MAX_WINDOWS as u32,
            self.in_window(w as int, t as int),
            forall|j: int| 0 <= j < MAX_WINDOWS as int && j != w as int
                ==> !self.in_window(j, t as int),
    {
        let w: u32 = if t < self.offset[1] {
            0
        } else if t < self.offset[2] {
            1
        } else if t < self.offset[3] {
            2
        } else {
            3
        };
        proof {
            // Containment: each branch's guard chain places t exactly in
            // window w (frame_inv's contiguity equalities turn the offset
            // comparisons into window bounds).
            assert(self.in_window(w as int, t as int));
            // Uniqueness: pairwise disjointness of the contiguous windows.
            assert forall|j: int| 0 <= j < MAX_WINDOWS as int && j != w as int implies
                !self.in_window(j, t as int)
            by {
                lemma_window_disjoint(*self, w as int, j, t as int);
            }
        }
        w
    }
}

/// Windows are pairwise disjoint: a tick inside window `i` is inside no other
/// window `j`. With `frame_inv`'s contiguity chain this is pure linear
/// arithmetic; the two concrete-enumeration asserts pin `i`/`j` to the 4
/// window indices so the solver case-splits instead of reasoning abstractly
/// (same finite-enumeration idiom as executor.rs's
/// `lemma_popcount_decreases`).
pub proof fn lemma_window_disjoint(f: MajorFrame, i: int, j: int, t: int)
    requires
        f.frame_inv(),
        0 <= i < MAX_WINDOWS as int,
        0 <= j < MAX_WINDOWS as int,
        i != j,
        f.in_window(i, t),
    ensures
        !f.in_window(j, t),
{
    assert(i == 0 || i == 1 || i == 2 || i == 3);
    assert(j == 0 || j == 1 || j == 2 || j == 3);
}

/// S3 — the frame is followed, no window skipped or repeated: if `t` is the
/// LAST tick of window `cur` (the boundary tick that fires the switch), then
/// the next tick on the timeline — `t + 1`, wrapping to 0 at `frame_len` —
/// falls inside exactly window `(cur + 1) % MAX_WINDOWS`, i.e. the window
/// index `mark_resumed` advances to. Combined with `current_window`'s
/// uniqueness, the post-switch window IS `current_window` of the next tick.
pub proof fn lemma_no_skip(f: MajorFrame, cur: int, t: int)
    requires
        f.frame_inv(),
        0 <= cur < MAX_WINDOWS as int,
        f.in_window(cur, t),
        t + 1 == f.win_end(cur),
    ensures
        ({
            let next_t = if t + 1 == f.frame_len as int { 0int } else { t + 1 };
            let next_w = if cur == MAX_WINDOWS as int - 1 { 0int } else { cur + 1 };
            f.in_window(next_w, next_t)
        }),
{
    // Concrete case split over the 4 windows: for cur < 3 the boundary tick's
    // successor is offset[cur+1] (contiguity) and lies strictly below
    // frame_len (the remaining budgets are positive); for cur == 3 the
    // successor wraps to 0 == offset[0], inside window 0 (budget[0] > 0).
    assert(cur == 0 || cur == 1 || cur == 2 || cur == 3);
}

/// The switch FSM state: the static frame, the current window index, the
/// phase, and the `swapped` ledger bit that records whether the region swap
/// for the in-flight switch has already been performed. `swapped` is what
/// turns "region-swap-before-resume" from an API-reading into a
/// machine-checked state invariant (`inv`'s last three conjuncts).
pub struct Switcher {
    /// The static major frame (validated once via `MajorFrame::check`).
    pub frame: MajorFrame,
    /// Current window index, always < MAX_WINDOWS.
    pub cur: u32,
    /// Where the switch is in its preemption sequence.
    pub phase: SwPhase,
    /// True IFF the memory-protection regions for the incoming partition
    /// have been programmed during the in-flight switch. Cleared on entering
    /// SaveCtx, set only by `mark_swapped` (ProgramRegions → Resume).
    pub swapped: bool,
}

impl Switcher {
    /// Representation invariant. The last three conjuncts are S2
    /// (region-swap-before-resume) as a state-reachability invariant:
    /// `swapped` is false throughout SaveCtx and ProgramRegions (it is
    /// cleared at preemption and only `mark_swapped` sets it), so reaching
    /// Resume — which REQUIRES `swapped` — is possible only by passing
    /// through ProgramRegions' exit edge. A stale `swapped` from a previous
    /// switch can never satisfy the Resume conjunct: `tick` clears it on the
    /// Running → SaveCtx edge.
    pub open spec fn inv(&self) -> bool {
        self.frame.frame_inv()
        && self.cur < MAX_WINDOWS as u32
        && ((self.phase === SwPhase::Resume) ==> self.swapped)
        && ((self.phase === SwPhase::SaveCtx) ==> !self.swapped)
        && ((self.phase === SwPhase::ProgramRegions) ==> !self.swapped)
    }

    /// Ghost: is `t` the LAST tick of the current window (the window-end
    /// boundary at which preemption MUST fire)?
    pub open spec fn boundary(&self, t: int) -> bool {
        t + 1 == self.frame.win_end(self.cur as int)
    }

    /// Start of the major frame: window 0, Running.
    pub fn new(frame: MajorFrame) -> (s: Switcher)
        requires frame.frame_inv(),
        ensures
            s.inv(),
            s.phase === SwPhase::Running,
            s.cur == 0u32,
            s.frame == frame,
    {
        Switcher { frame, cur: 0, phase: SwPhase::Running, swapped: false }
    }

    /// S1 — NON-MASKABLE window-end preemption. One timer tick: from Running
    /// at the current window's end boundary, the FSM ALWAYS enters SaveCtx —
    /// the postcondition is an unconditional implication over EVERY state and
    /// EVERY input; there is no transition, flag, or argument that suppresses
    /// it, because none exists in the code (mirroring wdg-thin's
    /// cannot-un-start: no disable path is provided at all). Off-boundary
    /// ticks and non-Running phases are no-ops (a total function — the
    /// boundary test is in the body, not in a strippable precondition, so
    /// the shipped code is exactly as defensive as the verified code).
    pub fn tick(&mut self, t: u32) -> (switched: bool)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.frame == old(self).frame,
            self.cur == old(self).cur,
            switched == ((old(self).phase === SwPhase::Running) && old(self).boundary(t as int)),
            // S1: Running at the boundary ALWAYS preempts — no escape hatch.
            ((old(self).phase === SwPhase::Running) && old(self).boundary(t as int))
                ==> (self.phase === SwPhase::SaveCtx && !self.swapped),
            ((old(self).phase === SwPhase::Running) && !old(self).boundary(t as int))
                ==> (self.phase === SwPhase::Running && self.swapped == old(self).swapped),
            !(old(self).phase === SwPhase::Running)
                ==> (self.phase === old(self).phase && self.swapped == old(self).swapped),
    {
        if matches!(self.phase, SwPhase::Running) {
            proof {
                // win_end(cur) is one of the u32 fields offset[1..3]/frame_len
                // (frame_inv contiguity), so the u32 sum below cannot
                // overflow; and budget[cur] > 0 makes it >= 1, so the
                // boundary comparison's `end - 1` cannot underflow.
                let c = self.cur as int;
                assert(c == 0 || c == 1 || c == 2 || c == 3);
            }
            let end = self.frame.offset[self.cur as usize] + self.frame.budget[self.cur as usize];
            if t == end - 1 {
                self.phase = SwPhase::SaveCtx;
                self.swapped = false;
                true
            } else {
                false
            }
        } else {
            false
        }
    }

    /// One-way step: SaveCtx → ProgramRegions (the outgoing partition's
    /// context is now saved). No-op from any other phase.
    pub fn mark_saved(&mut self) -> (ok: bool)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.frame == old(self).frame,
            self.cur == old(self).cur,
            self.swapped == old(self).swapped,
            ok == (old(self).phase === SwPhase::SaveCtx),
            (old(self).phase === SwPhase::SaveCtx) ==> self.phase === SwPhase::ProgramRegions,
            !(old(self).phase === SwPhase::SaveCtx) ==> self.phase === old(self).phase,
    {
        if matches!(self.phase, SwPhase::SaveCtx) {
            self.phase = SwPhase::ProgramRegions;
            true
        } else {
            false
        }
    }

    /// One-way step: ProgramRegions → Resume, setting the `swapped` ledger
    /// bit. This is the ONLY place `swapped` becomes true, which is why
    /// `inv`'s Resume conjunct proves S2 (region-swap-before-resume): no
    /// other edge can discharge it. No-op from any other phase.
    pub fn mark_swapped(&mut self) -> (ok: bool)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.frame == old(self).frame,
            self.cur == old(self).cur,
            ok == (old(self).phase === SwPhase::ProgramRegions),
            (old(self).phase === SwPhase::ProgramRegions)
                ==> (self.phase === SwPhase::Resume && self.swapped),
            !(old(self).phase === SwPhase::ProgramRegions)
                ==> (self.phase === old(self).phase && self.swapped == old(self).swapped),
    {
        if matches!(self.phase, SwPhase::ProgramRegions) {
            self.phase = SwPhase::Resume;
            self.swapped = true;
            true
        } else {
            false
        }
    }

    /// One-way step: Resume → Running, advancing the window index by exactly
    /// one (mod MAX_WINDOWS) — together with `lemma_no_skip`, the frame is
    /// followed with no window skipped or repeated (S3). No-op from any
    /// other phase.
    pub fn mark_resumed(&mut self) -> (ok: bool)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.frame == old(self).frame,
            ok == (old(self).phase === SwPhase::Resume),
            (old(self).phase === SwPhase::Resume) ==> (
                self.phase === SwPhase::Running
                && self.cur as int == if old(self).cur as int == MAX_WINDOWS as int - 1 {
                    0int
                } else {
                    old(self).cur as int + 1
                }
            ),
            !(old(self).phase === SwPhase::Resume) ==> (
                self.phase === old(self).phase
                && self.cur == old(self).cur
                && self.swapped == old(self).swapped
            ),
    {
        if matches!(self.phase, SwPhase::Resume) {
            self.phase = SwPhase::Running;
            self.cur = if self.cur + 1 == MAX_WINDOWS as u32 { 0 } else { self.cur + 1 };
            true
        } else {
            false
        }
    }

    /// The trusted seam, wrapped to the minimum trusted surface (cf.
    /// executor.rs's `dispatch_one`): save the outgoing partition's context.
    /// `#[verifier::external_body]` — no ensures at all, so no proof ever
    /// leans on what the hardware did; the FSM's ordering guarantees rest
    /// exclusively on the verified `mark_*` steps around these calls.
    #[verifier::external_body]
    #[allow(unsafe_code)] // see the trusted-seam note at the top of this file
    fn seam_ctx_save(part: u32) -> u32 {
        unsafe { ctx_save(part) }
    }

    /// Trusted seam wrapper: program the incoming partition's regions. Wired
    /// to the isolation core's partition programmer at integration.
    #[verifier::external_body]
    #[allow(unsafe_code)] // see the trusted-seam note at the top of this file
    fn seam_region_swap(part: u32) -> u32 {
        unsafe { region_swap(part) }
    }

    /// Trusted seam wrapper: resume the incoming partition's context.
    #[verifier::external_body]
    #[allow(unsafe_code)] // see the trusted-seam note at the top of this file
    fn seam_ctx_resume(part: u32) -> u32 {
        unsafe { ctx_resume(part) }
    }

    /// Drive one full switch after a boundary preemption: SaveCtx →
    /// ProgramRegions → Resume → Running, crossing each trusted seam in
    /// order. The `swapped` invariant machine-checks the FSM ordering:
    /// `mark_swapped` strictly precedes `mark_resumed` (S2 at the FSM
    /// level). The binding of `seam_region_swap` to `mark_swapped` — that
    /// the seam call is actually issued on that edge — is trusted code
    /// order in this body, not machine-checked (see the trusted-seam note
    /// at the top of this file). Ends back in Running with the window
    /// index advanced by exactly one (mod MAX_WINDOWS).
    pub fn run_switch(&mut self)
        requires
            old(self).inv(),
            old(self).phase === SwPhase::SaveCtx,
        ensures
            self.inv(),
            self.frame == old(self).frame,
            self.phase === SwPhase::Running,
            self.cur as int == if old(self).cur as int == MAX_WINDOWS as int - 1 {
                0int
            } else {
                old(self).cur as int + 1
            },
    {
        let outgoing = self.frame.partition_id[self.cur as usize];
        let next = if self.cur + 1 == MAX_WINDOWS as u32 { 0 } else { self.cur + 1 };
        let incoming = self.frame.partition_id[next as usize];
        let _ = Self::seam_ctx_save(outgoing);
        let _ = self.mark_saved();
        let _ = Self::seam_region_swap(incoming);
        let _ = self.mark_swapped();
        let _ = Self::seam_ctx_resume(incoming);
        let _ = self.mark_resumed();
    }
}

/// S2 stated point-wise (the state-reachability invariant, instantiated):
/// in ANY well-formed state that has reached Resume, the region swap for the
/// in-flight switch has already been performed. Since `mark_swapped`
/// (ProgramRegions → Resume) is the only edge that sets `swapped`, and
/// `tick` clears it at every preemption, Resume is reachable only through
/// ProgramRegions.
pub proof fn lemma_resume_implies_region_swap(s: Switcher)
    requires s.inv(), s.phase === SwPhase::Resume,
    ensures s.swapped,
{
}

/// Kani cross-check: the SAME shipped executable FSM (post-`verus-strip`,
/// the `mark_*`/`tick`/`current_window` bodies are plain Rust — Kani calls
/// those exact functions, not a mirror) under a second, independent engine
/// (SAT-based bounded model checking vs. Verus's SMT). The only substitution
/// is the trusted seam: `run_switch`'s FFI calls cannot be linked, so `k2`/
/// `k4` drive the verified `mark_*` sequence directly — exactly the FSM
/// steps `run_switch` performs between seam calls, and the FSM (not the
/// seam) is what carries the S1/S2/S3 properties.
#[cfg(kani)]
mod switch_kani {
    use super::*;

    /// An arbitrary VALID major frame: unconstrained positive budgets (bounded
    /// below 2^30 so the four u32 partial sums cannot overflow), offsets
    /// derived by the contiguity rule, arbitrary partition ids.
    fn any_valid_frame() -> MajorFrame {
        let b: [u32; MAX_WINDOWS] = kani::any();
        kani::assume(b[0] > 0 && b[0] < 0x4000_0000);
        kani::assume(b[1] > 0 && b[1] < 0x4000_0000);
        kani::assume(b[2] > 0 && b[2] < 0x4000_0000);
        kani::assume(b[3] > 0 && b[3] < 0x4000_0000);
        let o1 = b[0];
        let o2 = o1 + b[1];
        let o3 = o2 + b[2];
        let fl = o3 + b[3];
        MajorFrame {
            partition_id: kani::any(),
            offset: [0, o1, o2, o3],
            budget: b,
            frame_len: fl,
        }
    }

    /// k1 — S1 NON-MASKABLE: for EVERY valid frame, EVERY window index, and
    /// EVERY prior `swapped` value, a tick at the window-end boundary from
    /// Running ALWAYS enters SaveCtx (and clears the swap ledger); and NO
    /// off-boundary tick ever preempts. There is no input under which the
    /// boundary preemption is suppressed.
    #[kani::proof]
    fn k1_nonmaskable_boundary() {
        let f = any_valid_frame();
        let cur: u32 = kani::any();
        kani::assume(cur < MAX_WINDOWS as u32);
        let swapped: bool = kani::any();
        let mut s = Switcher { frame: f, cur, phase: SwPhase::Running, swapped };
        // the boundary: the LAST tick of the current window
        let end = f.offset[cur as usize] + f.budget[cur as usize];
        let switched = s.tick(end - 1);
        assert!(switched);
        assert!(matches!(s.phase, SwPhase::SaveCtx));
        assert!(!s.swapped);
        // and off the boundary, tick never preempts
        let t2: u32 = kani::any();
        kani::assume(t2 != end - 1);
        let mut s2 = Switcher { frame: f, cur, phase: SwPhase::Running, swapped };
        let switched2 = s2.tick(t2);
        assert!(!switched2);
        assert!(matches!(s2.phase, SwPhase::Running));
    }

    /// k2 — S2 ORDERING: over EVERY inv-satisfying state and EVERY single
    /// FSM operation, (a) if the state transitions INTO Resume, the prior
    /// phase was ProgramRegions — no other entry edge exists; (b) the
    /// `swapped` state invariant (Resume implies the region swap already
    /// happened; it has NOT happened while saving/programming) is preserved.
    #[kani::proof]
    fn k2_resume_only_via_program_regions() {
        let f = any_valid_frame();
        let cur: u32 = kani::any();
        kani::assume(cur < MAX_WINDOWS as u32);
        let phase = match kani::any::<u8>() % 4 {
            0 => SwPhase::Running,
            1 => SwPhase::SaveCtx,
            2 => SwPhase::ProgramRegions,
            _ => SwPhase::Resume,
        };
        let swapped: bool = kani::any();
        // entry state satisfies the switch invariant
        kani::assume(!matches!(phase, SwPhase::Resume) || swapped);
        kani::assume(!matches!(phase, SwPhase::SaveCtx) || !swapped);
        kani::assume(!matches!(phase, SwPhase::ProgramRegions) || !swapped);
        let mut s = Switcher { frame: f, cur, phase, swapped };
        let was_resume = matches!(s.phase, SwPhase::Resume);
        let was_program = matches!(s.phase, SwPhase::ProgramRegions);
        // any single operation
        match kani::any::<u8>() % 4 {
            0 => {
                let t: u32 = kani::any();
                let _ = s.tick(t);
            }
            1 => {
                let _ = s.mark_saved();
            }
            2 => {
                let _ = s.mark_swapped();
            }
            _ => {
                let _ = s.mark_resumed();
            }
        }
        // (a) Resume is entered only from ProgramRegions
        if matches!(s.phase, SwPhase::Resume) && !was_resume {
            assert!(was_program);
        }
        // (b) the region-swap-before-resume invariant is preserved
        if matches!(s.phase, SwPhase::Resume) {
            assert!(s.swapped);
        }
        if matches!(s.phase, SwPhase::SaveCtx) {
            assert!(!s.swapped);
        }
        if matches!(s.phase, SwPhase::ProgramRegions) {
            assert!(!s.swapped);
        }
    }

    /// k3 — frame coverage: for EVERY valid frame and EVERY tick
    /// `t < frame_len`, `current_window` returns a window that contains `t`,
    /// and brute force confirms NO other window does — every tick maps to
    /// exactly one window (coverage without overlap).
    #[kani::proof]
    #[kani::unwind(5)]
    fn k3_frame_covers_exactly_one() {
        let f = any_valid_frame();
        assert!(f.check());
        let t: u32 = kani::any();
        kani::assume(t < f.frame_len);
        let w = f.current_window(t);
        assert!(w < MAX_WINDOWS as u32);
        // containment (u64 sums: never overflows)
        assert!(f.offset[w as usize] <= t);
        assert!((t as u64) < f.offset[w as usize] as u64 + f.budget[w as usize] as u64);
        // uniqueness — brute force over all other windows
        let mut j: u32 = 0;
        while j < MAX_WINDOWS as u32 {
            if j != w {
                let inside = f.offset[j as usize] <= t
                    && (t as u64) < f.offset[j as usize] as u64 + f.budget[j as usize] as u64;
                assert!(!inside);
            }
            j += 1;
        }
    }

    /// k4 — S3 no-skip: from EVERY valid frame and EVERY window, a boundary
    /// preemption followed by the full switch sequence lands back in Running
    /// with the window index advanced by EXACTLY one (mod MAX_WINDOWS), and
    /// the next timeline tick falls in exactly that window per
    /// `current_window` — the frame is followed, nothing skipped or
    /// repeated.
    #[kani::proof]
    fn k4_no_skip_advances_by_one() {
        let f = any_valid_frame();
        let cur: u32 = kani::any();
        kani::assume(cur < MAX_WINDOWS as u32);
        let mut s = Switcher { frame: f, cur, phase: SwPhase::Running, swapped: kani::any() };
        let end = f.offset[cur as usize] + f.budget[cur as usize];
        let t = end - 1;
        assert!(s.tick(t));
        assert!(s.mark_saved());
        assert!(s.mark_swapped());
        assert!(s.mark_resumed());
        assert!(matches!(s.phase, SwPhase::Running));
        let expect = if cur + 1 == MAX_WINDOWS as u32 { 0 } else { cur + 1 };
        assert!(s.cur == expect);
        // the next tick on the timeline lands in exactly the new window
        let next_t = if t + 1 == f.frame_len { 0 } else { t + 1 };
        assert!(f.current_window(next_t) == s.cur);
    }
}

} // verus!

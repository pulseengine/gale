//! gust value-domain Health Monitor + failsafe state machine (v0.7.0, REQ-OS-HM-001).
//!
//! The tier-1/tier-2 supervision core of the partition-scheduler design (spec §4/§4.1):
//! a **pure scalar policy core** that catches ERRONEOUS-BUT-TIMELY output — a task that
//! meets its deadline while emitting garbage (stale data, implausible values, estimator
//! divergence) — not just liveness. Sensor IO is the partition's job; the HM consumes
//! scalar observations and drives a failsafe FSM with provably-terminating escalation.
//!
//! Structure:
//! - **Value-domain gates** (`fresh`, `plausible`, `innovation_ok`) + liveness gates
//!   (`budget_ok`, `deadline_ok`, `heartbeat_ok`) — total, verified pure functions over
//!   scalars, each with an `ensures` characterizing exactly when it passes. (Verifying
//!   the policy FSM does not verify the value-domain *detectors* upstream of these
//!   scalars — those carry their own analysis; spec §4.1.)
//! - **Failsafe FSM** `Normal → Degraded → PartitionFailsafe → CrossCoreTrip` with a
//!   bounded per-episode restart budget (`MAX_RESTARTS`), mirroring the wdg-thin
//!   cannot-un-start discipline: once failsafe-latched there is NO software path back —
//!   recovery is a hardware/external reset, out of scope here.
//!
//! Proven (Verus, this file):
//! - **H1 terminating escalation** — `measure` strictly decreases on every fault-driven
//!   transition (`lemma_fault_decreases_measure`), and an always-faulting input reaches
//!   `CrossCoreTrip` in ≤ `MAX_RESTARTS + 2` steps (`lemma_escalation_bound`).
//! - **H2 no-silent-clear** — `Degraded → Normal` requires the *triggering* gate to
//!   actually pass on the presented observation; a still-violating observation is
//!   rejected without mutating (`try_restart` ensures).
//! - **H3 absorbing failsafe** — no fault or restart leaves `PartitionFailsafe` except
//!   the liveness trip to `CrossCoreTrip`; `CrossCoreTrip` is terminal
//!   (`lemma_failsafe_absorbing`, `lemma_trip_absorbing`).
//! - **H4 cause preserved** — the recorded cause is the fault that entered `Degraded`,
//!   unchanged until cleared (`lemma_cause_preserved` + `on_fault`/`try_restart` ensures).
//!
//! Kani mirrors H1–H4 over `kani::any()` inputs in `hm_kani` below (run against the
//! stripped plain/ crate: `cargo kani --harness h1_...` etc.).
use vstd::prelude::*;

verus! {

/// Restart budget per Degraded episode. Re-armed on every Normal → Degraded entry.
/// Must be ≥ 1 (the FSM invariant `Degraded ==> restarts_remaining >= 1` is seeded by
/// this constant; `lemma_escalation_bound`'s `MAX_RESTARTS + 2` count relies on it).
pub const MAX_RESTARTS: u32 = 3;

/// Health-monitor supervision state, ordered by severity (see `rank`).
///
/// `PartitionFailsafe` and `CrossCoreTrip` are ABSORBING for software: no fault or
/// restart input ever lowers the rank (H3) — mirroring wdg-thin's cannot-un-start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HmState {
    /// Healthy: all gates passing.
    Normal,
    /// A fault was observed; bounded restart attempts are permitted while the
    /// per-episode budget lasts AND the triggering gate clears (H2).
    Degraded,
    /// Restart budget exhausted: the partition is held in its fail-to-safe
    /// configuration. Terminal for value faults; only a liveness fault escalates.
    PartitionFailsafe,
    /// The cross-core (tier-2) trip has been requested — the physically independent
    /// supervisor core asserts the hardware failsafe. Terminal for software.
    CrossCoreTrip,
}

/// What went wrong. `Stale`/`Implausible`/`Diverged` are the value-domain faults
/// (erroneous-but-timely output); `BudgetOverrun`/`DeadlineMiss`/`HeartbeatLoss` are
/// the liveness/timing faults the tier-1/tier-2 monitors raise.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Fault {
    /// Data freshness gate failed: observation older than its staleness limit.
    Stale,
    /// Range/plausibility gate failed: value outside its physical bounds.
    Implausible,
    /// Innovation gate failed: estimator innovation above the k·sigma threshold.
    Diverged,
    /// Partition consumed more than its execution budget in the window.
    BudgetOverrun,
    /// Partition output missed its deadline.
    DeadlineMiss,
    /// The partition's heartbeat was not observed.
    HeartbeatLoss,
}

/// One scalar observation frame — everything the HM policy core consumes. The
/// partition (or the tier-1 monitor) computes these scalars; the HM only gates them.
/// All integer form: `innov_abs`/`k_sigma` are the pre-scaled integer magnitudes
/// (|innovation| and k·sigma in the same fixed-point unit) — no floats in the core.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Obs {
    /// Age of the newest sample, in ms.
    pub age_ms: u32,
    /// Staleness limit for that sample, in ms.
    pub limit_ms: u32,
    /// The observed value (fixed-point / raw sensor units).
    pub value: i32,
    /// Inclusive lower plausibility bound for `value`.
    pub lo: i32,
    /// Inclusive upper plausibility bound for `value`.
    pub hi: i32,
    /// |estimator innovation| (integer, same unit as `k_sigma`).
    pub innov_abs: u32,
    /// The k·sigma divergence threshold (integer, pre-scaled).
    pub k_sigma: u32,
    /// Execution time consumed in the current window, in µs.
    pub used_us: u32,
    /// Execution budget for the window, in µs.
    pub budget_us: u32,
    /// How late the last output was, in µs (0 = on time).
    pub lateness_us: u32,
    /// Consecutive heartbeats missed (0 = heartbeat present).
    pub missed_beats: u32,
}

// ===========================================================================
// Value-domain gates (total, verified pure functions over scalars)
// ===========================================================================

/// Staleness gate: the observation is fresh iff its age is within the limit.
pub fn fresh(age_ms: u32, limit_ms: u32) -> (ok: bool)
    ensures ok == (age_ms <= limit_ms),
{
    age_ms <= limit_ms
}

/// Range/plausibility gate: the value is plausible iff it lies in [lo, hi].
pub fn plausible(value: i32, lo: i32, hi: i32) -> (ok: bool)
    ensures ok == (lo <= value && value <= hi),
{
    lo <= value && value <= hi
}

/// Divergence gate (scalar-int form): the estimator innovation is acceptable iff
/// |innovation| is within the k·sigma threshold. Caller pre-scales both to the same
/// integer unit, so the gate itself is exact integer comparison — total, no overflow.
pub fn innovation_ok(innov_abs: u32, k_sigma: u32) -> (ok: bool)
    ensures ok == (innov_abs <= k_sigma),
{
    innov_abs <= k_sigma
}

/// Budget gate: within budget iff consumed time does not exceed the window budget.
pub fn budget_ok(used_us: u32, budget_us: u32) -> (ok: bool)
    ensures ok == (used_us <= budget_us),
{
    used_us <= budget_us
}

/// Deadline gate: on time iff the last output had zero lateness.
pub fn deadline_ok(lateness_us: u32) -> (ok: bool)
    ensures ok == (lateness_us == 0),
{
    lateness_us == 0
}

/// Heartbeat gate: alive iff no consecutive heartbeat has been missed.
pub fn heartbeat_ok(missed_beats: u32) -> (ok: bool)
    ensures ok == (missed_beats == 0),
{
    missed_beats == 0
}

/// Ghost: does observation `obs` clear the gate that corresponds to fault `cause`?
/// This is THE no-silent-clear pivot (H2): `try_restart` may only return to Normal
/// when this holds for the recorded cause.
pub open spec fn gate_clears(cause: Fault, obs: Obs) -> bool {
    match cause {
        Fault::Stale => obs.age_ms <= obs.limit_ms,
        Fault::Implausible => obs.lo <= obs.value && obs.value <= obs.hi,
        Fault::Diverged => obs.innov_abs <= obs.k_sigma,
        Fault::BudgetOverrun => obs.used_us <= obs.budget_us,
        Fault::DeadlineMiss => obs.lateness_us == 0,
        Fault::HeartbeatLoss => obs.missed_beats == 0,
    }
}

/// Evaluate the gate corresponding to `cause` on `obs` — the exec twin of
/// `gate_clears`, built from the verified pure gates above.
pub fn gate_eval(cause: Fault, obs: Obs) -> (ok: bool)
    ensures ok == gate_clears(cause, obs),
{
    match cause {
        Fault::Stale => fresh(obs.age_ms, obs.limit_ms),
        Fault::Implausible => plausible(obs.value, obs.lo, obs.hi),
        Fault::Diverged => innovation_ok(obs.innov_abs, obs.k_sigma),
        Fault::BudgetOverrun => budget_ok(obs.used_us, obs.budget_us),
        Fault::DeadlineMiss => deadline_ok(obs.lateness_us),
        Fault::HeartbeatLoss => heartbeat_ok(obs.missed_beats),
    }
}

// ===========================================================================
// Failsafe FSM
// ===========================================================================

/// Ghost: severity rank of a state — the coarse component of the H1 measure.
/// Never decreases under any input (H3 is the rank-≥-2 half of that statement).
pub open spec fn rank(s: HmState) -> nat {
    match s {
        HmState::Normal => 0,
        HmState::Degraded => 1,
        HmState::PartitionFailsafe => 2,
        HmState::CrossCoreTrip => 3,
    }
}

/// Ghost: does fault `f` escalate `PartitionFailsafe` to `CrossCoreTrip`?
/// Only the liveness faults trip the cross-core supervisor (spec §4 tier 2:
/// "heartbeat-loss / window overrun"); value faults cannot cross-trip — the
/// partition is already held safe, and the trip is a containment action.
pub open spec fn trips_cross_core(f: Fault) -> bool {
    f === Fault::HeartbeatLoss || f === Fault::BudgetOverrun
}

/// The health-monitor policy state: supervision state + recorded cause + the
/// remaining per-episode restart budget.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hm {
    /// Current supervision state.
    pub state: HmState,
    /// The fault that entered the current Degraded episode (H4: first cause wins,
    /// preserved through escalation for the post-mortem record). Meaningful from the
    /// first fault on; `init` seeds it with an arbitrary variant.
    pub cause: Fault,
    /// Restart credits left in this Degraded episode. Invariant: ≥ 1 while Degraded
    /// (the fault that would consume the last credit escalates instead of idling).
    pub restarts_remaining: u32,
}

/// Ghost: the FSM transition on a fault event — the single source of truth all four
/// H-proofs quantify over; the exec `on_fault` is proven equal to it field-by-field.
///
/// - `Normal --fault--> Degraded`: records the cause, re-arms the budget.
/// - `Degraded --fault-->`: burns one restart credit (a fault while degraded is a
///   failed recovery); the fault that consumes the LAST credit escalates to
///   `PartitionFailsafe` (an exhausted partition gets no extra grace fault — this is
///   what makes the H1 bound `MAX_RESTARTS + 2` instead of `+ 3`). Cause unchanged (H4).
/// - `PartitionFailsafe --liveness-fault--> CrossCoreTrip`; value faults are absorbed
///   (already held safe; H3).
/// - `CrossCoreTrip`: absorbed (terminal; H3).
pub open spec fn step_fault(hm: Hm, f: Fault) -> Hm {
    match hm.state {
        HmState::Normal => Hm {
            state: HmState::Degraded,
            cause: f,
            restarts_remaining: MAX_RESTARTS,
        },
        HmState::Degraded => if hm.restarts_remaining <= 1 {
            Hm { state: HmState::PartitionFailsafe, cause: hm.cause, restarts_remaining: 0 }
        } else {
            Hm {
                state: HmState::Degraded,
                cause: hm.cause,
                restarts_remaining: (hm.restarts_remaining - 1) as u32,
            }
        },
        HmState::PartitionFailsafe => if trips_cross_core(f) {
            Hm { state: HmState::CrossCoreTrip, cause: hm.cause, restarts_remaining: 0 }
        } else {
            hm
        },
        HmState::CrossCoreTrip => hm,
    }
}

/// Ghost: the H1 termination measure over (state rank, restarts_remaining) —
/// lexicographic, flattened to one `nat`. Strictly decreases on every fault-driven
/// transition (`lemma_fault_decreases_measure`); constant on absorbed faults.
pub open spec fn measure(hm: Hm) -> nat {
    match hm.state {
        HmState::Normal => MAX_RESTARTS as nat + 3,
        HmState::Degraded => hm.restarts_remaining as nat + 2,
        HmState::PartitionFailsafe => 1,
        HmState::CrossCoreTrip => 0,
    }
}

/// Ghost: exact number of always-faulting steps from `hm` to `CrossCoreTrip` when
/// every fault is a liveness fault — the potential behind `lemma_escalation_bound`.
/// `phi == 0` iff already tripped.
pub open spec fn phi(hm: Hm) -> nat {
    match hm.state {
        HmState::Normal => MAX_RESTARTS as nat + 2,
        HmState::Degraded => hm.restarts_remaining as nat + 1,
        HmState::PartitionFailsafe => 1,
        HmState::CrossCoreTrip => 0,
    }
}

impl Hm {
    /// Representation invariant: the restart budget is bounded by `MAX_RESTARTS`,
    /// and while Degraded at least one credit remains (the transition that would
    /// consume the last credit escalates instead — see `step_fault`).
    pub open spec fn inv(&self) -> bool {
        self.restarts_remaining <= MAX_RESTARTS
            && (self.state === HmState::Degraded ==> self.restarts_remaining >= 1)
    }

    /// The healthy monitor. `cause` is seeded arbitrarily (it is meaningful only
    /// once a fault has been recorded — H4 speaks from Degraded entry onward).
    pub fn init() -> (r: Hm)
        ensures r.inv(), r.state === HmState::Normal,
            r.restarts_remaining == MAX_RESTARTS,
    {
        Hm { state: HmState::Normal, cause: Fault::HeartbeatLoss, restarts_remaining: MAX_RESTARTS }
    }

    /// Apply one fault event. Exec twin of `step_fault` — the ensures tie every
    /// field to the ghost transition, so all H-lemmas about `step_fault` transfer
    /// verbatim to the running code.
    pub fn on_fault(&mut self, f: Fault)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.state === step_fault(*old(self), f).state,
            self.cause === step_fault(*old(self), f).cause,
            self.restarts_remaining == step_fault(*old(self), f).restarts_remaining,
    {
        if matches!(self.state, HmState::Normal) {
            self.state = HmState::Degraded;
            self.cause = f;
            self.restarts_remaining = MAX_RESTARTS;
        } else if matches!(self.state, HmState::Degraded) {
            if self.restarts_remaining <= 1 {
                self.state = HmState::PartitionFailsafe;
                self.restarts_remaining = 0;
            } else {
                self.restarts_remaining = self.restarts_remaining - 1;
            }
        } else if matches!(self.state, HmState::PartitionFailsafe) {
            if matches!(f, Fault::HeartbeatLoss | Fault::BudgetOverrun) {
                self.state = HmState::CrossCoreTrip;
                self.restarts_remaining = 0;
            }
        }
        // CrossCoreTrip: absorbed — terminal for software (H3).
    }

    /// Attempt a restart: `Degraded → Normal`, permitted ONLY IF a restart credit
    /// remains AND the observation clears the gate of the recorded cause (H2 —
    /// cannot clear while the fault persists). A rejected restart mutates NOTHING.
    /// From `PartitionFailsafe`/`CrossCoreTrip` a restart is always rejected (H3:
    /// no software path back; recovery is a hardware/external reset).
    pub fn try_restart(&mut self, obs: Obs) -> (ok: bool)
        requires old(self).inv(),
        ensures
            self.inv(),
            // exact acceptance characterization — passes IFF Degraded, budgeted,
            // and the triggering gate now clears on this observation
            ok == (old(self).state === HmState::Degraded
                && old(self).restarts_remaining >= 1
                && gate_clears(old(self).cause, obs)),
            // accepted: back to Normal, one credit burned, cause record intact
            ok ==> self.state === HmState::Normal
                && self.cause === old(self).cause
                && self.restarts_remaining + 1 == old(self).restarts_remaining,
            // rejected: NO mutation (H2's no-silent-clear + H3's restart half)
            !ok ==> self.state === old(self).state
                && self.cause === old(self).cause
                && self.restarts_remaining == old(self).restarts_remaining,
    {
        if matches!(self.state, HmState::Degraded) && self.restarts_remaining >= 1 {
            let cause = self.cause;
            if gate_eval(cause, obs) {
                self.state = HmState::Normal;
                self.restarts_remaining = self.restarts_remaining - 1;
                return true;
            }
        }
        false
    }

    /// True (exec) iff the monitor is failsafe-latched (PartitionFailsafe or
    /// CrossCoreTrip) — the "assert the partition's safe outputs" query.
    pub fn is_failsafe_latched(&self) -> (b: bool)
        ensures b == (rank(self.state) >= 2),
    {
        matches!(self.state, HmState::PartitionFailsafe | HmState::CrossCoreTrip)
    }
}

// ===========================================================================
// H1 — terminating escalation
// ===========================================================================

/// Every fault application preserves the invariant.
pub proof fn lemma_step_preserves_inv(hm: Hm, f: Fault)
    requires hm.inv(),
    ensures step_fault(hm, f).inv(),
{
}

/// H1 (measure half): on every fault-driven TRANSITION — i.e. every fault except the
/// absorbed ones (a value fault in PartitionFailsafe, anything in CrossCoreTrip) —
/// the measure over (state rank, restarts_remaining) strictly decreases. Absorbed
/// faults change nothing at all.
pub proof fn lemma_fault_decreases_measure(hm: Hm, f: Fault)
    requires hm.inv(),
    ensures
        // strict decrease on every transition that fires
        !(hm.state === HmState::CrossCoreTrip)
            && !(hm.state === HmState::PartitionFailsafe && !trips_cross_core(f))
            ==> measure(step_fault(hm, f)) < measure(hm),
        // absorbed faults are pure no-ops (state, cause, budget all untouched)
        (hm.state === HmState::CrossCoreTrip
            || (hm.state === HmState::PartitionFailsafe && !trips_cross_core(f)))
            ==> step_fault(hm, f).state === hm.state
                && step_fault(hm, f).cause === hm.cause
                && step_fault(hm, f).restarts_remaining == hm.restarts_remaining,
{
}

/// H1 (progress half): below the failsafe line (rank < 2), EVERY fault — value or
/// liveness — decreases the trip potential `phi` by exactly one. This is why fault
/// KIND never delays escalation before the failsafe latch.
pub proof fn lemma_fault_progress(hm: Hm, f: Fault)
    requires hm.inv(), rank(hm.state) < 2,
    ensures
        phi(step_fault(hm, f)) + 1 == phi(hm),
        step_fault(hm, f).inv(),
{
}

/// Ghost: `k` consecutive liveness faults (the always-faulting worst case that can
/// actually reach the cross-core trip — value faults are absorbed at the
/// PartitionFailsafe latch by design).
pub open spec fn apply_liveness_faults(hm: Hm, k: nat) -> Hm
    decreases k,
{
    if k == 0 {
        hm
    } else {
        apply_liveness_faults(step_fault(hm, Fault::HeartbeatLoss), (k - 1) as nat)
    }
}

/// H1 (bound, general form): from any well-formed state, `phi(hm)` consecutive
/// liveness faults reach `CrossCoreTrip`. Induction on `k`, driven by
/// `lemma_fault_progress` below the latch and the direct PF → CCT step at it.
pub proof fn lemma_liveness_faults_trip(hm: Hm, k: nat)
    requires hm.inv(), phi(hm) <= k,
    ensures apply_liveness_faults(hm, k).state === HmState::CrossCoreTrip,
    decreases k,
{
    if k == 0 {
        // phi == 0 only in CrossCoreTrip (Normal/Degraded/PartitionFailsafe all ≥ 1)
        assert(hm.state === HmState::CrossCoreTrip);
    } else {
        let next = step_fault(hm, Fault::HeartbeatLoss);
        if rank(hm.state) < 2 {
            lemma_fault_progress(hm, Fault::HeartbeatLoss);
        } else {
            // PartitionFailsafe: HeartbeatLoss trips (phi 1 → 0);
            // CrossCoreTrip: absorbed (phi stays 0). Either way phi(next) ≤ k - 1.
            lemma_step_preserves_inv(hm, Fault::HeartbeatLoss);
        }
        assert(phi(next) <= (k - 1) as nat);
        lemma_liveness_faults_trip(next, (k - 1) as nat);
    }
}

/// H1 (THE bound): an always-faulting input starting from Normal reaches
/// `CrossCoreTrip` in ≤ `MAX_RESTARTS + 2` steps: 1 (Normal → Degraded, budget = MAX)
/// + (MAX − 1) credit burns + 1 (last credit → PartitionFailsafe) + 1 (liveness trip).
pub proof fn lemma_escalation_bound(hm: Hm)
    requires hm.inv(), hm.state === HmState::Normal,
    ensures
        apply_liveness_faults(hm, (MAX_RESTARTS + 2) as nat).state === HmState::CrossCoreTrip,
{
    assert(phi(hm) == MAX_RESTARTS as nat + 2);
    lemma_liveness_faults_trip(hm, (MAX_RESTARTS + 2) as nat);
}

// ===========================================================================
// H3 — absorbing failsafe
// ===========================================================================

/// H3 (fault half, PartitionFailsafe): no fault leaves `PartitionFailsafe` except the
/// liveness trip UP to `CrossCoreTrip`; a value fault changes nothing. Combined with
/// `try_restart`'s rejected-means-untouched ensures (its guard requires Degraded),
/// no input sequence ever lowers the rank below 2 again.
pub proof fn lemma_failsafe_absorbing(hm: Hm, f: Fault)
    requires hm.state === HmState::PartitionFailsafe,
    ensures
        rank(step_fault(hm, f).state) >= 2,
        trips_cross_core(f) ==> step_fault(hm, f).state === HmState::CrossCoreTrip,
        !trips_cross_core(f) ==> step_fault(hm, f).state === hm.state
            && step_fault(hm, f).cause === hm.cause
            && step_fault(hm, f).restarts_remaining == hm.restarts_remaining,
{
}

/// H3 (fault half, CrossCoreTrip): terminal — every fault is absorbed unchanged.
pub proof fn lemma_trip_absorbing(hm: Hm, f: Fault)
    requires hm.state === HmState::CrossCoreTrip,
    ensures
        step_fault(hm, f).state === hm.state,
        step_fault(hm, f).cause === hm.cause,
        step_fault(hm, f).restarts_remaining == hm.restarts_remaining,
{
}

/// H3 (monotone rank): under ANY single fault the severity rank never decreases —
/// the FSM only ever escalates; de-escalation exists solely as the gated, budgeted
/// `try_restart` from Degraded.
pub proof fn lemma_rank_monotone(hm: Hm, f: Fault)
    requires hm.inv(),
    ensures rank(step_fault(hm, f).state) >= rank(hm.state),
{
}

// ===========================================================================
// H4 — cause preserved
// ===========================================================================

/// H4: once Degraded, the recorded cause survives every further fault unchanged
/// (first cause wins — through credit burns AND through the escalation into
/// `PartitionFailsafe`/`CrossCoreTrip`, so the post-mortem record is the entering
/// fault). Entry from Normal records exactly the entering fault.
pub proof fn lemma_cause_preserved(hm: Hm, f: Fault)
    requires hm.inv(),
    ensures
        !(hm.state === HmState::Normal) ==> step_fault(hm, f).cause === hm.cause,
        hm.state === HmState::Normal ==> step_fault(hm, f).cause === f,
{
}

// ===========================================================================
// Kani harnesses — H1–H4 over kani::any() inputs, against the stripped
// plain/ executable (same two-tool discipline as executor.rs: Verus proves the
// spec-level FSM above via SMT/Z3; Kani model-checks the SAME shipped exec
// code below via CBMC, no hand-copied mirror of the transition logic).
// ===========================================================================
#[cfg(kani)]
mod hm_kani {
    use super::*;

    fn any_fault() -> Fault {
        match kani::any::<u8>() % 6 {
            0 => Fault::Stale,
            1 => Fault::Implausible,
            2 => Fault::Diverged,
            3 => Fault::BudgetOverrun,
            4 => Fault::DeadlineMiss,
            _ => Fault::HeartbeatLoss,
        }
    }

    fn any_state() -> HmState {
        match kani::any::<u8>() % 4 {
            0 => HmState::Normal,
            1 => HmState::Degraded,
            2 => HmState::PartitionFailsafe,
            _ => HmState::CrossCoreTrip,
        }
    }

    fn any_obs() -> Obs {
        Obs {
            age_ms: kani::any(),
            limit_ms: kani::any(),
            value: kani::any(),
            lo: kani::any(),
            hi: kani::any(),
            innov_abs: kani::any(),
            k_sigma: kani::any(),
            used_us: kani::any(),
            budget_us: kani::any(),
            lateness_us: kani::any(),
            missed_beats: kani::any(),
        }
    }

    /// An arbitrary well-formed Hm: budget bounded, and ≥ 1 while Degraded —
    /// the exec mirror of `Hm::inv()` (spec fns are stripped from plain/).
    fn any_hm() -> Hm {
        let h = Hm { state: any_state(), cause: any_fault(), restarts_remaining: kani::any() };
        kani::assume(h.restarts_remaining <= MAX_RESTARTS);
        if matches!(h.state, HmState::Degraded) {
            kani::assume(h.restarts_remaining >= 1);
        }
        h
    }

    /// Exec mirror of the ghost `measure` (harness-only helper).
    fn measure_exec(h: &Hm) -> u32 {
        match h.state {
            HmState::Normal => MAX_RESTARTS + 3,
            HmState::Degraded => h.restarts_remaining + 2,
            HmState::PartitionFailsafe => 1,
            HmState::CrossCoreTrip => 0,
        }
    }

    fn is_liveness(f: Fault) -> bool {
        matches!(f, Fault::HeartbeatLoss | Fault::BudgetOverrun)
    }

    /// H1a — the measure strictly decreases on every fault-driven transition, over an
    /// ARBITRARY fault sequence of MAX_RESTARTS + 3 steps from init; absorbed faults
    /// (value fault at PartitionFailsafe, anything at CrossCoreTrip) change nothing.
    /// After MAX_RESTARTS + 1 arbitrary faults the FSM is failsafe-latched.
    #[kani::proof]
    #[kani::unwind(8)]
    fn h1_escalation_terminates() {
        let mut h = Hm::init();
        let mut i: u32 = 0;
        while i < MAX_RESTARTS + 3 {
            let f = any_fault();
            let before = measure_exec(&h);
            let absorbed = matches!(h.state, HmState::CrossCoreTrip)
                || (matches!(h.state, HmState::PartitionFailsafe) && !is_liveness(f));
            let pre = h;
            h.on_fault(f);
            if absorbed {
                assert!(h == pre);
            } else {
                assert!(measure_exec(&h) < before);
            }
            i += 1;
            if i >= MAX_RESTARTS + 1 {
                // escalation has terminated in a latched failsafe state
                assert!(h.is_failsafe_latched());
            }
        }
    }

    /// H1b — THE bound: an always-faulting liveness input from init reaches
    /// CrossCoreTrip in ≤ MAX_RESTARTS + 2 steps, and stays there (sequence bounded
    /// to MAX_RESTARTS + 3 steps to observe the bound plus one absorbing step).
    #[kani::proof]
    #[kani::unwind(8)]
    fn h1_trip_bound() {
        let mut h = Hm::init();
        let mut i: u32 = 0;
        while i < MAX_RESTARTS + 3 {
            // liveness fault every step (HeartbeatLoss or BudgetOverrun, any mix)
            let f = if kani::any() { Fault::HeartbeatLoss } else { Fault::BudgetOverrun };
            h.on_fault(f);
            i += 1;
            if i >= MAX_RESTARTS + 2 {
                assert!(matches!(h.state, HmState::CrossCoreTrip));
            }
        }
    }

    /// H2 — no-silent-clear: a restart succeeds ONLY from Degraded with budget left
    /// AND the recorded cause's gate actually passing on the observation; if the
    /// observation still violates the gate, the restart is rejected WITHOUT mutating.
    #[kani::proof]
    fn h2_no_silent_clear() {
        let mut h = any_hm();
        let obs = any_obs();
        let pre = h;
        let ok = h.try_restart(obs);
        if ok {
            assert!(matches!(pre.state, HmState::Degraded));
            assert!(pre.restarts_remaining >= 1);
            assert!(gate_eval(pre.cause, obs)); // the gate genuinely passes
            assert!(matches!(h.state, HmState::Normal));
            assert!(h.restarts_remaining == pre.restarts_remaining - 1);
            assert!(h.cause == pre.cause);
        } else {
            assert!(h == pre); // rejected restarts never mutate
        }
        // the still-faulting observation can NEVER clear
        if matches!(pre.state, HmState::Degraded) && !gate_eval(pre.cause, obs) {
            assert!(!ok);
        }
    }

    /// H3 — absorbing failsafe: no fault and no restart leaves PartitionFailsafe
    /// except the liveness trip to CrossCoreTrip; CrossCoreTrip is terminal.
    #[kani::proof]
    fn h3_absorbing_failsafe() {
        // PartitionFailsafe under an arbitrary fault
        let mut pf = any_hm();
        kani::assume(matches!(pf.state, HmState::PartitionFailsafe));
        let f = any_fault();
        let pf_pre = pf;
        pf.on_fault(f);
        assert!(matches!(pf.state, HmState::PartitionFailsafe | HmState::CrossCoreTrip));
        if is_liveness(f) {
            assert!(matches!(pf.state, HmState::CrossCoreTrip));
        } else {
            assert!(pf == pf_pre);
        }
        // PartitionFailsafe under a restart attempt: always rejected, untouched
        let mut pf2 = any_hm();
        kani::assume(matches!(pf2.state, HmState::PartitionFailsafe));
        let pf2_pre = pf2;
        assert!(!pf2.try_restart(any_obs()));
        assert!(pf2 == pf2_pre);
        // CrossCoreTrip: terminal under both inputs
        let mut ct = any_hm();
        kani::assume(matches!(ct.state, HmState::CrossCoreTrip));
        let ct_pre = ct;
        ct.on_fault(any_fault());
        assert!(ct == ct_pre);
        assert!(!ct.try_restart(any_obs()));
        assert!(ct == ct_pre);
    }

    /// H4 — cause preserved: the fault that enters Degraded is recorded and survives
    /// every further fault (credit burns AND the escalation into the failsafe states)
    /// unchanged, until a successful restart clears the episode.
    #[kani::proof]
    #[kani::unwind(8)]
    fn h4_cause_preserved() {
        let mut h = Hm::init();
        let entering = any_fault();
        h.on_fault(entering); // Normal → Degraded records the cause
        assert!(matches!(h.state, HmState::Degraded));
        assert!(h.cause == entering);
        let mut i: u32 = 0;
        while i < MAX_RESTARTS + 2 {
            h.on_fault(any_fault());
            assert!(h.cause == entering); // unchanged through burns + escalation
            i += 1;
        }
        // and a REJECTED restart preserves it too
        let mut d = any_hm();
        kani::assume(matches!(d.state, HmState::Degraded));
        let d_cause = d.cause;
        let obs = any_obs();
        kani::assume(!gate_eval(d_cause, obs));
        assert!(!d.try_restart(obs));
        assert!(d.cause == d_cause);
    }

    /// Gates — definitional characterization of every pure gate (the Verus ensures,
    /// re-checked by CBMC on the shipped exec code).
    #[kani::proof]
    fn h5_gates_characterized() {
        let a: u32 = kani::any();
        let l: u32 = kani::any();
        assert!(fresh(a, l) == (a <= l));
        let v: i32 = kani::any();
        let lo: i32 = kani::any();
        let hi: i32 = kani::any();
        assert!(plausible(v, lo, hi) == (lo <= v && v <= hi));
        let inn: u32 = kani::any();
        let ks: u32 = kani::any();
        assert!(innovation_ok(inn, ks) == (inn <= ks));
        let u: u32 = kani::any();
        let b: u32 = kani::any();
        assert!(budget_ok(u, b) == (u <= b));
        let lat: u32 = kani::any();
        assert!(deadline_ok(lat) == (lat == 0));
        let mb: u32 = kani::any();
        assert!(heartbeat_ok(mb) == (mb == 0));
    }
}

} // verus!

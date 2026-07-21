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
//!   restart budget bounded by **restart-count + cooldown** (spec §4 tier 1): a restart
//!   consumes a credit, faults NEVER replenish credits, and the budget re-arms to
//!   `MAX_RESTARTS` only after `COOLDOWN_QUIET` consecutive all-gates-clear
//!   observations in `Normal` (a proven-quiet interval). A fault arriving in `Normal`
//!   with the budget exhausted latches `PartitionFailsafe` directly. Once
//!   failsafe-latched there is NO software path back — mirroring the wdg-thin
//!   cannot-un-start discipline; recovery is a hardware/external reset, out of scope.
//!
//! Proven (Verus, this file):
//! - **H1 terminating escalation** — `measure` strictly decreases on every fault-driven
//!   transition (`lemma_fault_decreases_measure`), and an always-faulting input reaches
//!   `CrossCoreTrip` in ≤ `MAX_RESTARTS + 2` steps from any well-formed `Normal`
//!   (`lemma_escalation_bound`; the exact step count is `phi`).
//! - **H2 no-silent-clear** — `Degraded → Normal` requires ALL six gates to pass on
//!   the presented restart observation (`all_gates_clear` — strictly stronger than the
//!   triggering gate alone, `lemma_all_clear_implies_cause_gate`); an observation still
//!   violating ANY gate is rejected without mutating (`try_restart` ensures).
//! - **H3 absorbing failsafe** — no fault or restart leaves `PartitionFailsafe` except
//!   the liveness trip to `CrossCoreTrip`; `CrossCoreTrip` is terminal
//!   (`lemma_failsafe_absorbing`, `lemma_trip_absorbing`).
//! - **H4 cause preserved** — the recorded cause is the fault that ended `Normal`
//!   (entering `Degraded`, or `PartitionFailsafe` directly on an exhausted budget),
//!   unchanged until cleared (`lemma_cause_preserved` + `on_fault`/`try_restart` ensures).
//! - **H5 cooldown-bounded restarts (long-run, cross-episode)** — over an ARBITRARY
//!   finite sequence of fault/restart/quiet events: with no all-clear quiet observation,
//!   at most the starting budget (≤ `MAX_RESTARTS`) restarts are EVER accepted — an
//!   intermittent or alternating fault cannot restart indefinitely
//!   (`lemma_chatter_bounded`); the budget increases ONLY as the `COOLDOWN_QUIET`-th
//!   consecutive all-clear observation in `Normal`
//!   (`lemma_replenish_requires_quiet_interval`, `lemma_quiet_streak_mechanics`); and
//!   globally `COOLDOWN_QUIET·restarts ≤ pot₀ + MAX_RESTARTS·quiet_clears`
//!   (`lemma_long_run_restart_bound`) — restarts are rate-limited by proven-quiet time.
//!
//! Kani mirrors H1–H5 over `kani::any()` inputs in `hm_kani` below (run against the
//! stripped plain/ crate): `h1_escalation_terminates`, `h1_trip_bound`,
//! `h2_no_silent_clear`, `h3_absorbing_failsafe`, `h4_cause_preserved`,
//! `h5_gates_characterized` (gate definitions), `h6_long_run_restart_bound`
//! (arbitrary fault/restart/quiet interleavings), `h7_replenish_requires_quiet`.
//!
//! Track scoping (recorded for VER-OS-HM-001): this module is verified on the
//! Verus + Kani tracks (the executor.rs two-tool discipline — Verus proves the
//! spec-level FSM, Kani model-checks the shipped stripped exec code). The Rocq track
//! covers the nine standalone IPC primitives and does NOT include this module.
use vstd::prelude::*;

verus! {

/// Restart budget ceiling. Credits are consumed by faults while `Degraded` (failed
/// recoveries) and by accepted restarts; they are NEVER re-armed by a fault — the
/// budget replenishes to this ceiling only after a proven-quiet cooldown interval
/// (see `COOLDOWN_QUIET`). Must be ≥ 1 (the FSM invariant
/// `Degraded ==> restarts_remaining >= 1` is seeded by this constant;
/// `lemma_escalation_bound`'s `MAX_RESTARTS + 2` count relies on it).
pub const MAX_RESTARTS: u32 = 3;

/// Cooldown length: consecutive all-gates-clear observations required in `Normal`
/// (with no intervening fault or restart) before the restart budget replenishes to
/// `MAX_RESTARTS` — the spec §4 "restart-count + cooldown" bound. A policy parameter
/// like `MAX_RESTARTS`: kept small so the Kani interleaving harness can witness a
/// replenish inside its unwind bound; production tunes it to the observation cadence.
pub const COOLDOWN_QUIET: u32 = 4;

/// Health-monitor supervision state, ordered by severity (see `rank`).
///
/// `PartitionFailsafe` and `CrossCoreTrip` are ABSORBING for software: no fault or
/// restart input ever lowers the rank (H3) — mirroring wdg-thin's cannot-un-start.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HmState {
    /// Healthy: all gates passing. The restart budget replenishes here (and only
    /// here) after `COOLDOWN_QUIET` consecutive all-clear observations (H5).
    Normal,
    /// A fault was observed; bounded restart attempts are permitted while the
    /// budget lasts AND every gate clears on the restart observation (H2).
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
    /// Cross-sensor voting gate failed: the three redundant sensor replicas do
    /// not reach a 2-of-3 majority within tolerance (TMR disagreement). A
    /// value-domain fault — absorbed at `PartitionFailsafe` like the other
    /// value faults; it is not a liveness fault, so it does not cross-trip.
    VoteMismatch,
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
    /// Redundant sensor replica 0 (raw units) — the TMR voting input.
    pub s0: i32,
    /// Redundant sensor replica 1 (raw units) — the TMR voting input.
    pub s1: i32,
    /// Redundant sensor replica 2 (raw units) — the TMR voting input.
    pub s2: i32,
    /// Agreement tolerance for the 2-of-3 vote: two replicas "agree" iff their
    /// absolute difference is within this bound. A negative tolerance makes no
    /// pair agree (|a−b| ≥ 0), so the vote can never clear — total by design.
    pub vote_tol: i32,
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

/// Ghost: do replicas `a` and `b` agree within `tol`? — `|a − b| ≤ tol`, in
/// mathematical integers (overflow-free by construction; the exec `vote_ok`
/// realizes it by widening to i64 so no i32 subtraction/abs can wrap).
pub open spec fn agree(a: i32, b: i32, tol: i32) -> bool {
    (if a >= b { a - b } else { b - a }) <= tol
}

/// Ghost: does the 2-of-3 cross-sensor vote clear? — at least TWO of the three
/// pairwise agreements `(s0,s1)`, `(s0,s2)`, `(s1,s2)` hold within `tol` (the
/// canonical TMR majority gate). The exec twin is `vote_ok`.
pub open spec fn vote_clears(s0: i32, s1: i32, s2: i32, tol: i32) -> bool {
    let a01 = agree(s0, s1, tol);
    let a02 = agree(s0, s2, tol);
    let a12 = agree(s1, s2, tol);
    (a01 && a02) || (a01 && a12) || (a02 && a12)
}

/// Cross-sensor voting gate (TMR 2-of-3): the redundant replicas are consistent
/// iff at least two of the three pairwise absolute differences are within `tol`.
/// Each `|sᵢ − sⱼ|` is computed by widening to i64, so the subtraction and the
/// magnitude are exact for EVERY i32 input (including `i32::MIN`) — total, no
/// overflow UB. Exact integer comparison against `tol` thereafter.
pub fn vote_ok(s0: i32, s1: i32, s2: i32, tol: i32) -> (ok: bool)
    ensures ok == vote_clears(s0, s1, s2, tol),
{
    let t = tol as i64;
    let d01 = s0 as i64 - s1 as i64;
    let d02 = s0 as i64 - s2 as i64;
    let d12 = s1 as i64 - s2 as i64;
    let a01 = (if d01 >= 0 { d01 } else { -d01 }) <= t;
    let a02 = (if d02 >= 0 { d02 } else { -d02 }) <= t;
    let a12 = (if d12 >= 0 { d12 } else { -d12 }) <= t;
    (a01 && a02) || (a01 && a12) || (a02 && a12)
}

/// Ghost: does observation `obs` clear the single gate that corresponds to fault
/// `cause`? Restart acceptance is gated on `all_gates_clear` (strictly stronger —
/// H2); this per-cause form remains the vocabulary for the cause-specific lemmas.
pub open spec fn gate_clears(cause: Fault, obs: Obs) -> bool {
    match cause {
        Fault::Stale => obs.age_ms <= obs.limit_ms,
        Fault::Implausible => obs.lo <= obs.value && obs.value <= obs.hi,
        Fault::Diverged => obs.innov_abs <= obs.k_sigma,
        Fault::BudgetOverrun => obs.used_us <= obs.budget_us,
        Fault::DeadlineMiss => obs.lateness_us == 0,
        Fault::HeartbeatLoss => obs.missed_beats == 0,
        Fault::VoteMismatch => vote_clears(obs.s0, obs.s1, obs.s2, obs.vote_tol),
    }
}

/// Ghost: does observation `obs` clear ALL six gates? THE no-silent-clear pivot
/// (H2) and the proven-quiet criterion of the cooldown (H5): `try_restart` may
/// return to `Normal`, and `on_quiet` may advance the cooldown streak, only when
/// this holds — evidence showing ANY active violation can never clear or cool down.
pub open spec fn all_gates_clear(obs: Obs) -> bool {
    obs.age_ms <= obs.limit_ms
        && obs.lo <= obs.value && obs.value <= obs.hi
        && obs.innov_abs <= obs.k_sigma
        && obs.used_us <= obs.budget_us
        && obs.lateness_us == 0
        && obs.missed_beats == 0
        && vote_clears(obs.s0, obs.s1, obs.s2, obs.vote_tol)
}

/// All-gates-clear implies every per-cause gate clears: acceptance on
/// `all_gates_clear` is strictly stronger than the triggering gate alone (H2).
pub proof fn lemma_all_clear_implies_cause_gate(cause: Fault, obs: Obs)
    ensures all_gates_clear(obs) ==> gate_clears(cause, obs),
{
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

/// Evaluate ALL six gates on `obs` — the exec twin of `all_gates_clear`, built
/// from the verified pure gates above.
pub fn all_clear(obs: Obs) -> (ok: bool)
    ensures ok == all_gates_clear(obs),
{
    fresh(obs.age_ms, obs.limit_ms)
        && plausible(obs.value, obs.lo, obs.hi)
        && innovation_ok(obs.innov_abs, obs.k_sigma)
        && budget_ok(obs.used_us, obs.budget_us)
        && deadline_ok(obs.lateness_us)
        && heartbeat_ok(obs.missed_beats)
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
/// cooldown-replenished restart budget + the proven-quiet cooldown streak.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Hm {
    /// Current supervision state.
    pub state: HmState,
    /// The fault that ended the last `Normal` (H4: first cause wins, preserved
    /// through escalation for the post-mortem record). Meaningful from the first
    /// fault on; `init` seeds it with an arbitrary variant.
    pub cause: Fault,
    /// Restart credits remaining. Consumed by faults while `Degraded` (failed
    /// recoveries) and by accepted restarts; NEVER re-armed by a fault. Replenishes
    /// to `MAX_RESTARTS` only via `COOLDOWN_QUIET` consecutive all-clear
    /// observations in `Normal` (H5 — spec §4 "restart-count + cooldown").
    /// Invariant: ≥ 1 while `Degraded` (the fault that would consume the last
    /// credit escalates instead of idling; a fault in `Normal` with 0 credits
    /// latches `PartitionFailsafe` directly).
    pub restarts_remaining: u32,
    /// Consecutive all-gates-clear observations seen in `Normal` since the last
    /// fault, accepted restart, or replenish. Reaching `COOLDOWN_QUIET` replenishes
    /// the budget and resets to 0. Invariant: < `COOLDOWN_QUIET`, and 0 outside
    /// `Normal` (quiet accrues only while healthy).
    pub quiet_streak: u32,
}

/// Ghost: the FSM transition on a fault event — the single source of truth the
/// H-proofs quantify over; the exec `on_fault` is proven equal to it field-by-field.
///
/// - `Normal --fault-->`: records the cause and zeroes the cooldown streak; enters
///   `Degraded` if a restart credit remains, else latches `PartitionFailsafe`
///   directly (budget exhausted and not yet replenished — spec §4 escalation).
///   The budget itself is NOT re-armed (H5: only the cooldown replenishes it).
/// - `Degraded --fault-->`: burns one restart credit (a fault while degraded is a
///   failed recovery); the fault that consumes the LAST credit escalates to
///   `PartitionFailsafe` (an exhausted partition gets no extra grace fault — this is
///   what makes the H1 bound `MAX_RESTARTS + 2` instead of `+ 3`). Cause unchanged (H4).
/// - `PartitionFailsafe --liveness-fault--> CrossCoreTrip`; value faults are absorbed
///   (already held safe; H3).
/// - `CrossCoreTrip`: absorbed (terminal; H3).
pub open spec fn step_fault(hm: Hm, f: Fault) -> Hm {
    match hm.state {
        HmState::Normal => if hm.restarts_remaining >= 1 {
            Hm {
                state: HmState::Degraded,
                cause: f,
                restarts_remaining: hm.restarts_remaining,
                quiet_streak: 0,
            }
        } else {
            Hm {
                state: HmState::PartitionFailsafe,
                cause: f,
                restarts_remaining: hm.restarts_remaining,
                quiet_streak: 0,
            }
        },
        HmState::Degraded => if hm.restarts_remaining <= 1 {
            Hm {
                state: HmState::PartitionFailsafe,
                cause: hm.cause,
                restarts_remaining: 0,
                quiet_streak: hm.quiet_streak,
            }
        } else {
            Hm {
                state: HmState::Degraded,
                cause: hm.cause,
                restarts_remaining: (hm.restarts_remaining - 1) as u32,
                quiet_streak: hm.quiet_streak,
            }
        },
        HmState::PartitionFailsafe => if trips_cross_core(f) {
            Hm {
                state: HmState::CrossCoreTrip,
                cause: hm.cause,
                restarts_remaining: 0,
                quiet_streak: hm.quiet_streak,
            }
        } else {
            hm
        },
        HmState::CrossCoreTrip => hm,
    }
}

/// Ghost: is a restart accepted? — Degraded, a credit remains, and the presented
/// observation clears ALL six gates (H2).
pub open spec fn restart_accepted(hm: Hm, obs: Obs) -> bool {
    hm.state === HmState::Degraded
        && hm.restarts_remaining >= 1
        && all_gates_clear(obs)
}

/// Ghost: the FSM transition on a restart attempt — accepted restarts return to
/// `Normal` with one credit burned and the cooldown streak zeroed; rejected
/// restarts change NOTHING. Exec twin: `try_restart`.
pub open spec fn step_restart(hm: Hm, obs: Obs) -> Hm {
    if restart_accepted(hm, obs) {
        Hm {
            state: HmState::Normal,
            cause: hm.cause,
            restarts_remaining: (hm.restarts_remaining - 1) as u32,
            quiet_streak: 0,
        }
    } else {
        hm
    }
}

/// Ghost: does this quiet observation replenish the budget? — `Normal`, ALL gates
/// clear, and it is the `COOLDOWN_QUIET`-th consecutive such observation.
pub open spec fn replenishes(hm: Hm, obs: Obs) -> bool {
    hm.state === HmState::Normal
        && all_gates_clear(obs)
        && hm.quiet_streak + 1 >= COOLDOWN_QUIET
}

/// Ghost: the FSM transition on a proven-quiet observation tick — the cooldown of
/// the spec §4 "restart-count + cooldown" bound. In `Normal`: an all-gates-clear
/// observation advances the streak, and the `COOLDOWN_QUIET`-th replenishes the
/// budget to `MAX_RESTARTS` (resetting the streak); an observation violating ANY
/// gate zeroes the streak (no silent cooldown on unhealthy evidence). Outside
/// `Normal`: a no-op — quiet never accrues while degraded or latched. Exec twin:
/// `on_quiet`.
pub open spec fn step_quiet(hm: Hm, obs: Obs) -> Hm {
    if hm.state === HmState::Normal {
        if all_gates_clear(obs) {
            if hm.quiet_streak + 1 >= COOLDOWN_QUIET {
                Hm {
                    state: hm.state,
                    cause: hm.cause,
                    restarts_remaining: MAX_RESTARTS,
                    quiet_streak: 0,
                }
            } else {
                Hm {
                    state: hm.state,
                    cause: hm.cause,
                    restarts_remaining: hm.restarts_remaining,
                    quiet_streak: (hm.quiet_streak + 1) as u32,
                }
            }
        } else {
            Hm {
                state: hm.state,
                cause: hm.cause,
                restarts_remaining: hm.restarts_remaining,
                quiet_streak: 0,
            }
        }
    } else {
        hm
    }
}

/// Ghost: the H1 termination measure over (state rank, restarts_remaining) —
/// lexicographic, flattened to one `nat`. Strictly decreases on every fault-driven
/// transition (`lemma_fault_decreases_measure`); constant on absorbed faults.
pub open spec fn measure(hm: Hm) -> nat {
    match hm.state {
        HmState::Normal => hm.restarts_remaining as nat + 3,
        HmState::Degraded => hm.restarts_remaining as nat + 2,
        HmState::PartitionFailsafe => 1,
        HmState::CrossCoreTrip => 0,
    }
}

/// Ghost: exact number of always-faulting steps from `hm` to `CrossCoreTrip` when
/// every fault is a liveness fault — the potential behind `lemma_escalation_bound`.
/// `phi == 0` iff already tripped. From `Normal` this is `restarts_remaining + 2`
/// (≤ `MAX_RESTARTS + 2`): budget exhausted in `Normal` means 2 steps (straight to
/// `PartitionFailsafe`, then the trip).
pub open spec fn phi(hm: Hm) -> nat {
    match hm.state {
        HmState::Normal => hm.restarts_remaining as nat + 2,
        HmState::Degraded => hm.restarts_remaining as nat + 1,
        HmState::PartitionFailsafe => 1,
        HmState::CrossCoreTrip => 0,
    }
}

impl Hm {
    /// Representation invariant: the restart budget is bounded by `MAX_RESTARTS`;
    /// while Degraded at least one credit remains (the transition that would
    /// consume the last credit escalates instead — see `step_fault`); the cooldown
    /// streak is strictly below `COOLDOWN_QUIET` (reaching it replenishes and
    /// resets) and zero outside `Normal`.
    pub open spec fn inv(&self) -> bool {
        self.restarts_remaining <= MAX_RESTARTS
            && (self.state === HmState::Degraded ==> self.restarts_remaining >= 1)
            && self.quiet_streak < COOLDOWN_QUIET
            && (!(self.state === HmState::Normal) ==> self.quiet_streak == 0)
    }

    /// The healthy monitor. `cause` is seeded arbitrarily (it is meaningful only
    /// once a fault has been recorded — H4 speaks from the first fault onward).
    pub fn init() -> (r: Hm)
        ensures r.inv(), r.state === HmState::Normal,
            r.restarts_remaining == MAX_RESTARTS,
            r.quiet_streak == 0,
    {
        Hm {
            state: HmState::Normal,
            cause: Fault::HeartbeatLoss,
            restarts_remaining: MAX_RESTARTS,
            quiet_streak: 0,
        }
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
            self.quiet_streak == step_fault(*old(self), f).quiet_streak,
    {
        if matches!(self.state, HmState::Normal) {
            self.cause = f;
            self.quiet_streak = 0;
            if self.restarts_remaining >= 1 {
                self.state = HmState::Degraded;
            } else {
                self.state = HmState::PartitionFailsafe;
            }
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
    /// remains AND the observation clears ALL six gates (H2 — cannot clear while
    /// the presented evidence shows ANY active violation). An accepted restart
    /// burns one credit that only the cooldown gives back (H5). A rejected restart
    /// mutates NOTHING. From `PartitionFailsafe`/`CrossCoreTrip` a restart is
    /// always rejected (H3: no software path back; recovery is a hardware/external
    /// reset).
    pub fn try_restart(&mut self, obs: Obs) -> (ok: bool)
        requires old(self).inv(),
        ensures
            self.inv(),
            // exact acceptance characterization — passes IFF Degraded, budgeted,
            // and EVERY gate clears on this observation
            ok == restart_accepted(*old(self), obs),
            // exec twin of the ghost transition, field by field
            self.state === step_restart(*old(self), obs).state,
            self.cause === step_restart(*old(self), obs).cause,
            self.restarts_remaining == step_restart(*old(self), obs).restarts_remaining,
            self.quiet_streak == step_restart(*old(self), obs).quiet_streak,
            // accepted: back to Normal, one credit burned, cause record intact
            ok ==> self.state === HmState::Normal
                && self.cause === old(self).cause
                && self.restarts_remaining + 1 == old(self).restarts_remaining
                && self.quiet_streak == 0,
            // rejected: NO mutation (H2's no-silent-clear + H3's restart half)
            !ok ==> self.state === old(self).state
                && self.cause === old(self).cause
                && self.restarts_remaining == old(self).restarts_remaining
                && self.quiet_streak == old(self).quiet_streak,
    {
        if matches!(self.state, HmState::Degraded) && self.restarts_remaining >= 1 {
            if all_clear(obs) {
                self.state = HmState::Normal;
                self.restarts_remaining = self.restarts_remaining - 1;
                self.quiet_streak = 0;
                return true;
            }
        }
        false
    }

    /// Present one proven-quiet observation tick — the cooldown half of the spec §4
    /// "restart-count + cooldown" bound. Exec twin of `step_quiet`: in `Normal`, an
    /// all-gates-clear observation advances the cooldown streak and the
    /// `COOLDOWN_QUIET`-th consecutive one replenishes the restart budget to
    /// `MAX_RESTARTS`; an observation violating ANY gate zeroes the streak. Outside
    /// `Normal` this is a no-op (quiet never accrues while degraded or latched).
    pub fn on_quiet(&mut self, obs: Obs)
        requires old(self).inv(),
        ensures
            self.inv(),
            self.state === step_quiet(*old(self), obs).state,
            self.cause === step_quiet(*old(self), obs).cause,
            self.restarts_remaining == step_quiet(*old(self), obs).restarts_remaining,
            self.quiet_streak == step_quiet(*old(self), obs).quiet_streak,
    {
        if matches!(self.state, HmState::Normal) {
            if all_clear(obs) {
                if self.quiet_streak + 1 >= COOLDOWN_QUIET {
                    self.restarts_remaining = MAX_RESTARTS;
                    self.quiet_streak = 0;
                } else {
                    self.quiet_streak = self.quiet_streak + 1;
                }
            } else {
                self.quiet_streak = 0;
            }
        }
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
        // absorbed faults are pure no-ops (state, cause, budget, streak untouched)
        (hm.state === HmState::CrossCoreTrip
            || (hm.state === HmState::PartitionFailsafe && !trips_cross_core(f)))
            ==> step_fault(hm, f).state === hm.state
                && step_fault(hm, f).cause === hm.cause
                && step_fault(hm, f).restarts_remaining == hm.restarts_remaining
                && step_fault(hm, f).quiet_streak == hm.quiet_streak,
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

/// H1 (THE bound): an always-faulting input starting from any well-formed Normal
/// reaches `CrossCoreTrip` in ≤ `MAX_RESTARTS + 2` steps — with a full budget that
/// is exactly 1 (Normal → Degraded) + (MAX − 1) credit burns + 1 (last credit →
/// PartitionFailsafe) + 1 (liveness trip); with a partially-spent budget it is
/// faster (`phi` = budget + 2).
pub proof fn lemma_escalation_bound(hm: Hm)
    requires hm.inv(), hm.state === HmState::Normal,
    ensures
        apply_liveness_faults(hm, (MAX_RESTARTS + 2) as nat).state === HmState::CrossCoreTrip,
{
    assert(phi(hm) <= MAX_RESTARTS as nat + 2);
    lemma_liveness_faults_trip(hm, (MAX_RESTARTS + 2) as nat);
}

// ===========================================================================
// H3 — absorbing failsafe
// ===========================================================================

/// H3 (fault half, PartitionFailsafe): no fault leaves `PartitionFailsafe` except the
/// liveness trip UP to `CrossCoreTrip`; a value fault changes nothing. Combined with
/// `try_restart`'s rejected-means-untouched ensures (its guard requires Degraded)
/// and `on_quiet`'s outside-Normal no-op, no input sequence ever lowers the rank
/// below 2 again.
pub proof fn lemma_failsafe_absorbing(hm: Hm, f: Fault)
    requires hm.state === HmState::PartitionFailsafe,
    ensures
        rank(step_fault(hm, f).state) >= 2,
        trips_cross_core(f) ==> step_fault(hm, f).state === HmState::CrossCoreTrip,
        !trips_cross_core(f) ==> step_fault(hm, f).state === hm.state
            && step_fault(hm, f).cause === hm.cause
            && step_fault(hm, f).restarts_remaining == hm.restarts_remaining
            && step_fault(hm, f).quiet_streak == hm.quiet_streak,
{
}

/// H3 (fault half, CrossCoreTrip): terminal — every fault is absorbed unchanged.
pub proof fn lemma_trip_absorbing(hm: Hm, f: Fault)
    requires hm.state === HmState::CrossCoreTrip,
    ensures
        step_fault(hm, f).state === hm.state,
        step_fault(hm, f).cause === hm.cause,
        step_fault(hm, f).restarts_remaining == hm.restarts_remaining,
        step_fault(hm, f).quiet_streak == hm.quiet_streak,
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

/// H4: once out of Normal, the recorded cause survives every further fault unchanged
/// (first cause wins — through credit burns AND through the escalation into
/// `PartitionFailsafe`/`CrossCoreTrip`, so the post-mortem record is the fault that
/// ended Normal). A fault in Normal records exactly the entering fault, whether it
/// enters Degraded or latches PartitionFailsafe directly on an exhausted budget.
pub proof fn lemma_cause_preserved(hm: Hm, f: Fault)
    requires hm.inv(),
    ensures
        !(hm.state === HmState::Normal) ==> step_fault(hm, f).cause === hm.cause,
        hm.state === HmState::Normal ==> step_fault(hm, f).cause === f,
{
}

// ===========================================================================
// H5 — cooldown-bounded restarts (long-run, cross-episode)
// ===========================================================================

/// One supervision input event — the alphabet the long-run (cross-episode) H5
/// proofs quantify over: everything a caller can ever do to the monitor. Spec-level
/// vocabulary only: no exec path constructs it (rustc drops it from the shipped
/// object as dead code).
pub enum Event {
    /// A fault report (`on_fault`).
    Fault(Fault),
    /// A restart attempt with its observation (`try_restart`).
    Restart(Obs),
    /// A proven-quiet observation tick (`on_quiet`).
    Quiet(Obs),
}

/// Ghost: the FSM transition on any single input event — total over the full
/// caller-visible input alphabet.
pub open spec fn step_event(hm: Hm, e: Event) -> Hm {
    match e {
        Event::Fault(f) => step_fault(hm, f),
        Event::Restart(obs) => step_restart(hm, obs),
        Event::Quiet(obs) => step_quiet(hm, obs),
    }
}

/// Ghost: 1 iff `e` is a restart attempt that `hm` accepts, else 0.
pub open spec fn event_restart_accepted(hm: Hm, e: Event) -> nat {
    match e {
        Event::Restart(obs) => if restart_accepted(hm, obs) { 1 } else { 0 },
        _ => 0,
    }
}

/// Ghost: 1 iff `e` is a quiet tick whose observation clears ALL gates, else 0
/// (state-independent — an upper-bound currency for the rate theorem).
pub open spec fn event_quiet_clear(e: Event) -> nat {
    match e {
        Event::Quiet(obs) => if all_gates_clear(obs) { 1 } else { 0 },
        _ => 0,
    }
}

/// Ghost: is `e` a quiet tick that replenishes the budget from `hm`?
pub open spec fn is_replenish_event(hm: Hm, e: Event) -> bool {
    match e {
        Event::Quiet(obs) => replenishes(hm, obs),
        _ => false,
    }
}

/// Ghost: run an arbitrary finite event sequence.
pub open spec fn run(hm: Hm, es: Seq<Event>) -> Hm
    decreases es.len(),
{
    if es.len() == 0 {
        hm
    } else {
        run(step_event(hm, es[0]), es.subrange(1, es.len() as int))
    }
}

/// Ghost: number of ACCEPTED restarts along the run of `es` from `hm`.
pub open spec fn accepted_restarts(hm: Hm, es: Seq<Event>) -> nat
    decreases es.len(),
{
    if es.len() == 0 {
        0
    } else {
        event_restart_accepted(hm, es[0])
            + accepted_restarts(step_event(hm, es[0]), es.subrange(1, es.len() as int))
    }
}

/// Ghost: number of all-gates-clear quiet ticks in `es` (state-independent count).
pub open spec fn quiet_clears(es: Seq<Event>) -> nat
    decreases es.len(),
{
    if es.len() == 0 {
        0
    } else {
        event_quiet_clear(es[0]) + quiet_clears(es.subrange(1, es.len() as int))
    }
}

/// Ghost: the H5 rate potential — restart capacity measured in cooldown units.
/// Each all-clear quiet tick can add at most `MAX_RESTARTS` to it; every accepted
/// restart costs `COOLDOWN_QUIET` of it; faults never increase it.
pub open spec fn pot(hm: Hm) -> nat {
    (COOLDOWN_QUIET * hm.restarts_remaining + MAX_RESTARTS * hm.quiet_streak) as nat
}

/// Every event preserves the invariant (restart/quiet halves of
/// `lemma_step_preserves_inv`).
pub proof fn lemma_event_preserves_inv(hm: Hm, e: Event)
    requires hm.inv(),
    ensures step_event(hm, e).inv(),
{
    match e {
        Event::Fault(f) => lemma_step_preserves_inv(hm, f),
        Event::Restart(_) => {},
        Event::Quiet(_) => {},
    }
}

/// H5 (per-event potential): a single event pays `COOLDOWN_QUIET` per accepted
/// restart out of `pot`, and only an all-clear quiet tick can add to it — by at
/// most `MAX_RESTARTS`. Faults strictly cannot fund restarts.
pub proof fn lemma_event_potential(hm: Hm, e: Event)
    requires hm.inv(),
    ensures
        step_event(hm, e).inv(),
        COOLDOWN_QUIET * event_restart_accepted(hm, e) + pot(step_event(hm, e))
            <= pot(hm) + MAX_RESTARTS * event_quiet_clear(e),
{
    lemma_event_preserves_inv(hm, e);
}

/// H5 (no-cooldown step): on any event that is NOT an all-clear quiet tick, the
/// budget is monotone non-increasing, and an accepted restart strictly consumes it.
pub proof fn lemma_no_quiet_step(hm: Hm, e: Event)
    requires hm.inv(), event_quiet_clear(e) == 0,
    ensures
        step_event(hm, e).inv(),
        step_event(hm, e).restarts_remaining + event_restart_accepted(hm, e)
            <= hm.restarts_remaining,
{
    lemma_event_preserves_inv(hm, e);
}

/// H5 (replenish characterization): a single event INCREASES the restart budget
/// only if it is an all-clear quiet tick in `Normal` arriving as the
/// `COOLDOWN_QUIET`-th consecutive one (`quiet_streak == COOLDOWN_QUIET - 1`), and
/// then the budget becomes exactly `MAX_RESTARTS` with the streak reset. No fault
/// and no restart ever raises the budget.
pub proof fn lemma_replenish_requires_quiet_interval(hm: Hm, e: Event)
    requires hm.inv(),
    ensures
        step_event(hm, e).restarts_remaining > hm.restarts_remaining
            ==> is_replenish_event(hm, e)
                && hm.state === HmState::Normal
                && hm.quiet_streak == COOLDOWN_QUIET - 1
                && step_event(hm, e).restarts_remaining == MAX_RESTARTS
                && step_event(hm, e).quiet_streak == 0,
{
}

/// H5 (streak mechanics): the cooldown streak grows by AT MOST one per event, grows
/// only on an all-clear quiet tick in `Normal`, and is zeroed by every fault and
/// every accepted restart — so `COOLDOWN_QUIET` consecutive all-clear quiet ticks
/// with no intervening fault/accepted-restart are the ONLY road to a replenish
/// (with `lemma_replenish_requires_quiet_interval`).
pub proof fn lemma_quiet_streak_mechanics(hm: Hm, e: Event)
    requires hm.inv(),
    ensures
        step_event(hm, e).quiet_streak <= hm.quiet_streak + 1,
        step_event(hm, e).quiet_streak == hm.quiet_streak + 1
            ==> event_quiet_clear(e) == 1 && hm.state === HmState::Normal,
        (match e {
            Event::Fault(_) => true,
            Event::Restart(obs) => restart_accepted(hm, obs),
            Event::Quiet(_) => false,
        }) ==> step_event(hm, e).quiet_streak == 0,
{
}

/// H5 (THE long-run rate bound): over an ARBITRARY finite event sequence,
/// `COOLDOWN_QUIET · accepted_restarts ≤ pot(start) + MAX_RESTARTS · quiet_clears`.
/// Restarts are rate-limited by proven-quiet time: each block of `MAX_RESTARTS`
/// further restarts costs `COOLDOWN_QUIET` all-clear quiet ticks, however faults,
/// rejected restarts, and unhealthy observations interleave.
pub proof fn lemma_long_run_restart_bound(hm: Hm, es: Seq<Event>)
    requires hm.inv(),
    ensures
        run(hm, es).inv(),
        COOLDOWN_QUIET * accepted_restarts(hm, es) + pot(run(hm, es))
            <= pot(hm) + MAX_RESTARTS * quiet_clears(es),
    decreases es.len(),
{
    if es.len() == 0 {
    } else {
        lemma_event_potential(hm, es[0]);
        lemma_long_run_restart_bound(step_event(hm, es[0]), es.subrange(1, es.len() as int));
    }
}

/// H5 (chattering-fault corollary): across ANY event sequence containing NO
/// all-clear quiet observation — however intermittent or alternating faults and
/// restart attempts chatter — the number of accepted restarts never exceeds the
/// STARTING budget (≤ `MAX_RESTARTS`). An intermittent fault therefore cannot
/// restart indefinitely: further restarts require proven-quiet cooldown intervals
/// (`lemma_replenish_requires_quiet_interval`). This closes the cross-episode gap:
/// the budget is monotone between cooldowns, never re-armed by fault entry.
pub proof fn lemma_chatter_bounded(hm: Hm, es: Seq<Event>)
    requires hm.inv(), quiet_clears(es) == 0,
    ensures
        run(hm, es).inv(),
        accepted_restarts(hm, es) + run(hm, es).restarts_remaining
            <= hm.restarts_remaining,
    decreases es.len(),
{
    if es.len() == 0 {
    } else {
        assert(event_quiet_clear(es[0]) == 0);
        assert(quiet_clears(es.subrange(1, es.len() as int)) == 0);
        lemma_no_quiet_step(hm, es[0]);
        lemma_chatter_bounded(step_event(hm, es[0]), es.subrange(1, es.len() as int));
    }
}

// ===========================================================================
// Kani harnesses — H1–H5 over kani::any() inputs, against the stripped
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

    /// An arbitrary well-formed Hm: budget bounded, ≥ 1 while Degraded, cooldown
    /// streak < COOLDOWN_QUIET and zero outside Normal — the exec mirror of
    /// `Hm::inv()` (spec fns are stripped from plain/).
    fn any_hm() -> Hm {
        let h = Hm {
            state: any_state(),
            cause: any_fault(),
            restarts_remaining: kani::any(),
            quiet_streak: kani::any(),
        };
        kani::assume(h.restarts_remaining <= MAX_RESTARTS);
        if matches!(h.state, HmState::Degraded) {
            kani::assume(h.restarts_remaining >= 1);
        }
        kani::assume(h.quiet_streak < COOLDOWN_QUIET);
        if !matches!(h.state, HmState::Normal) {
            kani::assume(h.quiet_streak == 0);
        }
        h
    }

    /// Exec mirror of the ghost `measure` (harness-only helper).
    fn measure_exec(h: &Hm) -> u32 {
        match h.state {
            HmState::Normal => h.restarts_remaining + 3,
            HmState::Degraded => h.restarts_remaining + 2,
            HmState::PartitionFailsafe => 1,
            HmState::CrossCoreTrip => 0,
        }
    }

    /// Exec mirror of the ghost `pot` (harness-only helper).
    fn pot_exec(h: &Hm) -> u32 {
        COOLDOWN_QUIET * h.restarts_remaining + MAX_RESTARTS * h.quiet_streak
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
    /// AND EVERY gate passing on the observation (in particular the triggering
    /// gate); an observation still violating ANY gate is rejected WITHOUT mutating.
    #[kani::proof]
    fn h2_no_silent_clear() {
        let mut h = any_hm();
        let obs = any_obs();
        let pre = h;
        let ok = h.try_restart(obs);
        if ok {
            assert!(matches!(pre.state, HmState::Degraded));
            assert!(pre.restarts_remaining >= 1);
            assert!(all_clear(obs)); // ALL gates genuinely pass...
            assert!(gate_eval(pre.cause, obs)); // ...including the triggering one
            assert!(matches!(h.state, HmState::Normal));
            assert!(h.restarts_remaining == pre.restarts_remaining - 1);
            assert!(h.cause == pre.cause);
            assert!(h.quiet_streak == 0);
        } else {
            assert!(h == pre); // rejected restarts never mutate
        }
        // an observation showing ANY active violation can NEVER clear
        if matches!(pre.state, HmState::Degraded) && !all_clear(obs) {
            assert!(!ok);
        }
        // in particular one still violating the triggering gate
        if matches!(pre.state, HmState::Degraded) && !gate_eval(pre.cause, obs) {
            assert!(!ok);
        }
    }

    /// H3 — absorbing failsafe: no fault, restart, or quiet tick leaves
    /// PartitionFailsafe except the liveness trip to CrossCoreTrip; CrossCoreTrip
    /// is terminal.
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
        // PartitionFailsafe under a restart attempt or quiet tick: untouched
        let mut pf2 = any_hm();
        kani::assume(matches!(pf2.state, HmState::PartitionFailsafe));
        let pf2_pre = pf2;
        assert!(!pf2.try_restart(any_obs()));
        assert!(pf2 == pf2_pre);
        pf2.on_quiet(any_obs());
        assert!(pf2 == pf2_pre);
        // CrossCoreTrip: terminal under all inputs
        let mut ct = any_hm();
        kani::assume(matches!(ct.state, HmState::CrossCoreTrip));
        let ct_pre = ct;
        ct.on_fault(any_fault());
        assert!(ct == ct_pre);
        assert!(!ct.try_restart(any_obs()));
        assert!(ct == ct_pre);
        ct.on_quiet(any_obs());
        assert!(ct == ct_pre);
    }

    /// H4 — cause preserved: the fault that ends Normal is recorded and survives
    /// every further fault (credit burns AND the escalation into the failsafe
    /// states) unchanged, until a successful restart clears the episode.
    #[kani::proof]
    #[kani::unwind(8)]
    fn h4_cause_preserved() {
        let mut h = Hm::init();
        let entering = any_fault();
        h.on_fault(entering); // Normal → Degraded records the cause (init budget ≥ 1)
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

    /// Gates — definitional characterization of every pure gate and their
    /// conjunction (the Verus ensures, re-checked by CBMC on the shipped exec code).
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
        let obs = any_obs();
        assert!(all_clear(obs)
            == (fresh(obs.age_ms, obs.limit_ms)
                && plausible(obs.value, obs.lo, obs.hi)
                && innovation_ok(obs.innov_abs, obs.k_sigma)
                && budget_ok(obs.used_us, obs.budget_us)
                && deadline_ok(obs.lateness_us)
                && heartbeat_ok(obs.missed_beats)));
    }

    /// H5a — long-run cooldown bound over ARBITRARY interleavings of fault, restart,
    /// and quiet events from an arbitrary well-formed state (also the combined-input
    /// sequence harness): at every prefix, COOLDOWN_QUIET·accepted + pot ≤
    /// pot(start) + MAX_RESTARTS·quiet_clears; and with NO all-clear quiet
    /// observation, accepted restarts never exceed the STARTING budget — an
    /// intermittent/alternating fault cannot restart indefinitely.
    #[kani::proof]
    #[kani::unwind(7)]
    fn h6_long_run_restart_bound() {
        let mut h = any_hm();
        let start_credits = h.restarts_remaining;
        let pot0 = pot_exec(&h);
        let mut accepted: u32 = 0;
        let mut clears: u32 = 0;
        let mut i: u32 = 0;
        while i < 6 {
            match kani::any::<u8>() % 3 {
                0 => h.on_fault(any_fault()),
                1 => {
                    if h.try_restart(any_obs()) {
                        accepted += 1;
                    }
                }
                _ => {
                    let obs = any_obs();
                    if all_clear(obs) {
                        clears += 1;
                    }
                    h.on_quiet(obs);
                }
            }
            // the rate bound holds at every prefix of the run
            assert!(COOLDOWN_QUIET * accepted + pot_exec(&h) <= pot0 + MAX_RESTARTS * clears);
            // no proven-quiet interval ⇒ the starting budget is ALL you ever get
            if clears == 0 {
                assert!(accepted <= start_credits);
                assert!(accepted <= MAX_RESTARTS);
            }
            i += 1;
        }
    }

    /// H5b — replenish characterization: a single arbitrary event increases the
    /// restart budget ONLY as the COOLDOWN_QUIET-th consecutive all-clear
    /// observation in Normal (streak == COOLDOWN_QUIET − 1), setting it to exactly
    /// MAX_RESTARTS; the streak grows by at most one per event, grows only on an
    /// all-clear quiet tick in Normal, and every fault or accepted restart zeroes it.
    #[kani::proof]
    fn h7_replenish_requires_quiet() {
        let mut h = any_hm();
        let pre = h;
        let kind: u8 = kani::any::<u8>() % 3;
        let obs = any_obs();
        let mut accepted = false;
        match kind {
            0 => h.on_fault(any_fault()),
            1 => accepted = h.try_restart(obs),
            _ => h.on_quiet(obs),
        }
        // budget increase ⇒ the cooldown fired, nothing else can raise it
        if h.restarts_remaining > pre.restarts_remaining {
            assert!(kind == 2);
            assert!(all_clear(obs));
            assert!(matches!(pre.state, HmState::Normal));
            assert!(pre.quiet_streak == COOLDOWN_QUIET - 1);
            assert!(h.restarts_remaining == MAX_RESTARTS);
            assert!(h.quiet_streak == 0);
        }
        // streak mechanics: +1 at most, and only on an all-clear quiet tick in Normal
        assert!(h.quiet_streak <= pre.quiet_streak + 1);
        if h.quiet_streak == pre.quiet_streak + 1 {
            assert!(kind == 2);
            assert!(all_clear(obs));
            assert!(matches!(pre.state, HmState::Normal));
        }
        // every fault and every accepted restart zeroes the streak
        if kind == 0 || accepted {
            assert!(h.quiet_streak == 0);
        }
    }
}

} // verus!

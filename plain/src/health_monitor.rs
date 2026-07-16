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
/// Staleness gate: the observation is fresh iff its age is within the limit.
pub fn fresh(age_ms: u32, limit_ms: u32) -> bool {
    age_ms <= limit_ms
}
/// Range/plausibility gate: the value is plausible iff it lies in [lo, hi].
pub fn plausible(value: i32, lo: i32, hi: i32) -> bool {
    lo <= value && value <= hi
}
/// Divergence gate (scalar-int form): the estimator innovation is acceptable iff
/// |innovation| is within the k·sigma threshold. Caller pre-scales both to the same
/// integer unit, so the gate itself is exact integer comparison — total, no overflow.
pub fn innovation_ok(innov_abs: u32, k_sigma: u32) -> bool {
    innov_abs <= k_sigma
}
/// Budget gate: within budget iff consumed time does not exceed the window budget.
pub fn budget_ok(used_us: u32, budget_us: u32) -> bool {
    used_us <= budget_us
}
/// Deadline gate: on time iff the last output had zero lateness.
pub fn deadline_ok(lateness_us: u32) -> bool {
    lateness_us == 0
}
/// Heartbeat gate: alive iff no consecutive heartbeat has been missed.
pub fn heartbeat_ok(missed_beats: u32) -> bool {
    missed_beats == 0
}
/// Evaluate the gate corresponding to `cause` on `obs` — the exec twin of
/// `gate_clears`, built from the verified pure gates above.
pub fn gate_eval(cause: Fault, obs: Obs) -> bool {
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
pub fn all_clear(obs: Obs) -> bool {
    fresh(obs.age_ms, obs.limit_ms) && plausible(obs.value, obs.lo, obs.hi)
        && innovation_ok(obs.innov_abs, obs.k_sigma)
        && budget_ok(obs.used_us, obs.budget_us) && deadline_ok(obs.lateness_us)
        && heartbeat_ok(obs.missed_beats)
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
impl Hm {
    /// The healthy monitor. `cause` is seeded arbitrarily (it is meaningful only
    /// once a fault has been recorded — H4 speaks from the first fault onward).
    pub fn init() -> Hm {
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
    pub fn on_fault(&mut self, f: Fault) {
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
    }
    /// Attempt a restart: `Degraded → Normal`, permitted ONLY IF a restart credit
    /// remains AND the observation clears ALL six gates (H2 — cannot clear while
    /// the presented evidence shows ANY active violation). An accepted restart
    /// burns one credit that only the cooldown gives back (H5). A rejected restart
    /// mutates NOTHING. From `PartitionFailsafe`/`CrossCoreTrip` a restart is
    /// always rejected (H3: no software path back; recovery is a hardware/external
    /// reset).
    pub fn try_restart(&mut self, obs: Obs) -> bool {
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
    pub fn on_quiet(&mut self, obs: Obs) {
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
    pub fn is_failsafe_latched(&self) -> bool {
        matches!(self.state, HmState::PartitionFailsafe | HmState::CrossCoreTrip)
    }
}
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
                assert!(measure_exec(& h) < before);
            }
            i += 1;
            if i >= MAX_RESTARTS + 1 {
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
            let f = if kani::any() {
                Fault::HeartbeatLoss
            } else {
                Fault::BudgetOverrun
            };
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
            assert!(all_clear(obs));
            assert!(gate_eval(pre.cause, obs));
            assert!(matches!(h.state, HmState::Normal));
            assert!(h.restarts_remaining == pre.restarts_remaining - 1);
            assert!(h.cause == pre.cause);
            assert!(h.quiet_streak == 0);
        } else {
            assert!(h == pre);
        }
        if matches!(pre.state, HmState::Degraded) && !all_clear(obs) {
            assert!(! ok);
        }
        if matches!(pre.state, HmState::Degraded) && !gate_eval(pre.cause, obs) {
            assert!(! ok);
        }
    }
    /// H3 — absorbing failsafe: no fault, restart, or quiet tick leaves
    /// PartitionFailsafe except the liveness trip to CrossCoreTrip; CrossCoreTrip
    /// is terminal.
    #[kani::proof]
    fn h3_absorbing_failsafe() {
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
        let mut pf2 = any_hm();
        kani::assume(matches!(pf2.state, HmState::PartitionFailsafe));
        let pf2_pre = pf2;
        assert!(! pf2.try_restart(any_obs()));
        assert!(pf2 == pf2_pre);
        pf2.on_quiet(any_obs());
        assert!(pf2 == pf2_pre);
        let mut ct = any_hm();
        kani::assume(matches!(ct.state, HmState::CrossCoreTrip));
        let ct_pre = ct;
        ct.on_fault(any_fault());
        assert!(ct == ct_pre);
        assert!(! ct.try_restart(any_obs()));
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
        h.on_fault(entering);
        assert!(matches!(h.state, HmState::Degraded));
        assert!(h.cause == entering);
        let mut i: u32 = 0;
        while i < MAX_RESTARTS + 2 {
            h.on_fault(any_fault());
            assert!(h.cause == entering);
            i += 1;
        }
        let mut d = any_hm();
        kani::assume(matches!(d.state, HmState::Degraded));
        let d_cause = d.cause;
        let obs = any_obs();
        kani::assume(!gate_eval(d_cause, obs));
        assert!(! d.try_restart(obs));
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
        assert!(
            all_clear(obs) == (fresh(obs.age_ms, obs.limit_ms) && plausible(obs.value,
            obs.lo, obs.hi) && innovation_ok(obs.innov_abs, obs.k_sigma) && budget_ok(obs
            .used_us, obs.budget_us) && deadline_ok(obs.lateness_us) && heartbeat_ok(obs
            .missed_beats))
        );
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
            assert!(
                COOLDOWN_QUIET * accepted + pot_exec(& h) <= pot0 + MAX_RESTARTS * clears
            );
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
        if h.restarts_remaining > pre.restarts_remaining {
            assert!(kind == 2);
            assert!(all_clear(obs));
            assert!(matches!(pre.state, HmState::Normal));
            assert!(pre.quiet_streak == COOLDOWN_QUIET - 1);
            assert!(h.restarts_remaining == MAX_RESTARTS);
            assert!(h.quiet_streak == 0);
        }
        assert!(h.quiet_streak <= pre.quiet_streak + 1);
        if h.quiet_streak == pre.quiet_streak + 1 {
            assert!(kind == 2);
            assert!(all_clear(obs));
            assert!(matches!(pre.state, HmState::Normal));
        }
        if kind == 0 || accepted {
            assert!(h.quiet_streak == 0);
        }
    }
}

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
impl Hm {
    /// The healthy monitor. `cause` is seeded arbitrarily (it is meaningful only
    /// once a fault has been recorded — H4 speaks from Degraded entry onward).
    pub fn init() -> Hm {
        Hm {
            state: HmState::Normal,
            cause: Fault::HeartbeatLoss,
            restarts_remaining: MAX_RESTARTS,
        }
    }
    /// Apply one fault event. Exec twin of `step_fault` — the ensures tie every
    /// field to the ghost transition, so all H-lemmas about `step_fault` transfer
    /// verbatim to the running code.
    pub fn on_fault(&mut self, f: Fault) {
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
    }
    /// Attempt a restart: `Degraded → Normal`, permitted ONLY IF a restart credit
    /// remains AND the observation clears the gate of the recorded cause (H2 —
    /// cannot clear while the fault persists). A rejected restart mutates NOTHING.
    /// From `PartitionFailsafe`/`CrossCoreTrip` a restart is always rejected (H3:
    /// no software path back; recovery is a hardware/external reset).
    pub fn try_restart(&mut self, obs: Obs) -> bool {
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
    pub fn is_failsafe_latched(&self) -> bool {
        matches!(self.state, HmState::PartitionFailsafe | HmState::CrossCoreTrip)
    }
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
    /// An arbitrary well-formed Hm: budget bounded, and ≥ 1 while Degraded —
    /// the exec mirror of `Hm::inv()` (spec fns are stripped from plain/).
    fn any_hm() -> Hm {
        let h = Hm {
            state: any_state(),
            cause: any_fault(),
            restarts_remaining: kani::any(),
        };
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
            assert!(gate_eval(pre.cause, obs));
            assert!(matches!(h.state, HmState::Normal));
            assert!(h.restarts_remaining == pre.restarts_remaining - 1);
            assert!(h.cause == pre.cause);
        } else {
            assert!(h == pre);
        }
        if matches!(pre.state, HmState::Degraded) && !gate_eval(pre.cause, obs) {
            assert!(! ok);
        }
    }
    /// H3 — absorbing failsafe: no fault and no restart leaves PartitionFailsafe
    /// except the liveness trip to CrossCoreTrip; CrossCoreTrip is terminal.
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
        let mut ct = any_hm();
        kani::assume(matches!(ct.state, HmState::CrossCoreTrip));
        let ct_pre = ct;
        ct.on_fault(any_fault());
        assert!(ct == ct_pre);
        assert!(! ct.try_restart(any_obs()));
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

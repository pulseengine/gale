//! gust-hm-probe — per-cause fault-injection demonstrator for the value-domain
//! Health Monitor (VER-OS-HM-001, part 2). The FSM core (gale::health_monitor,
//! src/health_monitor.rs) is ALREADY Verus+Kani proven total + terminating
//! (H1-H5, PR #189) — this probe does NOT re-prove the FSM. It exercises the
//! VERIFIED gates + Hm FSM end to end, per NAMED mission-loss cause, on qemu
//! (lm3s6965evb, cortex-m3), and reads back the REAL `Hm::state` after each
//! step — no shadow/parallel bookkeeping.
//!
//! CAUSE -> DETECTOR MAPPING (a demonstrator concern, kept here — NOT added to
//! the verified src, which stays cause-agnostic over `Fault`):
//!   RC-loss              -> HeartbeatLoss   (liveness)  -> terminal CrossCoreTrip
//!   datalink-loss         -> Stale           (value)     -> terminal PartitionFailsafe
//!   GPS/estimator-loss    -> Diverged        (value)     -> terminal PartitionFailsafe
//!   geofence breach       -> Implausible     (value)     -> terminal PartitionFailsafe
//!   low-battery           -> BudgetOverrun   (liveness)  -> terminal CrossCoreTrip
//!     (REQ-OS-HM-001 names this cause "low-battery/power-budget exhaustion" —
//!     `used_us`/`budget_us` is a generic consumed-vs-allotted resource gate;
//!     here it stands for the power budget of the window, not execution time.)
//!   sensor-disagreement   -> VoteMismatch    (value)     -> terminal PartitionFailsafe
//!     (three redundant replicas `s0`/`s1`/`s2` fail the TMR 2-of-3 majority
//!     within `vote_tol` — the cross-sensor voting detector, the last DC gap.)
//!
//! WHY TWO DIFFERENT TERMINAL STATES (this is the core's PROVEN, INTENDED
//! behaviour, not a probe shortcoming — see src/health_monitor.rs `trips_cross_core`
//! and lemma_failsafe_absorbing/H3): only the LIVENESS faults (HeartbeatLoss,
//! BudgetOverrun) escalate `PartitionFailsafe -> CrossCoreTrip`. A VALUE fault
//! (Stale/Implausible/Diverged) is absorbed at `PartitionFailsafe` forever under
//! repetition of the SAME fault kind — the partition's outputs are already held
//! safe, so a further value-domain trip is a no-op; only a liveness fault (the
//! partition itself going unresponsive or overrunning its window) escalates to
//! the physically independent cross-core trip. Each of the 6 causes below
//! reaches exactly the terminal state its OWN detector class implies.
//!
//! LATENCY BOUND (derived from the core's own proven constants/lemmas, not
//! re-derived here): from `Hm::init()` (Normal, full `MAX_RESTARTS` budget),
//! `Normal`/`Degraded` transitions are fault-KIND-agnostic (only the
//! `PartitionFailsafe` transition inspects `trips_cross_core`), so ANY fault
//! kind reaches `PartitionFailsafe` in EXACTLY `MAX_RESTARTS + 1` on_fault
//! calls (1 to enter Degraded + (MAX_RESTARTS - 1) credit burns + 1 exhausting
//! call). A liveness fault reaches `CrossCoreTrip` in EXACTLY one more call:
//! `MAX_RESTARTS + 2` total — this is `lemma_escalation_bound`'s bound,
//! confirmed here by direct on-chip evaluation of the real `Hm::on_fault`.
//!
//! Each cause block: (a) builds the tripping `Obs`; (b) confirms
//! `gate_eval(cause, obs) == false` and `all_clear(obs) == false`; (c) drives
//! the REAL `Hm` via `on_fault`, checking a no-silent-clear `try_restart` with
//! the SAME faulty obs is rejected without mutation, then reads the terminal
//! state at the proven step bound and confirms it matches the mapped terminal
//! state; (e) for the two liveness causes, confirms `CrossCoreTrip` absorbs a
//! further fault, a restart attempt, AND a healthy quiet tick (no software path
//! out); for the three value causes, confirms `PartitionFailsafe` absorbs a
//! further same-kind fault and a restart attempt with a HEALTHY obs (H3: no
//! software path back from PartitionFailsafe either). (d) NON-VACUITY, run
//! once globally: a healthy `Obs` clears every gate (including every mapped
//! cause's own gate), a `Normal` `Hm` stays `Normal` under a healthy quiet
//! tick, and a `Degraded` `Hm` genuinely restarts to `Normal` on a healthy
//! obs — a good frame never trips failsafe. A final standalone check re-drives
//! `CrossCoreTrip` and applies EVERY `Fault` variant plus a healthy restart/quiet
//! attempt, confirming the state and full `Hm` record are untouched by all of
//! them (H3, exhaustively over the fault alphabet).
//!
//! No fake passes: every assertion reads `hm.state`/`ok` from the REAL
//! `gale::health_monitor::Hm` (the `plain/` stripped exec twin the Kani
//! harnesses model-check) — no parallel/shadow FSM in this probe. Exit codes:
//! semihosting EXIT_SUCCESS / EXIT_FAILURE, no silent fall-through OK path.
#![no_std]
#![no_main]

use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use gale::health_monitor::{all_clear, gate_eval, Fault, Hm, HmState, Obs, MAX_RESTARTS};
use panic_halt as _;

macro_rules! fail {
    ($($t:tt)*) => {{
        hprintln!($($t)*);
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }};
}

/// The proven step bound to `PartitionFailsafe` from `Hm::init()`, any fault kind.
const STEPS_TO_FAILSAFE: u32 = MAX_RESTARTS + 1;
/// The proven step bound to `CrossCoreTrip` from `Hm::init()`, liveness faults only.
const STEPS_TO_TRIP: u32 = MAX_RESTARTS + 2;

/// A fully healthy observation: clears every one of the seven gates — including
/// the TMR vote, whose three redundant replicas (`s0`/`s1`/`s2`) agree exactly
/// within `vote_tol` (2-of-3 majority holds).
fn healthy_obs() -> Obs {
    Obs {
        age_ms: 0,
        limit_ms: 100,
        value: 50,
        lo: 0,
        hi: 100,
        innov_abs: 0,
        k_sigma: 10,
        used_us: 0,
        budget_us: 1000,
        lateness_us: 0,
        missed_beats: 0,
        // three AGREEING replicas within tolerance — the vote gate clears
        s0: 50,
        s1: 50,
        s2: 50,
        vote_tol: 2,
    }
}

/// All seven fault variants, for the exhaustive CrossCoreTrip absorbing check.
fn all_faults() -> [Fault; 7] {
    [
        Fault::Stale,
        Fault::Implausible,
        Fault::Diverged,
        Fault::BudgetOverrun,
        Fault::DeadlineMiss,
        Fault::HeartbeatLoss,
        Fault::VoteMismatch,
    ]
}

/// Drive one named cause end to end: tripping-gate check, no-silent-clear,
/// escalation to the mapped terminal state within the proven bound, and
/// absorbing-at-terminal check. `liveness` selects the CrossCoreTrip branch
/// (HeartbeatLoss/BudgetOverrun) vs. the PartitionFailsafe branch (the three
/// value-domain faults).
fn drive_cause(name: &str, cause: Fault, obs: Obs, liveness: bool) {
    // (a)/(b) — the mapped gate genuinely fails on the tripping observation,
    // and it is not masked by the other five gates (all_clear is also false).
    if gate_eval(cause, obs) {
        fail!("gust-hm-probe FAIL: {} — gate_eval(cause, obs) == true (fault not detected)", name);
    }
    if all_clear(obs) {
        fail!("gust-hm-probe FAIL: {} — all_clear(obs) == true (fault not detected)", name);
    }

    // (c) — Normal -> Degraded on the first fault.
    let mut hm = Hm::init();
    hm.on_fault(cause);
    if hm.state != HmState::Degraded {
        fail!(
            "gust-hm-probe FAIL: {} — after 1st on_fault state={:?} (want Degraded)",
            name,
            hm.state
        );
    }

    // No-silent-clear: a restart attempt with the SAME still-faulty obs must
    // be rejected AND must not mutate the Hm at all (H2, checked on the real
    // exec `try_restart`, not the ghost twin).
    let pre = hm;
    let restart_ok = hm.try_restart(obs);
    if restart_ok || hm != pre {
        fail!(
            "gust-hm-probe FAIL: {} — try_restart(still-faulty obs) ok={} pre={:?} post={:?} (silent clear)",
            name,
            restart_ok,
            pre,
            hm
        );
    }

    // Drive the remaining faults to exhaust the restart budget: STEPS_TO_FAILSAFE
    // total on_fault calls (fault-kind-agnostic below PartitionFailsafe).
    let mut steps: u32 = 1;
    while steps < STEPS_TO_FAILSAFE {
        hm.on_fault(cause);
        steps += 1;
    }
    if hm.state != HmState::PartitionFailsafe {
        fail!(
            "gust-hm-probe FAIL: {} — after {} on_fault calls state={:?} (want PartitionFailsafe at bound {})",
            name,
            steps,
            hm.state,
            STEPS_TO_FAILSAFE
        );
    }

    if liveness {
        // One more liveness fault trips the cross-core supervisor.
        hm.on_fault(cause);
        steps += 1;
        if hm.state != HmState::CrossCoreTrip {
            fail!(
                "gust-hm-probe FAIL: {} — after {} on_fault calls state={:?} (want CrossCoreTrip at bound {})",
                name,
                steps,
                hm.state,
                STEPS_TO_TRIP
            );
        }
        // (e) absorbing CrossCoreTrip: a further fault, a restart attempt (even
        // with a HEALTHY obs), and a quiet tick all leave the Hm untouched.
        let latched = hm;
        hm.on_fault(cause);
        if hm != latched {
            fail!("gust-hm-probe FAIL: {} — CrossCoreTrip mutated by a further fault", name);
        }
        let ok = hm.try_restart(healthy_obs());
        if ok || hm != latched {
            fail!(
                "gust-hm-probe FAIL: {} — CrossCoreTrip accepted/mutated a restart (ok={})",
                name,
                ok
            );
        }
        hm.on_quiet(healthy_obs());
        if hm != latched {
            fail!("gust-hm-probe FAIL: {} — CrossCoreTrip mutated by a healthy quiet tick", name);
        }
        hprintln!(
            "  {} -> HeartbeatLoss/BudgetOverrun-class -> CrossCoreTrip in {} on_fault steps (bound MAX_RESTARTS+2={}), absorbing confirmed",
            name,
            steps,
            STEPS_TO_TRIP
        );
    } else {
        // (e) absorbing PartitionFailsafe under a value-domain fault: one more
        // same-kind fault is a no-op, and a restart attempt is rejected even
        // with a HEALTHY obs (H3: no software path back from PartitionFailsafe).
        let latched = hm;
        hm.on_fault(cause);
        if hm != latched {
            fail!(
                "gust-hm-probe FAIL: {} — PartitionFailsafe mutated by a further value fault (should be absorbed)",
                name
            );
        }
        let ok = hm.try_restart(healthy_obs());
        if ok || hm != latched {
            fail!(
                "gust-hm-probe FAIL: {} — PartitionFailsafe accepted/mutated a restart (ok={}, should have no software path back)",
                name,
                ok
            );
        }
        hprintln!(
            "  {} -> value-domain fault -> PartitionFailsafe in {} on_fault steps (bound MAX_RESTARTS+1={}), absorbing under continued fault + healthy restart attempt confirmed",
            name,
            steps,
            STEPS_TO_FAILSAFE
        );
    }
}

#[entry]
fn main() -> ! {
    hprintln!("gust-hm-probe: driving the verified value-domain Health Monitor per named mission-loss cause");

    // ---- The 5 named mission-loss causes (REQ-OS-HM-001) --------------------
    let obs_rc_loss = Obs { missed_beats: 5, ..healthy_obs() };
    let obs_datalink_loss = Obs { age_ms: 500, ..healthy_obs() }; // limit_ms=100
    let obs_gps_loss = Obs { innov_abs: 50, ..healthy_obs() }; // k_sigma=10
    let obs_geofence = Obs { value: 999, ..healthy_obs() }; // lo=0, hi=100
    let obs_low_battery = Obs { used_us: 2000, ..healthy_obs() }; // budget_us=1000
    // Sensor-disagreement: three replicas with every pairwise |diff| > vote_tol(2),
    // so no 2-of-3 majority — the TMR vote gate fails (a value-domain fault).
    let obs_sensor_disagree = Obs { s0: 0, s1: 1000, s2: 2000, ..healthy_obs() };

    drive_cause("RC-loss", Fault::HeartbeatLoss, obs_rc_loss, true);
    drive_cause("datalink-loss", Fault::Stale, obs_datalink_loss, false);
    drive_cause("GPS/estimator-loss", Fault::Diverged, obs_gps_loss, false);
    drive_cause("geofence-breach", Fault::Implausible, obs_geofence, false);
    drive_cause("low-battery", Fault::BudgetOverrun, obs_low_battery, true);
    drive_cause("sensor-disagreement", Fault::VoteMismatch, obs_sensor_disagree, false);

    // ---- (d) NON-VACUITY: a healthy frame never trips failsafe --------------
    let healthy = healthy_obs();
    if !all_clear(healthy) {
        fail!("gust-hm-probe FAIL: non-vacuity — healthy Obs does not clear all_clear()");
    }
    for &(name, cause) in &[
        ("RC-loss", Fault::HeartbeatLoss),
        ("datalink-loss", Fault::Stale),
        ("GPS/estimator-loss", Fault::Diverged),
        ("geofence-breach", Fault::Implausible),
        ("low-battery", Fault::BudgetOverrun),
        ("sensor-disagreement", Fault::VoteMismatch),
    ] {
        if !gate_eval(cause, healthy) {
            fail!(
                "gust-hm-probe FAIL: non-vacuity — gate_eval({}, healthy) == false",
                name
            );
        }
    }
    // Normal stays Normal under a healthy quiet tick.
    let mut hn = Hm::init();
    hn.on_quiet(healthy);
    if hn.state != HmState::Normal {
        fail!(
            "gust-hm-probe FAIL: non-vacuity — Normal Hm left Normal under a healthy quiet tick (state={:?})",
            hn.state
        );
    }
    // A Degraded Hm genuinely restarts to Normal on a healthy obs (the real
    // recovery path, not just the rejection path exercised above).
    let mut hd = Hm::init();
    hd.on_fault(Fault::Implausible);
    if hd.state != HmState::Degraded {
        fail!("gust-hm-probe FAIL: non-vacuity setup — Hm did not enter Degraded");
    }
    let restart_ok = hd.try_restart(healthy);
    if !restart_ok || hd.state != HmState::Normal {
        fail!(
            "gust-hm-probe FAIL: non-vacuity — Degraded Hm did not restart to Normal on a healthy obs (ok={}, state={:?})",
            restart_ok,
            hd.state
        );
    }
    hprintln!("  non-vacuity: healthy Obs clears all 7 gates (incl. the TMR vote — three agreeing replicas) + every mapped cause's gate; Normal stays Normal; Degraded restarts to Normal — a good frame does not trip failsafe");

    // ---- (e) TERMINATION: CrossCoreTrip absorbs EVERY fault kind ------------
    let mut ct = Hm::init();
    let mut i: u32 = 0;
    while i < STEPS_TO_TRIP {
        ct.on_fault(Fault::HeartbeatLoss);
        i += 1;
    }
    if ct.state != HmState::CrossCoreTrip {
        fail!(
            "gust-hm-probe FAIL: termination setup — expected CrossCoreTrip after {} HeartbeatLoss faults, got {:?}",
            STEPS_TO_TRIP,
            ct.state
        );
    }
    let latched = ct;
    for f in all_faults() {
        ct.on_fault(f);
        if ct != latched {
            fail!(
                "gust-hm-probe FAIL: termination — CrossCoreTrip mutated by on_fault({:?})",
                f
            );
        }
    }
    let ok = ct.try_restart(healthy_obs());
    if ok || ct != latched {
        fail!(
            "gust-hm-probe FAIL: termination — CrossCoreTrip accepted/mutated a healthy restart (ok={})",
            ok
        );
    }
    ct.on_quiet(healthy_obs());
    if ct != latched {
        fail!("gust-hm-probe FAIL: termination — CrossCoreTrip mutated by a healthy quiet tick");
    }
    hprintln!("  termination: CrossCoreTrip absorbed all 7 fault kinds (incl. VoteMismatch) + a healthy restart attempt + a healthy quiet tick — no software path out");

    hprintln!(
        "gust-hm-probe OK: 6/6 named mission-loss causes (RC-loss, datalink-loss, GPS/estimator-loss, geofence-breach, low-battery, sensor-disagreement) each reached their mapped terminal state (2 CrossCoreTrip via HeartbeatLoss/BudgetOverrun, 4 PartitionFailsafe via Stale/Diverged/Implausible/VoteMismatch — the cross-sensor TMR 2-of-3 voting detector is now exercised end to end) within the proven bound (MAX_RESTARTS+1={} / MAX_RESTARTS+2={} on_fault steps), no silent clear on any still-faulty restart attempt, non-vacuous healthy-frame check passed, CrossCoreTrip confirmed absorbing over all 7 fault kinds",
        STEPS_TO_FAILSAFE,
        STEPS_TO_TRIP
    );
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

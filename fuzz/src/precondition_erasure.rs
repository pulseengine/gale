//! Precondition-erasure fuzz harness: case table + generic runner.
//!
//! Each `PreconditionCase` wraps one decision function exported to C via
//! `ffi/src/lib.rs`. For every case we record:
//!
//!   * `name`     — stable identifier for crash reports.
//!   * `ffi_sym`  — the matching `extern "C"` symbol in `ffi/src/lib.rs`.
//!   * `uca`      — list of UCAs this case exercises (see
//!                  artifacts/stpa_controllers_ucas.yaml).
//!   * `valid`    — the Verus `requires` predicate, evaluated on the input.
//!                  Returns `true` iff the input lies in the proved domain.
//!   * `invoke`   — calls the plain (non-Verus) decision function and
//!                  returns an `Outcome`. The runner wraps it in
//!                  `catch_unwind` so a panic in dev-profile (with
//!                  overflow-checks on) is captured rather than aborting.
//!   * `check`    — on success, re-derives the `ensures` postcondition
//!                  from scratch and returns `None` if it matches,
//!                  `Some(msg)` on divergence. Only evaluated on valid
//!                  inputs (outside the valid region, the proof says
//!                  nothing, so we cannot make a claim).
//!
//! A `Report` is produced for every run; the fuzzer swallows it silently
//! but also prints any divergence/crash to stderr so libfuzzer operators
//! see the signal without having to decode the corpus.
//!
//! The primitives covered at present are:
//!
//! | Case              | FFI symbol                        | UCAs          |
//! |-------------------|-----------------------------------|---------------|
//! | sem_give_decide   | gale_k_sem_give_decide            | U-1           |
//! | sem_take_decide   | gale_k_sem_take_decide            | U-1 (adj)     |
//! | mutex_lock_decide | gale_k_mutex_lock_decide          | U-1 (adj), M10|
//! | mutex_unlock_decide| gale_k_mutex_unlock_decide       | U-8 (adj)     |
//! | spin_lock_valid   | gale_spin_lock_valid              | U-8, U-9      |
//! | spin_unlock_valid | gale_spin_unlock_valid            | U-8           |
//! | futex_wake_decide | gale_k_futex_wake_decide          | U-11 (adj)    |
//! | condvar_broadcast | gale_k_condvar_broadcast_decide   | U-11          |
//! | timer_expire_decide| gale_k_timer_expire_decide       | U-12          |
//!
//! "adj" = the primitive shares the precondition-erasure pattern with the
//! named UCA even if the UCA itself cites a sibling function. These are
//! the four primitives explicitly requested plus five adjacent decision
//! points that share the same bug class.

use std::panic::{self, AssertUnwindSafe};

use gale::condvar::broadcast_decide;
use gale::futex::wake_decide;
use gale::mutex::{lock_decide, unlock_decide, LockDecision, UnlockDecisionKind};
use gale::sem::{give_decide, take_decide, GiveDecision, TakeDecision};
use gale::spinlock_validate::{spin_lock_valid, spin_unlock_valid, CPU_MASK, MAX_CPUS};
use gale::timer::expire_decide;

/// Packed argument bundle — every case picks what it needs.
#[derive(Clone, Copy, Debug)]
pub struct Args {
    pub a: u32,
    pub b: u32,
    pub c: u32,
    pub d: u32,
    pub flag0: bool,
    pub flag1: bool,
    pub flag2: bool,
    pub flag3: bool,
}

/// Outcome of one invocation.
#[derive(Debug)]
pub enum Outcome {
    /// Call returned, postcondition re-check passed (or we had no right
    /// to claim one because the precondition was violated).
    Ok,
    /// Call returned but the re-derived postcondition disagreed. This is
    /// a verification-vs-implementation divergence and should never fire
    /// on a sound proof — it is the primary signal we are looking for.
    PostconditionMismatch(&'static str),
    /// Call panicked (usually via overflow-checks in dev profile) even
    /// though the input satisfied the Verus `requires`. This means the
    /// proof is unsound or the impl has a bug the proof did not catch.
    PanicInsideValidRegion(String),
    /// Call panicked and input was *outside* the valid region. This is
    /// the expected "FFI precondition erasure" behaviour — we log it
    /// for coverage but do not treat it as a bug. A C caller that
    /// supplies these inputs would hit this exact failure.
    PanicInsideViolationRegion(String),
}

pub struct Report {
    pub case: &'static str,
    pub args: Args,
    pub valid: bool,
    pub outcome: Outcome,
}

pub struct PreconditionCase {
    pub name: &'static str,
    pub ffi_sym: &'static str,
    pub uca: &'static [&'static str],
    pub valid: fn(&Args) -> bool,
    pub invoke: fn(&Args) -> Result<Option<&'static str>, String>,
}

/// Run one case and return a Report. Panics are captured via
/// `catch_unwind`; the runner itself never aborts the fuzzer.
pub fn run_case(case: &'static PreconditionCase, args: &Args) -> Report {
    let valid = (case.valid)(args);
    let result = panic::catch_unwind(AssertUnwindSafe(|| (case.invoke)(args)));

    let outcome = match result {
        Ok(Ok(None)) => Outcome::Ok,
        Ok(Ok(Some(msg))) => Outcome::PostconditionMismatch(msg),
        Ok(Err(e)) => {
            // Caller reported a non-panic error path (unused today but
            // reserved for future cases that can distinguish).
            if valid {
                Outcome::PanicInsideValidRegion(e)
            } else {
                Outcome::PanicInsideViolationRegion(e)
            }
        }
        Err(payload) => {
            let msg = panic_msg(payload);
            if valid {
                Outcome::PanicInsideValidRegion(msg)
            } else {
                Outcome::PanicInsideViolationRegion(msg)
            }
        }
    };

    // Surface high-signal outcomes to stderr so libfuzzer logs are useful
    // without needing the structured report.
    match &outcome {
        Outcome::PostconditionMismatch(msg) => {
            eprintln!(
                "DIVERGE case={} uca={:?} args={:?} -> {}",
                case.name, case.uca, args, msg
            );
        }
        Outcome::PanicInsideValidRegion(msg) => {
            eprintln!(
                "PANIC-VALID case={} uca={:?} args={:?} -> {}",
                case.name, case.uca, args, msg
            );
        }
        _ => {}
    }

    Report {
        case: case.name,
        args: *args,
        valid,
        outcome,
    }
}

fn panic_msg(payload: Box<dyn std::any::Any + Send>) -> String {
    if let Some(s) = payload.downcast_ref::<&'static str>() {
        (*s).to_string()
    } else if let Some(s) = payload.downcast_ref::<String>() {
        s.clone()
    } else {
        "<unknown panic payload>".to_string()
    }
}

// ---------------------------------------------------------------------------
// Case table
// ---------------------------------------------------------------------------

pub static CASES: &[PreconditionCase] = &[
    // U-1: sem give. Verus requires limit > 0 && count <= limit. The FFI
    // adds `count + 1` in the Increment branch — wraps/panics on overflow
    // when called with `count == u32::MAX, limit > count` (impossible in
    // the valid region but reachable from C).
    PreconditionCase {
        name: "sem_give_decide",
        ffi_sym: "gale_k_sem_give_decide",
        uca: &["U-1"],
        valid: |a| a.b > 0 && a.a <= a.b,
        invoke: |a| {
            let d = give_decide(a.a, a.b, a.flag0);
            // Ensures (from sem.rs:70-77):
            //   has_waiter ==> WakeThread
            //   !has_waiter && count < limit ==> Increment
            //   !has_waiter && count >= limit ==> Saturated
            let expected = if a.flag0 {
                GiveDecision::WakeThread
            } else if a.a < a.b {
                GiveDecision::Increment
            } else {
                GiveDecision::Saturated
            };
            if d == expected {
                // Also simulate the FFI `count + 1` step to catch the
                // release-mode wrap hidden behind overflow-checks=off
                // builds. In dev this panics (caught by runner) and in
                // release-no-checks we would see a silently wrapped
                // value — which the FFI post-condition check catches.
                if d == GiveDecision::Increment {
                    let new_count = a.a.checked_add(1);
                    if new_count.is_none() {
                        return Ok(Some("Increment branch with count == u32::MAX wraps"));
                    }
                }
                Ok(None)
            } else {
                Ok(Some("give_decide result disagrees with ensures clause"))
            }
        },
    },
    // U-1 (adjacent): sem take has `requires true`, so everything is
    // valid. This exists as a negative control: the fuzzer should
    // never flag this case — if it does, the postcondition derivation
    // is wrong.
    PreconditionCase {
        name: "sem_take_decide",
        ffi_sym: "gale_k_sem_take_decide",
        uca: &["U-1 (negative control)"],
        valid: |_| true,
        invoke: |a| {
            let d = take_decide(a.a, a.flag0);
            let expected = if a.a > 0 {
                TakeDecision::Acquired
            } else if a.flag0 {
                TakeDecision::WouldBlock
            } else {
                TakeDecision::Pend
            };
            if d == expected {
                // FFI does `count - 1` on Acquired — underflow impossible
                // since count > 0 is required by the branch.
                if d == TakeDecision::Acquired && a.a == 0 {
                    return Ok(Some("Acquired with count == 0 would underflow"));
                }
                Ok(None)
            } else {
                Ok(Some("take_decide result disagrees with ensures clause"))
            }
        },
    },
    // M10 overflow: mutex_lock with owner_is_current and lock_count near
    // u32::MAX. Proof returns Overflow at u32::MAX but the *FFI* then
    // does `lock_count + 1` in the Reentrant branch — the branch
    // boundary is the interesting edge.
    PreconditionCase {
        name: "mutex_lock_decide",
        ffi_sym: "gale_k_mutex_lock_decide",
        uca: &["M10", "U-1 (adj)"],
        valid: |_| true,
        invoke: |a| {
            let d = lock_decide(a.a, a.flag0, a.flag1, a.flag2);
            let expected = if a.flag0 {
                LockDecision::Acquire
            } else if a.flag1 {
                if a.a < u32::MAX {
                    LockDecision::Reentrant
                } else {
                    LockDecision::Overflow
                }
            } else if a.flag2 {
                LockDecision::Busy
            } else {
                LockDecision::Pend
            };
            if d != expected {
                return Ok(Some("lock_decide result disagrees with ensures"));
            }
            // FFI shim for Reentrant does `lock_count + 1` — must never
            // be reached at u32::MAX (Overflow path covers that).
            if d == LockDecision::Reentrant && a.a == u32::MAX {
                return Ok(Some("Reentrant reached at lock_count == u32::MAX"));
            }
            // Acquire path hardcodes new_lock_count = 1 — must only
            // occur when owner_is_null.
            if d == LockDecision::Acquire && !a.flag0 {
                return Ok(Some("Acquire without owner_is_null"));
            }
            Ok(None)
        },
    },
    // U-8 (adj): mutex unlock has `requires true`, but the FFI's
    // Released path does `lock_count - 1` — proof bounds it but we
    // still cross-check at the boundary lock_count == 0 / 1.
    PreconditionCase {
        name: "mutex_unlock_decide",
        ffi_sym: "gale_k_mutex_unlock_decide",
        uca: &["U-8 (adj)"],
        valid: |_| true,
        invoke: |a| {
            let d = unlock_decide(a.a, a.flag0, a.flag1);
            let expected = if a.flag0 {
                UnlockDecisionKind::NotLocked
            } else if !a.flag1 {
                UnlockDecisionKind::NotOwner
            } else if a.a > 1 {
                UnlockDecisionKind::Released
            } else {
                UnlockDecisionKind::FullyUnlocked
            };
            if d != expected {
                return Ok(Some("unlock_decide disagrees with ensures"));
            }
            // FFI `new_lock_count = lock_count - 1` — safe only when
            // the Released branch was taken, which requires lock_count > 1.
            if d == UnlockDecisionKind::Released && a.a == 0 {
                return Ok(Some("Released branch with lock_count == 0 underflows"));
            }
            Ok(None)
        },
    },
    // U-8 and U-9: spin_lock_valid has Verus `requires cpu_id_valid(current_cpu_id)`
    // (i.e. current_cpu_id < MAX_CPUS == 4). CPU_MASK == 3 masks the low
    // 2 bits. A caller with cpu_id == 5 hits `5 & 3 == 1`, which can
    // collide with a legitimate other CPU — deadlock hidden. This case
    // is the one that was fixed 2026-04-23 via runtime fast-reject; we
    // still exercise it to pin the behaviour.
    PreconditionCase {
        name: "spin_lock_valid",
        ffi_sym: "gale_spin_lock_valid",
        uca: &["U-8", "U-9"],
        valid: |a| a.b < MAX_CPUS,
        invoke: |a| {
            let thread_cpu = a.a as usize;
            let current_cpu_id = a.b;
            let r = spin_lock_valid(thread_cpu, current_cpu_id);
            // Ensures (spinlock_validate.rs:110-122):
            //   thread_cpu == 0 ==> valid
            //   thread_cpu != 0 && (thread_cpu & CPU_MASK) == (current_cpu_id as usize) ==> !valid
            //   thread_cpu != 0 && (thread_cpu & CPU_MASK) != (current_cpu_id as usize) ==> valid
            let expected = if thread_cpu == 0 {
                true
            } else {
                (thread_cpu & CPU_MASK) != (current_cpu_id as usize)
            };
            if r != expected {
                return Ok(Some("spin_lock_valid disagrees with ensures"));
            }
            // U-9 indicator: in the violation region (cpu_id >= MAX_CPUS),
            // the function silently returns a valid-looking answer. We
            // flag this only inside the violation region so it shows up
            // as coverage, not as a "bug" — the proof has no claim here.
            if current_cpu_id >= MAX_CPUS && thread_cpu != 0 && r {
                // Returned "valid to acquire" under an out-of-domain
                // cpu_id. The 2026-04-23 fix makes the FFI fast-reject
                // this case; the plain fn does not. Report as coverage
                // note (not a divergence — see U-9 narrative).
                eprintln!(
                    "COVERAGE U-9 spin_lock_valid returned TRUE with cpu_id={} (>= MAX_CPUS), thread_cpu=0x{:x}",
                    current_cpu_id, thread_cpu
                );
            }
            Ok(None)
        },
    },
    // U-8: spin_unlock_valid has `requires cpu_id_valid && thread_ptr_valid`.
    // The thread_ptr_valid part is where U-8 lives: the FFI encodes
    // "no owner" as 0, aliasing it with a legitimate tid=0.
    PreconditionCase {
        name: "spin_unlock_valid",
        ffi_sym: "gale_spin_unlock_valid",
        uca: &["U-8"],
        valid: |a| a.b < MAX_CPUS && (a.c as usize & CPU_MASK) == 0,
        invoke: |a| {
            let thread_cpu = a.a as usize;
            let current_cpu_id = a.b;
            let current_thread = a.c as usize;
            let r = spin_unlock_valid(thread_cpu, current_cpu_id, current_thread);
            let expected_encoded = (current_cpu_id as usize) | current_thread;
            let expected = thread_cpu == expected_encoded;
            if r != expected {
                return Ok(Some("spin_unlock_valid disagrees with ensures"));
            }
            // U-8 indicator: current_thread == 0 is the aliased sentinel.
            if current_thread == 0 && r {
                eprintln!(
                    "COVERAGE U-8 spin_unlock_valid accepted current_thread=0 with cpu_id={}",
                    current_cpu_id
                );
            }
            Ok(None)
        },
    },
    // U-11 (adj): futex wake_decide. No `requires` clause, but FX6
    // claims no overflow on the remaining count. Boundary is
    // `num_waiters = 0` and `num_waiters = u32::MAX`.
    PreconditionCase {
        name: "futex_wake_decide",
        ffi_sym: "gale_k_futex_wake_decide",
        uca: &["U-11 (adj)"],
        valid: |_| true,
        invoke: |a| {
            let d = wake_decide(a.a, a.flag0);
            // Ensures (futex.rs:76-88):
            //   num_waiters == 0 ==> woken == 0 && remaining == 0
            //   wake_all && num_waiters > 0 ==> woken == num_waiters, remaining == 0
            //   !wake_all && num_waiters > 0 ==> woken == 1, remaining == num_waiters - 1
            let (exp_woken, exp_remaining) = if a.a == 0 {
                (0u32, 0u32)
            } else if a.flag0 {
                (a.a, 0u32)
            } else {
                (1u32, a.a - 1)
            };
            if d.woken != exp_woken || d.remaining != exp_remaining {
                return Ok(Some("wake_decide disagrees with ensures"));
            }
            Ok(None)
        },
    },
    // U-11: condvar broadcast_decide is declared `ensures result == num_waiters`,
    // but the caller-side contract says num_waiters must be bounded by
    // MAX_WAITERS == 64 (wait_queue.rs:31). The Rust fn is a pass-through
    // with no runtime cap — precondition is entirely erased at the FFI.
    PreconditionCase {
        name: "condvar_broadcast_decide",
        ffi_sym: "gale_k_condvar_broadcast_decide",
        uca: &["U-11"],
        // MAX_WAITERS == 64 lives in wait_queue; inlined here because it
        // is not re-exported.
        valid: |a| a.a <= 64,
        invoke: |a| {
            let r = broadcast_decide(a.a);
            if r != a.a {
                return Ok(Some("broadcast_decide is not pass-through"));
            }
            // Coverage note: the function accepts out-of-domain input
            // silently. A downstream caller using `r` as an index into
            // a length-64 wait queue will OOB.
            if a.a > 64 {
                eprintln!(
                    "COVERAGE U-11 broadcast_decide accepted num_waiters={} (> MAX_WAITERS=64)",
                    a.a
                );
            }
            Ok(None)
        },
    },
    // U-12: timer expire_decide saturates at u32::MAX. The proof is
    // fine; the FFI silently wraps downstream, so the caller cannot
    // distinguish "saturated" from "one increment".
    PreconditionCase {
        name: "timer_expire_decide",
        ffi_sym: "gale_k_timer_expire_decide",
        uca: &["U-12"],
        valid: |_| true,
        invoke: |a| {
            let r = expire_decide(a.a, a.b);
            let exp_new_status = if a.a < u32::MAX { a.a + 1 } else { u32::MAX };
            let exp_is_periodic = a.b > 0;
            if r.new_status != exp_new_status || r.is_periodic != exp_is_periodic {
                return Ok(Some("expire_decide disagrees with ensures"));
            }
            // Coverage note: the saturated case is indistinguishable to
            // the caller from a fresh expiration.
            if a.a == u32::MAX {
                eprintln!(
                    "COVERAGE U-12 timer status saturated at u32::MAX (period={})",
                    a.b
                );
            }
            Ok(None)
        },
    },
];

/// Deterministic sweep over the case table — feeds each case a set of
/// boundary and random inputs, covering both the valid region and the
/// violation region. Returns a summary suitable for printing.
pub fn deterministic_sweep() -> SweepSummary {
    let mut summary = SweepSummary::default();

    // Seeded deterministic pseudo-random: xorshift so we do not drag in
    // a rand dependency.
    let mut rng_state: u64 = 0x9E37_79B9_7F4A_7C15;
    let mut next_u32 = move || -> u32 {
        rng_state ^= rng_state << 13;
        rng_state ^= rng_state >> 7;
        rng_state ^= rng_state << 17;
        rng_state as u32
    };

    for case in CASES {
        let mut per_case = PerCase {
            name: case.name,
            ffi_sym: case.ffi_sym,
            uca: case.uca,
            ..Default::default()
        };

        // Static boundary sweep — picks values that matter for each
        // decision function's branches.
        let boundaries: &[Args] = &[
            // Zero everywhere
            Args { a: 0, b: 0, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            Args { a: 0, b: 1, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            Args { a: 0, b: 1, c: 0, d: 0, flag0: true,  flag1: false, flag2: false, flag3: false },
            // Precondition boundary: count == limit, limit > 0
            Args { a: 5, b: 5, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            Args { a: 4, b: 5, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            // U-1 violation region: count > limit
            Args { a: u32::MAX, b: 5, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            Args { a: u32::MAX, b: 5, c: 0, d: 0, flag0: true,  flag1: false, flag2: false, flag3: false },
            // limit == 0 (always invalid)
            Args { a: 0, b: 0, c: 0, d: 0, flag0: true,  flag1: false, flag2: false, flag3: false },
            // mutex boundary: lock_count near u32::MAX
            Args { a: u32::MAX, b: 0, c: 0, d: 0, flag0: false, flag1: true,  flag2: false, flag3: false },
            Args { a: u32::MAX - 1, b: 0, c: 0, d: 0, flag0: false, flag1: true, flag2: false, flag3: false },
            // mutex unlock edges: lock_count == 0 / 1 / 2
            Args { a: 0, b: 0, c: 0, d: 0, flag0: true,  flag1: true,  flag2: false, flag3: false },
            Args { a: 1, b: 0, c: 0, d: 0, flag0: false, flag1: true,  flag2: false, flag3: false },
            Args { a: 2, b: 0, c: 0, d: 0, flag0: false, flag1: true,  flag2: false, flag3: false },
            // U-9 violation: cpu_id >= MAX_CPUS (4)
            Args { a: 0x8, b: 5, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            Args { a: 0x1, b: 5, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            Args { a: 0x1, b: 1, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            // U-8 violation: tid / thread ptr == 0
            Args { a: 0, b: 0, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            // U-11 violation: num_waiters > 64
            Args { a: 65, b: 0, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            Args { a: 10_000, b: 0, c: 0, d: 0, flag0: true, flag1: false, flag2: false, flag3: false },
            Args { a: u32::MAX, b: 0, c: 0, d: 0, flag0: true, flag1: false, flag2: false, flag3: false },
            // U-12: timer status saturation
            Args { a: u32::MAX, b: 0, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            Args { a: u32::MAX, b: 1000, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
            Args { a: u32::MAX - 1, b: 0, c: 0, d: 0, flag0: false, flag1: false, flag2: false, flag3: false },
        ];

        for args in boundaries {
            record(&mut per_case, run_case(case, args));
        }

        // Randomised sweep — 256 cases per primitive.
        for _ in 0..256 {
            let args = Args {
                a: next_u32(),
                b: next_u32(),
                c: next_u32(),
                d: next_u32(),
                flag0: next_u32() & 1 != 0,
                flag1: next_u32() & 1 != 0,
                flag2: next_u32() & 1 != 0,
                flag3: next_u32() & 1 != 0,
            };
            record(&mut per_case, run_case(case, &args));
        }

        summary.cases.push(per_case);
    }

    summary
}

fn record(pc: &mut PerCase, r: Report) {
    pc.total += 1;
    if r.valid {
        pc.valid_region += 1;
    } else {
        pc.violation_region += 1;
    }
    match r.outcome {
        Outcome::Ok => pc.ok += 1,
        Outcome::PostconditionMismatch(_) => pc.postcondition_mismatch += 1,
        Outcome::PanicInsideValidRegion(_) => pc.panic_valid += 1,
        Outcome::PanicInsideViolationRegion(_) => pc.panic_violation += 1,
    }
}

#[derive(Default)]
pub struct PerCase {
    pub name: &'static str,
    pub ffi_sym: &'static str,
    pub uca: &'static [&'static str],
    pub total: u32,
    pub valid_region: u32,
    pub violation_region: u32,
    pub ok: u32,
    pub postcondition_mismatch: u32,
    pub panic_valid: u32,
    pub panic_violation: u32,
}

#[derive(Default)]
pub struct SweepSummary {
    pub cases: Vec<PerCase>,
}

impl SweepSummary {
    pub fn print(&self) {
        println!("Precondition-erasure fuzz sweep — {} cases", self.cases.len());
        println!(
            "{:<28} {:<36} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} UCAs",
            "name", "ffi_sym", "tot", "valid", "viol", "ok", "diff", "p!val", "p!viol"
        );
        for c in &self.cases {
            println!(
                "{:<28} {:<36} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:>5} {:?}",
                c.name,
                c.ffi_sym,
                c.total,
                c.valid_region,
                c.violation_region,
                c.ok,
                c.postcondition_mismatch,
                c.panic_valid,
                c.panic_violation,
                c.uca
            );
        }
        println!();
        let total: u32 = self.cases.iter().map(|c| c.total).sum();
        let mismatch: u32 = self.cases.iter().map(|c| c.postcondition_mismatch).sum();
        let panic_valid: u32 = self.cases.iter().map(|c| c.panic_valid).sum();
        let panic_viol: u32 = self.cases.iter().map(|c| c.panic_violation).sum();
        println!(
            "TOTALS: {} runs, {} postcondition mismatches, {} panics in valid region, {} panics in violation region",
            total, mismatch, panic_valid, panic_viol
        );
    }

    pub fn any_issues(&self) -> bool {
        self.cases
            .iter()
            .any(|c| c.postcondition_mismatch > 0 || c.panic_valid > 0)
    }
}

//! gust:hal **thin-seam** PWM driver — driver breadth (the 9th verified iodev after
//! GPIO/timer/SPI/UART/I2C/ADC/DAC/IWDG). The whole STM32 advanced-timer PWM output path
//! — the PWM-mode config (CCMR PWM mode 1 + preload), the period (ARR) / duty (CCR) load,
//! and the output-enable/main-output-enable gating — lives here in verified wasm, importing
//! ONLY `gust:hal/mmio` (read32/write32): **zero new TCB atoms**.
//!
//! This is the **actuator-output driver**. On the Pixhawk 6X-RT its failsafe is precisely
//! the **4× failsafe-PWM** the cross-core Health Monitor (gale#63) trips on FMU loss /
//! window overrun. So its two safety properties are load-bearing:
//!   1. **duty ≤ period** — a compare value above the auto-reload means an unintended 100%
//!      (or undefined) output; on an ESC/servo that is full throttle / hard-over. Every
//!      commanded duty is clamped to the period, always.
//!   2. **failsafe is total and latching** — `failsafe` forces the outputs off (clears MOE)
//!      from ANY state, and cannot be undone by a stray `start` (only an explicit
//!      reconfigure re-arms). A failsafe you can accidentally clear is worthless.
//!
//! Build:   cargo build --release --target wasm32-unknown-unknown
//! Dissolve: loom optimize --passes inline | synth compile --target cortex-m3
//!           --all-exports --relocatable          (scalar in/out → 0 SRAM)
//! Verify:  cargo kani   (duty-clamp · failsafe-total-off · failsafe-latches ·
//!                        phase-gating · config-well-formed · pack-roundtrip · start-keeps-duty)
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// gust:hal mmio capability — resolved at link by the SAME ~10-line TCB bridge the
// other thin-seam drivers use.
extern "C" {
    fn mmio_read32(addr: u32) -> u32;
    fn mmio_write32(addr: u32, val: u32);
}

// STM32 advanced-timer register map (offsets from the timer base, e.g. TIM1=0x4001_2C00).
// Device knowledge as *data* (offsets/bit math), not trusted code.
const CR1: u32 = 0x00; // control 1 (CEN, ARPE)
const EGR: u32 = 0x14; // event generation (UG — latch preloaded PSC/ARR/CCR)
const CCMR1: u32 = 0x18; // capture/compare mode 1 (OC1M PWM mode, OC1PE preload)
const CCER: u32 = 0x20; // capture/compare enable (CC1E output enable)
const PSC: u32 = 0x28; // prescaler
const ARR: u32 = 0x2C; // auto-reload (the PWM period)
const CCR1: u32 = 0x34; // capture/compare 1 (the PWM duty)
const BDTR: u32 = 0x44; // break/dead-time (MOE main output enable — advanced timers)

const CR1_CEN: u32 = 1 << 0; // counter enable
const CR1_ARPE: u32 = 1 << 7; // auto-reload preload enable
const EGR_UG: u32 = 1 << 0; // update generation (latch)
const CCMR1_OC1M_PWM1: u32 = 0b110 << 4; // OC1M = PWM mode 1
const CCMR1_OC1PE: u32 = 1 << 3; // OC1 preload enable
const CCER_CC1E: u32 = 1 << 0; // CC1 output enable
const BDTR_MOE: u32 = 1 << 15; // main output enable (clearing it forces outputs off)

/// 16-bit timer counter/compare width — periods and duties live in [0, 65535].
pub const MAX_16: u32 = 0xFFFF;

// ───────────────────── pure config (table-free) ─────────────────────

/// The CCMR1 word for PWM mode 1 with output preload — a pure constant, no table.
#[inline]
pub fn ccmr1_pwm1() -> u32 {
    CCMR1_OC1M_PWM1 | CCMR1_OC1PE
}

/// **The duty clamp** (the load-bearing PWM safety property): the compare value can never
/// exceed the period. Both masked to 16 bits, then `min`. Pure arithmetic.
#[inline]
pub const fn clamp_duty(duty: u32, period: u32) -> u32 {
    let d = duty & MAX_16;
    let p = period & MAX_16;
    if d > p { p } else { d }
}

// ───────────────────── PWM output FSM ─────────────────────
//
// The output lifecycle as a proven state machine. `Phase` IS the state; `duty` carries the
// last clamped compare value. The period (ARR) is a configure-time argument, not carried in
// the packed state (so the scalar seam stays one u32: phase[31:30] + duty[15:0]).

/// Where the PWM output is in its lifecycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Timer off, not configured.
    Off,
    /// PWM mode + period configured; duty stageable; not yet counting.
    Configured,
    /// Counting + driving the output (CEN + MOE set).
    Running,
    /// **Failsafe**: outputs forced off (MOE cleared). Latching — only an explicit
    /// reconfigure leaves this state; no `start` can undo it.
    Safe,
}

/// Why a transition was rejected. A rejected op never corrupts the FSM.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// A transition issued from the wrong phase (start off-Configured, set_duty while
    /// Off/Safe — you cannot drive a duty onto a failsafed or unconfigured output).
    WrongPhase,
}

/// A PWM channel's state: phase + last clamped duty.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Pwm {
    pub phase: Phase,
    pub duty: u32,
}

impl Pwm {
    /// The off channel.
    pub const fn off() -> Self {
        Pwm { phase: Phase::Off, duty: 0 }
    }
}

/// Armed predicate: a duty may be staged **iff** the channel is Configured or Running.
#[inline]
pub const fn is_armed(p: Pwm) -> bool {
    matches!(p.phase, Phase::Configured | Phase::Running)
}

/// Configure PWM mode + period: Off → Configured (also the re-arm from Safe). Duty cleared.
#[inline]
pub const fn configure(p: Pwm) -> Result<Pwm, Fault> {
    match p.phase {
        Phase::Off | Phase::Safe => Ok(Pwm { phase: Phase::Configured, duty: 0 }),
        _ => Err(Fault::WrongPhase),
    }
}

/// Stage a duty — only while armed (Configured/Running). The value is **clamped to the
/// period** so it can never exceed it. Rejected while Off/Safe.
#[inline]
pub const fn set_duty(p: Pwm, duty: u32, period: u32) -> Result<Pwm, Fault> {
    match p.phase {
        Phase::Configured | Phase::Running => Ok(Pwm { phase: p.phase, duty: clamp_duty(duty, period) }),
        _ => Err(Fault::WrongPhase),
    }
}

/// Start counting + enable outputs: Configured → Running (CEN + MOE). Rejected elsewhere —
/// crucially, cannot start from Safe (a failsafe is not undone by start). Duty preserved.
#[inline]
pub const fn start(p: Pwm) -> Result<Pwm, Fault> {
    match p.phase {
        Phase::Configured => Ok(Pwm { phase: Phase::Running, duty: p.duty }),
        _ => Err(Fault::WrongPhase),
    }
}

/// **Failsafe**: force outputs off (clear MOE) from ANY state → Safe, duty 0. Total and
/// latching — the safety action, defined for every state; only `configure` re-arms.
#[inline]
pub const fn failsafe(_p: Pwm) -> Pwm {
    Pwm { phase: Phase::Safe, duty: 0 }
}

// ───────────────────────────── dissolve ABI ─────────────────────────────────
//
// Scalar in/out, no linmem/data segment → 0 SRAM. State packed as: phase in bits[31:30],
// duty (16-bit) in bits[15:0]. The period is a configure-time argument, not state.

/// Sentinel for a rejected FSM transition — keeps the ABI scalar (cf. adc-thin's ADC_FAULT).
pub const PWM_FAULT: u32 = 0xFFFF_FFFF;

const PHASE_SHIFT: u32 = 30;
const DUTY_MASK: u32 = 0xFFFF;

#[inline]
fn ph_enc(p: Phase) -> u32 {
    match p {
        Phase::Off => 0,
        Phase::Configured => 1,
        Phase::Running => 2,
        Phase::Safe => 3,
    }
}
#[inline]
fn ph_dec(b: u32) -> Phase {
    match b {
        0 => Phase::Off,
        1 => Phase::Configured,
        2 => Phase::Running,
        _ => Phase::Safe,
    }
}
/// Pack a `Pwm` into the scalar carried across the dissolve seam.
#[inline]
fn pack(p: Pwm) -> u32 {
    (ph_enc(p.phase) << PHASE_SHIFT) | (p.duty & DUTY_MASK)
}
/// Unpack the scalar back into a `Pwm`.
#[inline]
fn unpack(s: u32) -> Pwm {
    Pwm { phase: ph_dec(s >> PHASE_SHIFT), duty: s & DUTY_MASK }
}

#[inline(always)]
fn rd(a: u32) -> u32 {
    unsafe { mmio_read32(a) }
}
#[inline(always)]
fn wr(a: u32, v: u32) {
    unsafe { mmio_write32(a, v) }
}

// ---- exported protocol primitives (the driver's gust:hal-facing surface) ----

/// Configure PWM mode 1 on channel 1 of the timer at `base`: PSC, ARR (period), CCMR1
/// PWM-mode + preload, CC1E output-enable, ARPE, then latch via EGR.UG. Off/Safe →
/// Configured. Returns the packed state or `PWM_FAULT`. Table-free (pure bit config).
#[no_mangle]
pub extern "C" fn pwm_configure(base: u32, state: u32, psc: u32, period: u32) -> u32 {
    match configure(unpack(state)) {
        Ok(next) => {
            wr(base + PSC, psc & MAX_16);
            wr(base + ARR, period & MAX_16);
            wr(base + CCMR1, ccmr1_pwm1());
            wr(base + CCER, CCER_CC1E);
            wr(base + CR1, CR1_ARPE);
            wr(base + EGR, EGR_UG); // latch preloaded registers
            pack(next)
        }
        Err(_) => PWM_FAULT,
    }
}

/// Stage a duty (CCR1) — clamped to the period so it can NEVER exceed it. Only while armed.
/// Returns the packed state (with the clamped duty) or `PWM_FAULT` if Off/Safe.
#[no_mangle]
pub extern "C" fn pwm_set_duty(base: u32, state: u32, duty: u32, period: u32) -> u32 {
    match set_duty(unpack(state), duty, period) {
        Ok(next) => {
            wr(base + CCR1, next.duty);
            pack(next)
        }
        Err(_) => PWM_FAULT,
    }
}

/// Start the PWM: enable the counter (CEN) + main output (MOE). Configured → Running.
/// Returns the packed Running state or `PWM_FAULT` (including from Safe — a failsafe is
/// never undone by start).
#[no_mangle]
pub extern "C" fn pwm_start(base: u32, state: u32) -> u32 {
    match start(unpack(state)) {
        Ok(next) => {
            wr(base + BDTR, BDTR_MOE);
            wr(base + CR1, CR1_ARPE | CR1_CEN);
            pack(next)
        }
        Err(_) => PWM_FAULT,
    }
}

/// **Failsafe**: clear MOE (force all outputs off) and stop the counter, from ANY state →
/// Safe. Total — always succeeds. This is the call the Health Monitor issues to trip the
/// failsafe-PWM. Returns the packed Safe state.
#[no_mangle]
pub extern "C" fn pwm_failsafe(base: u32, state: u32) -> u32 {
    wr(base + BDTR, 0); // MOE cleared → outputs Hi-Z / off
    wr(base + CR1, 0); // counter stopped
    pack(failsafe(unpack(state)))
}

/// Read back the last clamped duty from a packed state (0..65535). Pure query.
#[no_mangle]
pub extern "C" fn pwm_duty(state: u32) -> u32 {
    unpack(state).duty
}

/// True (1) once the channel is in the latched failsafe (Safe) state, else 0.
#[no_mangle]
pub extern "C" fn pwm_is_safe(state: u32) -> u32 {
    matches!(unpack(state).phase, Phase::Safe) as u32
}

// ─────────────────────────────── Kani proofs ────────────────────────────────
//
// The safety properties over the full input space. Run: `cargo kani`.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    fn any_phase() -> Phase {
        match kani::any::<u8>() % 4 {
            0 => Phase::Off,
            1 => Phase::Configured,
            2 => Phase::Running,
            _ => Phase::Safe,
        }
    }
    // Seam-reachable state: `duty` is a 16-bit field in the packed u32, so `unpack` masks it.
    // Modeling that is faithful — the ABI can never hold a wider duty; the clamp of the
    // *commanded* duty is tested in p1 with an unconstrained input.
    fn any_pwm() -> Pwm {
        Pwm { phase: any_phase(), duty: kani::any::<u32>() & DUTY_MASK }
    }

    /// P1 — **duty clamp** (the load-bearing PWM safety property): `set_duty`, when accepted,
    /// yields a duty ≤ the period, for ANY commanded duty and period. The compare value can
    /// never exceed the auto-reload → never an unintended 100% output.
    #[kani::proof]
    fn p1_duty_clamp() {
        let p = any_pwm();
        let duty: u32 = kani::any();
        let period: u32 = kani::any();
        if let Ok(next) = set_duty(p, duty, period) {
            assert!(next.duty <= (period & MAX_16));
            assert!(next.duty <= MAX_16);
            // exactly the clamp: min(duty, period) over the 16-bit domain
            let d = duty & MAX_16;
            let per = period & MAX_16;
            assert_eq!(next.duty, if d > per { per } else { d });
        }
        // the pure clamp too, unconditionally
        assert!(clamp_duty(duty, period) <= (period & MAX_16));
    }

    /// P2 — failsafe is total and forces outputs off: from EVERY state, `failsafe` lands Safe
    /// with duty 0 (outputs off). Always succeeds — the safety action is never rejected.
    #[kani::proof]
    fn p2_failsafe_total_off() {
        let p = any_pwm();
        let n = failsafe(p);
        assert_eq!(n.phase, Phase::Safe);
        assert_eq!(n.duty, 0);
    }

    /// P3 — failsafe latches: from Safe, neither `start` nor `set_duty` is accepted — a
    /// failsafe cannot be casually undone; only `configure` re-arms. So a tripped failsafe
    /// stays tripped until an explicit reconfigure.
    #[kani::proof]
    fn p3_failsafe_latches() {
        let safe = Pwm { phase: Phase::Safe, duty: kani::any::<u32>() & DUTY_MASK };
        assert_eq!(start(safe), Err(Fault::WrongPhase));
        assert_eq!(set_duty(safe, kani::any(), kani::any()), Err(Fault::WrongPhase));
        // only configure leaves Safe
        assert_eq!(configure(safe).unwrap().phase, Phase::Configured);
    }

    /// P4 — phase gating: `start` requires Configured; `set_duty` requires armed
    /// (Configured/Running). Off-phase ops are rejected and corrupt nothing.
    #[kani::proof]
    fn p4_phase_gating() {
        let p = any_pwm();
        if p.phase != Phase::Configured {
            assert_eq!(start(p), Err(Fault::WrongPhase));
        }
        if !is_armed(p) {
            assert_eq!(set_duty(p, kani::any(), kani::any()), Err(Fault::WrongPhase));
        } else {
            assert!(set_duty(p, kani::any(), kani::any()).is_ok());
        }
        assert_eq!(is_armed(p), p.phase == Phase::Configured || p.phase == Phase::Running);
    }

    /// P5 — config well-formed: `clamp_duty` never exceeds the period, and `ccmr1_pwm1` sets
    /// only the PWM-mode + preload bits (no stray bit), for any input.
    #[kani::proof]
    fn p5_config_well_formed() {
        let d: u32 = kani::any();
        let per: u32 = kani::any();
        let c = clamp_duty(d, per);
        assert!(c <= (per & MAX_16));
        assert!(c <= MAX_16);
        let m = ccmr1_pwm1();
        assert_eq!(m & !(CCMR1_OC1M_PWM1 | CCMR1_OC1PE), 0);
        assert!(m & CCMR1_OC1M_PWM1 != 0); // PWM mode selected
    }

    /// P6 — pack/unpack round-trips every reachable state losslessly (phase + 16-bit duty).
    #[kani::proof]
    fn p6_pack_roundtrip() {
        let p = any_pwm();
        let u = unpack(pack(p));
        assert_eq!(u.phase, p.phase);
        assert_eq!(u.duty, p.duty);
    }

    /// P7 — start preserves duty: `start` (Configured → Running) carries the staged duty
    /// through unchanged — arming the output never glitches the commanded value.
    #[kani::proof]
    fn p7_start_keeps_duty() {
        let p = Pwm { phase: Phase::Configured, duty: kani::any::<u32>() & DUTY_MASK };
        let n = start(p).unwrap();
        assert_eq!(n.phase, Phase::Running);
        assert_eq!(n.duty, p.duty);
    }
}

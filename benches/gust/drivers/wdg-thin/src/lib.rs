//! gust:hal **thin-seam** IWDG (independent watchdog) driver — driver breadth (the 8th
//! verified iodev after GPIO/timer/SPI/UART/I2C/ADC/DAC). The whole STM32F1 IWDG
//! key-sequence + lifecycle — the 0x5555 unlock / PR+RLR config / 0xCCCC start / 0xAAAA
//! refresh — lives here in verified wasm, importing ONLY `gust:hal/mmio` (read32/write32):
//! the SAME subset the other thin-seam drivers use, so it adds **zero new TCB atoms**.
//!
//! This is the **module-level hardware backstop** the partition-scheduler Health Monitor
//! design (gale#63) names: if the verified HM/switch core itself hangs, it stops servicing
//! the IWDG and the hardware forces a reset → fail-to-safe. So its own correctness matters.
//!
//! IWDG's distinctive safety property is **cannot-un-start**: once the watchdog is started
//! (0xCCCC) it can never be disabled in software — only a system reset stops it. A watchdog
//! you can accidentally turn off is worthless, so the FSM provides NO disable transition and
//! proves that no operation ever leaves the Running state (except a refresh back to Running).
//! Companion invariants: the config registers (PR/RLR) are **write-protected** until the
//! 0x5555 key unlocks them, and a refresh only has effect once Running.
//!
//! Build:   cargo build --release --target wasm32-unknown-unknown
//! Dissolve: loom optimize --passes inline | synth compile --target cortex-m3
//!           --all-exports --relocatable          (scalar in/out → 0 SRAM)
//! Verify:  cargo kani   (write-protection · cannot-un-start · refresh-only-running ·
//!                        start-once · config-bounds · pack-roundtrip · unlock-gates-config)
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// gust:hal mmio capability — resolved at link by the SAME ~10-line TCB bridge the
// other thin-seam drivers use. The IWDG is a pure register-poke peripheral.
extern "C" {
    fn mmio_read32(addr: u32) -> u32;
    fn mmio_write32(addr: u32, val: u32);
}

// STM32F1 IWDG register map (offsets from the peripheral base, IWDG=0x4000_3000).
// Device knowledge as *data* (offsets/bit math), not trusted code.
const KR: u32 = 0x00; // key register (write-only command port)
const PR: u32 = 0x04; // prescaler (write-protected)
const RLR: u32 = 0x08; // reload (write-protected)
const SR: u32 = 0x0C; // status (PVU, RVU — update-in-progress flags)

// The three IWDG key commands, written to KR.
const KEY_ENABLE: u32 = 0x5555; // unlock write access to PR + RLR
const KEY_REFRESH: u32 = 0xAAAA; // reload the counter ("kick the dog")
const KEY_START: u32 = 0xCCCC; // start the watchdog (irreversible)

const SR_PVU: u32 = 1 << 0; // prescaler update in progress
const SR_RVU: u32 = 1 << 1; // reload update in progress

/// Largest prescaler code (divider /4 … /256 for 0…6; 7 is reserved-as-/256). 3-bit field.
pub const MAX_PRESCALER: u32 = 7;
/// Largest reload value — the down-counter is 12-bit.
pub const MAX_RELOAD: u32 = 0x0FFF;

// ───────────────────── pure config (table-free) ─────────────────────
//
// TABLE-FREE by construction (see adc-thin/i2c-thin): all config is pure bit arithmetic,
// so nothing lowers to a `.rodata` linmem table a `--relocatable` node would read as 0.

/// PR value = the prescaler code masked to 3 bits — can never overflow the field.
#[inline]
pub fn pr_bits(prescaler: u32) -> u32 {
    prescaler & 0x7
}

/// RLR value = the reload masked to the 12-bit counter width.
#[inline]
pub fn rlr_bits(reload: u32) -> u32 {
    reload & MAX_RELOAD
}

// ───────────────────── watchdog lifecycle FSM ─────────────────────
//
// The IWDG lifecycle as a proven state machine. `Phase` IS the state; `prescaler`/`reload`
// carry the staged config. The one-way `start` and the absence of any disable transition
// are what make "cannot-un-start" a structural, checkable property.

/// Where the watchdog is in its lifecycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Untouched — key not yet written; PR/RLR write-protected.
    Idle,
    /// 0x5555 written — PR/RLR are writable. The only phase in which config takes effect.
    Unlocked,
    /// Config staged + re-locked; ready to start.
    Configured,
    /// 0xCCCC written — the watchdog is counting. **Terminal for software**: no transition
    /// leaves Running except a `refresh` back to Running; only a hardware reset stops it.
    Running,
}

/// Why a transition was rejected. A rejected op never corrupts the FSM.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// A `set_config` while the registers are write-protected (not Unlocked) — the
    /// write-protection property.
    WriteProtected,
    /// A transition issued from the wrong phase (unlock while Running, start off-Configured,
    /// refresh before Running, …).
    WrongPhase,
}

/// The watchdog state: phase + staged prescaler + staged reload.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Iwdg {
    pub phase: Phase,
    pub prescaler: u32,
    pub reload: u32,
}

impl Iwdg {
    /// The untouched watchdog.
    pub const fn idle() -> Self {
        Iwdg { phase: Phase::Idle, prescaler: 0, reload: MAX_RELOAD }
    }
}

/// Running predicate: true once the watchdog is counting (and can no longer be stopped).
#[inline]
pub const fn is_running(w: Iwdg) -> bool {
    matches!(w.phase, Phase::Running)
}

/// Write the 0x5555 unlock key: Idle → Unlocked, or Configured → Unlocked (reconfigure
/// **before** start). Rejected once Running — a running safety watchdog is not reconfigured
/// in this (conservative) model, which is part of what makes Running a software sink.
#[inline]
pub const fn unlock(w: Iwdg) -> Result<Iwdg, Fault> {
    match w.phase {
        Phase::Idle | Phase::Configured => {
            Ok(Iwdg { phase: Phase::Unlocked, prescaler: w.prescaler, reload: w.reload })
        }
        _ => Err(Fault::WrongPhase),
    }
}

/// Stage prescaler + reload — **only** while Unlocked (write-protection). Values are masked
/// to their field widths. Rejected from any other phase with `WriteProtected`.
#[inline]
pub const fn set_config(w: Iwdg, prescaler: u32, reload: u32) -> Result<Iwdg, Fault> {
    match w.phase {
        Phase::Unlocked => {
            Ok(Iwdg { phase: Phase::Unlocked, prescaler: prescaler & 0x7, reload: reload & MAX_RELOAD })
        }
        _ => Err(Fault::WriteProtected),
    }
}

/// Re-lock after staging config: Unlocked → Configured. Rejected elsewhere.
#[inline]
pub const fn lock(w: Iwdg) -> Result<Iwdg, Fault> {
    match w.phase {
        Phase::Unlocked => Ok(Iwdg { phase: Phase::Configured, prescaler: w.prescaler, reload: w.reload }),
        _ => Err(Fault::WrongPhase),
    }
}

/// Write the 0xCCCC start key: Configured → Running. **Irreversible** — there is no inverse
/// transition. Rejected from any other phase (including Running: no double-start).
#[inline]
pub const fn start(w: Iwdg) -> Result<Iwdg, Fault> {
    match w.phase {
        Phase::Configured => Ok(Iwdg { phase: Phase::Running, prescaler: w.prescaler, reload: w.reload }),
        _ => Err(Fault::WrongPhase),
    }
}

/// Write the 0xAAAA refresh key ("kick"): Running → Running. Rejected before Running — a
/// refresh only has effect once the watchdog is counting.
#[inline]
pub const fn refresh(w: Iwdg) -> Result<Iwdg, Fault> {
    match w.phase {
        Phase::Running => Ok(w),
        _ => Err(Fault::WrongPhase),
    }
}

// NOTE: there is deliberately NO `stop`/`disable`. That absence IS the cannot-un-start
// safety property — proven in `p2_cannot_un_start` over every provided transition.

// ───────────────────────────── dissolve ABI ─────────────────────────────────
//
// Scalar in/out, no linmem/data segment → 0 SRAM. State is carried by the caller as a
// packed u32: phase in bits[31:30], prescaler (3-bit) in bits[14:12], reload (12-bit) in
// bits[11:0].

/// Sentinel for a rejected FSM transition — keeps the ABI scalar (cf. adc-thin's ADC_FAULT).
pub const WDG_FAULT: u32 = 0xFFFF_FFFF;

const PHASE_SHIFT: u32 = 30;
const PSC_SHIFT: u32 = 12;
const PSC_MASK: u32 = 0x7; // 3 bits
const RLD_MASK: u32 = 0x0FFF; // 12 bits

#[inline]
fn ph_enc(p: Phase) -> u32 {
    match p {
        Phase::Idle => 0,
        Phase::Unlocked => 1,
        Phase::Configured => 2,
        Phase::Running => 3,
    }
}
#[inline]
fn ph_dec(b: u32) -> Phase {
    match b {
        0 => Phase::Idle,
        1 => Phase::Unlocked,
        2 => Phase::Configured,
        _ => Phase::Running,
    }
}
/// Pack an `Iwdg` into the scalar carried across the dissolve seam.
#[inline]
fn pack(w: Iwdg) -> u32 {
    (ph_enc(w.phase) << PHASE_SHIFT) | ((w.prescaler & PSC_MASK) << PSC_SHIFT) | (w.reload & RLD_MASK)
}
/// Unpack the scalar back into an `Iwdg`.
#[inline]
fn unpack(s: u32) -> Iwdg {
    Iwdg {
        phase: ph_dec(s >> PHASE_SHIFT),
        prescaler: (s >> PSC_SHIFT) & PSC_MASK,
        reload: s & RLD_MASK,
    }
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

/// Unlock write access (write 0x5555 to KR): Idle/Configured → Unlocked. Returns the packed
/// Unlocked state or `WDG_FAULT` if Running (or otherwise wrong-phase).
#[no_mangle]
pub extern "C" fn wdg_unlock(base: u32, state: u32) -> u32 {
    match unlock(unpack(state)) {
        Ok(next) => {
            wr(base + KR, KEY_ENABLE);
            pack(next)
        }
        Err(_) => WDG_FAULT,
    }
}

/// Stage prescaler + reload into PR/RLR — only while Unlocked. Spins on SR.PVU/RVU so the
/// writes are accepted, then writes the masked fields. Returns the packed state or
/// `WDG_FAULT` if write-protected.
#[no_mangle]
pub extern "C" fn wdg_configure(base: u32, state: u32, prescaler: u32, reload: u32) -> u32 {
    match set_config(unpack(state), prescaler, reload) {
        Ok(next) => {
            while rd(base + SR) & SR_PVU != 0 {}
            wr(base + PR, pr_bits(prescaler));
            while rd(base + SR) & SR_RVU != 0 {}
            wr(base + RLR, rlr_bits(reload));
            pack(next)
        }
        Err(_) => WDG_FAULT,
    }
}

/// Re-lock after configuration: Unlocked → Configured. No register write (the lock is
/// implicit once a non-key value settles); returns the packed state or `WDG_FAULT`.
#[no_mangle]
pub extern "C" fn wdg_lock(state: u32) -> u32 {
    match lock(unpack(state)) {
        Ok(next) => pack(next),
        Err(_) => WDG_FAULT,
    }
}

/// Start the watchdog (write 0xCCCC to KR): Configured → Running. IRREVERSIBLE. Returns the
/// packed Running state or `WDG_FAULT` if not Configured.
#[no_mangle]
pub extern "C" fn wdg_start(base: u32, state: u32) -> u32 {
    match start(unpack(state)) {
        Ok(next) => {
            wr(base + KR, KEY_START);
            pack(next)
        }
        Err(_) => WDG_FAULT,
    }
}

/// Refresh / kick the watchdog (write 0xAAAA to KR): Running → Running. Returns the packed
/// state or `WDG_FAULT` if not yet Running. This is the call the Health Monitor issues each
/// service cycle; missing it (a hung HM) lets the hardware reset fire — fail-to-safe.
#[no_mangle]
pub extern "C" fn wdg_refresh(base: u32, state: u32) -> u32 {
    match refresh(unpack(state)) {
        Ok(next) => {
            wr(base + KR, KEY_REFRESH);
            pack(next)
        }
        Err(_) => WDG_FAULT,
    }
}

/// True (1) once the watchdog is running (and can no longer be stopped in software), else 0.
#[no_mangle]
pub extern "C" fn wdg_is_running(state: u32) -> u32 {
    matches!(unpack(state).phase, Phase::Running) as u32
}

// ─────────────────────────────── Kani proofs ────────────────────────────────
//
// The safety properties over the full input space. Run: `cargo kani`.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    fn any_phase() -> Phase {
        match kani::any::<u8>() % 4 {
            0 => Phase::Idle,
            1 => Phase::Unlocked,
            2 => Phase::Configured,
            _ => Phase::Running,
        }
    }
    // Seam-reachable state: prescaler is a 3-bit field and reload a 12-bit field in the
    // packed u32, so `unpack` masks both. Modeling that is faithful — the ABI can never
    // hold wider values; the field-masking of *inputs* is tested in p5 with unconstrained args.
    fn any_iwdg() -> Iwdg {
        Iwdg { phase: any_phase(), prescaler: kani::any::<u32>() & PSC_MASK, reload: kani::any::<u32>() & RLD_MASK }
    }

    /// P1 — write-protection: `set_config` is accepted IFF the phase is Unlocked; from any
    /// other phase it is rejected with `WriteProtected` and the state is untouched. On success
    /// the staged fields are masked to their widths.
    #[kani::proof]
    fn p1_write_protection() {
        let w = any_iwdg();
        let psc: u32 = kani::any();
        let rlr: u32 = kani::any();
        match set_config(w, psc, rlr) {
            Ok(next) => {
                assert_eq!(w.phase, Phase::Unlocked);
                assert_eq!(next.phase, Phase::Unlocked);
                assert_eq!(next.prescaler, psc & 0x7);
                assert_eq!(next.reload, rlr & MAX_RELOAD);
            }
            Err(f) => {
                assert!(w.phase != Phase::Unlocked);
                assert_eq!(f, Fault::WriteProtected);
            }
        }
    }

    /// P2 — **cannot-un-start** (the IWDG invariant): applied to a Running watchdog, EVERY
    /// provided transition either leaves it Running (refresh) or is rejected without mutating
    /// — none returns a non-Running phase. There is no software path out of Running.
    #[kani::proof]
    fn p2_cannot_un_start() {
        let w = Iwdg { phase: Phase::Running, prescaler: kani::any::<u32>() & PSC_MASK, reload: kani::any::<u32>() & RLD_MASK };
        // every constructor that can succeed from Running keeps it Running
        if let Ok(n) = refresh(w) { assert_eq!(n.phase, Phase::Running); }
        // all others are rejected from Running (no state change, so no escape)
        assert!(unlock(w).is_err());
        assert!(set_config(w, kani::any(), kani::any()).is_err());
        assert!(lock(w).is_err());
        assert!(start(w).is_err());
        // therefore no provided transition yields a phase below Running
        assert!(is_running(w));
    }

    /// P3 — refresh-only-when-running: `refresh` is accepted IFF Running; before Running it is
    /// rejected (a kick has no effect on a watchdog that isn't counting). On success the whole
    /// state is preserved (a kick reloads the HW counter, not the FSM config).
    #[kani::proof]
    fn p3_refresh_only_running() {
        let w = any_iwdg();
        match refresh(w) {
            Ok(next) => {
                assert_eq!(w.phase, Phase::Running);
                assert_eq!(next, w);
            }
            Err(f) => {
                assert!(w.phase != Phase::Running);
                assert_eq!(f, Fault::WrongPhase);
            }
        }
    }

    /// P4 — start-once: `start` is accepted IFF Configured → Running; from Running it is
    /// rejected (no double-start), and from Idle/Unlocked rejected (must configure first).
    #[kani::proof]
    fn p4_start_once() {
        let w = any_iwdg();
        match start(w) {
            Ok(next) => {
                assert_eq!(w.phase, Phase::Configured);
                assert_eq!(next.phase, Phase::Running);
                assert_eq!(next.prescaler, w.prescaler); // config carried into Running
                assert_eq!(next.reload, w.reload);
            }
            Err(f) => {
                assert!(w.phase != Phase::Configured);
                assert_eq!(f, Fault::WrongPhase);
            }
        }
    }

    /// P5 — config bounds well-formed: for ANY input, the staged/emitted prescaler ≤ 7 and
    /// reload ≤ 4095 (masked), and the pure PR/RLR encoders never set a stray bit.
    #[kani::proof]
    fn p5_config_bounds() {
        let psc: u32 = kani::any();
        let rlr: u32 = kani::any();
        assert!(pr_bits(psc) <= MAX_PRESCALER);
        assert_eq!(pr_bits(psc) & !0x7, 0);
        assert!(rlr_bits(rlr) <= MAX_RELOAD);
        assert_eq!(rlr_bits(rlr) & !MAX_RELOAD, 0);
        // set_config from Unlocked stores only masked values
        let w = Iwdg { phase: Phase::Unlocked, prescaler: 0, reload: 0 };
        let n = set_config(w, psc, rlr).unwrap();
        assert!(n.prescaler <= MAX_PRESCALER);
        assert!(n.reload <= MAX_RELOAD);
    }

    /// P6 — pack/unpack round-trips every reachable state losslessly across the seam
    /// (phase + 3-bit prescaler + 12-bit reload).
    #[kani::proof]
    fn p6_pack_roundtrip() {
        let w = any_iwdg();
        let u = unpack(pack(w));
        assert_eq!(u.phase, w.phase);
        assert_eq!(u.prescaler, w.prescaler);
        assert_eq!(u.reload, w.reload);
    }

    /// P7 — unlock gates config (the key sequence): config only ever takes effect after an
    /// unlock. From Idle a `set_config` is write-protected; the accepted path is
    /// Idle → unlock → Unlocked → set_config, and no shortcut bypasses the unlock.
    #[kani::proof]
    fn p7_unlock_gates_config() {
        // straight from Idle, config is protected
        let idle = Iwdg::idle();
        assert_eq!(set_config(idle, kani::any(), kani::any()), Err(Fault::WriteProtected));
        // only after unlock does it take
        let unlocked = unlock(idle).unwrap();
        assert_eq!(unlocked.phase, Phase::Unlocked);
        assert!(set_config(unlocked, kani::any(), kani::any()).is_ok());
        // and unlock itself is refused once Running (can't re-open a live watchdog)
        let running = Iwdg { phase: Phase::Running, prescaler: 0, reload: 0 };
        assert_eq!(unlock(running), Err(Fault::WrongPhase));
    }
}

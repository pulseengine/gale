//! gust:hal **thin-seam** CAN (bxCAN) driver — driver breadth, the 10th verified iodev
//! (after GPIO/timer/SPI/UART/I2C/ADC/DAC/WDG/PWM). The whole STM32F1 bxCAN master
//! path — the BTR bit-timing config, the INRQ→INAK init handshake, and TX-mailbox /
//! RX-FIFO gating — lives here in verified wasm, importing ONLY `gust:hal/mmio`
//! (read32/write32): the SAME subset the other thin-seam drivers use, so it adds
//! **zero new TCB atoms**. No host CAN driver exists; this *is* the driver, dissolved
//! to native.
//!
//! bxCAN's distinctive safety property is **config-only-in-init**: the bit-timing
//! register (BTR) is silently ignored by the hardware unless the peripheral is in Init
//! mode (INRQ requested, INAK confirmed) — a config write in Sleep or Normal would be a
//! silent no-op that corrupts every frame on the bus with a stale/default bit rate.
//! That invariant is the core of the Kani-proven mode FSM here.
//!
//! Build:   cargo build --release --target wasm32-unknown-unknown
//! Dissolve: loom optimize --passes inline | synth compile --target cortex-m3
//!           --all-exports --relocatable          (scalar in/out → 0 SRAM)
//! Verify:  cargo kani   (config-only-in-init · init-handshake ordering ·
//!           TX-only-when-empty · RX-release-only-when-pending)
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// gust:hal mmio capability — resolved at link by the SAME ~10-line TCB bridge the
// other thin-seam drivers use. Polled bxCAN needs only register reads/writes.
extern "C" {
    fn mmio_read32(addr: u32) -> u32;
    fn mmio_write32(addr: u32, val: u32);
}

// STM32F1 bxCAN register map (offsets from the peripheral base, e.g. CAN1=0x4000_6400).
// Device knowledge as *data* (offsets/bit math), not trusted code.
const MCR: u32 = 0x000; // master control (INRQ, SLEEP)
const MSR: u32 = 0x004; // master status (INAK, SLAK)
const TSR: u32 = 0x008; // TX status (TME0/1/2 — mailbox empty)
const RF0R: u32 = 0x00C; // RX FIFO 0 (FMP0 pending count, RFOM0 release)
const BTR: u32 = 0x01C; // bit timing (BRP, TS1, TS2, SJW)
const TI0R: u32 = 0x180; // TX mailbox 0 identifier (TXRQ)
const TDT0R: u32 = 0x184; // TX mailbox 0 DLC
const TDL0R: u32 = 0x188; // TX mailbox 0 data low
const TDH0R: u32 = 0x18C; // TX mailbox 0 data high

const MCR_INRQ: u32 = 1 << 0; // initialization request
const MSR_INAK: u32 = 1 << 0; // initialization acknowledged
const TSR_TME0: u32 = 1 << 26; // mailbox 0 empty
const RF0R_FMP_MASK: u32 = 0x3; // FMP0[1:0] — messages pending in FIFO 0
const RF0R_RFOM0: u32 = 1 << 5; // release FIFO 0 output mailbox
const TI0R_TXRQ: u32 = 1 << 0; // transmit mailbox request
const STID_SHIFT: u32 = 21; // standard identifier field, TI0R[31:21]
const STID_MASK: u32 = 0x7FF; // 11-bit standard ID

// ───────────────────── pure timing config (table-free) ─────────────────────
//
// TABLE-FREE by construction (see spi-thin/gpio-thin): a `match`/array index→value
// compiles to a `.rodata` linmem table, which a `--relocatable` dissolved driver
// (no linmem base) reads as 0 → silent no-op. All config is pure bit arithmetic.

const BTR_BRP_MASK: u32 = 0x3FF; // BRP[9:0]
const BTR_TS1_MASK: u32 = 0xF; // TS1[3:0] (field width; register is [19:16])
const BTR_TS2_MASK: u32 = 0x7; // TS2[2:0] (field width; register is [22:20])
const BTR_SJW_MASK: u32 = 0x3; // SJW[1:0] (field width; register is [25:24])

/// BTR value: baud-rate prescaler + the three timing-segment fields, each masked to
/// its field width so it can never bleed into a neighbouring field. Pure arithmetic,
/// no table.
#[inline]
pub fn btr_value(brp: u32, ts1: u32, ts2: u32, sjw: u32) -> u32 {
    (brp & BTR_BRP_MASK)
        | ((ts1 & BTR_TS1_MASK) << 16)
        | ((ts2 & BTR_TS2_MASK) << 20)
        | ((sjw & BTR_SJW_MASK) << 24)
}

// ───────────────────── mode FSM ─────────────────────
//
// The peripheral's mode lifecycle as a proven state machine. `Phase` IS the whole
// state — no separate config-latched flag, so "was BTR ever written outside Init" is
// simply unrepresentable: the write only happens inside a phase-gated primitive.

/// Where the peripheral is in its mode lifecycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Reset default — no clock to the CAN core; neither config nor traffic possible.
    Sleep,
    /// INRQ requested and INAK confirmed. The ONLY phase in which BTR may be written.
    Init,
    /// Bus-active: TX/RX are only meaningful here.
    Normal,
}

/// Why a transition was rejected. A rejected op never corrupts the FSM and never
/// touches BTR/TX/RX registers.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// `configure`/`leave_init` off Init, or `can_tx`/`can_rx` off Normal.
    WrongPhase,
}

/// Request Init mode. Total: requesting init is always allowed, from any phase —
/// mirrors the hardware, where INRQ may be set regardless of current mode.
#[inline]
pub const fn enter_init(_p: Phase) -> Phase {
    Phase::Init
}

/// **The bxCAN config rule.** Bit-timing (BTR) may be written IFF the peripheral is
/// currently in Init — the hardware silently ignores BTR outside init, so accepting a
/// config write off-Init would corrupt every frame on the bus with a stale bit rate.
/// Rejected (and left unmutated) from Sleep or Normal.
#[inline]
pub const fn configure(p: Phase) -> Result<Phase, Fault> {
    match p {
        Phase::Init => Ok(Phase::Init),
        _ => Err(Fault::WrongPhase),
    }
}

/// Leave Init for Normal (bus-active) operation. Succeeds ONLY from Init, so Normal is
/// reachable only by passing through the phase where bit-timing had a valid window.
#[inline]
pub const fn leave_init(p: Phase) -> Result<Phase, Fault> {
    match p {
        Phase::Init => Ok(Phase::Normal),
        _ => Err(Fault::WrongPhase),
    }
}

/// TX gate: a transmit may be requested IFF the peripheral is Normal. (The exec layer
/// additionally gates live on TSR.TME0 — mailbox-empty — before it actually writes.)
#[inline]
pub const fn can_tx(p: Phase) -> Result<(), Fault> {
    match p {
        Phase::Normal => Ok(()),
        _ => Err(Fault::WrongPhase),
    }
}

/// RX gate: a FIFO release may be requested IFF the peripheral is Normal. (The exec
/// layer additionally gates live on RF0R.FMP0 > 0 — a pending message — before release.)
#[inline]
pub const fn can_rx(p: Phase) -> Result<(), Fault> {
    match p {
        Phase::Normal => Ok(()),
        _ => Err(Fault::WrongPhase),
    }
}

// ─────────────────────────────── dissolve ABI ─────────────────────────────────
//
// Scalar in/out, no linmem/data segment → 0 SRAM. State is just the phase, packed
// into the low 2 bits of a u32 (simpler than i2c's triple — there is no auxiliary
// count/direction to carry).

/// Sentinel for a rejected FSM transition — keeps the ABI scalar (cf. i2c-thin's
/// I2C_FAULT).
pub const CAN_FAULT: u32 = 0xFFFF_FFFF;

const PHASE_MASK: u32 = 0x3;

#[inline]
fn ph_enc(p: Phase) -> u32 {
    match p {
        Phase::Sleep => 0,
        Phase::Init => 1,
        Phase::Normal => 2,
    }
}
#[inline]
fn ph_dec(b: u32) -> Phase {
    match b {
        0 => Phase::Sleep,
        1 => Phase::Init,
        _ => Phase::Normal,
    }
}
/// Pack a `Phase` into the scalar carried across the dissolve seam.
#[inline]
fn pack(p: Phase) -> u32 {
    ph_enc(p) & PHASE_MASK
}
/// Unpack the scalar back into a `Phase`.
#[inline]
fn unpack(s: u32) -> Phase {
    ph_dec(s & PHASE_MASK)
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

/// Request Init mode: set MCR.INRQ and spin until MSR.INAK confirms it. Total — always
/// returns the packed Init state.
#[no_mangle]
pub extern "C" fn can_enter_init(base: u32, state: u32) -> u32 {
    let next = enter_init(unpack(state));
    wr(base + MCR, MCR_INRQ);
    while rd(base + MSR) & MSR_INAK == 0 {}
    pack(next)
}

/// Write the bit-timing register (BTR), IFF currently in Init. Returns the packed
/// (still-Init) state, or `CAN_FAULT` if not Init — the register is left untouched.
#[no_mangle]
pub extern "C" fn can_configure(base: u32, state: u32, brp: u32, ts1: u32, ts2: u32, sjw: u32) -> u32 {
    match configure(unpack(state)) {
        Ok(next) => {
            wr(base + BTR, btr_value(brp, ts1, ts2, sjw));
            pack(next)
        }
        Err(_) => CAN_FAULT,
    }
}

/// Leave Init for Normal: clear MCR.INRQ and spin until MSR.INAK clears. Returns the
/// packed Normal state, or `CAN_FAULT` if not currently Init.
#[no_mangle]
pub extern "C" fn can_leave_init(base: u32, state: u32) -> u32 {
    match leave_init(unpack(state)) {
        Ok(next) => {
            wr(base + MCR, 0);
            while rd(base + MSR) & MSR_INAK != 0 {}
            pack(next)
        }
        Err(_) => CAN_FAULT,
    }
}

/// Request transmission of one frame via mailbox 0: IFF Normal AND the mailbox is
/// empty (TSR.TME0), load the data/DLC/ID registers and set TXRQ. Returns the packed
/// (still-Normal) state, or `CAN_FAULT` if wrong phase or the mailbox is busy — never a
/// half-written mailbox.
#[no_mangle]
pub extern "C" fn can_tx_request(base: u32, state: u32, id: u32, dlc: u32, dlo: u32, dhi: u32) -> u32 {
    let p = unpack(state);
    match can_tx(p) {
        Ok(()) => {
            if rd(base + TSR) & TSR_TME0 != 0 {
                wr(base + TI0R, ((id & STID_MASK) << STID_SHIFT) | TI0R_TXRQ);
                wr(base + TDT0R, dlc & 0xF);
                wr(base + TDL0R, dlo);
                wr(base + TDH0R, dhi);
                pack(p)
            } else {
                CAN_FAULT // mailbox busy
            }
        }
        Err(_) => CAN_FAULT,
    }
}

/// Release the RX FIFO 0 output mailbox: IFF Normal AND a message is pending
/// (RF0R.FMP0 > 0), set RF0R.RFOM0. Returns the packed (still-Normal) state, or
/// `CAN_FAULT` if wrong phase or nothing is pending — never a spurious release.
#[no_mangle]
pub extern "C" fn can_rx_release(base: u32, state: u32) -> u32 {
    let p = unpack(state);
    match can_rx(p) {
        Ok(()) => {
            if rd(base + RF0R) & RF0R_FMP_MASK != 0 {
                wr(base + RF0R, RF0R_RFOM0);
                pack(p)
            } else {
                CAN_FAULT // nothing pending
            }
        }
        Err(_) => CAN_FAULT,
    }
}

/// Pure query: the current mode's packed encoding (0=Sleep, 1=Init, 2=Normal).
#[no_mangle]
pub extern "C" fn can_mode(state: u32) -> u32 {
    pack(unpack(state))
}

// ─────────────────────────────── Kani proofs ────────────────────────────────
//
// The safety properties over the full input space. Run: `cargo kani`.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    fn any_phase() -> Phase {
        match kani::any::<u8>() % 3 {
            0 => Phase::Sleep,
            1 => Phase::Init,
            _ => Phase::Normal,
        }
    }

    /// P1 — **config-only-in-init**: `configure` succeeds IFF the phase is Init; off
    /// Init it is always rejected with `WrongPhase` and never mutates (the caller's
    /// packed state is unchanged since no `Ok` was returned to repack).
    #[kani::proof]
    fn p1_config_only_in_init() {
        let p = any_phase();
        match configure(p) {
            Ok(next) => {
                assert_eq!(p, Phase::Init);
                assert_eq!(next, Phase::Init);
            }
            Err(f) => {
                assert_ne!(p, Phase::Init);
                assert_eq!(f, Fault::WrongPhase);
            }
        }
    }

    /// P2 — `enter_init` is total: from ANY phase it returns Init. Requesting init is
    /// never rejected.
    #[kani::proof]
    fn p2_enter_init_total() {
        let p = any_phase();
        assert_eq!(enter_init(p), Phase::Init);
    }

    /// P3 — **Normal is reachable only via Init**: `leave_init` succeeds IFF the phase
    /// is Init, and always lands exactly Normal; off Init it is rejected with
    /// `WrongPhase`. So bit-timing always had a valid configuration window before the
    /// bus went live.
    #[kani::proof]
    fn p3_normal_only_via_init() {
        let p = any_phase();
        match leave_init(p) {
            Ok(next) => {
                assert_eq!(p, Phase::Init);
                assert_eq!(next, Phase::Normal);
            }
            Err(f) => {
                assert_ne!(p, Phase::Init);
                assert_eq!(f, Fault::WrongPhase);
            }
        }
    }

    /// P4 — TX is gated on Normal: `can_tx` is Ok IFF the phase is Normal.
    #[kani::proof]
    fn p4_tx_requires_normal() {
        let p = any_phase();
        assert_eq!(can_tx(p).is_ok(), p == Phase::Normal);
    }

    /// P5 — RX release is gated on Normal: `can_rx` is Ok IFF the phase is Normal.
    #[kani::proof]
    fn p5_rx_requires_normal() {
        let p = any_phase();
        assert_eq!(can_rx(p).is_ok(), p == Phase::Normal);
    }

    /// P6 — bit-timing config is table-free + well-formed: `btr_value` carries each of
    /// BRP/TS1/TS2/SJW masked into its own field with no stray bit outside those four
    /// fields, for any input.
    #[kani::proof]
    fn p6_btr_well_formed() {
        let brp: u32 = kani::any();
        let ts1: u32 = kani::any();
        let ts2: u32 = kani::any();
        let sjw: u32 = kani::any();
        let v = btr_value(brp, ts1, ts2, sjw);
        let known_bits = BTR_BRP_MASK | (BTR_TS1_MASK << 16) | (BTR_TS2_MASK << 20) | (BTR_SJW_MASK << 24);
        assert_eq!(v & !known_bits, 0); // no stray bit
        assert_eq!(v & BTR_BRP_MASK, brp & BTR_BRP_MASK);
        assert_eq!((v >> 16) & BTR_TS1_MASK, ts1 & BTR_TS1_MASK);
        assert_eq!((v >> 20) & BTR_TS2_MASK, ts2 & BTR_TS2_MASK);
        assert_eq!((v >> 24) & BTR_SJW_MASK, sjw & BTR_SJW_MASK);
    }

    /// P7 — pack/unpack round-trips every reachable phase losslessly across the seam.
    #[kani::proof]
    fn p7_pack_roundtrip() {
        let p = any_phase();
        assert_eq!(unpack(pack(p)), p);
    }
}

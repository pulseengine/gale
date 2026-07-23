//! gust:hal **thin-seam** ADC driver — driver breadth (the 6th verified iodev after
//! GPIO/timer/SPI/UART/I2C). The whole STM32F1 ADC single-conversion path — the
//! sample-time (SMPR) + regular-sequence (SQR) config and the
//! enable→start→EOC→read master conversion cycle — lives here in verified wasm,
//! importing ONLY `gust:hal/mmio` (read32/write32): the SAME subset the other
//! thin-seam drivers use, so it adds **zero new TCB atoms**. No host ADC driver
//! exists; this *is* the driver, dissolved to native.
//!
//! ADC's distinctive safety property is **read-after-EOC, exactly-once**: the data
//! register may be read only after End-Of-Conversion, and reading it consumes the
//! sample (EOC clears on read). Read early → a stale/garbage sample; read twice or
//! let the ADC free-run (CONT=1) → a torn or wrong value silently used as a control
//! input. The FSM makes "one start → one EOC → one read → back to Ready" a checkable
//! function of the state, and single-shot (never Converting again without an explicit
//! start) provable — the ADC analog of I2C's ACK-all-but-last.
//!
//! Build:   cargo build --release --target wasm32-unknown-unknown
//! Dissolve: loom optimize --passes inline | synth compile --target cortex-m3
//!           --all-exports --relocatable          (scalar in/out → 0 SRAM)
//! Verify:  cargo kani   (channel-bounds · read-after-EOC · single-shot · phase-gating
//!                        · disable-total · pack-roundtrip · config-well-formed)
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// gust:hal mmio capability — resolved at link by the SAME ~10-line TCB bridge the
// other thin-seam drivers use. Polled single-conversion needs only reads/writes.
extern "C" {
    fn mmio_read32(addr: u32) -> u32;
    fn mmio_write32(addr: u32, val: u32);
}

// STM32F1 ADC register map (offsets from the peripheral base, e.g. ADC1=0x4001_2400).
// Device knowledge as *data* (offsets/bit math), not trusted code.
const SR: u32 = 0x00; // status (EOC, STRT)
const CR2: u32 = 0x08; // control 2 (ADON, CONT, ALIGN, SWSTART)
const SMPR1: u32 = 0x0C; // sample time, channels 10..17
const SMPR2: u32 = 0x10; // sample time, channels 0..9
const SQR1: u32 = 0x2C; // regular sequence 1 (L[3:0] = length-1)
const SQR3: u32 = 0x34; // regular sequence 3 (SQ1[4:0] = first channel)
const DR: u32 = 0x4C; // data (12-bit result)

const SR_EOC: u32 = 1 << 1; // end of conversion

const CR2_ADON: u32 = 1 << 0; // A/D on
const CR2_CONT: u32 = 1 << 1; // continuous conversion (MUST be 0 for single-shot)
const CR2_SWSTART: u32 = 1 << 22; // start conversion of regular channels

const DR_MASK: u32 = 0x0FFF; // 12-bit right-aligned result

/// Highest valid regular channel on STM32F1: 0..=15 external, 16 = temperature,
/// 17 = V_REFINT. Selecting above this reads an undefined mux input.
pub const MAX_CHANNEL: u32 = 17;

// ───────────────────── pure config (table-free) ─────────────────────
//
// TABLE-FREE by construction (see spi-thin/i2c-thin): a `match`/array index→value
// compiles to a `.rodata` linmem table, which a `--relocatable` dissolved driver
// (no linmem base) reads as 0 → silent no-op. All config is pure bit arithmetic.

/// SQR3 SQ1[4:0] = the single regular channel to convert, masked to 5 bits so it can
/// never overflow the field. Pure mask, no table.
#[inline]
pub fn sqr3_channel(channel: u32) -> u32 {
    channel & 0x1F
}

/// SQR1 L[3:0] = (sequence length − 1). Single-conversion is length 1 → 0. `len` is
/// clamped to 1..=16 then encoded; masked to the 4-bit field at bit 20. Pure arithmetic.
#[inline]
pub fn sqr1_length(len: u32) -> u32 {
    let n = if len == 0 { 1 } else if len > 16 { 16 } else { len };
    ((n - 1) & 0xF) << 20
}

/// Sample-time field value for `channel`, given a 3-bit sample-code (0..=7). Each
/// channel owns a 3-bit field; channels 0..9 live in SMPR2 and 10..17 in SMPR1, both
/// indexed by `(channel % 10) * 3`. Returns the value to OR into the SMPRx the caller
/// picks by `channel < 10`. Pure shift arithmetic — the shift is at most 27, so it
/// never overflows the 32-bit register.
#[inline]
pub fn smpr_bits(channel: u32, code: u32) -> u32 {
    (code & 0x7) << ((channel % 10) * 3)
}

/// Which SMPR register offset holds `channel`'s sample-time field: SMPR2 for 0..9,
/// SMPR1 for 10..17. Pure branch, no table.
#[inline]
pub fn smpr_reg(channel: u32) -> u32 {
    if channel < 10 { SMPR2 } else { SMPR1 }
}

// ───────────────────── single-conversion FSM ─────────────────────
//
// The conversion lifecycle as a proven state machine. `Phase` IS the state; `channel`
// records the selected mux input; `sample` carries the last 12-bit result across the
// seam. Single-shot: a completed conversion returns to Ready, never re-enters
// Converting without an explicit `begin`.

/// Where a single conversion is in its lifecycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// ADC powered down (ADON = 0). No conversion possible until `enable`.
    Off,
    /// Powered + configured, no conversion in flight. A new `begin` may be issued.
    Ready,
    /// SWSTART issued; awaiting EOC. The DR is NOT yet valid — reading it here is
    /// the stale-sample bug the FSM forbids.
    Converting,
    /// EOC observed; exactly one DR read (`read`) is due, which consumes the sample.
    Complete,
}

/// Why a transition was rejected. A rejected op never corrupts the FSM.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// `enable` with a channel above `MAX_CHANNEL`.
    BadChannel,
    /// A transition issued from the wrong phase (`begin` off-Ready, `complete`
    /// off-Converting, `read` off-Complete).
    WrongPhase,
}

/// A conversion's state: phase + selected channel + last sample. `channel` is
/// meaningful from Ready onward; `sample` is only written by `read`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Adc {
    pub phase: Phase,
    pub channel: u32,
    pub sample: u32,
}

impl Adc {
    /// The powered-down ADC.
    pub const fn off() -> Self {
        Adc { phase: Phase::Off, channel: 0, sample: 0 }
    }
}

/// Ready predicate: a new conversion may `begin` **iff** the ADC is Ready.
#[inline]
pub const fn is_ready(a: Adc) -> bool {
    matches!(a.phase, Phase::Ready)
}

/// Power on + select `channel`. Succeeds only from Off with a valid channel; lands
/// Ready with the channel latched and the sample cleared.
#[inline]
pub const fn enable(a: Adc, channel: u32) -> Result<Adc, Fault> {
    match a.phase {
        Phase::Off if channel <= MAX_CHANNEL => {
            Ok(Adc { phase: Phase::Ready, channel, sample: 0 })
        }
        Phase::Off => Err(Fault::BadChannel),
        _ => Err(Fault::WrongPhase),
    }
}

/// Start one conversion (SWSTART): Ready → Converting. Rejected from any other phase.
/// The channel is preserved; the previous sample is left untouched until `read`.
#[inline]
pub const fn begin(a: Adc) -> Result<Adc, Fault> {
    match a.phase {
        Phase::Ready => Ok(Adc { phase: Phase::Converting, channel: a.channel, sample: a.sample }),
        _ => Err(Fault::WrongPhase),
    }
}

/// EOC observed: Converting → Complete. Rejected from any other phase — you cannot
/// declare a conversion complete that was never started. Sample still unread here.
#[inline]
pub const fn complete(a: Adc) -> Result<Adc, Fault> {
    match a.phase {
        Phase::Converting => Ok(Adc { phase: Phase::Complete, channel: a.channel, sample: a.sample }),
        _ => Err(Fault::WrongPhase),
    }
}

/// **The read-after-EOC rule.** Consume the conversion result: from Complete only,
/// store the 12-bit `raw` DR value into `sample` and return to Ready. Rejected from
/// any other phase — reading before EOC (Converting) or twice (already Ready) is
/// exactly the stale/torn-sample bug this forbids. Single-shot: lands Ready, NOT
/// Converting, so the ADC never free-runs.
#[inline]
pub const fn read(a: Adc, raw: u32) -> Result<Adc, Fault> {
    match a.phase {
        Phase::Complete => Ok(Adc { phase: Phase::Ready, channel: a.channel, sample: raw & DR_MASK }),
        _ => Err(Fault::WrongPhase),
    }
}

/// Power down / teardown: return to Off from ANY state — never leaves a conversion
/// wedged. Total; defined for every state. Clears the latched sample.
#[inline]
pub const fn disable(_a: Adc) -> Adc {
    Adc::off()
}

// ───────────────────────────── dissolve ABI ─────────────────────────────────
//
// Scalar in/out, no linmem/data segment → 0 SRAM. State is carried by the caller as a
// packed u32: phase in bits[31:30], channel (0..17) in bits[29:25], sample (12-bit) in
// bits[11:0]. All three fit with room to spare.

/// Sentinel for a rejected FSM transition — keeps the ABI scalar (cf. i2c-thin's I2C_FAULT).
pub const ADC_FAULT: u32 = 0xFFFF_FFFF;

const PHASE_SHIFT: u32 = 30;
const CHAN_SHIFT: u32 = 25;
const CHAN_MASK: u32 = 0x1F; // 5 bits (0..31, covers 0..17)
const SAMPLE_MASK: u32 = 0x0FFF; // 12 bits

#[inline]
fn ph_enc(p: Phase) -> u32 {
    match p {
        Phase::Off => 0,
        Phase::Ready => 1,
        Phase::Converting => 2,
        Phase::Complete => 3,
    }
}
#[inline]
fn ph_dec(b: u32) -> Phase {
    match b {
        0 => Phase::Off,
        1 => Phase::Ready,
        2 => Phase::Converting,
        _ => Phase::Complete,
    }
}
/// Pack an `Adc` into the scalar carried across the dissolve seam.
#[inline]
fn pack(a: Adc) -> u32 {
    (ph_enc(a.phase) << PHASE_SHIFT) | ((a.channel & CHAN_MASK) << CHAN_SHIFT) | (a.sample & SAMPLE_MASK)
}
/// Unpack the scalar back into an `Adc`.
#[inline]
fn unpack(s: u32) -> Adc {
    Adc {
        phase: ph_dec(s >> PHASE_SHIFT),
        channel: (s >> CHAN_SHIFT) & CHAN_MASK,
        sample: s & SAMPLE_MASK,
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

/// Configure the ADC at `base` for a single-channel single conversion: sample time
/// (SMPRx picked by channel) and regular sequence (SQR3 channel, SQR1 length 1).
/// Table-free: every value is bit arithmetic.
#[no_mangle]
pub extern "C" fn adc_configure(base: u32, channel: u32, sample_code: u32) {
    wr(base + smpr_reg(channel), smpr_bits(channel, sample_code));
    wr(base + SQR3, sqr3_channel(channel));
    wr(base + SQR1, sqr1_length(1));
    // Register config ONLY — does NOT touch CR2. ADON was set by `adc_enable`
    // (and stays set), so `adc_configure` writing ADON again would, on real F1,
    // trigger a spurious conversion (a repeated ADON-write-while-on starts a
    // conversion). Leaving CR2 alone here makes the conversion a strict
    // single-shot: exactly one SWSTART in `adc_start`. CONT stays 0 throughout
    // (never set by enable/configure/start), so single-shot is preserved. gale#216.
}

/// Power on + select `channel`: Off → Ready. Returns the packed Ready state or
/// `ADC_FAULT` (bad channel / wrong phase). Sets CR2.ADON. `cr2_extra` = caller-
/// supplied CR2 bits OR'd into the managed write (e.g. `TSVREFE` (1<<23) to read F1
/// internal channels Vrefint/temp; also ALIGN, EXTTRIG). The managed bits (ADON,
/// CONT=0) stay authoritative — gale#216.
#[no_mangle]
pub extern "C" fn adc_enable(base: u32, state: u32, channel: u32, cr2_extra: u32) -> u32 {
    match enable(unpack(state), channel) {
        Ok(next) => {
            wr(base + CR2, CR2_ADON | cr2_extra);
            pack(next)
        }
        Err(_) => ADC_FAULT,
    }
}

/// Start one conversion (SWSTART): Ready → Converting. Returns the packed Converting
/// state or `ADC_FAULT` if not Ready. Sets CR2.SWSTART (keeps ADON, CONT stays 0).
/// `cr2_extra` = caller-supplied CR2 bits OR'd into the managed write (e.g.
/// `TSVREFE` (1<<23) to read F1 internal channels Vrefint/temp; also ALIGN,
/// EXTTRIG). The managed bits (ADON, SWSTART, CONT=0) stay authoritative — gale#216.
#[no_mangle]
pub extern "C" fn adc_start(base: u32, state: u32, cr2_extra: u32) -> u32 {
    match begin(unpack(state)) {
        Ok(next) => {
            wr(base + CR2, CR2_ADON | CR2_SWSTART | cr2_extra);
            pack(next)
        }
        Err(_) => ADC_FAULT,
    }
}

/// Poll to completion: spin until SR.EOC, then Converting → Complete. Returns the
/// packed Complete state or `ADC_FAULT` if not Converting. Does NOT read DR (EOC
/// stays set for `adc_read` to consume).
#[no_mangle]
pub extern "C" fn adc_poll(base: u32, state: u32) -> u32 {
    match complete(unpack(state)) {
        Ok(next) => {
            while rd(base + SR) & SR_EOC == 0 {}
            pack(next)
        }
        Err(_) => ADC_FAULT,
    }
}

/// Consume the result: from Complete, read DR (clears EOC) and store the 12-bit
/// sample into the packed state, returning to Ready. Returns the packed next state
/// (with the sample embedded) or `ADC_FAULT` if not Complete. Reading off-Complete —
/// the stale/torn-sample bug — is rejected here.
#[no_mangle]
pub extern "C" fn adc_read(base: u32, state: u32) -> u32 {
    let a = unpack(state);
    match read(a, rd(base + DR)) {
        Ok(next) => pack(next),
        Err(_) => ADC_FAULT,
    }
}

/// Extract the latched 12-bit sample from a packed state (0..4095). Pure query — the
/// value `adc_read` stored, for the caller to use as a control input.
#[no_mangle]
pub extern "C" fn adc_sample(state: u32) -> u32 {
    unpack(state).sample
}

/// True (1) once EOC has been observed and the sample is due to be read, else 0.
#[no_mangle]
pub extern "C" fn adc_is_complete(state: u32) -> u32 {
    matches!(unpack(state).phase, Phase::Complete) as u32
}

/// Power down: drive CR2 to 0 (ADON off) and return the ADC to Off from any state.
#[no_mangle]
pub extern "C" fn adc_disable(base: u32, state: u32) -> u32 {
    wr(base + CR2, 0);
    pack(disable(unpack(state)))
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
            1 => Phase::Ready,
            2 => Phase::Converting,
            _ => Phase::Complete,
        }
    }
    fn any_adc() -> Adc {
        Adc { phase: any_phase(), channel: kani::any(), sample: kani::any() }
    }

    /// P1 — channel bounds: `enable` succeeds IFF the ADC is Off and the channel is
    /// within range; the selected channel is exactly the requested one and never
    /// exceeds `MAX_CHANNEL`. An out-of-range channel is always rejected, never latched.
    #[kani::proof]
    fn p1_channel_bounds() {
        let a = any_adc();
        let ch: u32 = kani::any();
        match enable(a, ch) {
            Ok(next) => {
                assert_eq!(a.phase, Phase::Off);
                assert!(ch <= MAX_CHANNEL);
                assert_eq!(next.phase, Phase::Ready);
                assert_eq!(next.channel, ch);
                assert!(next.channel <= MAX_CHANNEL);
                assert_eq!(next.sample, 0);
            }
            Err(Fault::BadChannel) => {
                assert_eq!(a.phase, Phase::Off);
                assert!(ch > MAX_CHANNEL);
            }
            Err(Fault::WrongPhase) => {
                assert!(a.phase != Phase::Off);
            }
        }
        assert_eq!(is_ready(a), a.phase == Phase::Ready);
    }

    /// P2 — read-after-EOC only: `read` is accepted IFF the phase is Complete; from
    /// anywhere else it is rejected and the state is untouched. On success it lands
    /// Ready with the 12-bit-masked sample — never a wider value.
    #[kani::proof]
    fn p2_read_after_eoc() {
        let a = any_adc();
        let raw: u32 = kani::any();
        match read(a, raw) {
            Ok(next) => {
                assert_eq!(a.phase, Phase::Complete);
                assert_eq!(next.phase, Phase::Ready);
                assert_eq!(next.sample, raw & DR_MASK);
                assert!(next.sample <= DR_MASK);
                assert_eq!(next.channel, a.channel); // channel retained
            }
            Err(f) => {
                assert!(a.phase != Phase::Complete);
                assert_eq!(f, Fault::WrongPhase);
            }
        }
    }

    /// P3 — **single-shot** (the ADC invariant): the only path back to Converting is
    /// an explicit `begin` from Ready. A completed conversion `read`s to Ready, never
    /// to Converting, so the ADC never free-runs; and one `begin` yields exactly one
    /// Converting. Also: `sample` changes ONLY on `read` — begin/complete carry it
    /// through untouched (no torn value mid-conversion).
    #[kani::proof]
    fn p3_single_shot() {
        let a = any_adc();
        // read never re-enters Converting
        if let Ok(next) = read(a, kani::any()) {
            assert_eq!(next.phase, Phase::Ready);
            assert!(next.phase != Phase::Converting);
        }
        // begin is the ONLY producer of Converting, and only from Ready
        if let Ok(next) = begin(a) {
            assert_eq!(a.phase, Phase::Ready);
            assert_eq!(next.phase, Phase::Converting);
            assert_eq!(next.sample, a.sample); // sample untouched by begin
            assert_eq!(next.channel, a.channel);
        }
        // complete carries the sample through untouched (only read writes it)
        if let Ok(next) = complete(a) {
            assert_eq!(a.phase, Phase::Converting);
            assert_eq!(next.phase, Phase::Complete);
            assert_eq!(next.sample, a.sample);
            assert_eq!(next.channel, a.channel);
        }
    }

    /// P4 — phase gating: `begin` requires Ready, `complete` requires Converting,
    /// `read` requires Complete; each off-phase op is rejected and corrupts nothing.
    #[kani::proof]
    fn p4_phase_gating() {
        let a = any_adc();
        if a.phase != Phase::Ready {
            assert_eq!(begin(a), Err(Fault::WrongPhase));
        }
        if a.phase != Phase::Converting {
            assert_eq!(complete(a), Err(Fault::WrongPhase));
        }
        if a.phase != Phase::Complete {
            assert_eq!(read(a, kani::any()), Err(Fault::WrongPhase));
        }
    }

    /// P5 — disable is total and always powers down to Off (never wedged).
    #[kani::proof]
    fn p5_disable_total() {
        let a = any_adc();
        let n = disable(a);
        assert_eq!(n.phase, Phase::Off);
        assert_eq!(n.channel, 0);
        assert_eq!(n.sample, 0);
    }

    /// P6 — pack/unpack round-trips every reachable state losslessly across the seam
    /// (phase + channel + 12-bit sample).
    #[kani::proof]
    fn p6_pack_roundtrip() {
        let a = any_adc();
        kani::assume(a.channel <= CHAN_MASK);
        kani::assume(a.sample <= SAMPLE_MASK);
        let u = unpack(pack(a));
        assert_eq!(u.phase, a.phase);
        assert_eq!(u.channel, a.channel);
        assert_eq!(u.sample, a.sample);
    }

    /// P7 — config is table-free + well-formed: SQR3 channel ≤ 5 bits, SQR1 length
    /// lands in the 4-bit L field at bit 20, sample-time is a 3-bit field that never
    /// escapes its register, for any input.
    #[kani::proof]
    fn p7_config_well_formed() {
        let ch: u32 = kani::any();
        let len: u32 = kani::any();
        let code: u32 = kani::any();
        // SQR3 channel: 5 bits, no stray
        assert_eq!(sqr3_channel(ch) & !0x1F, 0);
        // SQR1 length: only the 4-bit field at [23:20] is ever set
        assert_eq!(sqr1_length(len) & !(0xF << 20), 0);
        // sample-time: 3-bit code, shift at most 27 → never overflows 32 bits, and
        // only 3 contiguous bits are set
        let s = smpr_bits(ch, code);
        let shift = (ch % 10) * 3;
        assert!(shift <= 27);
        assert_eq!(s, (code & 0x7) << shift);
        // smpr_reg picks exactly one of the two registers
        let r = smpr_reg(ch);
        assert!(r == SMPR1 || r == SMPR2);
    }
}

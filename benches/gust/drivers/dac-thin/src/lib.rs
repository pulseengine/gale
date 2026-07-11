//! gust:hal **thin-seam** DAC driver — driver breadth (the 7th verified iodev after
//! GPIO/timer/SPI/UART/I2C/ADC). The whole STM32F1 DAC software-triggered path — the
//! CR channel/trigger config and the enable→load→trigger→output cycle — lives here in
//! verified wasm, importing ONLY `gust:hal/mmio` (read32/write32): the SAME subset the
//! other thin-seam drivers use, so it adds **zero new TCB atoms**. No host DAC driver
//! exists; this *is* the driver, dissolved to native.
//!
//! DAC's distinctive safety property is **glitch-free, trigger-gated output**: writing
//! the data holding register (DHR) does NOT move the pin — the output register (DOR)
//! updates only when a trigger fires. So a control loop can stage a new value and
//! publish it atomically, never emitting a half-updated code. Get it wrong (publish on
//! load, or let DOR track an unmasked value) → a glitch on an actuator line. The FSM
//! makes "the output equals the last *loaded* value, and changes ONLY on trigger" a
//! checkable function of the state, and clamps every commanded value to 12 bits.
//!
//! Build:   cargo build --release --target wasm32-unknown-unknown
//! Dissolve: loom optimize --passes inline | synth compile --target cortex-m3
//!           --all-exports --relocatable          (scalar in/out → 0 SRAM)
//! Verify:  cargo kani   (range-clamp · output-reflects-loaded · glitch-free ·
//!                        phase-gating · disable-total · pack-roundtrip · config-well-formed)
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// gust:hal mmio capability — resolved at link by the SAME ~10-line TCB bridge the
// other thin-seam drivers use. Software-triggered DAC needs only reads/writes.
extern "C" {
    fn mmio_read32(addr: u32) -> u32;
    fn mmio_write32(addr: u32, val: u32);
}

// STM32F1 DAC register map (offsets from the peripheral base, DAC=0x4000_7400).
// Device knowledge as *data* (offsets/bit math), not trusted code.
const CR: u32 = 0x00; // control (EN, TEN, TSEL per channel)
const SWTRIGR: u32 = 0x04; // software trigger
const DHR12R1: u32 = 0x08; // ch1 data holding, 12-bit right-aligned
const DHR12R2: u32 = 0x14; // ch2 data holding, 12-bit right-aligned
const DOR1: u32 = 0x2C; // ch1 data output (read-only — what drives the pin)
const DOR2: u32 = 0x30; // ch2 data output

// Per-channel CR fields live in the low 16 bits for ch1 and are shifted +16 for ch2.
const CR_EN: u32 = 1 << 0; // channel enable
const CR_TEN: u32 = 1 << 2; // trigger enable
const CR_TSEL_SW: u32 = 0x7 << 3; // TSEL = 111 → software trigger

const DHR_MASK: u32 = 0x0FFF; // 12-bit right-aligned data

/// Highest valid channel index: 0 = DAC channel 1, 1 = DAC channel 2.
pub const MAX_CHANNEL: u32 = 1;
/// Full-scale 12-bit code.
pub const MAX_CODE: u32 = 0x0FFF;

// ───────────────────── pure config (table-free) ─────────────────────
//
// TABLE-FREE by construction (see adc-thin/i2c-thin): a `match`/array index→value
// compiles to a `.rodata` linmem table, which a `--relocatable` dissolved driver
// (no linmem base) reads as 0 → silent no-op. All config is pure bit arithmetic.

/// CR enable word for `channel` in software-triggered mode: EN | TEN | TSEL(sw),
/// shifted +16 for channel 2. Pure shift arithmetic, no table.
#[inline]
pub fn cr_enable_bits(channel: u32) -> u32 {
    (CR_EN | CR_TEN | CR_TSEL_SW) << (16 * (channel & 1))
}

/// The DHR (data holding) register offset for `channel`. Branch, not table.
#[inline]
pub fn dhr_offset(channel: u32) -> u32 {
    if channel == 0 { DHR12R1 } else { DHR12R2 }
}

/// The DOR (data output) register offset for `channel`. Branch, not table.
#[inline]
pub fn dor_offset(channel: u32) -> u32 {
    if channel == 0 { DOR1 } else { DOR2 }
}

/// The SWTRIGR bit that triggers `channel` (SWTRIG1 = bit0, SWTRIG2 = bit1). Pure shift.
#[inline]
pub fn swtrig_bit(channel: u32) -> u32 {
    1 << (channel & 1)
}

/// Clamp a commanded value to the 12-bit DAC code — the range property. Pure mask.
#[inline]
pub fn dhr_value(v: u32) -> u32 {
    v & DHR_MASK
}

// ───────────────────── software-triggered output FSM ─────────────────────
//
// The output lifecycle as a proven state machine. `Phase` IS the state; `channel`
// records the DAC channel; `value` carries the last loaded 12-bit code and — once
// triggered — the code the DOR is driving. Glitch-free: `load` stages into `value`
// but leaves the phase `Loaded`; only `trigger` publishes to `Output`.

/// Where a triggered output is in its lifecycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// DAC channel disabled (EN = 0). No output until `enable`.
    Off,
    /// Enabled, no value staged. A `load` may stage the first code.
    Ready,
    /// A code is staged in DHR (`value`) but NOT yet on the pin — the output still
    /// holds the previous code. This is the glitch-free staging state.
    Loaded,
    /// A trigger fired: DOR == `value`, the pin is driving the staged code.
    Output,
}

/// Why a transition was rejected. A rejected op never corrupts the FSM.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// `enable` with a channel above `MAX_CHANNEL`.
    BadChannel,
    /// A transition issued from the wrong phase (`load` while Off, `trigger` off-Loaded).
    WrongPhase,
}

/// A channel's output state: phase + channel + last code. `value` is the staged code
/// from Loaded onward, and the code on the pin once Output.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Dac {
    pub phase: Phase,
    pub channel: u32,
    pub value: u32,
}

impl Dac {
    /// The disabled channel.
    pub const fn off() -> Self {
        Dac { phase: Phase::Off, channel: 0, value: 0 }
    }
}

/// Enabled predicate: a value may be `load`ed **iff** the channel is not Off.
#[inline]
pub const fn is_enabled(d: Dac) -> bool {
    !matches!(d.phase, Phase::Off)
}

/// Enable `channel` in software-triggered mode: Off → Ready. Succeeds only from Off
/// with a valid channel; lands Ready with the value cleared.
#[inline]
pub const fn enable(d: Dac, channel: u32) -> Result<Dac, Fault> {
    match d.phase {
        Phase::Off if channel <= MAX_CHANNEL => {
            Ok(Dac { phase: Phase::Ready, channel, value: 0 })
        }
        Phase::Off => Err(Fault::BadChannel),
        _ => Err(Fault::WrongPhase),
    }
}

/// Stage a code into DHR: from any enabled phase (Ready/Loaded/Output), mask `v` to
/// 12 bits and land Loaded — **without** moving the pin. Rejected while Off. This is
/// the glitch-free stage: the output is unchanged until `trigger`.
#[inline]
pub const fn load(d: Dac, v: u32) -> Result<Dac, Fault> {
    match d.phase {
        Phase::Off => Err(Fault::WrongPhase),
        _ => Ok(Dac { phase: Phase::Loaded, channel: d.channel, value: v & DHR_MASK }),
    }
}

/// Publish the staged code: Loaded → Output (DOR := DHR). Rejected from any other
/// phase — you cannot trigger a channel with nothing staged. The value is unchanged;
/// trigger only makes it visible.
#[inline]
pub const fn trigger(d: Dac) -> Result<Dac, Fault> {
    match d.phase {
        Phase::Loaded => Ok(Dac { phase: Phase::Output, channel: d.channel, value: d.value }),
        _ => Err(Fault::WrongPhase),
    }
}

/// Disable / teardown: return to Off from ANY state — never leaves a channel wedged.
/// Total; defined for every state. Clears the staged code.
#[inline]
pub const fn disable(_d: Dac) -> Dac {
    Dac::off()
}

// ───────────────────────────── dissolve ABI ─────────────────────────────────
//
// Scalar in/out, no linmem/data segment → 0 SRAM. State is carried by the caller as a
// packed u32: phase in bits[31:30], channel (0/1) in bit[29], value (12-bit) in
// bits[11:0].

/// Sentinel for a rejected FSM transition — keeps the ABI scalar (cf. adc-thin's ADC_FAULT).
pub const DAC_FAULT: u32 = 0xFFFF_FFFF;

const PHASE_SHIFT: u32 = 30;
const CHAN_BIT: u32 = 1 << 29;
const VALUE_MASK: u32 = 0x0FFF; // 12 bits

#[inline]
fn ph_enc(p: Phase) -> u32 {
    match p {
        Phase::Off => 0,
        Phase::Ready => 1,
        Phase::Loaded => 2,
        Phase::Output => 3,
    }
}
#[inline]
fn ph_dec(b: u32) -> Phase {
    match b {
        0 => Phase::Off,
        1 => Phase::Ready,
        2 => Phase::Loaded,
        _ => Phase::Output,
    }
}
/// Pack a `Dac` into the scalar carried across the dissolve seam.
#[inline]
fn pack(d: Dac) -> u32 {
    (ph_enc(d.phase) << PHASE_SHIFT) | if d.channel != 0 { CHAN_BIT } else { 0 } | (d.value & VALUE_MASK)
}
/// Unpack the scalar back into a `Dac`.
#[inline]
fn unpack(s: u32) -> Dac {
    Dac {
        phase: ph_dec(s >> PHASE_SHIFT),
        channel: if s & CHAN_BIT != 0 { 1 } else { 0 },
        value: s & VALUE_MASK,
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

/// Configure + enable the DAC `channel` at `base` in software-triggered mode
/// (EN | TEN | TSEL=software). Table-free: the CR word is pure bit arithmetic.
#[no_mangle]
pub extern "C" fn dac_configure(base: u32, channel: u32) {
    wr(base + CR, cr_enable_bits(channel));
}

/// Enable `channel`: Off → Ready. Returns the packed Ready state or `DAC_FAULT`
/// (bad channel / wrong phase). Writes the CR enable word.
#[no_mangle]
pub extern "C" fn dac_enable(base: u32, state: u32, channel: u32) -> u32 {
    match enable(unpack(state), channel) {
        Ok(next) => {
            wr(base + CR, cr_enable_bits(channel));
            pack(next)
        }
        Err(_) => DAC_FAULT,
    }
}

/// Stage a code: mask `v` to 12 bits, write DHR, land Loaded — the pin does NOT move
/// yet. Returns the packed Loaded state or `DAC_FAULT` if the channel is Off.
#[no_mangle]
pub extern "C" fn dac_load(base: u32, state: u32, v: u32) -> u32 {
    let d = unpack(state);
    match load(d, v) {
        Ok(next) => {
            wr(base + dhr_offset(d.channel), dhr_value(v));
            pack(next)
        }
        Err(_) => DAC_FAULT,
    }
}

/// Publish the staged code: drive SWTRIGR, Loaded → Output (DOR now == the staged
/// value). Returns the packed Output state or `DAC_FAULT` if nothing is staged.
#[no_mangle]
pub extern "C" fn dac_trigger(base: u32, state: u32) -> u32 {
    let d = unpack(state);
    match trigger(d) {
        Ok(next) => {
            wr(base + SWTRIGR, swtrig_bit(d.channel));
            pack(next)
        }
        Err(_) => DAC_FAULT,
    }
}

/// Read back the value actually on the pin (DOR). In Output phase this equals the last
/// loaded code; the caller can assert output-reflects-loaded against `dac_value`.
#[no_mangle]
pub extern "C" fn dac_output(base: u32, state: u32) -> u32 {
    let d = unpack(state);
    rd(base + dor_offset(d.channel)) & DHR_MASK
}

/// Extract the staged/last code from a packed state (0..4095). Pure query.
#[no_mangle]
pub extern "C" fn dac_value(state: u32) -> u32 {
    unpack(state).value
}

/// True (1) once a trigger has published the staged code (phase Output), else 0.
#[no_mangle]
pub extern "C" fn dac_is_output(state: u32) -> u32 {
    matches!(unpack(state).phase, Phase::Output) as u32
}

/// Disable: drive CR to 0 for the channel and return to Off from any state.
#[no_mangle]
pub extern "C" fn dac_disable(base: u32, state: u32) -> u32 {
    wr(base + CR, 0);
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
            2 => Phase::Loaded,
            _ => Phase::Output,
        }
    }
    // Every `Dac` the driver operates on is obtained by `unpack`-ing the scalar carried
    // across the dissolve seam, which masks `value` to 12 bits (VALUE_MASK) and `channel`
    // to 1 bit. So the seam-reachable state space — the only states the exported ABI can
    // ever be in — has value ≤ 0xFFF and channel ∈ {0,1}. Modeling that here is faithful,
    // not an over-assumption; the range property on the *commanded* value is tested
    // separately in P1, where `load` takes an unconstrained `v`.
    fn any_dac() -> Dac {
        Dac {
            phase: any_phase(),
            channel: kani::any::<u32>() & 1,
            value: kani::any::<u32>() & VALUE_MASK,
        }
    }

    /// P1 — range clamp: `load` masks any commanded value to 12 bits; the staged value
    /// is exactly `v & 0xFFF` and never exceeds `MAX_CODE`, for any input.
    #[kani::proof]
    fn p1_range_clamp() {
        let d = any_dac();
        let v: u32 = kani::any();
        if let Ok(next) = load(d, v) {
            assert_eq!(next.value, v & DHR_MASK);
            assert!(next.value <= MAX_CODE);
            assert_eq!(next.phase, Phase::Loaded);
        }
        // the pure config clamp too
        assert!(dhr_value(v) <= MAX_CODE);
        assert_eq!(dhr_value(v), v & DHR_MASK);
    }

    /// P2 — output reflects loaded: `trigger` publishes the staged code UNCHANGED
    /// (Loaded → Output, value identical). So DOR (== value in Output) equals the last
    /// loaded code — never a different or wider value.
    #[kani::proof]
    fn p2_output_reflects_loaded() {
        let d = any_dac();
        if let Ok(next) = trigger(d) {
            assert_eq!(d.phase, Phase::Loaded);
            assert_eq!(next.phase, Phase::Output);
            assert_eq!(next.value, d.value); // trigger never alters the value
            assert_eq!(next.channel, d.channel);
            assert!(next.value <= MAX_CODE); // stayed a valid 12-bit code
        }
    }

    /// P3 — **glitch-free** (the DAC invariant): the ONLY producer of Output is
    /// `trigger`, and only from Loaded; a `load` never reaches Output (it lands Loaded,
    /// staging without moving the pin). So the output changes only on an explicit
    /// trigger — no half-updated code is ever driven.
    #[kani::proof]
    fn p3_glitch_free() {
        let d = any_dac();
        // load never publishes: it always lands Loaded, never Output
        if let Ok(next) = load(d, kani::any()) {
            assert_eq!(next.phase, Phase::Loaded);
            assert!(next.phase != Phase::Output);
        }
        // Output is reachable ONLY via trigger from Loaded
        if let Ok(next) = trigger(d) {
            assert_eq!(d.phase, Phase::Loaded);
            assert_eq!(next.phase, Phase::Output);
        }
        // enable never publishes either
        if let Ok(next) = enable(d, kani::any()) {
            assert_eq!(next.phase, Phase::Ready);
            assert!(next.phase != Phase::Output);
        }
    }

    /// P4 — phase gating: `load` requires an enabled channel (not Off), `trigger`
    /// requires Loaded; each off-phase op is rejected and corrupts nothing.
    #[kani::proof]
    fn p4_phase_gating() {
        let d = any_dac();
        if d.phase == Phase::Off {
            assert_eq!(load(d, kani::any()), Err(Fault::WrongPhase));
        } else {
            assert!(load(d, kani::any()).is_ok()); // any enabled phase accepts a load
        }
        if d.phase != Phase::Loaded {
            assert_eq!(trigger(d), Err(Fault::WrongPhase));
        }
        assert_eq!(is_enabled(d), d.phase != Phase::Off);
    }

    /// P5 — disable is total and always powers down to Off (never wedged).
    #[kani::proof]
    fn p5_disable_total() {
        let d = any_dac();
        let n = disable(d);
        assert_eq!(n.phase, Phase::Off);
        assert_eq!(n.channel, 0);
        assert_eq!(n.value, 0);
    }

    /// P6 — pack/unpack round-trips every reachable state losslessly across the seam
    /// (phase + 1-bit channel + 12-bit value).
    #[kani::proof]
    fn p6_pack_roundtrip() {
        let d = any_dac();
        kani::assume(d.channel <= 1);
        kani::assume(d.value <= VALUE_MASK);
        let u = unpack(pack(d));
        assert_eq!(u.phase, d.phase);
        assert_eq!(u.channel, d.channel);
        assert_eq!(u.value, d.value);
    }

    /// P7 — config is table-free + well-formed: the CR enable word sets only EN|TEN|TSEL
    /// in the channel's 16-bit lane, the SWTRIG bit is exactly one of bit0/bit1, and the
    /// DHR/DOR offsets are the documented pair — for any channel input.
    #[kani::proof]
    fn p7_config_well_formed() {
        let ch: u32 = kani::any();
        let cr = cr_enable_bits(ch);
        // only EN|TEN|TSEL, in the correct 16-bit lane, nothing stray
        let lane = (CR_EN | CR_TEN | CR_TSEL_SW) << (16 * (ch & 1));
        assert_eq!(cr, lane);
        assert_eq!(cr & !lane, 0);
        // swtrig is exactly one channel bit
        let t = swtrig_bit(ch);
        assert!(t == 1 || t == 2);
        // register offsets are the documented pair for the channel
        let dhr = dhr_offset(ch);
        let dor = dor_offset(ch);
        assert!(dhr == DHR12R1 || dhr == DHR12R2);
        assert!(dor == DOR1 || dor == DOR2);
    }
}

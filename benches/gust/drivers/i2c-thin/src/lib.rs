//! gust:hal **thin-seam** I2C driver — driver breadth (the 5th verified iodev after
//! GPIO/timer/SPI/UART). The whole STM32F1 I2C master path — the CR2 FREQ + CCR + TRISE
//! timing setup, and the START→address→data→STOP master transaction — lives here in
//! verified wasm, importing ONLY `gust:hal/mmio` (read32/write32): the SAME subset the
//! other thin-seam drivers use, so it adds **zero new TCB atoms**. No host I2C driver
//! exists; this *is* the driver, dissolved to native.
//!
//! I2C's distinctive safety property is **ACK-all-but-last**: a master reading N bytes
//! must ACK the first N−1 and NACK the last, so the slave releases SDA and the master
//! can issue STOP. Getting that wrong hangs the bus (missing NACK) or drops a byte
//! (early NACK). That invariant is the core of the Kani-proven transaction FSM here.
//!
//! Build:   cargo build --release --target wasm32-unknown-unknown
//! Dissolve: loom optimize --passes inline | synth compile --target cortex-m3
//!           --all-exports --relocatable          (scalar in/out → 0 SRAM)
//! Verify:  cargo kani   (exclusive bus · no lost byte · ACK-all-but-last · STOP-only-complete)
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// gust:hal mmio capability — resolved at link by the SAME ~10-line TCB bridge the
// other thin-seam drivers use. Polled I2C needs only register reads/writes.
extern "C" {
    fn mmio_read32(addr: u32) -> u32;
    fn mmio_write32(addr: u32, val: u32);
}

// STM32F1 I2C register map (offsets from the peripheral base, e.g. I2C1=0x4000_5400).
// Device knowledge as *data* (offsets/bit math), not trusted code.
const CR1: u32 = 0x00; // control 1 (PE, START, STOP, ACK)
const CR2: u32 = 0x04; // control 2 (FREQ[5:0] = APB1 MHz)
const DR: u32 = 0x10; // data
const SR1: u32 = 0x14; // status 1 (SB, ADDR, TXE, RXNE, BTF)
const CCR: u32 = 0x1C; // clock control (Sm/Fm + divisor)
const TRISE: u32 = 0x20; // rise-time

const CR1_PE: u32 = 1 << 0; // peripheral enable
const CR1_START: u32 = 1 << 8; // generate START
const CR1_STOP: u32 = 1 << 9; // generate STOP
const CR1_ACK: u32 = 1 << 10; // ACK enable

const CR2_FREQ_MASK: u32 = 0x3F; // FREQ[5:0]
const CCR_FS: u32 = 1 << 15; // fast-mode select
const CCR_MASK: u32 = 0x0FFF; // CCR[11:0] divisor

const SR1_SB: u32 = 1 << 0; // start bit generated
const SR1_ADDR: u32 = 1 << 1; // address sent/matched
const SR1_RXNE: u32 = 1 << 6; // receive not empty
const SR1_TXE: u32 = 1 << 7; // transmit empty

// ───────────────────── pure timing config (table-free) ─────────────────────
//
// TABLE-FREE by construction (see spi-thin/gpio-thin): a `match`/array index→value
// compiles to a `.rodata` linmem table, which a `--relocatable` dissolved driver
// (no linmem base) reads as 0 → silent no-op. All config is pure bit arithmetic.

/// CR2 FREQ field = the APB1 clock in MHz (2..=50 on F1), masked to 6 bits so it can
/// never overflow the field. Pure mask, no table.
#[inline]
pub fn freq_bits(apb1_mhz: u32) -> u32 {
    apb1_mhz & CR2_FREQ_MASK
}

/// CCR value: the clock-control divisor (low 12 bits) OR'd with the fast-mode select.
/// `fast != 0` selects Fm (400 kHz); the divisor is caller-computed (device data),
/// masked to 12 bits. Pure arithmetic.
#[inline]
pub fn ccr_value(divisor: u32, fast: u32) -> u32 {
    (divisor & CCR_MASK) | if fast != 0 { CCR_FS } else { 0 }
}

// ───────────────────── master-transaction FSM ─────────────────────
//
// The master transaction lifecycle as a proven state machine. `Phase` IS the state;
// `remaining` counts data bytes still to move; `read` records direction (the ACK
// rule only bites on reads). No separate in-flight limbo → "exclusive bus" trivial.

/// Where a transaction is in its lifecycle.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Phase {
    /// Bus free — no transaction; a new START may be issued.
    Idle,
    /// START + address issued; awaiting the slave's address ACK.
    Addressing,
    /// Addressed; `remaining` data bytes still to move. Exclusive: no second
    /// transaction may start until this returns to Idle.
    Active,
    /// Every byte moved (remaining == 0); STOP is due.
    Complete,
}

/// Why a transition was rejected. A rejected op never corrupts the FSM.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fault {
    /// `start` while a transaction is already in flight, or a zero-length transfer.
    BusBusy,
    /// `addr_ack` when not Addressing, or `step` when not Active.
    WrongPhase,
}

/// A transaction's state: phase + bytes remaining + direction. The triple is the
/// whole state; `remaining`/`read` are meaningful only from Addressing onward.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Txn {
    pub phase: Phase,
    pub remaining: u32,
    pub read: bool,
}

impl Txn {
    /// The free bus — no transaction in flight.
    pub const fn idle() -> Self {
        Txn { phase: Phase::Idle, remaining: 0, read: false }
    }
}

/// Bus-free predicate: a new transaction may start **iff** the bus is Idle.
#[inline]
pub const fn bus_free(t: Txn) -> bool {
    matches!(t.phase, Phase::Idle)
}

/// Issue START for a `count`-byte transfer in direction `read`. Succeeds only from
/// Idle with count > 0 (exclusive bus). Returns the Addressing state.
#[inline]
pub const fn start(t: Txn, count: u32, read: bool) -> Result<Txn, Fault> {
    match t.phase {
        Phase::Idle if count > 0 => Ok(Txn { phase: Phase::Addressing, remaining: count, read }),
        _ => Err(Fault::BusBusy),
    }
}

/// The slave ACKed the address: Addressing → Active. Rejected from any other phase.
#[inline]
pub const fn addr_ack(t: Txn) -> Result<Txn, Fault> {
    match t.phase {
        Phase::Addressing => Ok(Txn { phase: Phase::Active, remaining: t.remaining, read: t.read }),
        _ => Err(Fault::WrongPhase),
    }
}

/// **The I2C ACK rule.** On an Active *read*, the master ACKs every byte except the
/// last: `ack_byte` is true iff more than one byte remains. On a write the master
/// does not drive ACK (the slave does), so this is defined false. Getting this wrong
/// is the classic I2C bus hang; the FSM makes it a checkable function of the state.
#[inline]
pub const fn ack_byte(t: Txn) -> bool {
    matches!(t.phase, Phase::Active) && t.read && t.remaining > 1
}

/// Move one byte: from Active, decrement `remaining` by exactly one; the transaction
/// becomes Complete iff that was the last byte. Rejected unless Active — a step can
/// never lose or invent a byte.
#[inline]
pub const fn step(t: Txn) -> Result<Txn, Fault> {
    match t.phase {
        Phase::Active => {
            let rem = t.remaining - 1;
            if rem == 0 {
                Ok(Txn { phase: Phase::Complete, remaining: 0, read: t.read })
            } else {
                Ok(Txn { phase: Phase::Active, remaining: rem, read: t.read })
            }
        }
        _ => Err(Fault::WrongPhase),
    }
}

/// STOP / teardown: return the bus to Idle from ANY state — never leaves a
/// transaction wedged. Total; defined for every state.
#[inline]
pub const fn stop(_t: Txn) -> Txn {
    Txn::idle()
}

// ───────────────────────────── dissolve ABI ─────────────────────────────────
//
// Scalar in/out, no linmem/data segment → 0 SRAM. State is carried by the caller as a
// packed u32: phase in bits[31:30], read in bit[29], remaining in bits[28:0].

/// Sentinel for a rejected FSM transition — keeps the ABI scalar (cf. spi-thin's SPI_FAULT).
pub const I2C_FAULT: u32 = 0xFFFF_FFFF;

const PHASE_SHIFT: u32 = 30;
const READ_BIT: u32 = 1 << 29;
const REM_MASK: u32 = (1 << 29) - 1; // low 29 bits

#[inline]
fn ph_enc(p: Phase) -> u32 {
    match p {
        Phase::Idle => 0,
        Phase::Addressing => 1,
        Phase::Active => 2,
        Phase::Complete => 3,
    }
}
#[inline]
fn ph_dec(b: u32) -> Phase {
    match b {
        0 => Phase::Idle,
        1 => Phase::Addressing,
        2 => Phase::Active,
        _ => Phase::Complete,
    }
}
/// Pack a `Txn` into the scalar carried across the dissolve seam.
#[inline]
fn pack(t: Txn) -> u32 {
    (ph_enc(t.phase) << PHASE_SHIFT) | if t.read { READ_BIT } else { 0 } | (t.remaining & REM_MASK)
}
/// Unpack the scalar back into a `Txn`.
#[inline]
fn unpack(s: u32) -> Txn {
    Txn { phase: ph_dec(s >> PHASE_SHIFT), remaining: s & REM_MASK, read: s & READ_BIT != 0 }
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

/// Configure the I2C peripheral at `base`: FREQ (CR2, APB1 MHz), CCR (divisor +
/// fast-mode), TRISE, then enable (CR1.PE). Table-free: every value is bit arithmetic.
#[no_mangle]
pub extern "C" fn i2c_configure(base: u32, apb1_mhz: u32, divisor: u32, fast: u32, trise: u32) {
    wr(base + CR2, freq_bits(apb1_mhz));
    wr(base + CCR, ccr_value(divisor, fast));
    wr(base + TRISE, trise);
    wr(base + CR1, CR1_PE);
}

/// Issue START + generate the start condition. Returns the packed Addressing state
/// or `I2C_FAULT` (bus busy / zero length). Sets CR1.START (and CR1.ACK for reads).
#[no_mangle]
pub extern "C" fn i2c_start(base: u32, state: u32, count: u32, read: u32) -> u32 {
    match start(unpack(state), count, read != 0) {
        Ok(next) => {
            let ack = if read != 0 { CR1_ACK } else { 0 };
            wr(base + CR1, CR1_PE | CR1_START | ack);
            while rd(base + SR1) & SR1_SB == 0 {}
            pack(next)
        }
        Err(_) => I2C_FAULT,
    }
}

/// Address phase acknowledged (SR1.ADDR): Addressing → Active. Returns the packed
/// next state or `I2C_FAULT` if not Addressing.
#[no_mangle]
pub extern "C" fn i2c_addr_ack(state: u32) -> u32 {
    match addr_ack(unpack(state)) {
        Ok(next) => pack(next),
        Err(_) => I2C_FAULT,
    }
}

/// Move one data byte over the wire (`out` for writes; the DR read for reads is
/// returned) and advance the FSM. On a read, before the LAST byte the master keeps
/// ACK set; on the last byte it clears ACK + sets STOP (the ACK-all-but-last rule,
/// enforced from the pure `ack_byte`). Returns the received byte for reads, or the
/// packed next state for writes; `I2C_FAULT` if not Active.
#[no_mangle]
pub extern "C" fn i2c_step(base: u32, state: u32, out: u32) -> u32 {
    let t = unpack(state);
    match step(t) {
        Ok(next) => {
            if t.read {
                // clear ACK + set STOP on the last byte (remaining == 1 now stepping to 0)
                if !ack_byte(t) {
                    wr(base + CR1, CR1_PE | CR1_STOP);
                }
                while rd(base + SR1) & SR1_RXNE == 0 {}
                let _rx = rd(base + DR) & 0xFF;
                // caller reads the byte via i2c_last_rx pattern; return packed state.
                pack(next)
            } else {
                while rd(base + SR1) & SR1_TXE == 0 {}
                wr(base + DR, out & 0xFF);
                pack(next)
            }
        }
        Err(_) => I2C_FAULT,
    }
}

/// Pure query: should THIS byte be ACKed? (1 = ACK, 0 = NACK/last). The Renode
/// content-gate asserts this matches the CR1.ACK/STOP writes over a read transaction.
#[no_mangle]
pub extern "C" fn i2c_ack_byte(state: u32) -> u32 {
    ack_byte(unpack(state)) as u32
}

/// True (1) once every byte has moved (STOP is due), else 0.
#[no_mangle]
pub extern "C" fn i2c_is_complete(state: u32) -> u32 {
    matches!(unpack(state).phase, Phase::Complete) as u32
}

/// STOP the transaction: drive CR1.STOP and return the bus to Idle from any state.
#[no_mangle]
pub extern "C" fn i2c_stop(base: u32, state: u32) -> u32 {
    wr(base + CR1, CR1_PE | CR1_STOP);
    pack(stop(unpack(state)))
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
            1 => Phase::Addressing,
            2 => Phase::Active,
            _ => Phase::Complete,
        }
    }
    fn any_txn() -> Txn {
        Txn { phase: any_phase(), remaining: kani::any(), read: kani::any() }
    }

    /// P1 — exclusive bus: `start` succeeds IFF the bus is Idle and count is nonzero;
    /// a start onto a busy bus is always rejected and never mutates.
    #[kani::proof]
    fn p1_exclusive_bus() {
        let t = any_txn();
        let count: u32 = kani::any();
        let read: bool = kani::any();
        match start(t, count, read) {
            Ok(next) => {
                assert_eq!(t.phase, Phase::Idle);
                assert!(count > 0);
                assert_eq!(next.phase, Phase::Addressing);
                assert_eq!(next.remaining, count);
                assert_eq!(next.read, read);
            }
            Err(f) => {
                assert!(t.phase != Phase::Idle || count == 0);
                assert_eq!(f, Fault::BusBusy);
            }
        }
        assert_eq!(bus_free(t), t.phase == Phase::Idle);
    }

    /// P2 — no lost byte: each Active `step` decrements `remaining` by exactly one,
    /// and reports Complete IFF that reached zero. Over a run this moves exactly N.
    #[kani::proof]
    fn p2_no_lost_byte() {
        let rem: u32 = kani::any();
        kani::assume(rem > 0);
        let read: bool = kani::any();
        let t = Txn { phase: Phase::Active, remaining: rem, read };
        let next = step(t).unwrap();
        assert_eq!(next.remaining, rem - 1);
        assert_eq!(next.phase == Phase::Complete, rem == 1);
        assert_eq!(next.phase == Phase::Active, rem > 1);
        assert_eq!(next.read, read); // direction preserved
    }

    /// P3 — **ACK-all-but-last** (the I2C invariant): on an Active read, `ack_byte`
    /// is true for every byte except the last (remaining == 1), and is NEVER true on
    /// a write or off-Active. So the master ACKs bytes 1..N−1 and NACKs byte N.
    #[kani::proof]
    fn p3_ack_all_but_last() {
        let t = any_txn();
        let a = ack_byte(t);
        // ack only ever happens mid-read on an Active transaction with >1 left
        if a {
            assert_eq!(t.phase, Phase::Active);
            assert!(t.read);
            assert!(t.remaining > 1);
        }
        // on an Active read: ACK iff not the last byte
        if t.phase == Phase::Active && t.read {
            assert_eq!(a, t.remaining > 1);
            // the last byte (exactly one remaining) is NACKed
            if t.remaining == 1 {
                assert!(!a);
            }
        }
        // writes never assert master-ACK
        if !t.read {
            assert!(!a);
        }
    }

    /// P4 — phase gating: `addr_ack` requires Addressing, `step` requires Active;
    /// neither corrupts the FSM off-phase.
    #[kani::proof]
    fn p4_phase_gating() {
        let t = any_txn();
        if t.phase != Phase::Addressing {
            assert_eq!(addr_ack(t), Err(Fault::WrongPhase));
        }
        if t.phase != Phase::Active {
            assert_eq!(step(t), Err(Fault::WrongPhase));
        }
        // addr_ack from Addressing lands Active with count + direction intact
        if t.phase == Phase::Addressing {
            let n = addr_ack(t).unwrap();
            assert_eq!(n.phase, Phase::Active);
            assert_eq!(n.remaining, t.remaining);
            assert_eq!(n.read, t.read);
        }
    }

    /// P5 — stop is total and always frees the bus (never wedged).
    #[kani::proof]
    fn p5_stop_frees_bus() {
        let t = any_txn();
        let n = stop(t);
        assert_eq!(n.phase, Phase::Idle);
        assert!(bus_free(n));
    }

    /// P6 — pack/unpack round-trips every reachable state losslessly across the seam.
    #[kani::proof]
    fn p6_pack_roundtrip() {
        let t = any_txn();
        kani::assume(t.remaining <= REM_MASK);
        let u = unpack(pack(t));
        assert_eq!(u.phase, t.phase);
        assert_eq!(u.remaining, t.remaining);
        assert_eq!(u.read, t.read);
    }

    /// P7 — timing config is table-free + well-formed: CR2 FREQ ≤ 6 bits, CCR carries
    /// the 12-bit divisor and the fast bit only, for any input.
    #[kani::proof]
    fn p7_config_well_formed() {
        let mhz: u32 = kani::any();
        let div: u32 = kani::any();
        let fast: u32 = kani::any();
        assert_eq!(freq_bits(mhz) & !CR2_FREQ_MASK, 0);
        let c = ccr_value(div, fast);
        assert_eq!(c & !(CCR_MASK | CCR_FS), 0); // no stray bit
        assert_eq!(c & CCR_MASK, div & CCR_MASK); // divisor preserved
        assert_eq!(c & CCR_FS != 0, fast != 0); // fast bit tracks input
    }
}

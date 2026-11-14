//! gust:bus `i2c-master` over the STM32 I2C **v2** block (WB55, G0, G4): the bus driver
//! for wohl's air node (gale#511). Imports only `gust:hal/mmio`, so it adds no TCB atom.
//!
//! The v2 block is not the F1 block that `drivers/i2c-thin` drives. The master programs
//! the whole transfer into CR2 at once (address, direction, NBYTES, AUTOEND, START), and
//! the hardware then ACKs, NACKs and issues STOP by itself. So the F1 driver's
//! ACK-all-but-last FSM does not carry over. What this driver must get right instead:
//!
//! * CR2 is programmed with NBYTES == count, RELOAD == 0 and AUTOEND == 1. With those
//!   three, the reference manual has the block NACK the last received byte and then
//!   generate STOP. That hardware behaviour is an ASSUMPTION here, not proven; what Kani
//!   proves is that CR2 always carries exactly those values (`k1`, `k2`).
//! * `xfer` moves exactly `count` bytes and then refuses (`k3`).
//! * Every hardware error flag wins over a ready flag, so no byte is read or written
//!   after a NACK, arbitration loss or bus error (`k5`, `k9`).
//! * Every value crosses the seam in the gust:bus encoding (`k4`, `k6`).
//!
//! Register facts (offsets, bit positions) come from embassy-rs/stm32-data-generated
//! @7e8466ba `data/registers/i2c_v2.json`, the snapshot targets/reference/import.py pins;
//! WB55RG, G474RE and G031K8 all name `i2c v2` there. The 7-bit address sits in
//! SADD[7:1]; that placement is not in the register data and was cross-checked against
//! the embassy HAL (`i2c/config.rs`, `Address::sadd`). The I2C1 base comes from the
//! generated target model.
//!
//! Waits are bounded by an iteration count (`SPIN_MAX`), not by time.
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// wit-bindgen's glue must link against a global allocator. Every function here is
// scalar, so nothing calls it; a zero-state trapping allocator keeps 0 SRAM.
#[cfg(not(kani))]
use core::alloc::{GlobalAlloc, Layout};
#[cfg(not(kani))]
struct NoAlloc;
#[cfg(not(kani))]
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[cfg(not(kani))]
#[global_allocator]
static ALLOC: NoAlloc = NoAlloc;

#[cfg(not(kani))]
wit_bindgen::generate!({
    world: "gust:i2c-v2/driver",
    path: ["../../wit/hal", "../../wit/bus", "wit"],
    generate_all,
    // The default guard calls `run_ctors_once` at every export entry, which reads
    // and writes a flag in linear memory (0x10000C). This crate has no
    // constructors (`__wasm_call_ctors` is empty), and the dissolved object must
    // not touch linear memory.
    disable_run_ctors_once_workaround: true,
});

// ───────────────────────────── target ─────────────────────────────

#[cfg(any(
    all(feature = "stm32wb55", feature = "stm32g474"),
    all(feature = "stm32wb55", feature = "stm32g031"),
    all(feature = "stm32g474", feature = "stm32g031"),
))]
compile_error!("select exactly one target feature");

#[cfg(feature = "stm32wb55")]
#[allow(dead_code)]
mod target {
    include!("../../../targets/generated/gust_target_stm32wb55.rs");
}
#[cfg(feature = "stm32g474")]
#[allow(dead_code)]
mod target {
    include!("../../../targets/generated/gust_target_stm32g474.rs");
}
#[cfg(feature = "stm32g031")]
#[allow(dead_code)]
mod target {
    include!("../../../targets/generated/gust_target_stm32g031.rs");
}

/// The I2C1 base address, from the target model.
pub const BASE: u32 = target::periph::I2C1;

// ───────────────────────────── registers ─────────────────────────────

const CR1: u32 = 0x00;
const CR2: u32 = 0x04;
const TIMINGR: u32 = 0x10;
const ISR: u32 = 0x18;
const ICR: u32 = 0x1C;
const RXDR: u32 = 0x24;
const TXDR: u32 = 0x28;

pub const CR1_PE: u32 = 1 << 0;

pub const CR2_SADD_MASK: u32 = 0x3FF; // SADD[9:0]
pub const CR2_DIR_READ: u32 = 1 << 10;
pub const CR2_ADD10: u32 = 1 << 11;
pub const CR2_HEAD10R: u32 = 1 << 12;
pub const CR2_START: u32 = 1 << 13;
pub const CR2_STOP: u32 = 1 << 14;
pub const CR2_NACK: u32 = 1 << 15;
pub const CR2_NBYTES_SHIFT: u32 = 16;
pub const CR2_RELOAD: u32 = 1 << 24;
pub const CR2_AUTOEND: u32 = 1 << 25;
pub const CR2_PECBYTE: u32 = 1 << 26;

pub const ISR_TXE: u32 = 1 << 0;
pub const ISR_TXIS: u32 = 1 << 1;
pub const ISR_RXNE: u32 = 1 << 2;
pub const ISR_NACKF: u32 = 1 << 4;
pub const ISR_STOPF: u32 = 1 << 5;
pub const ISR_BERR: u32 = 1 << 8;
pub const ISR_ARLO: u32 = 1 << 9;
pub const ISR_BUSY: u32 = 1 << 15;

const ICR_NACKCF: u32 = 1 << 4;
const ICR_STOPCF: u32 = 1 << 5;
const ICR_BERRCF: u32 = 1 << 8;
const ICR_ARLOCF: u32 = 1 << 9;
const ICR_ALL: u32 = ICR_NACKCF | ICR_STOPCF | ICR_BERRCF | ICR_ARLOCF;

/// TIMINGR fields: SCLL 0..=7, SCLH 8..=15, SDADEL 16..=19, SCLDEL 20..=23,
/// PRESC 28..=31. Bits 24..=27 are reserved.
pub const TIMINGR_RESERVED: u32 = 0x0F00_0000;

/// Bound on every hardware wait, in loop iterations.
pub const SPIN_MAX: u32 = 100_000;

// ──────────────────────── gust:bus encoding (wit/bus) ────────────────────────

pub const ERR_BASE: u32 = 0xFFFF_FF00;
pub const NACK: u32 = 0x01;
pub const ARB_LOST: u32 = 0x02;
pub const BUS_BUSY: u32 = 0x03;
pub const NOT_ACTIVE: u32 = 0x04;
pub const TIMEOUT: u32 = 0x05;
pub const INVALID_ARG: u32 = 0x06;
pub const BUS_ERROR: u32 = 0x07;

pub const REM_MASK: u32 = 0xFF;
pub const READ_BIT: u32 = 1 << 8;
pub const PHASE_SHIFT: u32 = 9;
pub const PH_ACTIVE: u32 = 1;
pub const PH_COMPLETE: u32 = 2;
pub const STATE_LIMIT: u32 = 0x800;

#[inline]
pub const fn err(code: u32) -> u32 {
    ERR_BASE | code
}

/// A decoded `state` argument. Anything the encoding does not allow is `Invalid`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum State {
    Idle,
    Active { remaining: u32, read: bool },
    Complete,
    Invalid,
}

#[inline]
pub const fn active(remaining: u32, read: bool) -> u32 {
    (PH_ACTIVE << PHASE_SHIFT) | if read { READ_BIT } else { 0 } | (remaining & REM_MASK)
}

#[inline]
pub const fn complete(read: bool) -> u32 {
    (PH_COMPLETE << PHASE_SHIFT) | if read { READ_BIT } else { 0 }
}

/// Idle is exactly 0, Active needs 1..=255 bytes left, Complete needs 0 left.
#[inline]
pub const fn decode(s: u32) -> State {
    if s >= STATE_LIMIT {
        return State::Invalid;
    }
    // An if-chain, not a `match`: LLVM lowers a dense match to a `.rodata` lookup
    // table, and a relocatable dissolved driver has no data section (synth#1041).
    let remaining = s & REM_MASK;
    let read = s & READ_BIT != 0;
    let phase = s >> PHASE_SHIFT;
    if s == 0 {
        State::Idle
    } else if phase == PH_ACTIVE && remaining >= 1 {
        State::Active { remaining, read }
    } else if phase == PH_COMPLETE && remaining == 0 {
        State::Complete
    } else {
        State::Invalid
    }
}

/// The state after one byte moved: one fewer left, Complete after the last.
#[inline]
pub const fn advance(remaining: u32, read: bool) -> u32 {
    if remaining <= 1 {
        complete(read)
    } else {
        active(remaining - 1, read)
    }
}

// ───────────────────────────── pure decisions ─────────────────────────────

/// What `begin` writes to CR2 and returns, or the error code. Checks run in the
/// order: arguments, peripheral set up (CR1.PE), bus idle (ISR.BUSY).
#[inline]
pub const fn plan_begin(addr7: u32, count: u32, read: u32, cr1: u32, isr: u32) -> Result<(u32, u32), u32> {
    if addr7 > 0x7F || count == 0 || count > 255 || read > 1 {
        return Err(INVALID_ARG);
    }
    if cr1 & CR1_PE == 0 {
        return Err(NOT_ACTIVE);
    }
    if isr & ISR_BUSY != 0 {
        return Err(BUS_BUSY);
    }
    let r = read == 1;
    let cr2 = (addr7 << 1)
        | if r { CR2_DIR_READ } else { 0 }
        | (count << CR2_NBYTES_SHIFT)
        | CR2_AUTOEND
        | CR2_START;
    Ok((cr2, active(count, r)))
}

/// One poll of ISR while waiting to move a byte.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Poll {
    Wait,
    Ready,
    Fail(u32),
}

/// Error flags win over the data flag, so nothing moves after a fault.
#[inline]
pub const fn classify(isr: u32, read: bool) -> Poll {
    if isr & ISR_BERR != 0 {
        Poll::Fail(BUS_ERROR)
    } else if isr & ISR_ARLO != 0 {
        Poll::Fail(ARB_LOST)
    } else if isr & ISR_NACKF != 0 {
        Poll::Fail(NACK)
    } else if isr & (if read { ISR_RXNE } else { ISR_TXIS }) != 0 {
        Poll::Ready
    } else {
        Poll::Wait
    }
}

/// One poll of ISR while `end` waits for the automatic STOP. A NACK of the final
/// written byte is only certain once STOP has happened, so it is reported here.
#[inline]
pub const fn classify_end(isr: u32) -> Poll {
    if isr & ISR_BERR != 0 {
        Poll::Fail(BUS_ERROR)
    } else if isr & ISR_ARLO != 0 {
        Poll::Fail(ARB_LOST)
    } else if isr & ISR_STOPF == 0 {
        Poll::Wait
    } else if isr & ISR_NACKF != 0 {
        Poll::Fail(NACK)
    } else {
        Poll::Ready
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EndPlan {
    /// Already idle.
    Done,
    /// Every byte moved; the hardware issues STOP (AUTOEND), wait for it.
    AwaitStop,
    /// Abandoned mid-transfer or a malformed state: reset the peripheral.
    Reset,
}

#[inline]
pub const fn plan_end(s: u32) -> EndPlan {
    match decode(s) {
        State::Idle => EndPlan::Done,
        State::Complete => EndPlan::AwaitStop,
        _ => EndPlan::Reset,
    }
}

#[inline]
pub const fn configure_ok(timingr: u32) -> bool {
    timingr & TIMINGR_RESERVED == 0
}

// ───────────────────────────── register shell ─────────────────────────────

#[cfg(not(kani))]
use crate::gust::hal::mmio::{read32, write32};

#[cfg(not(kani))]
#[inline(always)]
fn rd(off: u32) -> u32 {
    read32(BASE + off)
}
#[cfg(not(kani))]
#[inline(always)]
fn wr(off: u32, v: u32) {
    write32(BASE + off, v)
}

/// Clear PE, wait (bounded) for it to read back clear, set it again. Resets the
/// block's state machine; TIMINGR is kept.
#[cfg(not(kani))]
fn soft_reset() {
    let c = rd(CR1);
    wr(CR1, c & !CR1_PE);
    let mut i = 0;
    while rd(CR1) & CR1_PE != 0 && i < SPIN_MAX {
        i += 1;
    }
    wr(CR1, c | CR1_PE);
}

/// After a fault: clear its flag, make sure the peripheral is idle, flush TXDR.
#[cfg(not(kani))]
fn recover(code: u32) {
    if code == NACK {
        // AUTOEND: the hardware issues STOP after a NACK; wait for it.
        wr(ICR, ICR_NACKCF);
        let mut i = 0;
        while rd(ISR) & ISR_STOPF == 0 && i < SPIN_MAX {
            i += 1;
        }
        if i >= SPIN_MAX {
            soft_reset();
        }
        wr(ICR, ICR_STOPCF);
    } else if code == ARB_LOST {
        wr(ICR, ICR_ARLOCF);
    } else {
        wr(ICR, ICR_ALL);
        soft_reset();
    }
    wr(ISR, ISR_TXE); // writing TXE = 1 flushes TXDR
}

#[cfg(not(kani))]
fn begin(addr7: u32, count: u32, read: u32) -> u32 {
    match plan_begin(addr7, count, read, rd(CR1), rd(ISR)) {
        Ok((cr2, state)) => {
            wr(ICR, ICR_ALL);
            wr(CR2, cr2);
            state
        }
        Err(code) => err(code),
    }
}

#[cfg(not(kani))]
fn xfer(state: u32, out: u32) -> u32 {
    let (remaining, read) = match decode(state) {
        State::Active { remaining, read } => (remaining, read),
        _ => return err(NOT_ACTIVE),
    };
    let mut i = 0;
    loop {
        match classify(rd(ISR), read) {
            Poll::Ready => break,
            Poll::Fail(code) => {
                recover(code);
                return err(code);
            }
            Poll::Wait => {
                i += 1;
                if i >= SPIN_MAX {
                    soft_reset();
                    return err(TIMEOUT);
                }
            }
        }
    }
    let next = advance(remaining, read);
    if read {
        ((rd(RXDR) & 0xFF) << 16) | next
    } else {
        wr(TXDR, out & 0xFF);
        next
    }
}

#[cfg(not(kani))]
fn end(state: u32) -> u32 {
    match plan_end(state) {
        EndPlan::Done => 0,
        EndPlan::Reset => {
            soft_reset();
            wr(ICR, ICR_ALL);
            0
        }
        EndPlan::AwaitStop => {
            let mut i = 0;
            loop {
                match classify_end(rd(ISR)) {
                    Poll::Ready => {
                        wr(ICR, ICR_STOPCF);
                        return 0;
                    }
                    Poll::Fail(code) => {
                        recover(code);
                        wr(ICR, ICR_ALL);
                        return err(code);
                    }
                    Poll::Wait => {
                        i += 1;
                        if i >= SPIN_MAX {
                            soft_reset();
                            wr(ICR, ICR_ALL);
                            return err(TIMEOUT);
                        }
                    }
                }
            }
        }
    }
}

#[cfg(not(kani))]
fn configure(timingr: u32) -> u32 {
    if !configure_ok(timingr) {
        return err(INVALID_ARG);
    }
    let c = rd(CR1);
    wr(CR1, c & !CR1_PE);
    wr(TIMINGR, timingr);
    wr(CR1, (c & !CR1_PE) | CR1_PE);
    0
}

#[cfg(not(kani))]
struct Driver;
#[cfg(not(kani))]
impl exports::gust::bus::i2c_master::Guest for Driver {
    fn begin(addr7: u32, count: u32, read: u32) -> u32 {
        begin(addr7, count, read)
    }
    fn xfer(state: u32, out: u32) -> u32 {
        xfer(state, out)
    }
    fn end(state: u32) -> u32 {
        end(state)
    }
}
#[cfg(not(kani))]
impl exports::gust::i2c_v2::setup::Guest for Driver {
    fn configure(timingr: u32) -> u32 {
        configure(timingr)
    }
}
#[cfg(not(kani))]
export!(Driver);

// ───────────────────────────── Kani proofs ─────────────────────────────
//
// Over the full u32 input space unless a harness says otherwise. `cargo kani`.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    /// k1: `begin` accepts exactly the legal arguments on a set-up, idle bus, and the
    /// CR2 word it writes carries every field the transfer needs and nothing else.
    #[kani::proof]
    fn k1_begin_contract() {
        let (addr7, count, read, cr1, isr): (u32, u32, u32, u32, u32) =
            (kani::any(), kani::any(), kani::any(), kani::any(), kani::any());
        let args_ok = addr7 <= 0x7F && count >= 1 && count <= 255 && read <= 1;
        match plan_begin(addr7, count, read, cr1, isr) {
            Ok((cr2, s)) => {
                assert!(args_ok && cr1 & CR1_PE != 0 && isr & ISR_BUSY == 0);
                assert_eq!(cr2 & CR2_SADD_MASK, addr7 << 1); // 7-bit address in SADD[7:1]
                assert_eq!(cr2 & 1, 0);
                assert_eq!(cr2 & CR2_ADD10, 0); // 7-bit mode
                assert_eq!(cr2 & CR2_HEAD10R, 0);
                assert_eq!(cr2 & CR2_DIR_READ != 0, read == 1);
                assert!(cr2 & CR2_START != 0);
                assert_eq!(cr2 & (CR2_STOP | CR2_NACK | CR2_PECBYTE), 0);
                assert_eq!((cr2 >> CR2_NBYTES_SHIFT) & 0xFF, count);
                assert_eq!(cr2 >> 27, 0); // nothing above PECBYTE
                assert_eq!(decode(s), State::Active { remaining: count, read: read == 1 });
            }
            Err(code) => {
                if !args_ok {
                    assert_eq!(code, INVALID_ARG);
                } else if cr1 & CR1_PE == 0 {
                    assert_eq!(code, NOT_ACTIVE);
                } else {
                    assert!(isr & ISR_BUSY != 0);
                    assert_eq!(code, BUS_BUSY);
                }
            }
        }
    }

    /// k2: the hardware's ACK-all-but-last precondition. Every accepted CR2 has
    /// RELOAD clear, AUTOEND set and NBYTES equal to the byte count, which is the
    /// configuration in which the block NACKs the last byte and then issues STOP.
    /// The hardware behaviour itself is assumed from the reference manual.
    #[kani::proof]
    fn k2_hw_nack_last_precondition() {
        let (addr7, count, read, cr1, isr): (u32, u32, u32, u32, u32) =
            (kani::any(), kani::any(), kani::any(), kani::any(), kani::any());
        if let Ok((cr2, _)) = plan_begin(addr7, count, read, cr1, isr) {
            assert_eq!(cr2 & CR2_RELOAD, 0);
            assert!(cr2 & CR2_AUTOEND != 0);
            assert_eq!((cr2 >> CR2_NBYTES_SHIFT) & 0xFF, count);
        }
    }

    /// k3: from the state `begin` returns, exactly `count` successful byte moves
    /// reach Complete, every earlier state is Active, and Complete refuses a further
    /// move (`xfer` only proceeds on Active).
    #[kani::proof]
    #[kani::unwind(257)]
    fn k3_exact_byte_count() {
        let count: u32 = kani::any();
        let read: bool = kani::any();
        kani::assume(count >= 1 && count <= 255);
        let mut s = active(count, read);
        let mut moved = 0u32;
        while let State::Active { remaining, read: r } = decode(s) {
            assert_eq!(remaining, count - moved);
            assert_eq!(r, read);
            s = advance(remaining, r);
            moved += 1;
        }
        assert_eq!(moved, count);
        assert_eq!(decode(s), State::Complete);
        assert_eq!(s, complete(read));
    }

    /// k4: what crosses the seam never collides. States are < 0x800, errors are
    /// >= 0xFFFF_FF00, a read result keeps the next state in its low 16 bits and the
    /// byte in bits 16..=23, and is never mistaken for an error.
    #[kani::proof]
    fn k4_seam_encoding() {
        let remaining: u32 = kani::any();
        let read: bool = kani::any();
        kani::assume(remaining >= 1 && remaining <= 255);
        let next = advance(remaining, read);
        assert!(next < STATE_LIMIT);
        assert!(decode(next) != State::Invalid);
        let byte: u32 = kani::any::<u32>() & 0xFF;
        let res = (byte << 16) | next;
        assert!(res < ERR_BASE);
        assert_eq!(res & 0xFFFF, next);
        assert_eq!((res >> 16) & 0xFF, byte);
        let code: u32 = kani::any();
        kani::assume(code >= NACK && code <= BUS_ERROR);
        assert!(err(code) >= ERR_BASE);
    }

    /// k5: a byte moves only when no error flag is set and the flag for this
    /// direction is; any error flag stops the transfer.
    #[kani::proof]
    fn k5_errors_win() {
        let isr: u32 = kani::any();
        let read: bool = kani::any();
        let errs = isr & (ISR_BERR | ISR_ARLO | ISR_NACKF) != 0;
        match classify(isr, read) {
            Poll::Ready => {
                assert!(!errs);
                assert!(isr & (if read { ISR_RXNE } else { ISR_TXIS }) != 0);
            }
            Poll::Fail(c) => {
                assert!(errs);
                assert!(c == BUS_ERROR || c == ARB_LOST || c == NACK);
            }
            Poll::Wait => assert!(!errs),
        }
    }

    /// k6: decode is total and only the encoding's states decode as valid.
    #[kani::proof]
    fn k6_decode() {
        let s: u32 = kani::any();
        match decode(s) {
            State::Idle => assert_eq!(s, 0),
            State::Active { remaining, read } => {
                assert!(remaining >= 1 && remaining <= 255);
                assert_eq!(s, active(remaining, read));
            }
            State::Complete => assert!(s == complete(false) || s == complete(true)),
            State::Invalid => {
                assert!(s != 0);
                assert!(s >= STATE_LIMIT || (s >> PHASE_SHIFT) == 3
                    || ((s >> PHASE_SHIFT) == 0)
                    || ((s >> PHASE_SHIFT) == PH_ACTIVE && s & REM_MASK == 0)
                    || ((s >> PHASE_SHIFT) == PH_COMPLETE && s & REM_MASK != 0));
            }
        }
        if s >= ERR_BASE {
            assert_eq!(decode(s), State::Invalid); // an error is never a state
        }
    }

    /// k7: `end` is total. It does nothing only for the idle state, waits for the
    /// automatic STOP only after every byte moved, and resets otherwise.
    #[kani::proof]
    fn k7_end_plan() {
        let s: u32 = kani::any();
        match plan_end(s) {
            EndPlan::Done => assert_eq!(s, 0),
            EndPlan::AwaitStop => assert_eq!(decode(s), State::Complete),
            EndPlan::Reset => {
                let d = decode(s);
                assert!(d != State::Idle && d != State::Complete);
            }
        }
    }

    /// k8: `configure` refuses exactly the values that set reserved TIMINGR bits.
    #[kani::proof]
    fn k8_configure() {
        let t: u32 = kani::any();
        assert_eq!(configure_ok(t), t & 0x0F00_0000 == 0);
    }

    /// k9: `end` reports success only after STOP with no error flag, so a NACKed
    /// final written byte is never reported as 0.
    #[kani::proof]
    fn k9_end_reports_nack() {
        let isr: u32 = kani::any();
        match classify_end(isr) {
            Poll::Ready => {
                assert!(isr & ISR_STOPF != 0);
                assert_eq!(isr & (ISR_NACKF | ISR_BERR | ISR_ARLO), 0);
            }
            Poll::Fail(c) => assert!(c == BUS_ERROR || c == ARB_LOST || c == NACK),
            Poll::Wait => {
                assert_eq!(isr & (ISR_STOPF | ISR_BERR | ISR_ARLO), 0);
            }
        }
    }
}

//! Sensirion SEN66 over `gust:bus/i2c-master`: the pure core (gale#511).
//!
//! Every fact here comes from the SEN6x datasheet, "D1 Version 1.0 – October 2026"
//! (sha256 34b8e5b69c3686c76955abe7781b52841aaff7bd67beed012c4655b4566bbeeb).
//! Section and table numbers refer to that revision.
//!
//! * The address is 0x6B, 7-bit. SCL runs at 100 kbit/s at most. The sensor does
//!   not stretch the clock: while it is processing a command it NACKs its own
//!   address (§4.4 Table 26, §4.5).
//! * Commands are 16 bits, MSB first, and have no CRC byte (§4.6). Data are 16-bit
//!   words, MSB first, each followed by a CRC-8 (§4.6).
//! * The CRC-8 uses polynomial 0x31 and init 0xFF, with no reflection and final
//!   XOR 0x00, over the two data bytes. CRC(0xBEEF) = 0x92 (§4.9 Table 67).
//! * Read Measured Values SEN66 (0x0300) returns 9 words, 27 bytes. A field that is
//!   unknown reads 0xFFFF if it is a uint16 field and 0x7FFF if it is an int16
//!   field (§4.8.7 Table 34).
//!
//! The driver exports `gust:sen66/air` (`wit/sensor/gust-sen66.wit`). A sample is
//! packed into one u64, as decided on gale#511 on 2026-11-14. Unknown, not-ready
//! and CRC failures are errors or cleared valid bits, never values.
//!
//! The driver keeps no memory. The 27-byte frame is consumed byte by byte through
//! a pure accumulator (`rx_step`), so no buffer exists. The sensor NACKs its
//! address while it processes a command, so every transaction is retried on NACK,
//! up to `RETRY_MAX` attempts. That bound is an iteration count, not a time.
#![cfg_attr(not(any(kani, test)), no_std)]

#[cfg(not(any(kani, test)))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// wit-bindgen's glue must link against a global allocator. Every function here is
// scalar, so nothing calls it; a zero-state trapping allocator keeps 0 SRAM.
#[cfg(not(any(kani, test)))]
use core::alloc::{GlobalAlloc, Layout};
#[cfg(not(any(kani, test)))]
struct NoAlloc;
#[cfg(not(any(kani, test)))]
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[cfg(not(any(kani, test)))]
#[global_allocator]
static ALLOC: NoAlloc = NoAlloc;

#[cfg(not(any(kani, test)))]
wit_bindgen::generate!({
    world: "gust:sen66/sen66-driver",
    path: ["../../wit/hal", "../../wit/bus", "../../wit/sensor"],
    generate_all,
    // The default guard calls `run_ctors_once` at every export entry, which reads
    // and writes a flag in linear memory (0x10000C). This crate has no
    // constructors (`__wasm_call_ctors` is empty), and the dissolved object must
    // not touch linear memory.
    disable_run_ctors_once_workaround: true,
});

/// 7-bit I2C address (§4.4 Table 26).
pub const ADDR7: u32 = 0x6B;

/// Command IDs with their execution times in ms (§4.7 Table 27).
pub mod cmd {
    pub const START_MEASUREMENT: u16 = 0x0021; // 50 ms, idle mode only
    pub const STOP_MEASUREMENT: u16 = 0x0104; // 1400 ms, measurement mode
    pub const GET_DATA_READY: u16 = 0x0202; // 20 ms, measurement mode, 3 bytes
    pub const READ_MEASURED_VALUES: u16 = 0x0300; // 20 ms, measurement mode, 27 bytes
    pub const READ_DEVICE_STATUS: u16 = 0xD206; // 20 ms, idle and measurement, 6 bytes
    pub const READ_AND_CLEAR_STATUS: u16 = 0xD210; // 20 ms, idle and measurement, 6 bytes
    pub const DEVICE_RESET: u16 = 0xD304; // 1200 ms, idle mode only
}

/// Number of words in Read Measured Values SEN66 (§4.8.7 Table 34).
pub const MEASURED_WORDS: usize = 9;
/// Its length on the wire: each word is MSB, LSB, then CRC.
pub const MEASURED_BYTES: u32 = 27;

/// The CRC-8 from §4.9 Table 67, computed over one 16-bit word sent MSB first.
#[inline(always)]
pub const fn crc8(word: u16) -> u8 {
    let mut crc: u8 = 0xFF;
    let bytes = [(word >> 8) as u8, word as u8];
    let mut i = 0;
    while i < 2 {
        crc ^= bytes[i];
        let mut b = 0;
        while b < 8 {
            crc = if crc & 0x80 != 0 { (crc << 1) ^ 0x31 } else { crc << 1 };
            b += 1;
        }
        i += 1;
    }
    crc
}

/// Check one received word. Returns the word only when its CRC matches.
#[inline(always)]
pub const fn check_word(msb: u8, lsb: u8, crc: u8) -> Option<u16> {
    let w = ((msb as u16) << 8) | lsb as u16;
    if crc8(w) == crc {
        Some(w)
    } else {
        None
    }
}

/// The three bytes that put `word` on the wire in the write direction, where the
/// CRC byte is mandatory (§4.6).
pub const fn frame_word(word: u16) -> [u8; 3] {
    [(word >> 8) as u8, word as u8, crc8(word)]
}

/// The two bytes of a command. A command carries no CRC byte (§4.6).
pub const fn frame_cmd(cmd: u16) -> [u8; 2] {
    [(cmd >> 8) as u8, cmd as u8]
}

/// The fields of Read Measured Values SEN66, in wire order (§4.8.7 Table 34).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Field {
    Pm1 = 0,  // uint16, µg/m³ × 10
    Pm25 = 1, // uint16, µg/m³ × 10
    Pm4 = 2,  // uint16, µg/m³ × 10
    Pm10 = 3, // uint16, µg/m³ × 10
    Rh = 4,   // int16, % × 100
    T = 5,    // int16, °C × 200
    Voc = 6,  // int16, index × 10
    Nox = 7,  // int16, index × 10
    Co2 = 8,  // uint16, ppm
}

impl Field {
    pub const fn from_index(i: u32) -> Option<Field> {
        match i {
            0 => Some(Field::Pm1),
            1 => Some(Field::Pm25),
            2 => Some(Field::Pm4),
            3 => Some(Field::Pm10),
            4 => Some(Field::Rh),
            5 => Some(Field::T),
            6 => Some(Field::Voc),
            7 => Some(Field::Nox),
            8 => Some(Field::Co2),
            _ => None,
        }
    }

    /// True for the int16 fields (Table 34): RH, T, VOC and NOx.
    pub const fn signed(self) -> bool {
        matches!(self, Field::Rh | Field::T | Field::Voc | Field::Nox)
    }

    /// The raw value that means "unknown" for this field: 0xFFFF for uint16,
    /// 0x7FFF for int16 (Table 34).
    pub const fn unknown(self) -> u16 {
        if self.signed() {
            0x7FFF
        } else {
            0xFFFF
        }
    }
}

/// A raw field value is known when it is not the field's "unknown" value.
pub const fn known(f: Field, raw: u16) -> bool {
    raw != f.unknown()
}

// ───────────────────────────── seam encoding ─────────────────────────────

/// Error codes (`wit/sensor/gust-sen66.wit`). 0x01..=0x07 pass through from
/// gust:bus.
pub mod code {
    pub const NACK: u8 = 0x01;
    pub const NOT_READY: u8 = 0x10;
    pub const CRC: u8 = 0x11;
    pub const NO_RESPONSE: u8 = 0x12;
}

/// The gust:bus error band: any u32 at or above this is an error.
pub const ERR32: u32 = 0xFFFF_FF00;
/// Bit 63 marks a u64 error.
pub const ERR64: u64 = 1 << 63;
pub const VALID_CO2: u64 = 1 << 48;
pub const VALID_PM25: u64 = 1 << 49;
pub const VALID_VOC: u64 = 1 << 50;

pub const fn is_err(v: u32) -> bool {
    v >= ERR32
}

pub const fn err32(c: u8) -> u32 {
    ERR32 | c as u32
}

pub const fn err64(c: u8) -> u64 {
    ERR64 | c as u64
}

/// Pack one sample. A field the sensor reports as unknown has its valid bit clear
/// and its value bits 0. Bit 63 is never set.
#[inline(always)]
pub const fn pack(co2: u16, pm25: u16, voc: u16) -> u64 {
    // Built as two u32 halves on purpose. Written over u64, each `if` lowers to an
    // i64 `select` on a computed compare, which synth 0.89.0 miscompiles on ARM
    // (synth#1711): the dissolved object returned wrong samples on Renode's I2C v2
    // model (os/gust/renode-test/gust_sen66.robot). The value is unchanged; c6 checks it.
    let kc = known(Field::Co2, co2);
    let kp = known(Field::Pm25, pm25);
    let kv = known(Field::Voc, voc);
    let mut lo: u32 = 0;
    let mut hi: u32 = 0;
    if kc {
        lo |= co2 as u32;
        hi |= 1 << 16; // VALID_CO2 is bit 48
    }
    if kp {
        lo |= (pm25 as u32) << 16;
        hi |= 1 << 17; // VALID_PM25 is bit 49
    }
    if kv {
        hi |= voc as u32;
        hi |= 1 << 18; // VALID_VOC is bit 50
    }
    join(hi, lo)
}

/// The u64 from its halves. Not inlined, so LLVM cannot push the zero-extension
/// into the selects in `pack` and turn them back into i64 selects (synth#1711).
#[inline(never)]
pub const fn join(hi: u32, lo: u32) -> u64 {
    ((hi as u64) << 32) | lo as u64
}

// ───────────────────────────── frame accumulator ─────────────────────────────

/// Running state while the 27-byte measured-values frame streams in. No buffer:
/// each word is checked when its CRC byte arrives, and only the three fields the
/// seam carries are kept.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct Rx {
    pub n: u8,
    hi: u8,
    lo: u8,
    pub co2: u16,
    pub pm25: u16,
    pub voc: u16,
    /// Set once any word fails its CRC; never cleared.
    pub bad: bool,
}

pub const RX0: Rx = Rx { n: 0, hi: 0, lo: 0, co2: 0, pm25: 0, voc: 0, bad: false };

/// Feed one received byte.
#[inline(always)]
pub const fn rx_step(a: Rx, byte: u8) -> Rx {
    let mut a = a;
    match a.n % 3 {
        0 => a.hi = byte,
        1 => a.lo = byte,
        _ => match check_word(a.hi, a.lo, byte) {
            Some(w) => match a.n / 3 {
                1 => a.pm25 = w,
                6 => a.voc = w,
                8 => a.co2 = w,
                _ => {}
            },
            None => a.bad = true,
        },
    }
    a.n = a.n.wrapping_add(1);
    a
}

/// The finished frame as the seam value: crc error if any word failed, else the
/// packed sample. Only meaningful once all 27 bytes are in.
#[inline(always)]
pub const fn rx_finish(a: Rx) -> u64 {
    if a.bad {
        err64(code::CRC)
    } else {
        pack(a.co2, a.pm25, a.voc)
    }
}

/// Get Data Ready's 3 bytes: padding, bool8, CRC (§4.8.3 Table 30). Ok(true) only
/// for a CRC-valid word whose flag byte is 0x01.
#[inline(always)]
pub const fn data_ready(b0: u8, b1: u8, crc: u8) -> Result<bool, u8> {
    match check_word(b0, b1, crc) {
        Some(_) => Ok(b1 == 0x01),
        None => Err(code::CRC),
    }
}

// ───────────────────────────── bus shell ─────────────────────────────

/// The three gust:bus/i2c-master calls. Static dispatch only: the wasm build
/// implements it over the WIT import, the tests and harnesses over models.
pub trait Bus {
    fn begin(&mut self, addr7: u32, count: u32, read: u32) -> u32;
    fn xfer(&mut self, state: u32, out: u32) -> u32;
    fn end(&mut self, state: u32) -> u32;
}

/// Attempts per transaction. The sensor NACKs its address for the command's
/// execution time: 20 ms for reads, 50 ms for start, 1400 ms for stop (Table 27).
/// A NACKed address attempt is about 11 bit times (START, 8 bits, NACK, STOP),
/// about 0.11 ms at 100 kbit/s, so 1400 ms is about 12700 attempts. 20000 leaves
/// margin. This is an iteration bound, not a time bound: a faster bus or a slower
/// caller changes how long it lasts.
#[cfg(not(kani))]
pub const RETRY_MAX: u32 = 20000;
#[cfg(kani)]
pub const RETRY_MAX: u32 = 2;

/// Send one command (2 bytes, no CRC). A NACK anywhere restarts the attempt.
#[inline(always)]
pub fn write_cmd<B: Bus>(b: &mut B, cmd: u16) -> Result<(), u8> {
    let f = frame_cmd(cmd);
    let mut tries = 0;
    while tries < RETRY_MAX {
        tries += 1;
        let s = b.begin(ADDR7, 2, 0);
        if is_err(s) {
            if s as u8 == code::NACK {
                continue;
            }
            return Err(s as u8);
        }
        let s = b.xfer(s, f[0] as u32);
        if is_err(s) {
            if s as u8 == code::NACK {
                continue;
            }
            return Err(s as u8);
        }
        let s = b.xfer(s, f[1] as u32);
        if is_err(s) {
            if s as u8 == code::NACK {
                continue;
            }
            return Err(s as u8);
        }
        let e = b.end(s);
        if e == 0 {
            return Ok(());
        }
        if is_err(e) && e as u8 == code::NACK {
            continue;
        }
        return Err(if is_err(e) { e as u8 } else { code::NO_RESPONSE });
    }
    Err(code::NO_RESPONSE)
}

/// What one read attempt produced.
enum Got {
    Done,
    Retry,
    Fail(u8),
}

/// Read one byte inside a read transaction. Returns the byte and the next state.
#[inline(always)]
fn rd_byte<B: Bus>(b: &mut B, s: u32) -> Result<(u8, u32), Got> {
    let r = b.xfer(s, 0);
    if is_err(r) {
        return Err(if r as u8 == code::NACK { Got::Retry } else { Got::Fail(r as u8) });
    }
    Ok(((r >> 16) as u8, r & 0xFFFF))
}

#[inline(always)]
fn close<B: Bus>(b: &mut B, s: u32) -> Got {
    let e = b.end(s);
    if e == 0 {
        Got::Done
    } else if is_err(e) && e as u8 == code::NACK {
        Got::Retry
    } else if is_err(e) {
        Got::Fail(e as u8)
    } else {
        Got::Fail(code::NO_RESPONSE)
    }
}

/// Read Get Data Ready's 3-byte response.
#[inline(always)]
pub fn read_ready<B: Bus>(b: &mut B) -> Result<bool, u8> {
    let mut tries = 0;
    while tries < RETRY_MAX {
        tries += 1;
        let s = b.begin(ADDR7, 3, 1);
        if is_err(s) {
            if s as u8 == code::NACK {
                continue;
            }
            return Err(s as u8);
        }
        let (b0, s) = match rd_byte(b, s) {
            Ok(v) => v,
            Err(Got::Retry) => continue,
            Err(Got::Fail(c)) => return Err(c),
            Err(Got::Done) => return Err(code::NO_RESPONSE),
        };
        let (b1, s) = match rd_byte(b, s) {
            Ok(v) => v,
            Err(Got::Retry) => continue,
            Err(Got::Fail(c)) => return Err(c),
            Err(Got::Done) => return Err(code::NO_RESPONSE),
        };
        let (b2, s) = match rd_byte(b, s) {
            Ok(v) => v,
            Err(Got::Retry) => continue,
            Err(Got::Fail(c)) => return Err(c),
            Err(Got::Done) => return Err(code::NO_RESPONSE),
        };
        return match close(b, s) {
            Got::Done => data_ready(b0, b1, b2),
            Got::Retry => continue,
            Got::Fail(c) => Err(c),
        };
    }
    Err(code::NO_RESPONSE)
}

/// Read the 27-byte measured-values frame through the accumulator and return the
/// seam value (the packed sample, or crc / a bus error). Returns u64, not
/// `Result<Rx, _>`: an aggregate return goes through a linear-memory stack slot,
/// which the dissolved object must not touch (drivers/check-cross-arch.py).
#[inline(always)]
pub fn read_values<B: Bus>(b: &mut B) -> u64 {
    let mut tries = 0;
    'attempt: while tries < RETRY_MAX {
        tries += 1;
        let mut s = b.begin(ADDR7, MEASURED_BYTES, 1);
        if is_err(s) {
            if s as u8 == code::NACK {
                continue;
            }
            return err64(s as u8);
        }
        let mut a = RX0;
        let mut i = 0;
        while i < MEASURED_BYTES {
            match rd_byte(b, s) {
                Ok((byte, next)) => {
                    a = rx_step(a, byte);
                    s = next;
                }
                Err(Got::Retry) => continue 'attempt,
                Err(Got::Fail(c)) => return err64(c),
                Err(Got::Done) => return err64(code::NO_RESPONSE),
            }
            i += 1;
        }
        return match close(b, s) {
            Got::Done => rx_finish(a),
            Got::Retry => continue,
            Got::Fail(c) => err64(c),
        };
    }
    err64(code::NO_RESPONSE)
}

/// `air.start`: Start Continuous Measurement (0x0021).
#[inline(always)]
pub fn start<B: Bus>(b: &mut B) -> u32 {
    match write_cmd(b, cmd::START_MEASUREMENT) {
        Ok(()) => 0,
        Err(c) => err32(c),
    }
}

/// `air.stop`: Stop Measurement (0x0104).
#[inline(always)]
pub fn stop<B: Bus>(b: &mut B) -> u32 {
    match write_cmd(b, cmd::STOP_MEASUREMENT) {
        Ok(()) => 0,
        Err(c) => err32(c),
    }
}

/// `air.read`: Get Data Ready, then Read Measured Values. Not-ready is an error,
/// so an old sample is never returned as a new one.
#[inline(always)]
pub fn read<B: Bus>(b: &mut B) -> u64 {
    if let Err(c) = write_cmd(b, cmd::GET_DATA_READY) {
        return err64(c);
    }
    match read_ready(b) {
        Ok(true) => {}
        Ok(false) => return err64(code::NOT_READY),
        Err(c) => return err64(c),
    }
    if let Err(c) = write_cmd(b, cmd::READ_MEASURED_VALUES) {
        return err64(c);
    }
    read_values(b)
}

// ───────────────────────────── component ─────────────────────────────

#[cfg(not(any(kani, test)))]
struct Hw;
#[cfg(not(any(kani, test)))]
impl Bus for Hw {
    fn begin(&mut self, addr7: u32, count: u32, read: u32) -> u32 {
        gust::bus::i2c_master::begin(addr7, count, read)
    }
    fn xfer(&mut self, state: u32, out: u32) -> u32 {
        gust::bus::i2c_master::xfer(state, out)
    }
    fn end(&mut self, state: u32) -> u32 {
        gust::bus::i2c_master::end(state)
    }
}

#[cfg(not(any(kani, test)))]
struct Driver;
#[cfg(not(any(kani, test)))]
impl exports::gust::sen66::air::Guest for Driver {
    fn start() -> u32 {
        start(&mut Hw)
    }
    fn read() -> u64 {
        read(&mut Hw)
    }
    fn stop() -> u32 {
        stop(&mut Hw)
    }
}
#[cfg(not(any(kani, test)))]
export!(Driver);

// ───────────────────────────── sensor model ─────────────────────────────

/// A byte-level SEN66 model behind gust:bus, for the tests and harnesses. It
/// records the last command written and serves `ready` after 0x0202 and `vals`
/// after 0x0300. `nacks` address NACKs precede each transaction.
#[cfg(any(test, kani))]
pub struct Model {
    pub ready: [u8; 3],
    pub vals: [u8; 27],
    pub cmd: u16,
    pub nacks: u32,
    pub left: u32,
    pub pos: u32,
    pub wbuf: [u8; 2],
    pub writing: bool,
    /// Kani: an arbitrary gust:bus error may replace any call's result.
    pub chaos: bool,
}

#[cfg(any(test, kani))]
impl Model {
    pub fn new(ready: [u8; 3], vals: [u8; 27], nacks: u32) -> Model {
        Model { ready, vals, cmd: 0, nacks, left: nacks, pos: 0, wbuf: [0; 2], writing: false, chaos: false }
    }
    #[cfg(kani)]
    fn fault(&self) -> Option<u32> {
        if self.chaos && kani::any() {
            let c: u8 = kani::any();
            kani::assume(c >= 1 && c <= 7);
            Some(err32(c))
        } else {
            None
        }
    }
    #[cfg(not(kani))]
    fn fault(&self) -> Option<u32> {
        None
    }
}

#[cfg(any(test, kani))]
impl Bus for Model {
    fn begin(&mut self, _addr7: u32, _count: u32, read: u32) -> u32 {
        if let Some(e) = self.fault() {
            return e;
        }
        if self.left > 0 {
            self.left -= 1;
            return err32(code::NACK);
        }
        self.left = self.nacks;
        self.pos = 0;
        self.writing = read == 0;
        0x200 // active
    }
    fn xfer(&mut self, state: u32, out: u32) -> u32 {
        if let Some(e) = self.fault() {
            return e;
        }
        let p = self.pos as usize;
        self.pos += 1;
        if self.writing {
            if p < 2 {
                self.wbuf[p] = out as u8;
            }
            state
        } else {
            let byte = if self.cmd == cmd::GET_DATA_READY {
                if p < 3 { self.ready[p] } else { 0 }
            } else if p < 27 {
                self.vals[p]
            } else {
                0
            };
            ((byte as u32) << 16) | state
        }
    }
    fn end(&mut self, _state: u32) -> u32 {
        if let Some(e) = self.fault() {
            return e;
        }
        if self.writing && self.pos == 2 {
            self.cmd = ((self.wbuf[0] as u16) << 8) | self.wbuf[1] as u16;
        }
        0
    }
}

/// Build a CRC-valid frame from 9 words.
#[cfg(any(test, kani))]
pub fn frame9(w: [u16; 9]) -> [u8; 27] {
    let mut f = [0u8; 27];
    let mut i = 0;
    while i < 9 {
        let t = frame_word(w[i]);
        f[3 * i] = t[0];
        f[3 * i + 1] = t[1];
        f[3 * i + 2] = t[2];
        i += 1;
    }
    f
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ready(yes: bool) -> [u8; 3] {
        frame_word(yes as u16)
    }

    #[test]
    fn read_through_model_with_nacks() {
        // PM2.5 12.3, VOC 101.0, CO2 612 ppm; the other words are arbitrary.
        let vals = frame9([5, 123, 7, 9, 4500, 5000, 1010, 10, 612]);
        let mut m = Model::new(ready(true), vals, 3);
        let v = read(&mut m);
        assert_eq!(v, 612 | (123 << 16) | (1010 << 32) | VALID_CO2 | VALID_PM25 | VALID_VOC);
    }

    #[test]
    fn not_ready_is_an_error() {
        let mut m = Model::new(ready(false), frame9([1; 9]), 0);
        assert_eq!(read(&mut m), err64(code::NOT_READY));
    }

    #[test]
    fn unknown_fields_clear_their_valid_bit() {
        let vals = frame9([0, 0xFFFF, 0, 0, 0, 0, 0x7FFF, 0, 0xFFFF]);
        let mut m = Model::new(ready(true), vals, 0);
        assert_eq!(read(&mut m), 0);
    }

    #[test]
    fn crc_failure_is_an_error() {
        let mut vals = frame9([1; 9]);
        vals[26] ^= 0x01; // CO2's CRC
        let mut m = Model::new(ready(true), vals, 0);
        assert_eq!(read(&mut m), err64(code::CRC));
    }

    #[test]
    fn negative_voc_keeps_its_bit_pattern() {
        let vals = frame9([0, 0, 0, 0, 0, 0, (-25i16) as u16, 0, 400]);
        let mut m = Model::new(ready(true), vals, 0);
        let v = read(&mut m);
        assert_eq!((v >> 32) as u16 as i16, -25);
        assert_ne!(v & VALID_VOC, 0);
    }

    #[test]
    fn sensor_that_never_answers_is_no_response() {
        let mut m = Model::new(ready(true), frame9([1; 9]), u32::MAX);
        assert_eq!(start(&mut m), err32(code::NO_RESPONSE));
    }

    #[test]
    fn datasheet_crc_example() {
        assert_eq!(crc8(0xBEEF), 0x92);
    }

    #[test]
    fn frame_and_check_round_trip() {
        let f = frame_word(0x1234);
        assert_eq!(check_word(f[0], f[1], f[2]), Some(0x1234));
        assert_eq!(check_word(f[0], f[1], f[2] ^ 1), None);
    }

    #[test]
    fn unknown_values_follow_signedness() {
        assert_eq!(Field::Pm25.unknown(), 0xFFFF);
        assert_eq!(Field::Co2.unknown(), 0xFFFF);
        assert_eq!(Field::Voc.unknown(), 0x7FFF);
        assert_eq!(Field::T.unknown(), 0x7FFF);
        assert!(known(Field::Voc, 0xFFFF)); // -1 is a real int16 reading of VOC
        assert!(!known(Field::Pm25, 0xFFFF));
    }
}

#[cfg(kani)]
mod proofs {
    use super::*;

    /// c1: the implementation matches the datasheet's worked example.
    #[kani::proof]
    fn c1_datasheet_example() {
        assert!(crc8(0xBEEF) == 0x92);
    }

    /// c2: a single flipped bit anywhere in a received word (16 data bits plus 8 CRC
    /// bits) is rejected.
    #[kani::proof]
    fn c2_single_bit_error_rejected() {
        let w: u16 = kani::any();
        let bit: u32 = kani::any();
        kani::assume(bit < 24);
        let mut f = frame_word(w);
        f[(bit / 8) as usize] ^= 1 << (bit % 8);
        assert!(check_word(f[0], f[1], f[2]).is_none());
    }

    /// c3: any two flipped bits in a received word are rejected.
    #[kani::proof]
    fn c3_double_bit_error_rejected() {
        let w: u16 = kani::any();
        let i: u32 = kani::any();
        let j: u32 = kani::any();
        kani::assume(i < j && j < 24);
        let mut f = frame_word(w);
        f[(i / 8) as usize] ^= 1 << (i % 8);
        f[(j / 8) as usize] ^= 1 << (j % 8);
        assert!(check_word(f[0], f[1], f[2]).is_none());
    }

    /// c4: check_word accepts exactly the words framed by frame_word.
    #[kani::proof]
    fn c4_check_accepts_exactly_valid() {
        let (m, l, c): (u8, u8, u8) = (kani::any(), kani::any(), kani::any());
        let w = ((m as u16) << 8) | l as u16;
        match check_word(m, l, c) {
            Some(v) => assert!(v == w && frame_word(w) == [m, l, c]),
            None => assert!(c != crc8(w)),
        }
    }

    /// c5: field decoding is total over 0..9, and each field has exactly one
    /// "unknown" raw value, which matches its signedness in Table 34.
    #[kani::proof]
    fn c5_unknown_is_exact() {
        let i: u32 = kani::any();
        let raw: u16 = kani::any();
        match Field::from_index(i) {
            Some(f) => {
                assert!(i < MEASURED_WORDS as u32);
                assert!(f as u32 == i);
                assert!(known(f, raw) == (raw != if i >= 4 && i <= 7 { 0x7FFF } else { 0xFFFF }));
            }
            None => assert!(i >= MEASURED_WORDS as u32),
        }
    }

    /// c6: pack follows the seam encoding for every input. A valid bit is set
    /// exactly when its field is known, an unknown field's value bits are 0, and
    /// bits 51..=63 are 0.
    #[kani::proof]
    fn c6_pack_encoding() {
        let (co2, pm25, voc): (u16, u16, u16) = (kani::any(), kani::any(), kani::any());
        let v = pack(co2, pm25, voc);
        assert!(v >> 51 == 0);
        assert!((v & VALID_CO2 != 0) == (co2 != 0xFFFF));
        assert!((v & VALID_PM25 != 0) == (pm25 != 0xFFFF));
        assert!((v & VALID_VOC != 0) == (voc != 0x7FFF));
        assert!((v & 0xFFFF) as u16 == if co2 != 0xFFFF { co2 } else { 0 });
        assert!(((v >> 16) & 0xFFFF) as u16 == if pm25 != 0xFFFF { pm25 } else { 0 });
        assert!(((v >> 32) & 0xFFFF) as u16 == if voc != 0x7FFF { voc } else { 0 });
    }

    /// c7: over every 27-byte frame, the accumulator flags a CRC failure exactly
    /// when some word fails, and otherwise keeps words 1, 6 and 8.
    #[kani::proof]
    #[kani::unwind(28)]
    fn c7_accumulator_matches_frame() {
        let f: [u8; 27] = kani::any();
        let mut a = RX0;
        let mut i = 0;
        while i < 27 {
            a = rx_step(a, f[i]);
            i += 1;
        }
        let mut any_bad = false;
        let mut k = 0;
        while k < 9 {
            if check_word(f[3 * k], f[3 * k + 1], f[3 * k + 2]).is_none() {
                any_bad = true;
            }
            k += 1;
        }
        assert!(a.n == 27);
        assert!(a.bad == any_bad);
        if !any_bad {
            assert!(a.pm25 == ((f[3] as u16) << 8 | f[4] as u16));
            assert!(a.voc == ((f[18] as u16) << 8 | f[19] as u16));
            assert!(a.co2 == ((f[24] as u16) << 8 | f[25] as u16));
        }
    }

    /// c8, the seam's kill criterion: against a sensor model serving arbitrary
    /// bytes, with any bus call able to fail with any gust:bus error and up to one
    /// address NACK per transaction, `read` returns a sample only when Get Data
    /// Ready was CRC-valid and true and every one of the 9 words passed its CRC.
    /// The sample is then exactly `pack` of words 8, 1 and 6. Every other outcome
    /// is a well-formed error with a defined code.
    #[kani::proof]
    #[kani::unwind(30)]
    fn c8_read_never_returns_a_bad_sample() {
        let ready: [u8; 3] = kani::any();
        let vals: [u8; 27] = kani::any();
        let nacks: u32 = kani::any();
        kani::assume(nacks <= 1);
        let mut m = Model::new(ready, vals, nacks);
        m.chaos = true;
        let v = read(&mut m);
        if v & ERR64 == 0 {
            assert!(matches!(data_ready(ready[0], ready[1], ready[2]), Ok(true)));
            let mut k = 0;
            while k < 9 {
                assert!(check_word(vals[3 * k], vals[3 * k + 1], vals[3 * k + 2]).is_some());
                k += 1;
            }
            let w = |i: usize| ((vals[3 * i] as u16) << 8) | vals[3 * i + 1] as u16;
            assert!(v == pack(w(8), w(1), w(6)));
        } else {
            let c = (v & 0xFF) as u8;
            assert!(v & !0xFF == ERR64);
            assert!((c >= 1 && c <= 7) || c == code::NOT_READY || c == code::CRC || c == code::NO_RESPONSE);
        }
    }

    /// c9: start and stop return 0 or a well-formed u32 error, under the same
    /// faulty bus.
    #[kani::proof]
    #[kani::unwind(6)]
    fn c9_start_stop_encoding() {
        let nacks: u32 = kani::any();
        kani::assume(nacks <= 2);
        let mut m = Model::new([0; 3], [0; 27], nacks);
        m.chaos = true;
        let r = if kani::any() { start(&mut m) } else { stop(&mut m) };
        assert!(r == 0 || (is_err(r) && (((r as u8) >= 1 && (r as u8) <= 7) || r as u8 == code::NO_RESPONSE)));
    }
}

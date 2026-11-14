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
//! This file holds what does not depend on how the reading crosses the seam to
//! wohl: CRC, word checking, the command table, and which raw values mean
//! "unknown". The exported interface follows the #511 decision on representation.
#![cfg_attr(not(any(kani, test)), no_std)]

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

#[cfg(test)]
mod tests {
    use super::*;

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
}

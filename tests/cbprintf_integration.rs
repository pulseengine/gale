//! Integration tests for the cbprintf validation model.
//!
//! Covers properties CB1–CB5 from src/cbprintf.rs:
//!   CB1: FormatSpec fields are within representable bounds
//!   CB2: PackageState never exceeds buffer capacity
//!   CB3: OutputState length tracking is monotone and bounded
//!   CB4: Dangerous conversion specifiers are rejected
//!   CB5: %n is always rejected

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::indexing_slicing
)]

use gale::cbprintf::{
    ConversionSpecifier, FormatSpec, LengthModifier, OutputState, PackageState,
    MAX_OUTPUT_LEN, MAX_PACKAGE_BUF, MAX_WIDTH_PREC,
    package_bounds_check, output_bounds_check, validate_format_spec,
    validate_specifier_char,
};
use gale::error::{EINVAL, ENOMEM};

// ==========================================================================
// CB1 — FormatSpec invariant: width and precision bounds
// ==========================================================================

#[test]
fn cb1_width_within_max() {
    let fs = FormatSpec::new(
        false, false, false, false, false,
        true, false, MAX_WIDTH_PREC,
        false, false, 0,
        LengthModifier::None,
        ConversionSpecifier::SignedInt,
    );
    assert!(fs.width_value <= MAX_WIDTH_PREC);
}

#[test]
fn cb1_width_clamped_on_overflow() {
    // Supply u32::MAX — new() must clamp to MAX_WIDTH_PREC
    let fs = FormatSpec::new(
        false, false, false, false, false,
        true, false, u32::MAX,
        false, false, 0,
        LengthModifier::None,
        ConversionSpecifier::SignedInt,
    );
    assert_eq!(fs.width_value, MAX_WIDTH_PREC);
}

#[test]
fn cb1_prec_within_max() {
    let fs = FormatSpec::new(
        false, false, false, false, false,
        false, false, 0,
        true, false, MAX_WIDTH_PREC,
        LengthModifier::None,
        ConversionSpecifier::String,
    );
    assert!(fs.prec_value <= MAX_WIDTH_PREC);
}

#[test]
fn cb1_prec_clamped_on_overflow() {
    let fs = FormatSpec::new(
        false, false, false, false, false,
        false, false, 0,
        true, false, u32::MAX,
        LengthModifier::None,
        ConversionSpecifier::String,
    );
    assert_eq!(fs.prec_value, MAX_WIDTH_PREC);
}

#[test]
fn cb1_flag_zero_cleared_when_dash_set() {
    // C99 §7.19.6.1: '-' overrides '0'
    let fs = FormatSpec::new(
        true,  // flag_dash
        false, false, false,
        true,  // flag_zero (should be cleared)
        false, false, 0,
        false, false, 0,
        LengthModifier::None,
        ConversionSpecifier::SignedInt,
    );
    assert!(fs.flag_dash);
    assert!(!fs.flag_zero, "flag_zero must be cleared when flag_dash is set");
}

#[test]
fn cb1_flag_zero_set_without_dash() {
    let fs = FormatSpec::new(
        false, // flag_dash
        false, false, false,
        true,  // flag_zero (should stay)
        false, false, 0,
        false, false, 0,
        LengthModifier::None,
        ConversionSpecifier::SignedInt,
    );
    assert!(!fs.flag_dash);
    assert!(fs.flag_zero, "flag_zero must be preserved when flag_dash is not set");
}

// ==========================================================================
// CB4 / CB5 — validate_format_spec and validate_specifier_char
// ==========================================================================

#[test]
fn cb5_writeback_rejected_by_validate() {
    let fs = FormatSpec::new(
        false, false, false, false, false,
        false, false, 0,
        false, false, 0,
        LengthModifier::None,
        ConversionSpecifier::WriteBack,
    );
    assert_eq!(
        validate_format_spec(&fs),
        Err(EINVAL),
        "%n must always be rejected"
    );
}

#[test]
fn cb4_invalid_specifier_rejected_by_validate() {
    let fs = FormatSpec::new(
        false, false, false, false, false,
        false, false, 0,
        false, false, 0,
        LengthModifier::None,
        ConversionSpecifier::Invalid,
    );
    assert_eq!(
        validate_format_spec(&fs),
        Err(EINVAL),
        "Invalid specifier must be rejected"
    );
}

#[test]
fn cb4_safe_specifiers_accepted() {
    let safe = [
        ConversionSpecifier::SignedInt,
        ConversionSpecifier::UnsignedInt,
        ConversionSpecifier::Octal,
        ConversionSpecifier::Hex,
        ConversionSpecifier::Char,
        ConversionSpecifier::String,
        ConversionSpecifier::Pointer,
        ConversionSpecifier::Percent,
    ];
    for spec in &safe {
        let fs = FormatSpec::new(
            false, false, false, false, false,
            false, false, 0,
            false, false, 0,
            LengthModifier::None,
            *spec,
        );
        assert_eq!(
            validate_format_spec(&fs),
            Ok(()),
            "{spec:?} should be accepted"
        );
    }
}

#[test]
fn cb5_validate_specifier_char_n_rejected() {
    assert_ne!(
        validate_specifier_char(b'n'),
        0,
        "%n character must be rejected"
    );
}

#[test]
fn cb5_validate_specifier_char_n_returns_einval() {
    assert_eq!(
        validate_specifier_char(b'n'),
        EINVAL,
        "%n must return -EINVAL"
    );
}

#[test]
fn cb4_validate_specifier_char_safe_chars_accepted() {
    let safe_chars = [b'd', b'i', b'u', b'o', b'x', b'X', b'c', b's', b'p', b'%'];
    for &ch in &safe_chars {
        assert_eq!(
            validate_specifier_char(ch),
            0,
            "'{}' should be accepted (returned non-zero)",
            ch as char
        );
    }
}

#[test]
fn cb4_validate_specifier_char_unknown_rejected() {
    // Arbitrary non-format characters
    for ch in [b'q', b'Q', b'y', b'Y', b'b', b'B', b'@', b'!'] {
        assert_ne!(
            validate_specifier_char(ch),
            0,
            "'{}' (unknown) should be rejected",
            ch as char
        );
    }
}

// ==========================================================================
// CB2 — PackageState: buffer bounds never exceeded
// ==========================================================================

#[test]
fn cb2_fresh_state_zero_pos() {
    let s = PackageState::new(256);
    assert_eq!(s.pos, 0);
    assert_eq!(s.capacity, 256);
    assert_eq!(s.remaining(), 256);
}

#[test]
fn cb2_advance_within_capacity_succeeds() {
    let s = PackageState::new(256);
    let s2 = s.advance(100).expect("advance within capacity must succeed");
    assert_eq!(s2.pos, 100);
    assert_eq!(s2.capacity, 256);
    assert_eq!(s2.remaining(), 156);
}

#[test]
fn cb2_advance_to_exact_capacity_succeeds() {
    let s = PackageState::new(256);
    let s2 = s.advance(256).expect("advance to capacity must succeed");
    assert_eq!(s2.pos, 256);
    assert_eq!(s2.remaining(), 0);
}

#[test]
fn cb2_advance_beyond_capacity_returns_enomem() {
    let s = PackageState::new(256);
    assert_eq!(
        s.advance(257),
        Err(ENOMEM),
        "advance beyond capacity must return ENOMEM"
    );
}

#[test]
fn cb2_advance_by_zero_always_succeeds() {
    let s = PackageState::new(0);
    let s2 = s.advance(0).expect("advance by zero must always succeed");
    assert_eq!(s2.pos, 0);
}

#[test]
fn cb2_total_len_tracks_position() {
    let s = PackageState::new(1024);
    let s2 = s.advance(512).unwrap();
    assert_eq!(s2.total_len(), 512);
}

#[test]
fn cb2_package_bounds_check_within_ok() {
    let s = PackageState::new(512);
    let s2 = package_bounds_check(s, 200).expect("within bounds must succeed");
    assert_eq!(s2.pos, 200);
}

#[test]
fn cb2_package_bounds_check_overflow_err() {
    let s = PackageState::new(512);
    assert_eq!(
        package_bounds_check(s, 600),
        Err(ENOMEM),
        "overflow must return ENOMEM"
    );
}

#[test]
fn cb2_sequential_advances_accumulate() {
    let s0 = PackageState::new(1024);
    let s1 = s0.advance(100).unwrap();
    let s2 = s1.advance(200).unwrap();
    let s3 = s2.advance(300).unwrap();
    assert_eq!(s3.pos, 600);
    assert_eq!(s3.remaining(), 424);
}

#[test]
fn cb2_advance_from_full_buffer_fails() {
    let s = PackageState::new(100);
    let full = s.advance(100).unwrap();
    assert_eq!(full.remaining(), 0);
    assert_eq!(
        full.advance(1),
        Err(ENOMEM),
        "cannot advance from a full buffer"
    );
}

#[test]
fn cb2_max_package_buf_capacity() {
    let s = PackageState::new(MAX_PACKAGE_BUF);
    assert_eq!(s.capacity, MAX_PACKAGE_BUF);
    // Writing the entire buffer in one shot must succeed
    let full = s.advance(MAX_PACKAGE_BUF).expect("fill entire buffer");
    assert_eq!(full.pos, MAX_PACKAGE_BUF);
}

// ==========================================================================
// CB3 — OutputState: monotone, bounded length tracking
// ==========================================================================

#[test]
fn cb3_fresh_state_is_zero() {
    let s = OutputState::new();
    assert_eq!(s.count, 0);
    assert!(!s.overflow);
    assert_eq!(s.result(), Some(0));
}

#[test]
fn cb3_add_bytes_accumulates() {
    let s = OutputState::new();
    let s2 = s.add_bytes(100);
    assert_eq!(s2.count, 100);
    assert!(!s2.overflow);

    let s3 = s2.add_bytes(200);
    assert_eq!(s3.count, 300);
    assert!(!s3.overflow);
}

#[test]
fn cb3_add_bytes_is_monotone() {
    let s = OutputState::new();
    let s2 = s.add_bytes(42);
    assert!(s2.count >= s.count, "count must never decrease");
}

#[test]
fn cb3_overflow_detected_on_saturation() {
    // Force overflow: add more than MAX_OUTPUT_LEN - current count
    let s = OutputState::new();
    // Set count close to max
    let big = OutputState { count: MAX_OUTPUT_LEN - 1, overflow: false };
    let overflowed = big.add_bytes(2);  // 2 > remaining 1
    assert!(overflowed.overflow, "overflow must be flagged");
    assert_eq!(overflowed.count, MAX_OUTPUT_LEN);
    assert_eq!(overflowed.result(), None, "overflowed result must be None");
    // Make sure the base case also works
    let _ = s;
}

#[test]
fn cb3_overflow_does_not_panic_on_exact_max() {
    let big = OutputState { count: MAX_OUTPUT_LEN, overflow: false };
    let s2 = big.add_bytes(0);
    assert_eq!(s2.count, MAX_OUTPUT_LEN);
    assert!(!s2.overflow);
    assert_eq!(s2.result(), Some(MAX_OUTPUT_LEN));
}

#[test]
fn cb3_result_returns_some_when_no_overflow() {
    let s = OutputState::new().add_bytes(42).add_bytes(58);
    assert_eq!(s.result(), Some(100));
}

#[test]
fn cb3_result_returns_none_on_overflow() {
    let s = OutputState { count: MAX_OUTPUT_LEN, overflow: true };
    assert_eq!(s.result(), None);
}

#[test]
fn cb3_output_bounds_check_accumulates() {
    let s = OutputState::new();
    let s2 = output_bounds_check(s, 500);
    assert_eq!(s2.count, 500);
    assert!(!s2.overflow);
}

#[test]
fn cb3_output_bounds_check_overflow_flagged() {
    let near_max = OutputState { count: MAX_OUTPUT_LEN - 1, overflow: false };
    let s = output_bounds_check(near_max, 2);
    assert!(s.overflow);
    assert_eq!(s.result(), None);
}

#[test]
fn cb3_add_zero_bytes_is_no_op() {
    let s = OutputState::new().add_bytes(100);
    let s2 = s.add_bytes(0);
    assert_eq!(s2.count, 100);
    assert!(!s2.overflow);
}

// ==========================================================================
// Combined / cross-property tests
// ==========================================================================

#[test]
fn combined_valid_spec_with_width_and_prec() {
    // A typical "%-10.5s" style spec
    let fs = FormatSpec::new(
        true,  // flag_dash
        false, false, false,
        false, // flag_zero (irrelevant, flag_dash wins anyway)
        true,  // width_present
        false, // width_star
        10,    // width_value
        true,  // prec_present
        false, // prec_star
        5,     // prec_value
        LengthModifier::None,
        ConversionSpecifier::String,
    );
    assert!(fs.flag_dash);
    assert!(!fs.flag_zero);
    assert_eq!(fs.width_value, 10);
    assert_eq!(fs.prec_value, 5);
    assert_eq!(validate_format_spec(&fs), Ok(()));
}

#[test]
fn combined_package_then_output_tracking() {
    // Simulate packaging 3 args then writing the output
    let pkg = PackageState::new(256);
    let pkg = pkg.advance(8).unwrap();   // header
    let pkg = pkg.advance(4).unwrap();   // arg 1 (u32)
    let pkg = pkg.advance(8).unwrap();   // arg 2 (u64, aligned)
    assert_eq!(pkg.total_len(), 20);

    let out = OutputState::new();
    let out = out.add_bytes(12);         // "hello world\n"
    let out = out.add_bytes(5);          // another token
    assert_eq!(out.result(), Some(17));
}

#[test]
fn combined_n_specifier_in_full_pipeline() {
    // Simulate what a format scanner would do with %n in the string
    let result = validate_specifier_char(b'n');
    assert_eq!(result, EINVAL);

    // Confirm this propagates through validate_format_spec too
    let fs = FormatSpec::new(
        false, false, false, false, false,
        false, false, 0,
        false, false, 0,
        LengthModifier::None,
        ConversionSpecifier::WriteBack,
    );
    assert!(validate_format_spec(&fs).is_err());
}

#[test]
fn length_modifiers_accepted() {
    let mods = [
        LengthModifier::None,
        LengthModifier::Hh,
        LengthModifier::H,
        LengthModifier::L,
        LengthModifier::Ll,
        LengthModifier::J,
        LengthModifier::Z,
        LengthModifier::T,
        LengthModifier::UpperL,
    ];
    for lm in &mods {
        let fs = FormatSpec::new(
            false, false, false, false, false,
            false, false, 0,
            false, false, 0,
            *lm,
            ConversionSpecifier::SignedInt,
        );
        assert_eq!(
            validate_format_spec(&fs),
            Ok(()),
            "{lm:?} with SignedInt must be accepted"
        );
    }
}

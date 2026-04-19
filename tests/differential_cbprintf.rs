//! Differential equivalence tests — cbprintf (FFI vs Model).
//!
//! Verifies that the FFI cbprintf validation functions produce the same results
//! as the Verus-verified model functions in gale::cbprintf.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::unwrap_used,
    clippy::fn_params_excessive_bools,
    clippy::absurd_extreme_comparisons,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::checked_conversions,
    clippy::wildcard_enum_match_arm,
    clippy::implicit_saturating_sub,
    clippy::branches_sharing_code,
    clippy::panic,
    clippy::expect_used
)]

use gale::error::*;
use gale::cbprintf::{
    ConversionSpecifier, FormatSpec, LengthModifier, PackageState,
    MAX_PACKAGE_BUF, MAX_WIDTH_PREC,
    validate_specifier_char, package_bounds_check,
};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_cbprintf_validate_specifier.
fn ffi_validate_specifier_char(ch: u8) -> i32 {
    // CB4 + CB5: %n and unknown specifiers always rejected
    match ch {
        b'd' | b'i' | b'u' | b'o' | b'x' | b'X' | b'c' | b's' | b'p' | b'%' => 0,
        b'n' => EINVAL,
        _ => EINVAL,
    }
}

/// Replica of gale_cbprintf_package_bounds_check.
fn ffi_package_bounds_check(pos: usize, capacity: usize, size: usize) -> i32 {
    // clamp capacity to MAX_PACKAGE_BUF
    let cap = if capacity > MAX_PACKAGE_BUF {
        MAX_PACKAGE_BUF
    } else {
        capacity
    };
    // pos > cap is already invalid
    if pos > cap {
        return ENOMEM;
    }
    // check if writing `size` bytes from pos would exceed cap
    if size > cap - pos {
        ENOMEM
    } else {
        0
    }
}

// =====================================================================
// Model wrapper helpers
// =====================================================================

fn make_spec(ch: u8, width: u32, prec: u32, flag_dash: bool, flag_zero: bool) -> FormatSpec {
    let specifier = match ch {
        b'd' | b'i' => ConversionSpecifier::SignedInt,
        b'u' => ConversionSpecifier::UnsignedInt,
        b'o' => ConversionSpecifier::Octal,
        b'x' | b'X' => ConversionSpecifier::Hex,
        b'c' => ConversionSpecifier::Char,
        b's' => ConversionSpecifier::String,
        b'p' => ConversionSpecifier::Pointer,
        b'%' => ConversionSpecifier::Percent,
        b'n' => ConversionSpecifier::WriteBack,
        _ => ConversionSpecifier::Invalid,
    };
    FormatSpec::new(
        flag_dash, false, false, false, flag_zero,
        width > 0, false, width,
        prec > 0, false, prec,
        LengthModifier::None,
        specifier,
    )
}

// =====================================================================
// Differential tests: validate_specifier_char
// =====================================================================

#[test]
fn cbprintf_validate_specifier_ffi_matches_model_valid_chars() {
    let valid = [b'd', b'i', b'u', b'o', b'x', b'X', b'c', b's', b'p', b'%'];
    for ch in valid {
        let ffi_rc = ffi_validate_specifier_char(ch);
        let model_rc = validate_specifier_char(ch);
        assert_eq!(ffi_rc, model_rc,
            "specifier mismatch for valid char: ch=0x{ch:02x} ('{}')",
            ch as char);
        assert_eq!(ffi_rc, 0,
            "valid specifier must return 0: ch='{}'", ch as char);
    }
}

#[test]
fn cbprintf_validate_specifier_ffi_matches_model_n_rejected() {
    // CB5: %n always rejected
    let ffi_rc = ffi_validate_specifier_char(b'n');
    let model_rc = validate_specifier_char(b'n');
    assert_eq!(ffi_rc, model_rc, "CB5: both FFI and model must reject %n");
    assert_eq!(ffi_rc, EINVAL, "CB5: %n must return EINVAL");
}

#[test]
fn cbprintf_validate_specifier_ffi_matches_model_unknown_rejected() {
    // CB4: unknown specifiers rejected
    let unknown = [b'a', b'b', b'e', b'f', b'g', b'h', b'j', b'k', b'l',
                   b'm', b'q', b'r', b't', b'v', b'w', b'y', b'z', b'A',
                   b'B', b'C', b'D', b'E', b'F', b'G', b'H', b'0', b'1',
                   b' ', b'!', b'@', b'#', b'$', 0u8, 127u8, 255u8];
    for ch in unknown {
        let ffi_rc = ffi_validate_specifier_char(ch);
        let model_rc = validate_specifier_char(ch);
        assert_eq!(ffi_rc, model_rc,
            "specifier mismatch for unknown char: ch=0x{ch:02x}");
        assert_eq!(ffi_rc, EINVAL,
            "CB4: unknown specifier 0x{ch:02x} must return EINVAL");
    }
}

#[test]
fn cbprintf_validate_specifier_exhaustive_ascii() {
    for ch in 0u8..=127u8 {
        let ffi_rc = ffi_validate_specifier_char(ch);
        let model_rc = validate_specifier_char(ch);
        assert_eq!(ffi_rc, model_rc,
            "specifier exhaustive mismatch: ch=0x{ch:02x} ('{}')",
            if ch.is_ascii_graphic() { ch as char } else { '?' });
    }
}

// =====================================================================
// Differential tests: FormatSpec validate (CB1, CB4, CB5)
// =====================================================================

#[test]
fn cbprintf_format_spec_valid_specifiers_pass() {
    for ch in [b'd', b'i', b'u', b'o', b'x', b'X', b'c', b's', b'p', b'%'] {
        let spec = make_spec(ch, 0, 0, false, false);
        let result = spec.validate();
        assert!(result.is_ok(),
            "valid specifier '{}' should pass validate", ch as char);
    }
}

#[test]
fn cbprintf_format_spec_writeback_rejected() {
    // CB5: WriteBack always rejected
    let spec = make_spec(b'n', 0, 0, false, false);
    let result = spec.validate();
    assert_eq!(result, Err(EINVAL), "CB5: WriteBack must be rejected");
}

#[test]
fn cbprintf_format_spec_invalid_rejected() {
    let spec = FormatSpec::new(
        false, false, false, false, false,
        false, false, 0,
        false, false, 0,
        LengthModifier::None,
        ConversionSpecifier::Invalid,
    );
    let result = spec.validate();
    assert_eq!(result, Err(EINVAL), "CB4: Invalid specifier must be rejected");
}

#[test]
fn cbprintf_format_spec_width_clamped_to_max() {
    // CB1: width/prec values beyond MAX_WIDTH_PREC are clamped
    let over = MAX_WIDTH_PREC + 1;
    let spec = FormatSpec::new(
        false, false, false, false, false,
        true, false, over,
        false, false, 0,
        LengthModifier::None,
        ConversionSpecifier::SignedInt,
    );
    assert_eq!(spec.width_value, MAX_WIDTH_PREC,
        "CB1: width clamped to MAX_WIDTH_PREC");
}

#[test]
fn cbprintf_format_spec_prec_clamped_to_max() {
    let over = MAX_WIDTH_PREC + 1;
    let spec = FormatSpec::new(
        false, false, false, false, false,
        false, false, 0,
        true, false, over,
        LengthModifier::None,
        ConversionSpecifier::SignedInt,
    );
    assert_eq!(spec.prec_value, MAX_WIDTH_PREC,
        "CB1: precision clamped to MAX_WIDTH_PREC");
}

#[test]
fn cbprintf_format_spec_dash_clears_zero_flag() {
    // '-' flag overrides '0' flag per C standard
    let spec = FormatSpec::new(
        true, false, false, false, true, // flag_dash=true, flag_zero=true
        false, false, 0,
        false, false, 0,
        LengthModifier::None,
        ConversionSpecifier::SignedInt,
    );
    assert!(spec.flag_dash, "flag_dash should be set");
    assert!(!spec.flag_zero, "flag_zero cleared when flag_dash set");
}

// =====================================================================
// Differential tests: package_bounds_check (CB2)
// =====================================================================

#[test]
fn cbprintf_package_bounds_check_ffi_matches_model_valid() {
    let cases = [
        (0usize, 256usize, 64usize),
        (64, 256, 64),
        (255, 256, 1),
        (256, 256, 0),
        (0, 1024, 512),
    ];
    for (pos, cap, size) in cases {
        let ffi_rc = ffi_package_bounds_check(pos, cap, size);
        let state = PackageState { pos, capacity: cap };
        let model = package_bounds_check(state, size);
        let model_rc = match model {
            Ok(_) => 0i32,
            Err(e) => e,
        };
        assert_eq!(ffi_rc, model_rc,
            "CB2: package_bounds_check mismatch: pos={pos}, cap={cap}, size={size}");
    }
}

#[test]
fn cbprintf_package_bounds_check_overflow_detected() {
    // Writing past end of buffer
    let ffi_rc = ffi_package_bounds_check(200, 256, 100);
    assert_eq!(ffi_rc, ENOMEM,
        "CB2: write past end must return ENOMEM");

    let state = PackageState { pos: 200, capacity: 256 };
    let model = package_bounds_check(state, 100);
    assert_eq!(model, Err(ENOMEM));
}

#[test]
fn cbprintf_package_bounds_check_exact_fit() {
    // Writing exactly the remaining bytes — should succeed
    let pos = 100usize;
    let cap = 200usize;
    let size = cap - pos;

    let ffi_rc = ffi_package_bounds_check(pos, cap, size);
    assert_eq!(ffi_rc, 0, "CB2: exact fit should succeed");

    let state = PackageState { pos, capacity: cap };
    let model = package_bounds_check(state, size);
    assert!(model.is_ok(), "CB2: model exact fit should succeed");
}

#[test]
fn cbprintf_package_bounds_check_capacity_clamped() {
    // capacity > MAX_PACKAGE_BUF is clamped to MAX_PACKAGE_BUF
    let huge_cap = MAX_PACKAGE_BUF + 1;
    let ffi_rc = ffi_package_bounds_check(0, huge_cap, 64);
    // After clamping pos=0 <= MAX_PACKAGE_BUF, size=64 <= MAX_PACKAGE_BUF
    assert_eq!(ffi_rc, 0,
        "clamped capacity: small write at pos=0 should succeed");
}

#[test]
fn cbprintf_package_state_advance_monotone() {
    // CB2: PackageState.advance is monotone — pos always increases
    let state0 = PackageState::new(1024);
    assert_eq!(state0.pos, 0);
    assert_eq!(state0.capacity, 1024);

    let state1 = state0.advance(100).expect("advance(100) should succeed");
    assert_eq!(state1.pos, 100);

    let state2 = state1.advance(200).expect("advance(200) should succeed");
    assert_eq!(state2.pos, 300);

    // overflow
    let over = state2.advance(800);
    assert_eq!(over, Err(ENOMEM),
        "CB2: advance past capacity must return ENOMEM");
}

// =====================================================================
// Property: CB3 — output length tracking monotone and bounded
// =====================================================================

#[test]
fn cbprintf_output_state_monotone() {
    use gale::cbprintf::{OutputState, MAX_OUTPUT_LEN};

    let mut state = OutputState::new();
    for _ in 0..10 {
        let prev = state.count;
        state = state.add_bytes(100);
        assert!(state.count >= prev, "CB3: output count must be non-decreasing");
        assert!(state.count <= MAX_OUTPUT_LEN, "CB3: output count must not exceed MAX");
    }
}

#[test]
fn cbprintf_output_state_overflow_saturates() {
    use gale::cbprintf::{OutputState, MAX_OUTPUT_LEN};

    let state = OutputState { count: MAX_OUTPUT_LEN - 1, overflow: false };
    let next = state.add_bytes(2); // would exceed MAX_OUTPUT_LEN
    assert!(next.overflow, "CB3: overflow flag must be set");
    assert_eq!(next.count, MAX_OUTPUT_LEN, "CB3: count saturates at MAX_OUTPUT_LEN");
    assert_eq!(next.result(), None, "CB3: result() is None on overflow");
}

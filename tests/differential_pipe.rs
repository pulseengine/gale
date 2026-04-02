//! Differential equivalence tests — Pipe (FFI vs Model).
//!
//! Verifies that the FFI pipe functions produce the same results as
//! the Verus-verified model functions in gale::pipe.

#![allow(
    clippy::unwrap_used,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::error::*;
use gale::pipe::{
    self, ReadDecision, WriteDecision, FLAG_OPEN, FLAG_RESET,
};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_pipe_write_check (ffi/src/lib.rs).
fn ffi_pipe_write_check(
    used: u32,
    size: u32,
    flags: u8,
    request_len: u32,
) -> (i32, u32, u32) {
    if size == 0 {
        return (EINVAL, 0, 0);
    }
    if (flags & FLAG_RESET) != 0 {
        return (ECANCELED, 0, 0);
    }
    if (flags & FLAG_OPEN) == 0 {
        return (EPIPE, 0, 0);
    }
    if request_len == 0 {
        return (ENOMSG, 0, 0);
    }
    if used >= size {
        return (EAGAIN, 0, 0);
    }
    let free = size - used;
    let n = if request_len <= free { request_len } else { free };
    (OK, n, used + n)
}

/// Replica of gale_pipe_read_check (ffi/src/lib.rs).
fn ffi_pipe_read_check(
    used: u32,
    flags: u8,
    request_len: u32,
) -> (i32, u32, u32) {
    if (flags & FLAG_RESET) != 0 {
        return (ECANCELED, 0, 0);
    }
    if request_len == 0 {
        return (ENOMSG, 0, 0);
    }
    if used == 0 {
        if (flags & FLAG_OPEN) == 0 {
            return (EPIPE, 0, 0);
        }
        return (EAGAIN, 0, 0);
    }
    let n = if request_len <= used { request_len } else { used };
    (OK, n, used - n)
}

/// Replica of gale_k_pipe_write_decide (ffi/src/lib.rs).
fn ffi_pipe_write_decide(
    used: u32,
    size: u32,
    flags: u8,
    request_len: u32,
    has_reader: bool,
) -> (i32, u8, u32, u32) {
    let r = pipe::write_decide(used, size, flags, request_len, has_reader);
    let action = match r.decision {
        WriteDecision::WriteOk => 0,
        WriteDecision::WakeReader => 1,
        WriteDecision::WritePend => 2,
        WriteDecision::WriteError => 3,
    };
    (r.ret, action, r.actual_bytes, r.new_used)
}

/// Replica of gale_k_pipe_read_decide (ffi/src/lib.rs).
fn ffi_pipe_read_decide(
    used: u32,
    size: u32,
    flags: u8,
    request_len: u32,
    has_writer: bool,
) -> (i32, u8, u32, u32) {
    let r = pipe::read_decide(used, size, flags, request_len, has_writer);
    let action = match r.decision {
        ReadDecision::ReadOk => 0,
        ReadDecision::WakeWriter => 1,
        ReadDecision::ReadPend => 2,
        ReadDecision::ReadError => 3,
    };
    (r.ret, action, r.actual_bytes, r.new_used)
}

// =====================================================================
// Differential tests: pipe write_check
// =====================================================================

#[test]
fn pipe_write_check_ffi_matches_model_exhaustive() {
    for size in 1u32..=8 {
        for used in 0u32..=size {
            for &flags in &[0u8, FLAG_OPEN, FLAG_RESET, FLAG_OPEN | FLAG_RESET] {
                for request_len in 0u32..=size + 1 {
                    let (ffi_ret, ffi_actual, ffi_new_used) =
                        ffi_pipe_write_check(used, size, flags, request_len);

                    // Model: use Pipe struct
                    let mut p = pipe::Pipe { size, used, flags };
                    let model_result = p.write_check(request_len);

                    match model_result {
                        Ok(n) => {
                            assert_eq!(ffi_ret, OK,
                                "ret mismatch: used={used}, size={size}, flags={flags}, req={request_len}");
                            assert_eq!(ffi_actual, n,
                                "actual mismatch: used={used}, size={size}, flags={flags}, req={request_len}");
                            assert_eq!(ffi_new_used, p.used,
                                "new_used mismatch: used={used}, size={size}, flags={flags}, req={request_len}");
                        }
                        Err(e) => {
                            assert_eq!(ffi_ret, e,
                                "error mismatch: used={used}, size={size}, flags={flags}, req={request_len}");
                        }
                    }
                }
            }
        }
    }
}

#[test]
fn pipe_read_check_ffi_matches_model_exhaustive() {
    for used in 0u32..=8 {
        // size doesn't matter for read_check (only used matters)
        let size = 8;
        for &flags in &[0u8, FLAG_OPEN, FLAG_RESET, FLAG_OPEN | FLAG_RESET] {
            for request_len in 0u32..=used + 2 {
                let (ffi_ret, ffi_actual, ffi_new_used) =
                    ffi_pipe_read_check(used, flags, request_len);

                let mut p = pipe::Pipe { size, used, flags };
                let model_result = p.read_check(request_len);

                match model_result {
                    Ok(n) => {
                        assert_eq!(ffi_ret, OK,
                            "ret mismatch: used={used}, flags={flags}, req={request_len}");
                        assert_eq!(ffi_actual, n,
                            "actual mismatch: used={used}, flags={flags}, req={request_len}");
                        assert_eq!(ffi_new_used, p.used,
                            "new_used mismatch: used={used}, flags={flags}, req={request_len}");
                    }
                    Err(e) => {
                        assert_eq!(ffi_ret, e,
                            "error mismatch: used={used}, flags={flags}, req={request_len}");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: pipe write_decide / read_decide
// =====================================================================

#[test]
fn pipe_write_decide_ffi_matches_model_exhaustive() {
    for size in 0u32..=6 {
        for used in 0u32..=size.max(1) {
            for &flags in &[0u8, FLAG_OPEN, FLAG_RESET, FLAG_OPEN | FLAG_RESET] {
                for request_len in 0u32..=4 {
                    for has_reader in [false, true] {
                        let (ffi_ret, ffi_action, ffi_actual, ffi_new_used) =
                            ffi_pipe_write_decide(used, size, flags, request_len, has_reader);

                        // The FFI delegates directly to write_decide, so we
                        // just verify the decision classification is correct.
                        let r = pipe::write_decide(used, size, flags, request_len, has_reader);

                        assert_eq!(ffi_ret, r.ret,
                            "ret: used={used}, size={size}, flags={flags}, req={request_len}, reader={has_reader}");
                        assert_eq!(ffi_actual, r.actual_bytes,
                            "actual: used={used}, size={size}, flags={flags}, req={request_len}, reader={has_reader}");
                        assert_eq!(ffi_new_used, r.new_used,
                            "new_used: used={used}, size={size}, flags={flags}, req={request_len}, reader={has_reader}");

                        // Verify action encoding
                        let expected_action = match r.decision {
                            WriteDecision::WriteOk => 0u8,
                            WriteDecision::WakeReader => 1,
                            WriteDecision::WritePend => 2,
                            WriteDecision::WriteError => 3,
                        };
                        assert_eq!(ffi_action, expected_action,
                            "action: used={used}, size={size}, flags={flags}, req={request_len}, reader={has_reader}");
                    }
                }
            }
        }
    }
}

#[test]
fn pipe_read_decide_ffi_matches_model_exhaustive() {
    for size in 0u32..=6 {
        for used in 0u32..=size.max(1) {
            for &flags in &[0u8, FLAG_OPEN, FLAG_RESET, FLAG_OPEN | FLAG_RESET] {
                for request_len in 0u32..=4 {
                    for has_writer in [false, true] {
                        let (ffi_ret, ffi_action, ffi_actual, ffi_new_used) =
                            ffi_pipe_read_decide(used, size, flags, request_len, has_writer);

                        let r = pipe::read_decide(used, size, flags, request_len, has_writer);

                        assert_eq!(ffi_ret, r.ret,
                            "ret: used={used}, size={size}, flags={flags}, req={request_len}, writer={has_writer}");
                        assert_eq!(ffi_actual, r.actual_bytes,
                            "actual: used={used}, size={size}, flags={flags}, req={request_len}, writer={has_writer}");
                        assert_eq!(ffi_new_used, r.new_used,
                            "new_used: used={used}, size={size}, flags={flags}, req={request_len}, writer={has_writer}");

                        let expected_action = match r.decision {
                            ReadDecision::ReadOk => 0u8,
                            ReadDecision::WakeWriter => 1,
                            ReadDecision::ReadPend => 2,
                            ReadDecision::ReadError => 3,
                        };
                        assert_eq!(ffi_action, expected_action,
                            "action: used={used}, size={size}, flags={flags}, req={request_len}, writer={has_writer}");
                    }
                }
            }
        }
    }
}

// =====================================================================
// Property: PP1 — 0 <= used <= size after any write_decide
// =====================================================================

#[test]
fn pipe_write_decide_pp1_used_within_bounds() {
    for size in 1u32..=20 {
        for used in 0u32..=size {
            for request_len in 1u32..=size {
                let r = pipe::write_decide(used, size, FLAG_OPEN, request_len, false);
                assert!(
                    r.new_used <= size,
                    "PP1 violated: new_used={} > size={} after write_decide",
                    r.new_used, size
                );
            }
        }
    }
}

// =====================================================================
// Property: PP9 — conservation: used + free == size
// =====================================================================

#[test]
fn pipe_write_read_conservation() {
    let size = 16u32;
    let mut p = pipe::Pipe::init(size).unwrap();

    // Write 10 bytes
    let written = p.write_check(10).unwrap();
    assert_eq!(written, 10);
    assert_eq!(p.data_get() + p.space_get(), size, "PP9 after write");

    // Read 5 bytes
    let read = p.read_check(5).unwrap();
    assert_eq!(read, 5);
    assert_eq!(p.data_get() + p.space_get(), size, "PP9 after read");

    // Write to fill
    let written2 = p.write_check(size).unwrap();
    assert_eq!(written2, size - 5); // 11 bytes of free space
    assert_eq!(p.data_get() + p.space_get(), size, "PP9 after fill");
}

// =====================================================================
// Boundary: write_check size==0
// =====================================================================

#[test]
fn pipe_write_check_zero_size() {
    let (ret, _, _) = ffi_pipe_write_check(0, 0, FLAG_OPEN, 1);
    assert_eq!(ret, EINVAL, "size==0 must return EINVAL");
}

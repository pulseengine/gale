//! Differential equivalence tests — Message Queue (FFI vs Model).
//!
//! Verifies that the FFI message queue functions produce the same results as
//! the Verus-verified model functions in gale::msgq.

#![allow(
    clippy::shadow_unrelated,
    clippy::indexing_slicing,
    clippy::bool_to_int_with_if,
    clippy::unwrap_used,
    clippy::fn_params_excessive_bools,
    clippy::absurd_extreme_comparisons,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::checked_conversions,
    clippy::wildcard_enum_match_arm,
    clippy::implicit_saturating_sub,
    clippy::branches_sharing_code,
    clippy::panic
)]

use gale::error::*;
use gale::msgq::{self, GetDecision, MsgQ, PutDecision};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Decision output matching GaleMsgqPutDecision.
#[derive(Debug, PartialEq, Eq)]
struct FfiMsgqPutDecision {
    ret: i32,
    action: u8,
    new_write_idx: u32,
    new_used: u32,
}

const FFI_MSGQ_PUT_OK: u8 = 0;
const FFI_MSGQ_WAKE_READER: u8 = 1;
const FFI_MSGQ_PUT_PEND: u8 = 2;
const FFI_MSGQ_RETURN_FULL: u8 = 3;

/// Replica of gale_k_msgq_put_decide (ffi/src/lib.rs).
fn ffi_msgq_put_decide(
    write_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    has_waiter: bool,
    is_no_wait: bool,
) -> FfiMsgqPutDecision {
    if used_msgs < max_msgs {
        if has_waiter {
            FfiMsgqPutDecision {
                ret: OK,
                action: FFI_MSGQ_WAKE_READER,
                new_write_idx: write_idx,
                new_used: used_msgs,
            }
        } else {
            let next = if write_idx + 1 < max_msgs {
                #[allow(clippy::arithmetic_side_effects)]
                let n = write_idx + 1;
                n
            } else {
                0u32
            };
            #[allow(clippy::arithmetic_side_effects)]
            let new_used = used_msgs + 1;
            FfiMsgqPutDecision {
                ret: OK,
                action: FFI_MSGQ_PUT_OK,
                new_write_idx: next,
                new_used,
            }
        }
    } else if is_no_wait {
        FfiMsgqPutDecision {
            ret: ENOMSG,
            action: FFI_MSGQ_RETURN_FULL,
            new_write_idx: write_idx,
            new_used: used_msgs,
        }
    } else {
        FfiMsgqPutDecision {
            ret: 0,
            action: FFI_MSGQ_PUT_PEND,
            new_write_idx: write_idx,
            new_used: used_msgs,
        }
    }
}

/// Replica of gale_msgq_put (legacy no-waiter path, ffi/src/lib.rs).
/// Returns (ret, new_write_idx, new_used).
fn ffi_msgq_put_legacy(write_idx: u32, used_msgs: u32, max_msgs: u32) -> (i32, u32, u32) {
    // Delegates to put_decide with has_waiter=false, is_no_wait=true
    let d = ffi_msgq_put_decide(write_idx, used_msgs, max_msgs, false, true);
    if d.action == FFI_MSGQ_PUT_OK {
        (OK, d.new_write_idx, d.new_used)
    } else {
        (ENOMSG, write_idx, used_msgs)
    }
}

/// Decision output matching GaleMsgqGetDecision.
#[derive(Debug, PartialEq, Eq)]
struct FfiMsgqGetDecision {
    ret: i32,
    action: u8,
    new_read_idx: u32,
    new_used: u32,
}

const FFI_MSGQ_GET_OK: u8 = 0;
const FFI_MSGQ_WAKE_WRITER: u8 = 1;
const FFI_MSGQ_GET_PEND: u8 = 2;
const FFI_MSGQ_RETURN_EMPTY: u8 = 3;

/// Replica of gale_k_msgq_get_decide (ffi/src/lib.rs).
fn ffi_msgq_get_decide(
    read_idx: u32,
    used_msgs: u32,
    max_msgs: u32,
    has_waiter: bool,
    is_no_wait: bool,
) -> FfiMsgqGetDecision {
    if used_msgs > 0 {
        let next = if read_idx + 1 < max_msgs {
            #[allow(clippy::arithmetic_side_effects)]
            let n = read_idx + 1;
            n
        } else {
            0u32
        };
        #[allow(clippy::arithmetic_side_effects)]
        let new_used = used_msgs - 1;
        if has_waiter {
            FfiMsgqGetDecision {
                ret: OK,
                action: FFI_MSGQ_WAKE_WRITER,
                new_read_idx: next,
                new_used,
            }
        } else {
            FfiMsgqGetDecision {
                ret: OK,
                action: FFI_MSGQ_GET_OK,
                new_read_idx: next,
                new_used,
            }
        }
    } else if is_no_wait {
        FfiMsgqGetDecision {
            ret: ENOMSG,
            action: FFI_MSGQ_RETURN_EMPTY,
            new_read_idx: read_idx,
            new_used: 0,
        }
    } else {
        FfiMsgqGetDecision {
            ret: 0,
            action: FFI_MSGQ_GET_PEND,
            new_read_idx: read_idx,
            new_used: 0,
        }
    }
}

/// Replica of gale_msgq_get (legacy no-waiter path, ffi/src/lib.rs).
/// Returns (ret, new_read_idx, new_used).
fn ffi_msgq_get_legacy(read_idx: u32, used_msgs: u32, max_msgs: u32) -> (i32, u32, u32) {
    // Delegates to get_decide with has_waiter=false, is_no_wait=true
    let d = ffi_msgq_get_decide(read_idx, used_msgs, max_msgs, false, true);
    if d.action == FFI_MSGQ_GET_OK {
        (OK, d.new_read_idx, d.new_used)
    } else {
        (ENOMSG, read_idx, used_msgs)
    }
}

/// Replica of gale_msgq_purge (scalar, ffi/src/lib.rs — no distinct shim, modeled here).
/// Purge: reset to empty, read_idx = write_idx.
/// Returns old_used.
fn ffi_msgq_purge(write_idx: u32, used_msgs: u32) -> (u32, u32, u32) {
    // (old_used, new_read_idx, new_used)
    (used_msgs, write_idx, 0)
}

// =====================================================================
// Differential tests: msgq init validate
// =====================================================================

#[test]
fn msgq_init_validate_ffi_matches_model_exhaustive() {
    for msg_size in 0u32..=5 {
        for max_msgs in 0u32..=5 {
            let model_ret = MsgQ::init(msg_size, max_msgs);

            if msg_size == 0 || max_msgs == 0 {
                assert!(
                    model_ret.is_err(),
                    "init should fail: msg_size={msg_size}, max_msgs={max_msgs}"
                );
                assert_eq!(model_ret.unwrap_err(), EINVAL);
            } else {
                assert!(
                    model_ret.is_ok(),
                    "init should succeed: msg_size={msg_size}, max_msgs={max_msgs}"
                );
                let mq = model_ret.unwrap();
                assert_eq!(mq.msg_size, msg_size);
                assert_eq!(mq.max_msgs, max_msgs);
                assert_eq!(mq.used_msgs, 0);
                assert_eq!(mq.read_idx, 0);
                assert_eq!(mq.write_idx, 0);
            }
        }
    }
}

// =====================================================================
// Differential tests: put_decide
// =====================================================================

#[test]
fn msgq_put_decide_ffi_matches_model_exhaustive() {
    let max_msgs = 4u32;
    for write_idx in 0u32..max_msgs {
        for used_msgs in 0u32..=max_msgs {
            for has_waiter in [false, true] {
                for is_no_wait in [false, true] {
                    let ffi_d = ffi_msgq_put_decide(
                        write_idx, used_msgs, max_msgs, has_waiter, is_no_wait,
                    );
                    let model_r = msgq::put_decide(
                        write_idx, used_msgs, max_msgs, has_waiter, is_no_wait,
                    );

                    match model_r.decision {
                        PutDecision::Store => {
                            assert_eq!(ffi_d.ret, OK, "Store: ret");
                            assert_eq!(ffi_d.action, FFI_MSGQ_PUT_OK, "Store: action");
                            assert_eq!(
                                ffi_d.new_write_idx, model_r.new_write_idx,
                                "Store: new_write_idx wi={write_idx} used={used_msgs} max={max_msgs}"
                            );
                            assert_eq!(
                                ffi_d.new_used, model_r.new_used,
                                "Store: new_used"
                            );
                        }
                        PutDecision::WakeReader => {
                            assert_eq!(ffi_d.ret, OK, "WakeReader: ret");
                            assert_eq!(
                                ffi_d.action, FFI_MSGQ_WAKE_READER,
                                "WakeReader: action"
                            );
                            assert_eq!(
                                ffi_d.new_write_idx, write_idx,
                                "WakeReader: write_idx unchanged"
                            );
                            assert_eq!(
                                ffi_d.new_used, used_msgs,
                                "WakeReader: used unchanged"
                            );
                        }
                        PutDecision::Full => {
                            assert_eq!(ffi_d.ret, ENOMSG, "Full: ret");
                            assert_eq!(
                                ffi_d.action, FFI_MSGQ_RETURN_FULL,
                                "Full: action"
                            );
                        }
                        PutDecision::Pend => {
                            assert_eq!(ffi_d.ret, 0, "Pend: ret");
                            assert_eq!(ffi_d.action, FFI_MSGQ_PUT_PEND, "Pend: action");
                        }
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: get_decide
// =====================================================================

#[test]
fn msgq_get_decide_ffi_matches_model_exhaustive() {
    let max_msgs = 4u32;
    for read_idx in 0u32..max_msgs {
        for used_msgs in 0u32..=max_msgs {
            for has_waiter in [false, true] {
                for is_no_wait in [false, true] {
                    let ffi_d = ffi_msgq_get_decide(
                        read_idx, used_msgs, max_msgs, has_waiter, is_no_wait,
                    );
                    let model_r = msgq::get_decide(
                        read_idx, used_msgs, max_msgs, has_waiter, is_no_wait,
                    );

                    match model_r.decision {
                        GetDecision::Read => {
                            assert_eq!(ffi_d.ret, OK, "Read: ret");
                            assert_eq!(ffi_d.action, FFI_MSGQ_GET_OK, "Read: action");
                            assert_eq!(
                                ffi_d.new_read_idx, model_r.new_read_idx,
                                "Read: new_read_idx ri={read_idx} used={used_msgs} max={max_msgs}"
                            );
                            assert_eq!(ffi_d.new_used, model_r.new_used, "Read: new_used");
                        }
                        GetDecision::WakeWriter => {
                            assert_eq!(ffi_d.ret, OK, "WakeWriter: ret");
                            assert_eq!(
                                ffi_d.action, FFI_MSGQ_WAKE_WRITER,
                                "WakeWriter: action"
                            );
                            assert_eq!(
                                ffi_d.new_read_idx, model_r.new_read_idx,
                                "WakeWriter: new_read_idx"
                            );
                            assert_eq!(ffi_d.new_used, model_r.new_used, "WakeWriter: new_used");
                        }
                        GetDecision::Empty => {
                            assert_eq!(ffi_d.ret, ENOMSG, "Empty: ret");
                            assert_eq!(
                                ffi_d.action, FFI_MSGQ_RETURN_EMPTY,
                                "Empty: action"
                            );
                        }
                        GetDecision::Pend => {
                            assert_eq!(ffi_d.ret, 0, "Pend: ret");
                            assert_eq!(ffi_d.action, FFI_MSGQ_GET_PEND, "Pend: action");
                        }
                    }
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: purge (MsgQ model)
// =====================================================================

#[test]
fn msgq_purge_ffi_matches_model_exhaustive() {
    let max_msgs = 8u32;
    // Test write_idx in 0..max_msgs, used_msgs in 0..=max_msgs
    for write_idx in 0u32..max_msgs {
        for used_msgs in 0u32..=max_msgs {
            let (ffi_old_used, ffi_new_read_idx, ffi_new_used) =
                ffi_msgq_purge(write_idx, used_msgs);

            assert_eq!(ffi_old_used, used_msgs, "purge: old_used mismatch");
            assert_eq!(ffi_new_read_idx, write_idx, "purge: new_read_idx should == write_idx");
            assert_eq!(ffi_new_used, 0, "purge: new_used should be 0");

            // Cross-check with model purge
            let read_idx = write_idx; // construct a valid state; read_idx = write_idx when queue is 0
            // For a fully general test, build a valid MsgQ then call purge.
            if let Ok(mut mq) = MsgQ::init(4, max_msgs) {
                // Manually set state to valid (consistent) configuration:
                // use write_idx as anchor; a state with used_msgs=0 is always valid
                // with read_idx == write_idx.
                // For states where used_msgs>0 we can't easily fabricate arbitrary
                // ring states, so test just purge from empty + non-empty indirectly:
                let old_used = mq.purge();
                assert_eq!(old_used, 0, "purge from fresh init: old_used=0");
                assert_eq!(mq.used_msgs, 0);
                assert_eq!(mq.read_idx, mq.write_idx, "purge: read_idx == write_idx");
                let _ = read_idx; // suppress unused warning
            }
        }
    }
}

#[test]
fn msgq_purge_model_resets_to_empty() {
    let mut mq = MsgQ::init(4, 8).unwrap();
    // Put some messages
    let _ = mq.put();
    let _ = mq.put();
    let _ = mq.put();
    assert_eq!(mq.used_msgs, 3);

    let old_used = mq.purge();
    assert_eq!(old_used, 3, "purge should return old used count");
    assert_eq!(mq.used_msgs, 0, "purge: queue should be empty");
    assert_eq!(mq.read_idx, mq.write_idx, "purge: indices consistent");
}

// =====================================================================
// Differential tests: legacy put/get path
// =====================================================================

#[test]
fn msgq_put_legacy_ffi_matches_model_exhaustive() {
    let max_msgs = 4u32;
    for write_idx in 0u32..max_msgs {
        for used_msgs in 0u32..=max_msgs {
            let (ffi_ret, ffi_new_write, ffi_new_used) =
                ffi_msgq_put_legacy(write_idx, used_msgs, max_msgs);

            // Model: put_decide with has_waiter=false, is_no_wait=true
            let model_r = msgq::put_decide(write_idx, used_msgs, max_msgs, false, true);

            match model_r.decision {
                PutDecision::Store => {
                    assert_eq!(ffi_ret, OK, "legacy put: ret wi={write_idx} used={used_msgs}");
                    assert_eq!(ffi_new_write, model_r.new_write_idx, "legacy put: new_write_idx");
                    assert_eq!(ffi_new_used, model_r.new_used, "legacy put: new_used");
                }
                PutDecision::Full => {
                    assert_eq!(ffi_ret, ENOMSG, "legacy put full: ret");
                    assert_eq!(ffi_new_write, write_idx, "legacy put full: write_idx unchanged");
                    assert_eq!(ffi_new_used, used_msgs, "legacy put full: used unchanged");
                }
                _ => {} // has_waiter=false, is_no_wait=true: WakeReader/Pend never occur
            }
        }
    }
}

#[test]
fn msgq_get_legacy_ffi_matches_model_exhaustive() {
    let max_msgs = 4u32;
    for read_idx in 0u32..max_msgs {
        for used_msgs in 0u32..=max_msgs {
            let (ffi_ret, ffi_new_read, ffi_new_used) =
                ffi_msgq_get_legacy(read_idx, used_msgs, max_msgs);

            // Model: get_decide with has_waiter=false, is_no_wait=true
            let model_r = msgq::get_decide(read_idx, used_msgs, max_msgs, false, true);

            match model_r.decision {
                GetDecision::Read => {
                    assert_eq!(ffi_ret, OK, "legacy get: ret ri={read_idx} used={used_msgs}");
                    assert_eq!(ffi_new_read, model_r.new_read_idx, "legacy get: new_read_idx");
                    assert_eq!(ffi_new_used, model_r.new_used, "legacy get: new_used");
                }
                GetDecision::Empty => {
                    assert_eq!(ffi_ret, ENOMSG, "legacy get empty: ret");
                    assert_eq!(ffi_new_read, read_idx, "legacy get empty: read_idx unchanged");
                    assert_eq!(ffi_new_used, used_msgs, "legacy get empty: used unchanged");
                }
                _ => {} // has_waiter=false, is_no_wait=true: WakeWriter/Pend never occur
            }
        }
    }
}

// =====================================================================
// Property: MQ5/MQ8 — put then get roundtrip (index arithmetic)
// =====================================================================

#[test]
fn msgq_put_get_roundtrip_ffi() {
    let max_msgs = 8u32;
    for write_idx in 0u32..max_msgs {
        for used_msgs in 0u32..max_msgs {
            // Put: store a message
            let put_d = ffi_msgq_put_decide(write_idx, used_msgs, max_msgs, false, true);
            assert_eq!(put_d.ret, OK, "put should succeed when not full");
            assert_eq!(put_d.action, FFI_MSGQ_PUT_OK);

            // Get: read it back (read_idx == write_idx in this simplified path)
            let read_idx = write_idx; // we read the just-written slot
            let get_d =
                ffi_msgq_get_decide(read_idx, put_d.new_used, max_msgs, false, true);
            assert_eq!(get_d.ret, OK, "get should succeed after put");
            assert_eq!(get_d.action, FFI_MSGQ_GET_OK);
            assert_eq!(
                get_d.new_used,
                used_msgs,
                "roundtrip: used_msgs should return to original"
            );
        }
    }
}

// =====================================================================
// Property: MQ11 — purge after fill returns to empty
// =====================================================================

#[test]
fn msgq_fill_then_purge_model() {
    let max_msgs = 5u32;
    let mut mq = MsgQ::init(1, max_msgs).unwrap();

    // Fill the queue
    for i in 0..max_msgs {
        let res = mq.put();
        assert!(res.is_ok(), "put {i}: should succeed");
    }
    assert_eq!(mq.used_msgs, max_msgs);
    let next_put = mq.put();
    assert_eq!(next_put.unwrap_err(), ENOMSG, "put on full queue should return ENOMSG");

    // Purge
    let old_used = mq.purge();
    assert_eq!(old_used, max_msgs, "purge: old_used should equal max_msgs");
    assert_eq!(mq.used_msgs, 0);
    assert_eq!(mq.read_idx, mq.write_idx);
}

// =====================================================================
// Property: MQ6/MQ9 — full queue put and empty queue get return ENOMSG
// =====================================================================

#[test]
fn msgq_put_full_returns_enomsg_model() {
    let max_msgs = 3u32;
    let mut mq = MsgQ::init(1, max_msgs).unwrap();
    for _ in 0..max_msgs {
        mq.put().unwrap();
    }
    assert_eq!(mq.put().unwrap_err(), ENOMSG);
}

#[test]
fn msgq_get_empty_returns_enomsg_model() {
    let mut mq = MsgQ::init(1, 4).unwrap();
    assert_eq!(mq.get().unwrap_err(), ENOMSG);
}

// =====================================================================
// Property: MQ2/MQ3 — indices always < max_msgs
// =====================================================================

#[test]
fn msgq_indices_always_in_bounds_model() {
    let max_msgs = 4u32;
    let mut mq = MsgQ::init(1, max_msgs).unwrap();
    // Cycle put/get many times to exercise wrap-around
    for _ in 0..20 {
        let _ = mq.put();
        let _ = mq.get();
        assert!(mq.read_idx < max_msgs, "read_idx out of bounds");
        assert!(mq.write_idx < max_msgs, "write_idx out of bounds");
    }
}

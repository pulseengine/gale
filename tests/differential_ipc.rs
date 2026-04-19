//! Differential equivalence tests — IPC service (FFI vs Model).
//!
//! Verifies that the FFI IPC functions produce the same results as
//! the Verus-verified model functions in gale::ipc.

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
    clippy::panic
)]

use gale::error::*;
use gale::ipc::{
    IpcEndpointState, IpcServiceState, MAX_ENDPOINTS, MAX_MSG_LEN,
    send_decide, send_critical_decide, validate_buffer_size,
};

// =====================================================================
// FFI replicas — pure Rust reimplementations of the FFI logic
// =====================================================================

/// Replica of gale_ipc_open_decide.
fn ffi_ipc_open_decide(instance_valid: bool) -> i32 {
    if instance_valid { OK } else { EINVAL }
}

/// Replica of gale_ipc_close_decide.
fn ffi_ipc_close_decide(instance_valid: bool) -> i32 {
    if instance_valid { OK } else { EINVAL }
}

/// Replica of gale_ipc_register_decide (stateless variant, no pointer path).
fn ffi_ipc_register_decide(
    params_valid: bool,
    registered_count: u32,
    max_endpoints: u32,
) -> (i32, u32) {
    if !params_valid {
        return (EINVAL, registered_count);
    }
    if max_endpoints > MAX_ENDPOINTS {
        return (EINVAL, registered_count);
    }
    if registered_count >= max_endpoints {
        return (ENOMEM, registered_count);
    }
    (OK, registered_count + 1)
}

/// Replica of gale_ipc_deregister_decide (stateless variant).
fn ffi_ipc_deregister_decide(
    endpoint_valid: bool,
    endpoint_registered: bool,
    registered_count: u32,
    max_endpoints: u32,
) -> (i32, u32) {
    if max_endpoints > MAX_ENDPOINTS {
        return (EINVAL, registered_count);
    }
    if !endpoint_valid {
        return (EINVAL, registered_count);
    }
    if !endpoint_registered {
        return (ENOENT, registered_count);
    }
    (OK, registered_count - 1)
}

/// Replica of gale_ipc_send_decide.
fn ffi_ipc_send_decide(
    endpoint_valid: bool,
    endpoint_registered: bool,
    state: IpcEndpointState,
    len: u32,
) -> i32 {
    if !endpoint_valid { return EINVAL; }
    if !endpoint_registered { return ENOENT; }
    if state != IpcEndpointState::Bound { return EINVAL; }
    if len == 0 || len > MAX_MSG_LEN { return EINVAL; }
    OK
}

/// Replica of gale_ipc_validate_buffer_size.
fn ffi_ipc_validate_buffer_size(
    endpoint_valid: bool,
    endpoint_registered: bool,
    reported_size: u32,
) -> i32 {
    if !endpoint_valid { return EINVAL; }
    if !endpoint_registered { return ENOENT; }
    if reported_size == 0 || reported_size > MAX_MSG_LEN { return EINVAL; }
    OK
}

// =====================================================================
// Differential tests: open_decide / close_decide
// =====================================================================

#[test]
fn ipc_open_decide_ffi_matches_model() {
    for instance_valid in [false, true] {
        let ffi_rc = ffi_ipc_open_decide(instance_valid);
        let model_rc = IpcServiceState::open_decide(instance_valid);
        assert_eq!(ffi_rc, model_rc,
            "open_decide mismatch: instance_valid={instance_valid}");
    }
}

#[test]
fn ipc_close_decide_ffi_matches_model() {
    for instance_valid in [false, true] {
        let ffi_rc = ffi_ipc_close_decide(instance_valid);
        let model_rc = IpcServiceState::close_decide(instance_valid);
        assert_eq!(ffi_rc, model_rc,
            "close_decide mismatch: instance_valid={instance_valid}");
    }
}

#[test]
fn ipc_open_invalid_instance_einval() {
    let rc = ffi_ipc_open_decide(false);
    assert_eq!(rc, EINVAL, "IPC2: null instance must return EINVAL");
}

#[test]
fn ipc_open_valid_instance_ok() {
    let rc = ffi_ipc_open_decide(true);
    assert_eq!(rc, OK, "IPC2: valid instance open must succeed");
}

// =====================================================================
// Differential tests: register_decide
// =====================================================================

#[test]
fn ipc_register_decide_ffi_matches_model_exhaustive() {
    let max = 4u32;
    for params_valid in [false, true] {
        for registered_count in 0u32..=max {
            let (ffi_rc, ffi_new) =
                ffi_ipc_register_decide(params_valid, registered_count, max);

            let mut svc = IpcServiceState { registered_count, max_endpoints: max };
            let model_rc = svc.register_decide(params_valid);
            let model_new = svc.registered_count;

            assert_eq!(ffi_rc, model_rc,
                "register rc: params_valid={params_valid}, count={registered_count}, max={max}");
            if ffi_rc == OK {
                assert_eq!(ffi_new, model_new,
                    "register new_count: params_valid={params_valid}, count={registered_count}");
            }
        }
    }
}

#[test]
fn ipc_register_invalid_params_einval() {
    let (rc, _) = ffi_ipc_register_decide(false, 0, 4);
    assert_eq!(rc, EINVAL, "IPC1: invalid params must return EINVAL");
}

#[test]
fn ipc_register_capacity_exhausted_enomem() {
    let max = 4u32;
    let (rc, _) = ffi_ipc_register_decide(true, max, max);
    assert_eq!(rc, ENOMEM, "IPC5: full endpoint table must return ENOMEM");
}

#[test]
fn ipc_register_increments_count() {
    let (rc, new_count) = ffi_ipc_register_decide(true, 2, 4);
    assert_eq!(rc, OK);
    assert_eq!(new_count, 3, "IPC5: register must increment count");
}

// =====================================================================
// Differential tests: deregister_decide
// =====================================================================

#[test]
fn ipc_deregister_decide_ffi_matches_model_exhaustive() {
    let max = 4u32;
    for endpoint_valid in [false, true] {
        for endpoint_registered in [false, true] {
            for registered_count in 1u32..=max {
                let (ffi_rc, ffi_new) =
                    ffi_ipc_deregister_decide(endpoint_valid, endpoint_registered,
                                              registered_count, max);

                let mut svc = IpcServiceState { registered_count, max_endpoints: max };
                let model_rc = svc.deregister_decide(endpoint_valid, endpoint_registered);
                let model_new = svc.registered_count;

                assert_eq!(ffi_rc, model_rc,
                    "deregister rc: valid={endpoint_valid}, reg={endpoint_registered}, \
                     count={registered_count}");
                if ffi_rc == OK {
                    assert_eq!(ffi_new, model_new,
                        "deregister new_count: count={registered_count}");
                }
            }
        }
    }
}

#[test]
fn ipc_deregister_invalid_endpoint_einval() {
    let (rc, _) = ffi_ipc_deregister_decide(false, true, 2, 4);
    assert_eq!(rc, EINVAL, "IPC1: invalid endpoint must return EINVAL");
}

#[test]
fn ipc_deregister_unregistered_enoent() {
    let (rc, _) = ffi_ipc_deregister_decide(true, false, 2, 4);
    assert_eq!(rc, ENOENT, "IPC4: unregistered endpoint must return ENOENT");
}

#[test]
fn ipc_deregister_decrements_count() {
    let (rc, new_count) = ffi_ipc_deregister_decide(true, true, 3, 4);
    assert_eq!(rc, OK);
    assert_eq!(new_count, 2, "IPC5: deregister must decrement count");
}

// =====================================================================
// Differential tests: send_decide
// =====================================================================

#[test]
fn ipc_send_decide_ffi_matches_model_exhaustive() {
    let states = [
        IpcEndpointState::Closed,
        IpcEndpointState::Open,
        IpcEndpointState::Bound,
    ];
    let lens = [0u32, 1, 64, MAX_MSG_LEN, MAX_MSG_LEN + 1];

    for endpoint_valid in [false, true] {
        for endpoint_registered in [false, true] {
            for state in states {
                for len in lens {
                    let ffi_rc = ffi_ipc_send_decide(
                        endpoint_valid, endpoint_registered, state, len);
                    let model_rc = send_decide(
                        endpoint_valid, endpoint_registered, state, len);

                    assert_eq!(ffi_rc, model_rc,
                        "send_decide mismatch: valid={endpoint_valid}, \
                         reg={endpoint_registered}, state={state:?}, len={len}");
                }
            }
        }
    }
}

#[test]
fn ipc_send_requires_bound_state() {
    // IPC3: send only from Bound
    let rc_closed = ffi_ipc_send_decide(true, true, IpcEndpointState::Closed, 64);
    let rc_open = ffi_ipc_send_decide(true, true, IpcEndpointState::Open, 64);
    let rc_bound = ffi_ipc_send_decide(true, true, IpcEndpointState::Bound, 64);

    assert_eq!(rc_closed, EINVAL, "IPC3: Closed state must not send");
    assert_eq!(rc_open, EINVAL, "IPC3: Open state must not send");
    assert_eq!(rc_bound, OK, "IPC3: Bound state may send");
}

#[test]
fn ipc_send_zero_len_rejected() {
    let rc = ffi_ipc_send_decide(true, true, IpcEndpointState::Bound, 0);
    assert_eq!(rc, EINVAL, "IPC6: zero-length send must be rejected");
}

#[test]
fn ipc_send_over_max_rejected() {
    let rc = ffi_ipc_send_decide(true, true, IpcEndpointState::Bound, MAX_MSG_LEN + 1);
    assert_eq!(rc, EINVAL, "IPC6: over-max-length send must be rejected");
}

#[test]
fn ipc_send_max_len_accepted() {
    let rc = ffi_ipc_send_decide(true, true, IpcEndpointState::Bound, MAX_MSG_LEN);
    assert_eq!(rc, OK, "IPC6: MAX_MSG_LEN send must be accepted");
}

// =====================================================================
// Differential tests: send_critical_decide (same rules as send)
// =====================================================================

#[test]
fn ipc_send_critical_matches_send_decide() {
    let states = [
        IpcEndpointState::Closed,
        IpcEndpointState::Open,
        IpcEndpointState::Bound,
    ];
    for endpoint_valid in [false, true] {
        for endpoint_registered in [false, true] {
            for state in states {
                for len in [0u32, 1, 64, MAX_MSG_LEN, MAX_MSG_LEN + 1] {
                    let rc_send = send_decide(endpoint_valid, endpoint_registered, state, len);
                    let rc_crit = send_critical_decide(endpoint_valid, endpoint_registered, state, len);
                    assert_eq!(rc_send, rc_crit,
                        "send_critical must equal send: valid={endpoint_valid}, \
                         reg={endpoint_registered}, state={state:?}, len={len}");
                }
            }
        }
    }
}

// =====================================================================
// Differential tests: validate_buffer_size
// =====================================================================

#[test]
fn ipc_validate_buffer_size_ffi_matches_model_exhaustive() {
    let sizes = [0u32, 1, 64, MAX_MSG_LEN, MAX_MSG_LEN + 1];

    for endpoint_valid in [false, true] {
        for endpoint_registered in [false, true] {
            for reported_size in sizes {
                let ffi_rc = ffi_ipc_validate_buffer_size(
                    endpoint_valid, endpoint_registered, reported_size);
                let model_rc = validate_buffer_size(
                    endpoint_valid, endpoint_registered, reported_size);

                assert_eq!(ffi_rc, model_rc,
                    "buffer_size mismatch: valid={endpoint_valid}, \
                     reg={endpoint_registered}, size={reported_size}");
            }
        }
    }
}

// =====================================================================
// Differential tests: IpcEndpoint state machine
// =====================================================================

#[test]
fn ipc_endpoint_open_from_closed_succeeds() {
    use gale::ipc::IpcEndpoint;
    let mut ep = IpcEndpoint::new();
    assert_eq!(ep.state(), IpcEndpointState::Closed);
    let rc = ep.transition_open();
    assert_eq!(rc, OK, "IPC2: Closed->Open must succeed");
    assert_eq!(ep.state(), IpcEndpointState::Open);
}

#[test]
fn ipc_endpoint_double_open_rejected() {
    use gale::ipc::IpcEndpoint;
    let mut ep = IpcEndpoint::new();
    ep.transition_open();
    let rc = ep.transition_open();
    assert_eq!(rc, EALREADY, "IPC2: double-open must return EALREADY");
}

#[test]
fn ipc_endpoint_bound_allows_send() {
    use gale::ipc::IpcEndpoint;
    let mut ep = IpcEndpoint::new();
    ep.transition_open();
    let rc = ep.transition_bound();
    assert_eq!(rc, OK, "Open->Bound must succeed");
    assert!(ep.can_send(), "IPC3: Bound endpoint must be able to send");
}

#[test]
fn ipc_endpoint_close_always_succeeds() {
    use gale::ipc::IpcEndpoint;
    for initial_state in [IpcEndpointState::Closed, IpcEndpointState::Open,
                          IpcEndpointState::Bound] {
        let mut ep = IpcEndpoint { state: initial_state };
        let rc = ep.transition_close();
        assert_eq!(rc, OK, "IPC4: close must always succeed");
        assert_eq!(ep.state(), IpcEndpointState::Closed, "IPC4: state must be Closed after close");
    }
}

// =====================================================================
// Property: IPC5 — registered count never exceeds MAX_ENDPOINTS
// =====================================================================

#[test]
fn ipc_register_count_never_exceeds_max() {
    let max = MAX_ENDPOINTS;
    let mut count = 0u32;
    for _ in 0..max {
        let (rc, new_count) = ffi_ipc_register_decide(true, count, max);
        assert_eq!(rc, OK);
        count = new_count;
    }
    assert_eq!(count, max, "IPC5: count should equal max after filling");

    // One more should fail
    let (rc, _) = ffi_ipc_register_decide(true, count, max);
    assert_eq!(rc, ENOMEM, "IPC5: count must not exceed MAX_ENDPOINTS");
}

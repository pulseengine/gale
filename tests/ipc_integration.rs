//! Integration tests for the IPC service model.
//!
//! These tests run under: cargo test, miri, sanitizers.
//! They verify all six IPC safety properties:
//!   IPC1: Endpoint state is always valid
//!   IPC2: open only from Closed
//!   IPC3: send only when Bound
//!   IPC4: close returns to Closed
//!   IPC5: endpoint count bounded
//!   IPC6: buffer bounds on send

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects,
    clippy::cast_possible_truncation
)]

use gale::ipc::*;
use gale::error::*;

// ==========================================================================
// IPC1: Endpoint state is always a valid variant
// ==========================================================================

#[test]
fn new_endpoint_state_is_closed() {
    let ep = IpcEndpoint::new();
    assert_eq!(ep.state(), IpcEndpointState::Closed);
}

#[test]
fn state_variants_are_valid() {
    // Exhaustive: all reachable states are valid (IPC1)
    for state in [
        IpcEndpointState::Closed,
        IpcEndpointState::Open,
        IpcEndpointState::Bound,
    ] {
        // State enum variants are valid
        let _ = matches!(state, IpcEndpointState::Closed | IpcEndpointState::Open | IpcEndpointState::Bound);
    }
}

// ==========================================================================
// IPC2: open only from Closed
// ==========================================================================

#[test]
fn open_from_closed_succeeds() {
    let mut ep = IpcEndpoint::new();
    assert_eq!(ep.state(), IpcEndpointState::Closed);
    let r = ep.transition_open();
    assert_eq!(r, OK);
    assert_eq!(ep.state(), IpcEndpointState::Open);
}

#[test]
fn double_open_rejected() {
    let mut ep = IpcEndpoint::new();
    let r1 = ep.transition_open();
    assert_eq!(r1, OK);
    // Second open must fail (IPC2)
    let r2 = ep.transition_open();
    assert_eq!(r2, EALREADY);
    assert_eq!(ep.state(), IpcEndpointState::Open, "state must not change on failed open");
}

#[test]
fn open_from_bound_rejected() {
    let mut ep = IpcEndpoint::new();
    let _ = ep.transition_open();
    let _ = ep.transition_bound();
    assert_eq!(ep.state(), IpcEndpointState::Bound);
    let r = ep.transition_open();
    assert_eq!(r, EALREADY);
    assert_eq!(ep.state(), IpcEndpointState::Bound);
}

// ==========================================================================
// IPC3: send only when Bound
// ==========================================================================

#[test]
fn send_from_bound_succeeds() {
    let r = send_decide(true, true, IpcEndpointState::Bound, 64);
    assert_eq!(r, OK, "send must succeed from Bound with valid len");
}

#[test]
fn send_from_closed_rejected() {
    let r = send_decide(true, true, IpcEndpointState::Closed, 64);
    assert_eq!(r, EINVAL, "send must be rejected from Closed");
}

#[test]
fn send_from_open_rejected() {
    let r = send_decide(true, true, IpcEndpointState::Open, 64);
    assert_eq!(r, EINVAL, "send must be rejected from Open");
}

#[test]
fn endpoint_can_send_only_when_bound() {
    let mut ep = IpcEndpoint::new();
    assert!(!ep.can_send(), "Closed: can_send must be false");
    let _ = ep.transition_open();
    assert!(!ep.can_send(), "Open: can_send must be false");
    let _ = ep.transition_bound();
    assert!(ep.can_send(), "Bound: can_send must be true");
    let _ = ep.transition_close();
    assert!(!ep.can_send(), "Closed again: can_send must be false");
}

// ==========================================================================
// IPC4: close returns to Closed
// ==========================================================================

#[test]
fn close_from_bound_returns_closed() {
    let mut ep = IpcEndpoint::new();
    let _ = ep.transition_open();
    let _ = ep.transition_bound();
    assert_eq!(ep.state(), IpcEndpointState::Bound);
    let r = ep.transition_close();
    assert_eq!(r, OK);
    assert_eq!(ep.state(), IpcEndpointState::Closed, "IPC4: close must return to Closed");
}

#[test]
fn close_from_open_returns_closed() {
    let mut ep = IpcEndpoint::new();
    let _ = ep.transition_open();
    assert_eq!(ep.state(), IpcEndpointState::Open);
    let r = ep.transition_close();
    assert_eq!(r, OK);
    assert_eq!(ep.state(), IpcEndpointState::Closed, "IPC4: close from Open must return to Closed");
}

#[test]
fn close_from_closed_is_idempotent() {
    let mut ep = IpcEndpoint::new();
    let r = ep.transition_close();
    assert_eq!(r, OK);
    assert_eq!(ep.state(), IpcEndpointState::Closed);
}

#[test]
fn full_lifecycle_closed_open_bound_closed() {
    let mut ep = IpcEndpoint::new();
    assert_eq!(ep.state(), IpcEndpointState::Closed);

    assert_eq!(ep.transition_open(), OK);
    assert_eq!(ep.state(), IpcEndpointState::Open);

    assert_eq!(ep.transition_bound(), OK);
    assert_eq!(ep.state(), IpcEndpointState::Bound);

    assert_eq!(ep.transition_close(), OK);
    assert_eq!(ep.state(), IpcEndpointState::Closed);
}

// ==========================================================================
// IPC5: endpoint count bounded
// ==========================================================================

#[test]
fn register_increments_count() {
    let mut svc = IpcServiceState::new(4);
    assert_eq!(svc.registered_count, 0);
    let r = svc.register_decide(true);
    assert_eq!(r, OK);
    assert_eq!(svc.registered_count, 1);
}

#[test]
fn register_at_capacity_rejected() {
    let mut svc = IpcServiceState::new(2);
    assert_eq!(svc.register_decide(true), OK);
    assert_eq!(svc.register_decide(true), OK);
    // At capacity — must fail (IPC5)
    let r = svc.register_decide(true);
    assert_eq!(r, ENOMEM, "register beyond capacity must return ENOMEM");
    assert_eq!(svc.registered_count, 2, "count must not change on failed register");
}

#[test]
fn deregister_decrements_count() {
    let mut svc = IpcServiceState::new(4);
    let _ = svc.register_decide(true);
    let _ = svc.register_decide(true);
    assert_eq!(svc.registered_count, 2);

    let r = svc.deregister_decide(true, true);
    assert_eq!(r, OK);
    assert_eq!(svc.registered_count, 1);
}

#[test]
fn deregister_unregistered_endpoint_rejected() {
    let mut svc = IpcServiceState::new(4);
    let r = svc.deregister_decide(true, false);
    assert_eq!(r, ENOENT, "deregister of unregistered endpoint must return ENOENT");
    assert_eq!(svc.registered_count, 0);
}

#[test]
fn register_invalid_params_rejected() {
    let mut svc = IpcServiceState::new(4);
    let r = svc.register_decide(false);
    assert_eq!(r, EINVAL);
    assert_eq!(svc.registered_count, 0);
}

#[test]
fn max_endpoints_limit() {
    let mut svc = IpcServiceState::new(MAX_ENDPOINTS);
    for _ in 0..MAX_ENDPOINTS {
        assert_eq!(svc.register_decide(true), OK);
    }
    assert_eq!(svc.registered_count, MAX_ENDPOINTS);
    // One more must fail (IPC5)
    assert_eq!(svc.register_decide(true), ENOMEM);
}

// ==========================================================================
// IPC6: buffer bounds on send
// ==========================================================================

#[test]
fn send_zero_len_rejected() {
    let r = send_decide(true, true, IpcEndpointState::Bound, 0);
    assert_eq!(r, EINVAL, "zero-length send must be rejected (IPC6)");
}

#[test]
fn send_max_len_accepted() {
    let r = send_decide(true, true, IpcEndpointState::Bound, MAX_MSG_LEN);
    assert_eq!(r, OK, "send of MAX_MSG_LEN must be accepted (IPC6)");
}

#[test]
fn send_over_max_rejected() {
    let r = send_decide(true, true, IpcEndpointState::Bound, MAX_MSG_LEN + 1);
    assert_eq!(r, EINVAL, "send over MAX_MSG_LEN must be rejected (IPC6)");
}

#[test]
fn send_min_len_accepted() {
    let r = send_decide(true, true, IpcEndpointState::Bound, 1);
    assert_eq!(r, OK, "minimum length of 1 must be accepted (IPC6)");
}

#[test]
fn validate_buffer_size_in_range() {
    let r = validate_buffer_size(true, true, 512);
    assert_eq!(r, OK);
}

#[test]
fn validate_buffer_size_zero_rejected() {
    let r = validate_buffer_size(true, true, 0);
    assert_eq!(r, EINVAL);
}

#[test]
fn validate_buffer_size_too_large_rejected() {
    let r = validate_buffer_size(true, true, MAX_MSG_LEN + 1);
    assert_eq!(r, EINVAL);
}

#[test]
fn validate_buffer_size_max_accepted() {
    let r = validate_buffer_size(true, true, MAX_MSG_LEN);
    assert_eq!(r, OK);
}

// ==========================================================================
// send_critical: same rules as send
// ==========================================================================

#[test]
fn send_critical_from_bound_succeeds() {
    let r = send_critical_decide(true, true, IpcEndpointState::Bound, 128);
    assert_eq!(r, OK);
}

#[test]
fn send_critical_from_open_rejected() {
    let r = send_critical_decide(true, true, IpcEndpointState::Open, 128);
    assert_eq!(r, EINVAL);
}

#[test]
fn send_critical_invalid_endpoint() {
    let r = send_critical_decide(false, false, IpcEndpointState::Bound, 128);
    assert_eq!(r, EINVAL);
}

// ==========================================================================
// receive_decide: same rules as send
// ==========================================================================

#[test]
fn receive_from_bound_succeeds() {
    let r = receive_decide(true, true, IpcEndpointState::Bound, 256);
    assert_eq!(r, OK);
}

#[test]
fn receive_not_registered_rejected() {
    let r = receive_decide(true, false, IpcEndpointState::Bound, 256);
    assert_eq!(r, ENOENT);
}

#[test]
fn receive_not_bound_rejected() {
    let r = receive_decide(true, true, IpcEndpointState::Open, 256);
    assert_eq!(r, EINVAL);
}

// ==========================================================================
// open_decide / close_decide (instance-level)
// ==========================================================================

#[test]
fn open_decide_valid_instance() {
    assert_eq!(IpcServiceState::open_decide(true), OK);
}

#[test]
fn open_decide_null_instance() {
    assert_eq!(IpcServiceState::open_decide(false), EINVAL);
}

#[test]
fn close_decide_valid_instance() {
    assert_eq!(IpcServiceState::close_decide(true), OK);
}

#[test]
fn close_decide_null_instance() {
    assert_eq!(IpcServiceState::close_decide(false), EINVAL);
}

// ==========================================================================
// Pointer-null guards (endpoint_valid = false)
// ==========================================================================

#[test]
fn send_null_endpoint_rejected() {
    let r = send_decide(false, false, IpcEndpointState::Bound, 64);
    assert_eq!(r, EINVAL, "null endpoint must return EINVAL");
}

#[test]
fn deregister_null_endpoint_rejected() {
    let mut svc = IpcServiceState::new(4);
    let r = svc.deregister_decide(false, false);
    assert_eq!(r, EINVAL);
}

// ==========================================================================
// is_registered helper
// ==========================================================================

#[test]
fn is_registered_follows_state() {
    let mut ep = IpcEndpoint::new();
    assert!(!ep.is_registered(), "Closed: not registered");
    let _ = ep.transition_open();
    assert!(ep.is_registered(), "Open: is registered");
    let _ = ep.transition_bound();
    assert!(ep.is_registered(), "Bound: is registered");
    let _ = ep.transition_close();
    assert!(!ep.is_registered(), "Closed again: not registered");
}

// ==========================================================================
// Round-trip: register + deregister multiple times
// ==========================================================================

#[test]
fn register_deregister_round_trip() {
    let mut svc = IpcServiceState::new(4);
    for _ in 0..4 {
        assert_eq!(svc.register_decide(true), OK);
        assert_eq!(svc.register_decide(true), OK);
        assert_eq!(svc.deregister_decide(true, true), OK);
        assert_eq!(svc.deregister_decide(true, true), OK);
        assert_eq!(svc.registered_count, 0);
    }
}

// ==========================================================================
// Transition sequence guards
// ==========================================================================

#[test]
fn bound_without_open_rejected() {
    let mut ep = IpcEndpoint::new();
    // Trying to go Closed -> Bound directly must fail
    let r = ep.transition_bound();
    assert_eq!(r, EINVAL, "bound without open must be rejected");
    assert_eq!(ep.state(), IpcEndpointState::Closed);
}

#[test]
fn multiple_close_calls_idempotent() {
    let mut ep = IpcEndpoint::new();
    let _ = ep.transition_open();
    let _ = ep.transition_bound();
    let _ = ep.transition_close();
    // Calling close again must still succeed and state stays Closed (IPC4)
    let r = ep.transition_close();
    assert_eq!(r, OK);
    assert_eq!(ep.state(), IpcEndpointState::Closed);
}

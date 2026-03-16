//! Integration tests for the work queue model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::work::*;

#[test]
fn init_is_idle() {
    let w = WorkItem::init();
    assert!(w.is_idle());
    assert!(!w.is_queued());
    assert!(!w.is_running());
    assert!(!w.is_canceling());
    assert_eq!(w.flags, 0);
    assert_eq!(w.busy_get(), 0);
}

#[test]
fn submit_from_idle() {
    let mut w = WorkItem::init();
    let rc = w.submit();
    assert_eq!(rc, 1); // newly queued
    assert!(w.is_queued());
    assert!(!w.is_idle());
}

#[test]
fn submit_idempotent_when_queued() {
    let mut w = WorkItem::init();
    assert_eq!(w.submit(), 1);
    assert_eq!(w.submit(), 0); // already queued
    assert!(w.is_queued());
}

#[test]
fn submit_while_canceling_returns_ebusy() {
    let mut w = WorkItem::init();
    // Get into RUNNING state
    w.submit();
    w.start_running();
    // Cancel while running -> sets CANCELING
    w.cancel();
    assert!(w.is_canceling());
    // Submit while canceling should fail
    assert_eq!(w.submit(), EBUSY);
}

#[test]
fn submit_while_running_returns_2() {
    let mut w = WorkItem::init();
    w.submit();
    w.start_running();
    assert!(w.is_running());
    assert!(!w.is_queued());
    // Re-submit while running
    let rc = w.submit();
    assert_eq!(rc, 2); // was running, re-queued
    assert!(w.is_queued());
    assert!(w.is_running());
}

#[test]
fn start_running_sets_running_clears_queued() {
    let mut w = WorkItem::init();
    w.submit();
    assert!(w.is_queued());
    w.start_running();
    assert!(w.is_running());
    assert!(!w.is_queued());
}

#[test]
fn finish_running_clears_running() {
    let mut w = WorkItem::init();
    w.submit();
    w.start_running();
    assert!(w.is_running());
    w.finish_running();
    assert!(!w.is_running());
    assert!(w.is_idle());
}

#[test]
fn cancel_idle_returns_zero_busy() {
    let mut w = WorkItem::init();
    let busy = w.cancel();
    assert_eq!(busy, 0);
    assert!(w.is_idle());
}

#[test]
fn cancel_queued_clears_queued() {
    let mut w = WorkItem::init();
    w.submit();
    assert!(w.is_queued());
    let busy = w.cancel();
    assert_eq!(busy, 0); // no longer busy after dequeue
    assert!(!w.is_queued());
    assert!(w.is_idle());
}

#[test]
fn cancel_running_sets_canceling() {
    let mut w = WorkItem::init();
    w.submit();
    w.start_running();
    let busy = w.cancel();
    assert!(busy != 0);
    assert!(w.is_canceling());
    assert!(w.is_running());
    assert!(!w.is_queued());
}

#[test]
fn finish_cancel_clears_canceling() {
    let mut w = WorkItem::init();
    w.submit();
    w.start_running();
    w.cancel();
    assert!(w.is_canceling());
    w.finish_cancel();
    assert!(!w.is_canceling());
}

#[test]
fn full_lifecycle_idle_to_idle() {
    let mut w = WorkItem::init();
    assert!(w.is_idle());

    // Submit
    assert_eq!(w.submit(), 1);
    assert!(w.is_queued());

    // Start running
    w.start_running();
    assert!(w.is_running());
    assert!(!w.is_queued());

    // Finish running
    w.finish_running();
    assert!(w.is_idle());
}

#[test]
fn cancel_running_lifecycle() {
    let mut w = WorkItem::init();
    w.submit();
    w.start_running();

    // Cancel while running
    w.cancel();
    assert!(w.is_canceling());
    assert!(w.is_running());

    // Handler finishes
    w.finish_running();
    assert!(w.is_canceling());
    assert!(!w.is_running());

    // Finalize cancel
    w.finish_cancel();
    assert!(w.is_idle());
}

#[test]
fn busy_get_reflects_flags() {
    let mut w = WorkItem::init();
    assert_eq!(w.busy_get(), 0);

    w.submit();
    assert_eq!(w.busy_get() & FLAG_QUEUED, FLAG_QUEUED);

    w.start_running();
    assert_eq!(w.busy_get() & FLAG_RUNNING, FLAG_RUNNING);
    assert_eq!(w.busy_get() & FLAG_QUEUED, 0);
}

#[test]
fn clone_and_eq() {
    let w1 = WorkItem::init();
    let w2 = w1.clone();
    assert_eq!(w1, w2);

    let mut w3 = w1.clone();
    w3.submit();
    assert_ne!(w1, w3);
}

#[test]
fn multiple_submit_cancel_cycles() {
    let mut w = WorkItem::init();

    for _ in 0..10 {
        assert_eq!(w.submit(), 1);
        w.start_running();
        w.finish_running();
        assert!(w.is_idle());
    }
}

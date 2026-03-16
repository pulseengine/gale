//! Integration tests for thread lifecycle management.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::priority::MAX_PRIORITY;
use gale::thread_lifecycle::{MAX_THREADS, StackInfo, ThreadInfo, ThreadTracker};

// =====================================================================
// StackInfo tests
// =====================================================================

#[test]
fn stack_init_valid() {
    let si = StackInfo::init(0x2000_0000, 4096).unwrap();
    assert_eq!(si.base, 0x2000_0000);
    assert_eq!(si.get_size(), 4096);
    assert_eq!(si.get_usage(), 0);
    assert_eq!(si.unused(), 4096);
}

#[test]
fn stack_init_rejects_zero_size() {
    assert_eq!(StackInfo::init(0x1000, 0), Err(EINVAL));
}

#[test]
fn stack_record_usage_increases_watermark() {
    let mut si = StackInfo::init(0x1000, 1024).unwrap();
    assert_eq!(si.record_usage(256), OK);
    assert_eq!(si.get_usage(), 256);
    assert_eq!(si.unused(), 768);
}

#[test]
fn stack_record_usage_watermark_only_increases() {
    let mut si = StackInfo::init(0x1000, 1024).unwrap();
    assert_eq!(si.record_usage(500), OK);
    assert_eq!(si.get_usage(), 500);

    // Lower observation does not decrease watermark
    assert_eq!(si.record_usage(200), OK);
    assert_eq!(si.get_usage(), 500);

    // Higher observation updates watermark
    assert_eq!(si.record_usage(700), OK);
    assert_eq!(si.get_usage(), 700);
}

#[test]
fn stack_record_usage_rejects_over_size() {
    let mut si = StackInfo::init(0x1000, 512).unwrap();
    assert_eq!(si.record_usage(513), EINVAL);
    assert_eq!(si.get_usage(), 0);
}

#[test]
fn stack_record_usage_at_exact_size() {
    let mut si = StackInfo::init(0x1000, 100).unwrap();
    assert_eq!(si.record_usage(100), OK);
    assert_eq!(si.get_usage(), 100);
    assert_eq!(si.unused(), 0);
}

#[test]
fn stack_conservation() {
    let mut si = StackInfo::init(0x1000, 2048).unwrap();
    for usage in [0, 100, 500, 1000, 2048] {
        si.record_usage(usage);
        assert_eq!(si.unused() + si.get_usage(), si.get_size());
    }
}

// =====================================================================
// ThreadInfo tests
// =====================================================================

#[test]
fn thread_info_new_valid() {
    let ti = ThreadInfo::new(1, 5, 0x2000_0000, 4096).unwrap();
    assert_eq!(ti.id, 1);
    assert_eq!(ti.priority_get(), 5);
    assert_eq!(ti.stack.get_size(), 4096);
    assert_eq!(ti.stack.get_usage(), 0);
}

#[test]
fn thread_info_rejects_invalid_priority() {
    assert_eq!(ThreadInfo::new(1, MAX_PRIORITY, 0x1000, 4096), Err(EINVAL));
    assert_eq!(
        ThreadInfo::new(1, MAX_PRIORITY + 1, 0x1000, 4096),
        Err(EINVAL)
    );
}

#[test]
fn thread_info_rejects_zero_stack() {
    assert_eq!(ThreadInfo::new(1, 0, 0x1000, 0), Err(EINVAL));
}

#[test]
fn thread_info_priority_set_valid() {
    let mut ti = ThreadInfo::new(1, 10, 0x1000, 4096).unwrap();
    assert_eq!(ti.priority_set(5), OK);
    assert_eq!(ti.priority_get(), 5);
}

#[test]
fn thread_info_priority_set_to_zero() {
    let mut ti = ThreadInfo::new(1, 10, 0x1000, 4096).unwrap();
    assert_eq!(ti.priority_set(0), OK);
    assert_eq!(ti.priority_get(), 0);
}

#[test]
fn thread_info_priority_set_to_max_minus_one() {
    let mut ti = ThreadInfo::new(1, 0, 0x1000, 4096).unwrap();
    assert_eq!(ti.priority_set(MAX_PRIORITY - 1), OK);
    assert_eq!(ti.priority_get(), MAX_PRIORITY - 1);
}

#[test]
fn thread_info_priority_set_rejects_invalid() {
    let mut ti = ThreadInfo::new(1, 10, 0x1000, 4096).unwrap();
    assert_eq!(ti.priority_set(MAX_PRIORITY), EINVAL);
    assert_eq!(ti.priority_get(), 10); // unchanged
}

#[test]
fn thread_info_priority_set_preserves_stack() {
    let mut ti = ThreadInfo::new(1, 10, 0x2000, 8192).unwrap();
    ti.stack.record_usage(1024);
    let stack_before = ti.stack.clone();

    ti.priority_set(3);
    assert_eq!(ti.stack, stack_before);
}

#[test]
fn thread_info_priority_set_preserves_id() {
    let mut ti = ThreadInfo::new(42, 10, 0x1000, 4096).unwrap();
    ti.priority_set(5);
    assert_eq!(ti.id, 42);
}

// =====================================================================
// ThreadTracker tests
// =====================================================================

#[test]
fn tracker_new_empty() {
    let t = ThreadTracker::new();
    assert_eq!(t.active_count(), 0);
    assert_eq!(t.peak_count(), 0);
    assert!(!t.has_active());
}

#[test]
fn tracker_create_increments() {
    let mut t = ThreadTracker::new();
    assert_eq!(t.create(), OK);
    assert_eq!(t.active_count(), 1);
    assert!(t.has_active());
}

#[test]
fn tracker_exit_decrements() {
    let mut t = ThreadTracker::new();
    t.create();
    assert_eq!(t.exit(), OK);
    assert_eq!(t.active_count(), 0);
    assert!(!t.has_active());
}

#[test]
fn tracker_exit_empty_returns_error() {
    let mut t = ThreadTracker::new();
    assert_eq!(t.exit(), EINVAL);
    assert_eq!(t.active_count(), 0);
}

#[test]
fn tracker_peak_tracks_maximum() {
    let mut t = ThreadTracker::new();
    for _ in 0..5 {
        t.create();
    }
    assert_eq!(t.peak_count(), 5);

    // Remove some
    for _ in 0..3 {
        t.exit();
    }
    assert_eq!(t.active_count(), 2);
    assert_eq!(t.peak_count(), 5); // peak unchanged

    // Add more, but not beyond peak
    t.create();
    assert_eq!(t.active_count(), 3);
    assert_eq!(t.peak_count(), 5); // still 5
}

#[test]
fn tracker_peak_updates_on_new_high() {
    let mut t = ThreadTracker::new();
    for _ in 0..3 {
        t.create();
    }
    assert_eq!(t.peak_count(), 3);

    t.exit();
    t.exit();

    // Push past old peak
    for _ in 0..4 {
        t.create();
    }
    assert_eq!(t.active_count(), 5);
    assert_eq!(t.peak_count(), 5);
}

#[test]
fn tracker_create_exit_roundtrip() {
    let mut t = ThreadTracker::new();
    assert_eq!(t.create(), OK);
    assert_eq!(t.exit(), OK);
    assert_eq!(t.active_count(), 0);
    assert_eq!(t, ThreadTracker { count: 0, peak: 1 });
}

#[test]
fn tracker_many_create_exit_cycles() {
    let mut t = ThreadTracker::new();
    for _ in 0..100 {
        assert_eq!(t.create(), OK);
        assert_eq!(t.exit(), OK);
    }
    assert_eq!(t.active_count(), 0);
    assert_eq!(t.peak_count(), 1);
}

#[test]
fn tracker_at_capacity_rejects_create() {
    let mut t = ThreadTracker {
        count: MAX_THREADS,
        peak: MAX_THREADS,
    };
    assert_eq!(t.create(), EAGAIN);
    assert_eq!(t.active_count(), MAX_THREADS);
}

#[test]
fn tracker_fill_to_capacity() {
    let mut t = ThreadTracker::new();
    for i in 0..MAX_THREADS {
        assert_eq!(t.create(), OK);
        assert_eq!(t.active_count(), i + 1);
    }
    assert_eq!(t.create(), EAGAIN);
    assert_eq!(t.active_count(), MAX_THREADS);
    assert_eq!(t.peak_count(), MAX_THREADS);
}

#[test]
fn tracker_drain_from_full() {
    let mut t = ThreadTracker {
        count: MAX_THREADS,
        peak: MAX_THREADS,
    };
    for i in 0..MAX_THREADS {
        assert_eq!(t.exit(), OK);
        assert_eq!(t.active_count(), MAX_THREADS - 1 - i);
    }
    assert_eq!(t.exit(), EINVAL);
    assert_eq!(t.active_count(), 0);
    assert_eq!(t.peak_count(), MAX_THREADS); // peak preserved
}

// =====================================================================
// Cross-component tests
// =====================================================================

#[test]
fn thread_lifecycle_full_scenario() {
    let mut tracker = ThreadTracker::new();

    // Create thread 1
    assert_eq!(tracker.create(), OK);
    let mut t1 = ThreadInfo::new(1, 10, 0x2000_0000, 8192).unwrap();

    // Create thread 2
    assert_eq!(tracker.create(), OK);
    let t2 = ThreadInfo::new(2, 5, 0x2000_2000, 4096).unwrap();

    assert_eq!(tracker.active_count(), 2);

    // Thread 1 changes priority
    assert_eq!(t1.priority_set(3), OK);
    assert_eq!(t1.priority_get(), 3);

    // Thread 1 uses some stack
    assert_eq!(t1.stack.record_usage(2048), OK);
    assert_eq!(t1.stack.unused(), 6144);

    // Thread 2 exits
    assert_eq!(tracker.exit(), OK);
    assert_eq!(tracker.active_count(), 1);
    assert_eq!(tracker.peak_count(), 2);

    // Thread 1 exits
    assert_eq!(tracker.exit(), OK);
    assert_eq!(tracker.active_count(), 0);

    // Verify thread 2 was not modified
    assert_eq!(t2.priority_get(), 5);
    assert_eq!(t2.stack.get_size(), 4096);
}

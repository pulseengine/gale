//! Property-based tests for thread lifecycle management.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::error::*;
use gale::priority::MAX_PRIORITY;
use gale::thread_lifecycle::{MAX_THREADS, StackInfo, ThreadInfo, ThreadTracker};
use proptest::prelude::*;

proptest! {
    // =================================================================
    // StackInfo property tests
    // =================================================================

    /// TH3: init with valid size always succeeds.
    #[test]
    fn stack_init_valid(base in 0u32..=u32::MAX, size in 1u32..=100_000) {
        let si = StackInfo::init(base, size).unwrap();
        prop_assert_eq!(si.get_size(), size);
        prop_assert_eq!(si.get_usage(), 0);
        prop_assert_eq!(si.unused(), size);
    }

    /// TH4: usage watermark never exceeds stack size.
    #[test]
    fn stack_usage_bounded(
        size in 1u32..=10_000,
        observations in proptest::collection::vec(0u32..=20_000, 0..50)
    ) {
        let mut si = StackInfo::init(0x1000, size).unwrap();
        for obs in observations {
            si.record_usage(obs);
            prop_assert!(si.get_usage() <= si.get_size());
        }
    }

    /// Stack conservation: unused + usage == size.
    #[test]
    fn stack_conservation(
        size in 1u32..=10_000,
        observations in proptest::collection::vec(0u32..=10_000, 0..20)
    ) {
        let mut si = StackInfo::init(0x1000, size).unwrap();
        for obs in observations {
            si.record_usage(obs);
            prop_assert_eq!(si.unused() + si.get_usage(), si.get_size());
        }
    }

    /// Watermark is monotonically non-decreasing.
    #[test]
    fn stack_watermark_monotonic(
        size in 1u32..=10_000,
        observations in proptest::collection::vec(0u32..=10_000, 1..30)
    ) {
        let mut si = StackInfo::init(0x1000, size).unwrap();
        let mut prev_usage = 0u32;
        for obs in observations {
            si.record_usage(obs);
            prop_assert!(si.get_usage() >= prev_usage);
            prev_usage = si.get_usage();
        }
    }

    /// Invalid observations are rejected without changing state.
    #[test]
    fn stack_rejects_over_size(size in 1u32..=10_000, excess in 1u32..=10_000) {
        let mut si = StackInfo::init(0x1000, size).unwrap();
        let over = size.saturating_add(excess);
        if over > size {
            let usage_before = si.get_usage();
            prop_assert_eq!(si.record_usage(over), EINVAL);
            prop_assert_eq!(si.get_usage(), usage_before);
        }
    }

    // =================================================================
    // ThreadInfo property tests
    // =================================================================

    /// TH1: valid priority always results in successful creation.
    #[test]
    fn thread_info_valid_creation(
        id in 0u32..=1000,
        priority in 0u32..MAX_PRIORITY,
        stack_size in 1u32..=100_000
    ) {
        let ti = ThreadInfo::new(id, priority, 0x1000, stack_size).unwrap();
        prop_assert_eq!(ti.id, id);
        prop_assert_eq!(ti.priority_get(), priority);
        prop_assert!(ti.priority_get() < MAX_PRIORITY);
    }

    /// TH1: invalid priority is rejected.
    #[test]
    fn thread_info_rejects_invalid_priority(
        priority in MAX_PRIORITY..=u32::MAX
    ) {
        prop_assert_eq!(ThreadInfo::new(0, priority, 0x1000, 4096), Err(EINVAL));
    }

    /// TH2: priority_set preserves invariant for valid priorities.
    #[test]
    fn priority_set_valid(
        initial in 0u32..MAX_PRIORITY,
        new_prio in 0u32..MAX_PRIORITY,
        stack_size in 1u32..=100_000
    ) {
        let mut ti = ThreadInfo::new(0, initial, 0x1000, stack_size).unwrap();
        prop_assert_eq!(ti.priority_set(new_prio), OK);
        prop_assert_eq!(ti.priority_get(), new_prio);
        prop_assert!(ti.priority_get() < MAX_PRIORITY);
    }

    /// TH2: priority_set rejects invalid and preserves old value.
    #[test]
    fn priority_set_rejects_invalid(
        initial in 0u32..MAX_PRIORITY,
        bad_prio in MAX_PRIORITY..=u32::MAX
    ) {
        let mut ti = ThreadInfo::new(0, initial, 0x1000, 4096).unwrap();
        prop_assert_eq!(ti.priority_set(bad_prio), EINVAL);
        prop_assert_eq!(ti.priority_get(), initial);
    }

    /// priority_set preserves id and stack.
    #[test]
    fn priority_set_preserves_other_fields(
        id in 0u32..=1000,
        initial in 0u32..MAX_PRIORITY,
        new_prio in 0u32..MAX_PRIORITY,
        stack_size in 1u32..=10_000
    ) {
        let mut ti = ThreadInfo::new(id, initial, 0x2000, stack_size).unwrap();
        let stack_before = ti.stack;
        ti.priority_set(new_prio);
        prop_assert_eq!(ti.id, id);
        prop_assert_eq!(ti.stack, stack_before);
    }

    // =================================================================
    // ThreadTracker property tests
    // =================================================================

    /// TH5/TH6: create-exit roundtrip returns to original count.
    #[test]
    fn tracker_create_exit_roundtrip(n in 1u32..=100) {
        let mut t = ThreadTracker::new();
        for _ in 0..n {
            prop_assert_eq!(t.create(), OK);
        }
        prop_assert_eq!(t.active_count(), n);
        for _ in 0..n {
            prop_assert_eq!(t.exit(), OK);
        }
        prop_assert_eq!(t.active_count(), 0);
    }

    /// TH6: count never exceeds MAX_THREADS.
    #[test]
    fn tracker_count_bounded(
        ops in proptest::collection::vec(prop_oneof![Just(true), Just(false)], 0..200)
    ) {
        let mut t = ThreadTracker::new();
        for is_create in ops {
            if is_create {
                t.create();
            } else {
                t.exit();
            }
            prop_assert!(t.active_count() <= MAX_THREADS);
        }
    }

    /// Peak is always >= count.
    #[test]
    fn tracker_peak_gte_count(
        ops in proptest::collection::vec(prop_oneof![Just(true), Just(false)], 0..200)
    ) {
        let mut t = ThreadTracker::new();
        for is_create in ops {
            if is_create {
                t.create();
            } else {
                t.exit();
            }
            prop_assert!(t.peak_count() >= t.active_count());
        }
    }

    /// Peak is monotonically non-decreasing.
    #[test]
    fn tracker_peak_monotonic(
        ops in proptest::collection::vec(prop_oneof![Just(true), Just(false)], 0..200)
    ) {
        let mut t = ThreadTracker::new();
        let mut prev_peak = 0u32;
        for is_create in ops {
            if is_create {
                t.create();
            } else {
                t.exit();
            }
            prop_assert!(t.peak_count() >= prev_peak);
            prev_peak = t.peak_count();
        }
    }

    /// Concurrent creates and exits maintain consistency.
    #[test]
    fn tracker_interleaved_ops(
        creates in 0u32..=50,
        exits_between in 0u32..=25
    ) {
        let mut t = ThreadTracker::new();
        let mut expected = 0u32;

        for _ in 0..creates {
            if t.create() == OK {
                expected += 1;
            }
        }
        prop_assert_eq!(t.active_count(), expected);

        let exits = exits_between.min(expected);
        for _ in 0..exits {
            prop_assert_eq!(t.exit(), OK);
            expected -= 1;
        }
        prop_assert_eq!(t.active_count(), expected);
    }
}

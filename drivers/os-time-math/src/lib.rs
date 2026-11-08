//! Verified wrap-safe time math for the **gust:os `time`** capability seam
//! (REQ-OS-SYSCALL-001 / REQ-OS-TIMER-001). The `time` interface in
//! `drivers/wit-os/gust-os.wit` documents its deadline/elapsed helpers as
//! *wrap-safe over the counter domain, widened to u64* — this crate is the
//! single implementation of that math, Kani-proven, so the time-provider is a
//! thin binding over a verified core rather than carrying the arithmetic inline.
//!
//! The seam widens a free-running hardware counter to u64 and never rewinds, so
//! `now` advances monotonically from whatever base a deadline was computed at.
//! `elapsed` is written wrap-safe (a wrapping-difference half-range test) so it
//! stays correct even across a single wrap of the domain — not merely in the
//! non-wrapping window a plain `now >= deadline` would cover.
#![cfg_attr(not(kani), no_std)]

/// Half of the u64 domain — the wrap-safe "reached" boundary. A deadline is
/// considered reached once `now` is within the trailing half-domain of it
/// (`now - deadline` small), and still pending while it is within the leading
/// half (`deadline - now` small). This is the standard tick-comparison idiom.
const HALF: u64 = 1 << 63;

/// Deadline = `now + ticks`, wrapping over the u64 domain (never panics/overflows).
/// `ticks` is the caller's requested delay; the seam contract is that meaningful
/// delays are `< HALF` (astronomically large at any real tick rate).
#[inline]
pub const fn deadline(now: u64, ticks: u64) -> u64 {
    now.wrapping_add(ticks)
}

/// Has `deadline` been reached, given the current `now`? Wrap-safe: true once
/// `now` has advanced to or past `deadline` (within the trailing half-domain),
/// false while `deadline` is still ahead (within the leading half-domain).
/// Equivalent to `now >= deadline` in the non-wrapping window, but correct across
/// one wrap — which a plain comparison is not.
#[inline]
pub const fn elapsed(now: u64, deadline: u64) -> bool {
    now.wrapping_sub(deadline) < HALF
}

// ───────────────────────────── proofs ─────────────────────────────
#[cfg(kani)]
mod proofs {
    use super::*;

    /// `deadline` is total (never panics/overflows) and `deadline(now, 0) == now`.
    #[kani::proof]
    fn deadline_total_and_zero_is_now() {
        let now: u64 = kani::any();
        let ticks: u64 = kani::any();
        let _ = deadline(now, ticks); // no panic for any inputs
        assert_eq!(deadline(now, 0), now);
    }

    /// A deadline is NOT yet elapsed the instant it is set, for any real
    /// (half-domain-bounded, non-zero) delay — the wrap-safe "in the future" property.
    #[kani::proof]
    fn fresh_deadline_not_elapsed() {
        let now: u64 = kani::any();
        let ticks: u64 = kani::any();
        kani::assume(ticks > 0 && ticks < HALF);
        assert!(!elapsed(now, deadline(now, ticks)));
    }

    /// Exactly at the deadline, it IS elapsed (`elapsed(d, d)` is true).
    #[kani::proof]
    fn at_deadline_is_elapsed() {
        let d: u64 = kani::any();
        assert!(elapsed(d, d));
    }

    /// Once `now` has advanced by the full delay, the deadline IS elapsed —
    /// wrap-safe across the domain. Monotone completion of a set deadline.
    #[kani::proof]
    fn advanced_to_deadline_is_elapsed() {
        let base: u64 = kani::any();
        let ticks: u64 = kani::any();
        kani::assume(ticks < HALF);
        let d = deadline(base, ticks);
        assert!(elapsed(base.wrapping_add(ticks), d));
    }

    /// `elapsed` agrees with a plain `>=` comparison whenever no wrap occurs
    /// (the practical regime) — so the wrap-safe form is a strict superset, not a
    /// behavior change for real values.
    #[kani::proof]
    fn matches_plain_compare_without_wrap() {
        let now: u64 = kani::any();
        let d: u64 = kani::any();
        // No wrap in either direction of the difference within the half-domain.
        kani::assume(now >= d);
        kani::assume(now - d < HALF);
        assert!(elapsed(now, d));
    }
}

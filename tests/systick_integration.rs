//! Integration tests for the SysTick timer driver model.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::arithmetic_side_effects
)]

use gale::systick::*;

// ======================================================================
// elapsed_cycles — normal (no wrap)
// ======================================================================

#[test]
fn elapsed_no_wrap_simple() {
    // Counter went from 100 down to 60 => 40 cycles elapsed
    assert_eq!(elapsed_cycles(100, 60, 1000), 40);
}

#[test]
fn elapsed_no_wrap_one_cycle() {
    // Counter went from 5 down to 4 => 1 cycle elapsed
    assert_eq!(elapsed_cycles(5, 4, 100), 1);
}

#[test]
fn elapsed_no_wrap_same_value() {
    // No time elapsed — same reading
    assert_eq!(elapsed_cycles(50, 50, 100), 0);
}

#[test]
fn elapsed_no_wrap_full_range() {
    // Counter went from load down to 1 => load - 1 cycles
    let load = 0x00FF_FFFF;
    assert_eq!(elapsed_cycles(load, 1, load), load - 1);
}

// ======================================================================
// elapsed_cycles — wrap-around
// ======================================================================

#[test]
fn elapsed_wrap_simple() {
    // Counter was at 10, wrapped through 0, now at 90 with load=100.
    // Elapsed = 10 + (100 - 90) = 20
    assert_eq!(elapsed_cycles(10, 90, 100), 20);
}

#[test]
fn elapsed_wrap_just_past_zero() {
    // Counter was at 1, wrapped, now at load (=100).
    // Elapsed = 1 + (100 - 100) = 1
    assert_eq!(elapsed_cycles(1, 100, 100), 1);
}

#[test]
fn elapsed_wrap_from_near_load() {
    // Counter was at 5, wrapped, now at 95 with load=100.
    // Elapsed = 5 + (100 - 95) = 10
    assert_eq!(elapsed_cycles(5, 95, 100), 10);
}

#[test]
fn elapsed_wrap_max_load() {
    let load = SYSTICK_MAX_LOAD;
    // Counter at 1, wrapped, now at load.
    // Elapsed = 1 + (load - load) = 1
    assert_eq!(elapsed_cycles(1, load, load), 1);
}

// ======================================================================
// elapsed_cycles — edge cases
// ======================================================================

#[test]
fn elapsed_load_one() {
    // Minimal load: 1. Counter can only be at 1.
    // No wrap: same reading => 0 elapsed
    assert_eq!(elapsed_cycles(1, 1, 1), 0);
}

#[test]
fn elapsed_count_equals_load() {
    // Both at load value (just after wrap in adjusted space)
    let load = 500;
    assert_eq!(elapsed_cycles(load, load, load), 0);
}

#[test]
fn elapsed_last_at_one_current_at_one() {
    // Counter at 1 both times => 0 elapsed
    assert_eq!(elapsed_cycles(1, 1, 1000), 0);
}

#[test]
fn elapsed_max_single_period() {
    // Maximum elapsed in one period: counter went from load to 1
    let load = SYSTICK_MAX_LOAD;
    assert_eq!(elapsed_cycles(load, 1, load), load - 1);
}

// ======================================================================
// cycles_to_ticks
// ======================================================================

#[test]
fn cycles_to_ticks_basic() {
    assert_eq!(cycles_to_ticks(1000, 100), Some(10));
}

#[test]
fn cycles_to_ticks_truncation() {
    // 1050 / 100 = 10 (truncated, not rounded)
    assert_eq!(cycles_to_ticks(1050, 100), Some(10));
}

#[test]
fn cycles_to_ticks_zero_cycles() {
    assert_eq!(cycles_to_ticks(0, 100), Some(0));
}

#[test]
fn cycles_to_ticks_one_cycle_per_tick() {
    assert_eq!(cycles_to_ticks(42, 1), Some(42));
}

#[test]
fn cycles_to_ticks_zero_cpt() {
    // Division by zero returns None
    assert_eq!(cycles_to_ticks(100, 0), None);
}

#[test]
fn cycles_to_ticks_large_values() {
    // u64::MAX / 1 = u64::MAX
    assert_eq!(cycles_to_ticks(u64::MAX, 1), Some(u64::MAX));
}

#[test]
fn cycles_to_ticks_fewer_than_one_tick() {
    // 50 cycles with 100 cycles_per_tick = 0 ticks
    assert_eq!(cycles_to_ticks(50, 100), Some(0));
}

// ======================================================================
// ticks_to_cycles
// ======================================================================

#[test]
fn ticks_to_cycles_basic() {
    assert_eq!(ticks_to_cycles(10, 100), Some(1000));
}

#[test]
fn ticks_to_cycles_zero_ticks() {
    assert_eq!(ticks_to_cycles(0, 100), Some(0));
}

#[test]
fn ticks_to_cycles_zero_cpt() {
    // 0 cycles per tick => always 0 cycles
    assert_eq!(ticks_to_cycles(100, 0), Some(0));
}

#[test]
fn ticks_to_cycles_one_cycle_per_tick() {
    assert_eq!(ticks_to_cycles(42, 1), Some(42));
}

#[test]
fn ticks_to_cycles_overflow() {
    // u64::MAX * 2 would overflow => None
    assert_eq!(ticks_to_cycles(u64::MAX, 2), None);
}

#[test]
fn ticks_to_cycles_max_no_overflow() {
    // u64::MAX / 1 = u64::MAX, so u64::MAX * 1 is fine
    assert_eq!(ticks_to_cycles(u64::MAX, 1), Some(u64::MAX));
}

#[test]
fn ticks_to_cycles_boundary() {
    // Largest ticks that won't overflow with cpt=1000
    let cpt: u32 = 1000;
    let max_ticks = u64::MAX / (cpt as u64);
    assert_eq!(ticks_to_cycles(max_ticks, cpt), Some(max_ticks * cpt as u64));
    // One more would overflow
    assert_eq!(ticks_to_cycles(max_ticks + 1, cpt), None);
}

// ======================================================================
// Conversion roundtrip
// ======================================================================

#[test]
fn roundtrip_exact() {
    // 10 ticks * 100 cpt = 1000 cycles; 1000 / 100 = 10 ticks
    let cpt = 100u32;
    let ticks = 10u64;
    let cycles = ticks_to_cycles(ticks, cpt).unwrap();
    assert_eq!(cycles_to_ticks(cycles, cpt), Some(ticks));
}

#[test]
fn roundtrip_various_cpt() {
    for cpt in [1u32, 2, 10, 100, 1000, 10000, 0x00FF_FFFF] {
        for ticks in [0u64, 1, 2, 100, 1000] {
            if let Some(cycles) = ticks_to_cycles(ticks, cpt) {
                assert_eq!(
                    cycles_to_ticks(cycles, cpt),
                    Some(ticks),
                    "roundtrip failed for ticks={}, cpt={}", ticks, cpt
                );
            }
        }
    }
}

#[test]
fn roundtrip_with_remainder_loses_partial_tick() {
    // 1050 cycles / 100 cpt = 10 ticks (50 cycles lost to truncation)
    // 10 ticks * 100 cpt = 1000 cycles (not 1050)
    let cpt = 100u32;
    let cycles = 1050u64;
    let ticks = cycles_to_ticks(cycles, cpt).unwrap();
    assert_eq!(ticks, 10);
    let recovered = ticks_to_cycles(ticks, cpt).unwrap();
    assert_eq!(recovered, 1000);
    assert!(recovered <= cycles);
    assert!(cycles - recovered < cpt as u64);
}

// ======================================================================
// max_ticks
// ======================================================================

#[test]
fn max_ticks_typical() {
    // With 100 cycles per tick: MAX_TICKS = 0x00FFFFFF / 100 - 1 = 167771
    let mt = max_ticks(100).unwrap();
    assert_eq!(mt, SYSTICK_MAX_LOAD / 100 - 1);
}

#[test]
fn max_ticks_one_cpt() {
    let mt = max_ticks(1).unwrap();
    assert_eq!(mt, SYSTICK_MAX_LOAD - 1);
}

#[test]
fn max_ticks_zero_cpt() {
    assert_eq!(max_ticks(0), None);
}

#[test]
fn max_ticks_equal_to_counter_max() {
    // If cpt == SYSTICK_MAX_LOAD, quotient = 1, max_ticks = 0
    let mt = max_ticks(SYSTICK_MAX_LOAD).unwrap();
    assert_eq!(mt, 0);
}

#[test]
fn max_ticks_larger_than_counter_max() {
    // If cpt > SYSTICK_MAX_LOAD, quotient = 0, returns 0
    let mt = max_ticks(SYSTICK_MAX_LOAD + 1);
    // This returns Some(0) due to the quotient==0 branch
    assert_eq!(mt, Some(0));
}

// ======================================================================
// elapsed_decide
// ======================================================================

#[test]
fn elapsed_decide_no_wrap() {
    let r = elapsed_decide(100, 60, false, 1000, 0);
    assert!(!r.wrap_detected);
    assert_eq!(r.new_overflow_cyc, 0);
    // (load - val2) + overflow = (1000 - 60) + 0 = 940
    assert_eq!(r.elapsed, 940);
}

#[test]
fn elapsed_decide_countflag_wrap() {
    let r = elapsed_decide(100, 60, true, 1000, 0);
    assert!(r.wrap_detected);
    assert_eq!(r.new_overflow_cyc, 1000);
    // (1000 - 60) + 1000 = 1940
    assert_eq!(r.elapsed, 1940);
}

#[test]
fn elapsed_decide_val_wrap() {
    // val1 < val2 implies wrap even without countflag
    let r = elapsed_decide(10, 90, false, 100, 0);
    assert!(r.wrap_detected);
    assert_eq!(r.new_overflow_cyc, 100);
    // (100 - 90) + 100 = 110
    assert_eq!(r.elapsed, 110);
}

#[test]
fn elapsed_decide_with_prior_overflow() {
    // Prior overflow of 500, no new wrap
    let r = elapsed_decide(100, 60, false, 1000, 500);
    assert!(!r.wrap_detected);
    assert_eq!(r.new_overflow_cyc, 500);
    // (1000 - 60) + 500 = 1440
    assert_eq!(r.elapsed, 1440);
}

// ======================================================================
// announce_decide
// ======================================================================

#[test]
fn announce_decide_basic() {
    // 1000 cycles, 0 announced, 0 overflow, 100 cpt => 10 ticks
    let r = announce_decide(1000, 0, 0, 100).unwrap();
    assert_eq!(r.dticks, 10);
    assert_eq!(r.new_cycle_count, 1000);
    assert_eq!(r.new_announced_cycles, 1000);
}

#[test]
fn announce_decide_with_overflow() {
    // 900 cycles, 0 announced, 200 overflow, 100 cpt
    // new_cc = 900 + 200 = 1100; dcycles = 1100; dticks = 11
    let r = announce_decide(900, 0, 200, 100).unwrap();
    assert_eq!(r.dticks, 11);
    assert_eq!(r.new_cycle_count, 1100);
    assert_eq!(r.new_announced_cycles, 1100);
}

#[test]
fn announce_decide_partial_tick() {
    // 150 cycles, 0 announced, 0 overflow, 100 cpt => 1 tick
    // announced advances by 100, leaving 50 unannounced
    let r = announce_decide(150, 0, 0, 100).unwrap();
    assert_eq!(r.dticks, 1);
    assert_eq!(r.new_cycle_count, 150);
    assert_eq!(r.new_announced_cycles, 100);
}

#[test]
fn announce_decide_already_announced() {
    // cycle_count == announced_cycles, no new overflow => 0 ticks
    let r = announce_decide(1000, 1000, 0, 100).unwrap();
    assert_eq!(r.dticks, 0);
    assert_eq!(r.new_cycle_count, 1000);
    assert_eq!(r.new_announced_cycles, 1000);
}

// ======================================================================
// Monotonicity property
// ======================================================================

#[test]
fn monotonicity_cycles_to_ticks() {
    let cpt = 100u32;
    let mut prev = 0u64;
    for cycles in (0u64..=2000).step_by(50) {
        let ticks = cycles_to_ticks(cycles, cpt).unwrap();
        assert!(ticks >= prev, "monotonicity violated at cycles={}", cycles);
        prev = ticks;
    }
}

// ======================================================================
// SYSTICK_MAX_LOAD constant
// ======================================================================

#[test]
fn max_load_is_24_bit() {
    assert_eq!(SYSTICK_MAX_LOAD, 0x00FF_FFFF);
    assert_eq!(SYSTICK_MAX_LOAD, (1 << 24) - 1);
}

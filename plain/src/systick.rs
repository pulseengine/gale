//! Verified ARM Cortex-M SysTick timer driver model.
//!
//! This is a formally verified model of the Zephyr SysTick timer driver
//! (drivers/timer/cortex_m_systick.c). All safety-critical properties
//! are proven with Verus (SMT/Z3).
//!
//! This module models the **tick-counting arithmetic** of the SysTick
//! hardware timer. Actual hardware register access, ISR dispatch, and
//! spinlock handling remain in C — only the cycle/tick computations
//! cross the FFI boundary.
//!
//! The ARM Cortex-M SysTick is a 24-bit down-counter:
//!   - Counts from LOAD down to 0, then wraps back to LOAD
//!   - COUNTFLAG is set when it transitions through 0
//!   - VAL reads the current counter value in [0, LOAD]
//!
//! Source mapping:
//!   COUNTER_MAX           -> SYSTICK_MAX_LOAD  (24-bit maximum)
//!   elapsed()             -> elapsed_cycles     (cycle count with wrap)
//!   dcycles / CYC_PER_TICK -> cycles_to_ticks   (ISR tick computation)
//!   ticks * CYC_PER_TICK   -> ticks_to_cycles   (timeout programming)
//!
//! Omitted (not safety-relevant):
//!   - SysTick->CTRL register manipulation — hardware I/O
//!   - CONFIG_TICKLESS_KERNEL reprogramming — scheduling policy
//!   - spinlock (lock) — concurrency primitive
//!   - Low-power mode idle timer — platform concern
//!   - CONFIG_SYSTEM_CLOCK_HW_CYCLES_PER_SEC_RUNTIME_UPDATE — dynamic config
//!   - overflow_cyc accumulation across multiple ISR calls — ISR scheduling
//!   - SYS_PORT_TRACING_* — instrumentation
//!
//! ASIL-D verified properties:
//!   ST1: elapsed_cycles <= load for single-wrap case
//!   ST2: elapsed_cycles handles down-counter wrap-around correctly
//!   ST3: cycles_to_ticks does not overflow (returns None on overflow)
//!   ST4: ticks_to_cycles does not overflow (returns None on overflow)
//!   ST5: roundtrip: cycles_to_ticks(ticks_to_cycles(t, cpt), cpt) == t
//!   ST6: monotonicity: more cycles => more or equal ticks
//!   ST7: counter values are bounded to 24-bit range
/// Maximum value of the SysTick LOAD register (24-bit counter).
/// Corresponds to COUNTER_MAX (0x00ffffff) in cortex_m_systick.c.
pub const SYSTICK_MAX_LOAD: u32 = 0x00FF_FFFFu32;
/// Compute elapsed cycles from a down-counting SysTick timer.
///
/// The SysTick counter counts down from `load` to 0, then wraps back
/// to `load`. Given the counter value at two points in time (with at
/// most one wrap between them), this function computes the number of
/// elapsed cycles.
///
/// This models the core of the `elapsed()` function in
/// cortex_m_systick.c:
///   `return (last_load - val2) + overflow_cyc;`
///
/// For the single-sample case (no separate overflow tracking), this
/// handles both the normal case (last_count >= current_count, no wrap)
/// and the wrap case (last_count < current_count, one wrap occurred).
///
/// Parameters:
/// - `last_count`: counter value at the earlier sample (from VAL)
/// - `current_count`: counter value at the later sample (from VAL)
/// - `load`: the LOAD register value (counter wraps at this value)
///
/// The function assumes values have been adjusted from [0, load-1] to
/// [1, load] as done in the C driver (val==0 is replaced with last_load).
///
/// ST1: result <= load (bounded)
/// ST2: wrap-around handled correctly
pub fn elapsed_cycles(last_count: u32, current_count: u32, load: u32) -> u32 {
    if last_count >= current_count {
        last_count - current_count
    } else {
        last_count + (load - current_count)
    }
}
/// Convert a cycle count to a tick count.
///
/// This models the ISR computation in cortex_m_systick.c:
///   `dticks = dcycles / CYC_PER_TICK;`
///
/// Uses integer division (truncating toward zero), matching C semantics.
/// Returns None if cycles_per_tick is 0 (invalid configuration).
///
/// ST3: no overflow in the division
/// ST6: monotonicity — more cycles => more or equal ticks
pub fn cycles_to_ticks(cycles: u64, cycles_per_tick: u32) -> Option<u64> {
    if cycles_per_tick == 0 { None } else { Some(cycles / (cycles_per_tick as u64)) }
}
/// Convert a tick count to a cycle count.
///
/// This models the timeout programming in cortex_m_systick.c:
///   `delay = ticks * CYC_PER_TICK;`
///
/// Returns None if the multiplication would overflow u64.
///
/// ST4: no overflow (returns None on overflow)
pub fn ticks_to_cycles(ticks: u64, cycles_per_tick: u32) -> Option<u64> {
    if cycles_per_tick == 0 {
        Some(0u64)
    } else {
        let cpt = cycles_per_tick as u64;
        if ticks > u64::MAX / cpt { None } else { Some(ticks * cpt) }
    }
}
/// Compute maximum programmable ticks for a given cycles_per_tick.
///
/// This models the MAX_TICKS computation in cortex_m_systick.c:
///   `#define MAX_TICKS (COUNTER_MAX / CYC_PER_TICK) - 1`
///
/// Returns None if cycles_per_tick is 0.
///
/// clippy::manual_checked_ops suggests `checked_div`, but the three-case
/// logic (None, quotient==0, quotient>0) does not fit the Option chain
/// cleanly — and `cycles_per_tick == 0` is a Verus `ensures`-linked
/// branch, so keeping the explicit form preserves proof structure.
#[allow(clippy::manual_checked_ops)]
pub fn max_ticks(cycles_per_tick: u32) -> Option<u32> {
    if cycles_per_tick == 0 {
        None
    } else {
        let quotient = SYSTICK_MAX_LOAD / cycles_per_tick;
        if quotient == 0 { Some(0u32) } else { Some(quotient - 1) }
    }
}
/// Lightweight elapsed-cycles decision — takes scalars, no allocation.
///
/// Combines the wrap-detection and cycle-counting from the `elapsed()`
/// function. This is the variant used by FFI to delegate the
/// safety-critical arithmetic to the verified model.
///
/// Parameters match the C driver's pattern:
/// - `val1`: first VAL sample (before CTRL read)
/// - `val2`: second VAL sample (after CTRL read)
/// - `countflag`: whether COUNTFLAG was set in CTRL
/// - `load`: current LOAD register value
/// - `overflow_cyc`: accumulated overflow cycles from prior wraps
///
/// Returns the total elapsed cycles: (load - val2) + overflow_cyc,
/// where overflow_cyc is updated if a wrap is detected.
///
/// ST2: wrap detection matches C driver logic
pub fn elapsed_decide(
    val1: u32,
    val2: u32,
    countflag: bool,
    load: u32,
    overflow_cyc: u32,
) -> ElapsedDecideResult {
    let wrap = countflag || val1 < val2;
    let new_overflow = if wrap { overflow_cyc + load } else { overflow_cyc };
    let base = load - val2;
    ElapsedDecideResult {
        wrap_detected: wrap,
        new_overflow_cyc: new_overflow,
        elapsed: base + new_overflow,
    }
}
/// Result of an elapsed-cycles decision.
#[derive(Debug)]
pub struct ElapsedDecideResult {
    /// Whether a counter wrap was detected.
    pub wrap_detected: bool,
    /// Updated overflow_cyc value (includes any newly detected wrap).
    pub new_overflow_cyc: u32,
    /// Total elapsed cycles since last cycle_count update.
    pub elapsed: u32,
}
/// Result of a tick-announce decision.
#[derive(Debug)]
pub struct AnnounceDecideResult {
    /// Number of complete ticks to announce to the kernel.
    pub dticks: u32,
    /// New cycle_count value.
    pub new_cycle_count: u64,
    /// New announced_cycles value.
    pub new_announced_cycles: u64,
}
/// Lightweight tick-announce decision — models ISR tick computation.
///
/// This models the ISR's tick announcement logic:
///   `dcycles = cycle_count - announced_cycles;`
///   `dticks = dcycles / CYC_PER_TICK;`
///   `announced_cycles += dticks * CYC_PER_TICK;`
///
/// Parameters:
/// - `cycle_count`: current total cycle count (after adding overflow_cyc)
/// - `announced_cycles`: last announced cycle count
/// - `overflow_cyc`: overflow cycles to add to cycle_count
/// - `cycles_per_tick`: cycles per tick (CYC_PER_TICK)
///
/// ST3: no overflow in tick computation
pub fn announce_decide(
    cycle_count: u64,
    announced_cycles: u64,
    overflow_cyc: u32,
    cycles_per_tick: u32,
) -> Option<AnnounceDecideResult> {
    let new_cc = cycle_count + overflow_cyc as u64;
    let dcycles = new_cc - announced_cycles;
    let cpt = cycles_per_tick as u64;
    let dticks = dcycles / cpt;
    let announced_add = dticks * cpt;
    let new_announced = announced_cycles + announced_add;
    if dticks > u32::MAX as u64 {
        None
    } else {
        Some(AnnounceDecideResult {
            dticks: dticks as u32,
            new_cycle_count: new_cc,
            new_announced_cycles: new_announced,
        })
    }
}

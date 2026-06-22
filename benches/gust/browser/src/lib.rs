//! gust in the browser — the SAME verified kiln-async scheduler that boots gust on
//! Cortex-M3 (benches/gust) and dissolves to native via wasm->loom->synth, here run
//! UNMODIFIED in a browser wasm engine. This is the "wasm as universal substrate"
//! leg of gale#74 (FIND-BYOOS-002): one verified component, three runtimes
//! (browser / wasmtime+kiln / dissolved-native).
//!
//! Browser-friendly ABI: the scheduler lives in a wasm-linmem static (no pointer
//! juggling from JS); JS calls gust_boot() once, then gust_poll(rc) per frame and
//! reads the mixed PWM back. The fixed-point Q8 mixer is the failsafe app body.
//! No imports — loads with an empty import object in any wasm engine.
#![no_std]

use core::sync::atomic::{AtomicU32, Ordering};
use kiln_async::{PollRound, SchedConfig, Scheduler, TaskOutcome};

#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

type Sched = Scheduler<6, 6, 4, 2, 2>;

// Same fixed-point Q8 failsafe mixer as benches/gust/src/main.rs.
#[inline]
fn mix(ch: u16) -> u16 {
    let v = 1500i32 + ((256i32 * (ch as i32 - 1024)) >> 8);
    (if v < 1000 { 1000 } else if v > 2000 { 2000 } else { v }) as u16
}

#[no_mangle]
pub extern "C" fn gust_mix(ch: u16) -> u16 {
    mix(ch)
}

static mut SCHED: Option<Sched> = None;
static ROUNDS: AtomicU32 = AtomicU32::new(0);
static LAST_PWM: AtomicU32 = AtomicU32::new(1500);

/// boot(): construct the scheduler + spawn the failsafe task. Call once.
#[no_mangle]
pub extern "C" fn gust_boot() {
    let mut s: Sched = Scheduler::new(SchedConfig::DEFAULT);
    let _ = s.spawn();
    unsafe { SCHED = Some(s); }
    ROUNDS.store(0, Ordering::Relaxed);
    LAST_PWM.store(1500, Ordering::Relaxed);
}

/// One scheduler round driving the failsafe task (mix rc -> pwm). Returns the pwm.
#[no_mangle]
pub extern "C" fn gust_poll(rc: u16) -> u32 {
    let s = match unsafe { SCHED.as_mut() } {
        Some(s) => s,
        None => return 0xFFFF_FFFF,
    };
    let r = s.poll_round(|_s, _id, _fuel| {
        LAST_PWM.store(mix(rc) as u32, Ordering::Relaxed);
        Ok(TaskOutcome::Yielded)
    });
    match r {
        Ok(PollRound::Polled(_)) => {
            ROUNDS.fetch_add(1, Ordering::Relaxed);
            LAST_PWM.load(Ordering::Relaxed)
        }
        Ok(PollRound::Idle) => LAST_PWM.load(Ordering::Relaxed),
        Err(_) => 0xFFFF_FFFF,
    }
}

#[no_mangle]
pub extern "C" fn gust_rounds() -> u32 {
    ROUNDS.load(Ordering::Relaxed)
}

// ── The ACTUAL formally-verified gale components (Verus/Rocq/Lean + Kani) ──────
// These call gale's proven decision functions directly — the same logic that
// ships in the wasm-dist modules and the Zephyr drop-in. The browser is the
// "apply" shell; the *decision* is the verified component. Each returns the
// decision enum as i32 (the kernel's Extract→Decide→Apply, with Decide proven).

/// gale::sem::give_decide — WAKE=0 / INCREMENT=1 / SATURATED=2 (Verus-proven: no overflow).
#[no_mangle]
pub extern "C" fn gale_sem_give(count: u32, limit: u32, has_waiter: u32) -> i32 {
    gale::sem::give_decide(count, limit, has_waiter != 0) as i32
}

/// gale::sem::take_decide — ACQUIRED=0 / WOULD_BLOCK=1 / PEND=2 (Verus-proven: no underflow).
#[no_mangle]
pub extern "C" fn gale_sem_take(count: u32, is_no_wait: u32) -> i32 {
    gale::sem::take_decide(count, is_no_wait != 0) as i32
}

/// gale::msgq::put_decide — STORE=0 / WAKE_READER=1 / PEND=2 / FULL=3 (Verus+Kani-proven ring arithmetic).
#[no_mangle]
pub extern "C" fn gale_msgq_put(
    write_idx: u32, used: u32, max: u32, has_waiter: u32, is_no_wait: u32,
) -> i32 {
    gale::msgq::put_decide(write_idx, used, max, has_waiter != 0, is_no_wait != 0).decision as i32
}

/// gale::msgq::get_decide — READ=0 / WAKE_WRITER=1 / PEND=2 / EMPTY=3 (Verus+Kani-proven).
#[no_mangle]
pub extern "C" fn gale_msgq_get(
    read_idx: u32, used: u32, max: u32, has_waiter: u32, is_no_wait: u32,
) -> i32 {
    gale::msgq::get_decide(read_idx, used, max, has_waiter != 0, is_no_wait != 0).decision as i32
}

/// gale::mutex::lock_decide — ACQUIRE=0 / REENTRANT=1 / PEND=2 / BUSY=3 / OVERFLOW=4
/// (Verus-proven: ownership M3–M5, reentrant count M4, no overflow M10).
#[no_mangle]
pub extern "C" fn gale_mutex_lock(
    lock_count: u32, owner_is_null: u32, owner_is_current: u32, is_no_wait: u32,
) -> i32 {
    gale::mutex::lock_decide(lock_count, owner_is_null != 0, owner_is_current != 0, is_no_wait != 0) as i32
}

/// gale::mutex::unlock_decide — NOT_LOCKED=0 / NOT_OWNER=1 / RELEASED=2 / FULLY_UNLOCKED=3
/// (Verus-proven: -EINVAL M6a, -EPERM M6b, reentrant M7, no underflow M10).
#[no_mangle]
pub extern "C" fn gale_mutex_unlock(
    lock_count: u32, owner_is_null: u32, owner_is_current: u32,
) -> i32 {
    gale::mutex::unlock_decide(lock_count, owner_is_null != 0, owner_is_current != 0) as i32
}

/// gale::event::post_decide — returns the new event bitmask `(current & !mask) | (new & mask)`
/// (Verus-proven bitmask algebra).
#[no_mangle]
pub extern "C" fn gale_event_post(current_events: u32, new_events: u32, mask: u32) -> u32 {
    gale::event::post_decide(current_events, new_events, mask)
}

/// gale::event::wait_decide — MATCHED=0 / PEND=1 / TIMEOUT=2 (wait_type: 0=ANY, 1=ALL).
#[no_mangle]
pub extern "C" fn gale_event_wait(
    current_events: u32, desired: u32, wait_type: u32, is_no_wait: u32,
) -> i32 {
    gale::event::wait_decide(current_events, desired, wait_type as u8, is_no_wait != 0).decision as i32
}

// ── The "bigger example": engine_control (the algorithm) ──────────────────────
// The SAME control step as benches/engine_control (../src/control.c and the WIT
// component wasm-component/src/lib.rs) — table lookups + integer corrections, no
// float, no alloc. Dissolves to a 378 B Cortex-M3 .text (2.1×); here it runs live
// in the browser. The ignition/fuel maps are the single source of truth shared
// with the C bench and the Component (generated by engine_control gen-tables.sh).
const RPM_BINS: usize = 20;
const LOAD_BINS: usize = 20;
include!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../engine_control/wasm-component/src/tables.rs"));

#[inline]
fn ec_rpm_bin(rpm: u32) -> usize {
    let b = (rpm / 500) as usize;
    if b >= RPM_BINS { RPM_BINS - 1 } else { b }
}
#[inline]
fn ec_load_bin(load_pct: u32) -> usize {
    let b = (load_pct / 5) as usize;
    if b >= LOAD_BINS { LOAD_BINS - 1 } else { b }
}
#[inline]
fn ec_enrich_permille(coolant_c: i32) -> u32 {
    if coolant_c >= 80 { 0 } else if coolant_c <= 0 { 300 } else { (((80 - coolant_c) * 300) / 80) as u32 }
}

/// engine_control `control.step` — the same algorithm the bench dissolves to native.
/// Inputs match the WIT `sensors` record; the packed return is
/// `(fuel_duration_us as u32) << 16 | (spark_advance_deg as u16 as u32)`.
#[no_mangle]
pub extern "C" fn ec_step(rpm: u32, load_pct: u32, coolant_c: i32, knock_retard: u32) -> u32 {
    let rb = ec_rpm_bin(rpm);
    let lb = ec_load_bin(load_pct);
    let mut advance = SPARK_ADVANCE_TABLE[rb][lb] as i32 - knock_retard as i32;
    if advance < 0 { advance = 0; }
    let base_fuel = FUEL_DURATION_TABLE[rb][lb] as u32;
    let mut corrected = base_fuel + (base_fuel * ec_enrich_permille(coolant_c) / 1000);
    if corrected > u16::MAX as u32 { corrected = u16::MAX as u32; }
    ((corrected & 0xFFFF) << 16) | ((advance as u16) as u32)
}

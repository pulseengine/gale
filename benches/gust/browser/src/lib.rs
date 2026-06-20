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

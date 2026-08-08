//! SPIKE — gust:hm thin-seam Health Monitor.
//! Bodies lifted VERBATIM from plain/src/health_monitor.rs (Verus+Kani verified).
#![no_std]
#[panic_handler]
fn ph(_: &core::panic::PanicInfo) -> ! { loop {} }

// wit-bindgen's canonical-ABI glue must LINK against a global allocator; this world
// is scalar-only (u32/i32/bool in, bool out), so nothing ever calls it and a
// zero-state trapping allocator keeps the 0-SRAM property intact. Same construction
// as the thin-seam drivers.
use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 { core::ptr::null_mut() }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[global_allocator]
static ALLOC: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "hm-thin", path: "wit" });

mod hm {
pub fn fresh(age_ms: u32, limit_ms: u32) -> bool {
    age_ms <= limit_ms
}

pub fn plausible(value: i32, lo: i32, hi: i32) -> bool {
    lo <= value && value <= hi
}

pub fn innovation_ok(innov_abs: u32, k_sigma: u32) -> bool {
    innov_abs <= k_sigma
}

pub fn budget_ok(used_us: u32, budget_us: u32) -> bool {
    used_us <= budget_us
}

pub fn deadline_ok(lateness_us: u32) -> bool {
    lateness_us == 0
}

pub fn heartbeat_ok(missed_beats: u32) -> bool {
    missed_beats == 0
}

pub fn vote_ok(s0: i32, s1: i32, s2: i32, tol: i32) -> bool {
    let t = tol as i64;
    let d01 = s0 as i64 - s1 as i64;
    let d02 = s0 as i64 - s2 as i64;
    let d12 = s1 as i64 - s2 as i64;
    let a01 = (if d01 >= 0 { d01 } else { -d01 }) <= t;
    let a02 = (if d02 >= 0 { d02 } else { -d02 }) <= t;
    let a12 = (if d12 >= 0 { d12 } else { -d12 }) <= t;
    (a01 && a02) || (a01 && a12) || (a02 && a12)
}}

struct P;
impl exports::gust::hm::detect::Guest for P {
    fn fresh(a: u32, l: u32) -> bool { hm::fresh(a, l) }
    fn plausible(v: i32, lo: i32, hi: i32) -> bool { hm::plausible(v, lo, hi) }
    fn innovation_ok(i: u32, k: u32) -> bool { hm::innovation_ok(i, k) }
    fn budget_ok(u: u32, b: u32) -> bool { hm::budget_ok(u, b) }
    fn deadline_ok(l: u32) -> bool { hm::deadline_ok(l) }
    fn heartbeat_ok(m: u32) -> bool { hm::heartbeat_ok(m) }
    fn vote_ok(a: i32, b: i32, c: i32, t: i32) -> bool { hm::vote_ok(a, b, c, t) }
}
export!(P);

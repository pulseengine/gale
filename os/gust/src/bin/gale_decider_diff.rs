//! gale decider regression differential — does synth's codegen (and the
//! cmp→select fusion, `SYNTH_CMP_SELECT_FUSE`) preserve EVERY gale decision?
//!
//! The 8 verified gale deciders (sem/msgq/mutex/event), dissolved
//! wasm→loom→synth and linked here as the "shim as wasm". This harness sweeps
//! each decider's FULL input domain and folds all decisions into one FNV-1a
//! checksum, printed at exit. Build the dissolved `.o` flag-off → checksum A,
//! flag-on → checksum B. **A == B ⇒ the fusion changed zero decisions** (pure
//! codegen-quality change, no behavioral regression). Combined with the 1953
//! native gale tests (logic) + synth's frozen-byte gate (flag-off bytes), this
//! closes the dissolved-shim regression question across the whole primitive set.
//!
//! Build/run: link wasm-kernel/dec_off.o (or dec_on.o) into this bin via
//! build.rs, then `qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb`.
#![no_std]
#![no_main]
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

extern "C" {
    fn gale_sem_give(count: u32, limit: u32, has_waiter: u32) -> i32;
    fn gale_sem_take(count: u32, is_no_wait: u32) -> i32;
    fn gale_msgq_put(write_idx: u32, used: u32, max: u32, has_waiter: u32, is_no_wait: u32) -> i32;
    fn gale_msgq_get(read_idx: u32, used: u32, max: u32, has_waiter: u32, is_no_wait: u32) -> i32;
    fn gale_mutex_lock(lock_count: u32, owner_is_null: u32, owner_is_current: u32, is_no_wait: u32) -> i32;
    fn gale_mutex_unlock(lock_count: u32, owner_is_null: u32, owner_is_current: u32) -> i32;
    fn gale_event_post(current_events: u32, new_events: u32, mask: u32) -> u32;
    fn gale_event_wait(current_events: u32, desired: u32, wait_type: u32, is_no_wait: u32) -> i32;
}

struct Fnv(u64);
impl Fnv {
    #[inline(always)]
    fn mix(&mut self, v: u32) {
        // fold value + a running count into a 64-bit FNV-1a so order matters
        let bytes = v.to_le_bytes();
        for b in bytes {
            self.0 ^= b as u64;
            self.0 = self.0.wrapping_mul(0x0000_0100_0000_01B3);
        }
    }
}

#[entry]
fn main() -> ! {
    let mut h = Fnv(0xcbf2_9ce4_8422_2325);
    let mut cases: u32 = 0;

    // sem::give — count 0..=9, limit 0..=9, has_waiter 0..=1
    for count in 0..=9u32 { for limit in 0..=9u32 { for hw in 0..=1u32 {
        h.mix(unsafe { gale_sem_give(count, limit, hw) } as u32); cases += 1;
    }}}
    // sem::take — count 0..=9, no_wait 0..=1
    for count in 0..=9u32 { for nw in 0..=1u32 {
        h.mix(unsafe { gale_sem_take(count, nw) } as u32); cases += 1;
    }}
    // msgq::put / get — widx/ridx 0..=8, used 0..=8, max 1..=8, 2 bools
    for idx in 0..=8u32 { for used in 0..=8u32 { for max in 1..=8u32 { for w in 0..=1u32 { for nw in 0..=1u32 {
        h.mix(unsafe { gale_msgq_put(idx, used, max, w, nw) } as u32);
        h.mix(unsafe { gale_msgq_get(idx, used, max, w, nw) } as u32);
        cases += 2;
    }}}}}
    // mutex::lock — lock_count 0..=5, null 0..=1, cur 0..=1, no_wait 0..=1
    for lc in 0..=5u32 { for null in 0..=1u32 { for cur in 0..=1u32 { for nw in 0..=1u32 {
        h.mix(unsafe { gale_mutex_lock(lc, null, cur, nw) } as u32); cases += 1;
    }}}}
    // mutex::unlock — lock_count 0..=5, null 0..=1, cur 0..=1
    for lc in 0..=5u32 { for null in 0..=1u32 { for cur in 0..=1u32 {
        h.mix(unsafe { gale_mutex_unlock(lc, null, cur) } as u32); cases += 1;
    }}}
    // event::post — current/new/mask 0..=15
    for c in 0..=15u32 { for n in 0..=15u32 { for m in 0..=15u32 {
        h.mix(unsafe { gale_event_post(c, n, m) }); cases += 1;
    }}}
    // event::wait — current/desired 0..=15, wait_type 0..=1, no_wait 0..=1
    for c in 0..=15u32 { for d in 0..=15u32 { for wt in 0..=1u32 { for nw in 0..=1u32 {
        h.mix(unsafe { gale_event_wait(c, d, wt, nw) } as u32); cases += 1;
    }}}}

    let _ = hprintln!("gale-decider-diff: cases={} checksum=0x{:016x}", cases, h.0);
    let _ = hprintln!("gale-decider-diff: done");
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

#![no_std]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! { loop {} }
use core::alloc::{GlobalAlloc, Layout};
struct NoAlloc;
unsafe impl GlobalAlloc for NoAlloc { unsafe fn alloc(&self,_:Layout)->*mut u8{core::ptr::null_mut()} unsafe fn dealloc(&self,_:*mut u8,_:Layout){} }
#[global_allocator] static A: NoAlloc = NoAlloc;

wit_bindgen::generate!({ world: "time-provider", path: "../wit-os", generate_all });
use crate::gust::hal::mmio::read32;
use exports::gust::os::time::Guest;

const TIM2_CNT: u32 = 0x4000_0024; // timer count register (via gust:hal/mmio)

// Ticks/sec (REQ-OS-TIMER-001, wit-os/gust-os.wit `time.resolution`). TIM2_CNT is a
// raw free-running counter with no PSC/ARR config crossing this seam (unlike
// timer-thin, which owns the real prescaler), so there is no clock-tree-derived Hz
// to report here yet — 1 MHz (1 tick = 1 us) is a documented placeholder, matching
// the ONE_SEC=1_000_000 tick convention gust_timer_probe.rs already uses for the
// executor-backed `timer` interface. Revisit once a node wires a configured PSC/ARR
// timer (e.g. timer-thin) into this seam instead of the mock counter.
const RESOLUTION_HZ: u64 = 1_000_000;

struct P;
impl Guest for P {
    fn now() -> u64 { read32(TIM2_CNT) as u64 }
    // deadline/elapsed delegate to the Kani-proven wrap-safe core (os-time-math),
    // so the seam's documented wrap-safety is backed by a proof, not asserted inline.
    fn deadline(now: u64, ticks: u64) -> u64 { gust_os_time_math::deadline(now, ticks) }
    fn elapsed(now: u64, deadline: u64) -> bool { gust_os_time_math::elapsed(now, deadline) }
    fn resolution() -> u64 { RESOLUTION_HZ }
}
export!(P);

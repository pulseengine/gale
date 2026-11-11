//! gust scheduler benchmark. Deterministic cost per scheduler op.
//! Local timing: SysTick delta under qemu `-icount` (instruction-proportional, deterministic;
//! per-op reported x1000 for resolution). TRUE Cortex-M3 cycles: the Renode stm32vldiscovery
//! run (renode/gust_f100.robot reads ExecutedInstructions). Run: `cargo run --release --bin bench`.
#![no_std]
#![no_main]
use cortex_m_rt::entry;
use cortex_m_semihosting::{hprintln, debug};
use cortex_m::peripheral::{Peripherals, SYST};
use cortex_m::peripheral::syst::SystClkSource;
use kiln_async::{Scheduler, SchedConfig, TaskOutcome};
use panic_halt as _;

struct Noop;
unsafe impl core::alloc::GlobalAlloc for Noop {
    unsafe fn alloc(&self, _: core::alloc::Layout) -> *mut u8 { core::ptr::null_mut() }
    unsafe fn dealloc(&self, _: *mut u8, _: core::alloc::Layout) {}
}
#[global_allocator] static GA: Noop = Noop;
type S = Scheduler<8, 8, 4, 2, 2>;
#[inline(always)] fn now() -> u32 { SYST::get_current() }
#[inline(always)] fn delta(a: u32, b: u32) -> u32 { a.wrapping_sub(b) & 0x00FF_FFFF }

#[inline(never)]
fn bench_poll(ntask: u32, iters: u32) -> u32 {
    let mut s = S::new(SchedConfig::DEFAULT);
    for _ in 0..ntask { let _ = s.spawn(); }
    let v0 = now();
    for _ in 0..iters { let _ = s.poll_round(|_, _, _| Ok(TaskOutcome::Yielded)); }
    delta(v0, now())
}

#[entry]
fn main() -> ! {
    let cp = Peripherals::take().unwrap();
    let mut syst = cp.SYST;
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(0x00FF_FFFF);
    syst.clear_current();
    syst.enable_counter();

    let _ = hprintln!("# gust scheduler bench  Scheduler<8,8,4,2,2>");
    let _ = hprintln!("# ticks = SysTick under qemu -icount (deterministic, instruction-proportional; true M3 cycles via Renode)");
    let _ = hprintln!("# footprint: Scheduler={}B  SchedConfig={}B", core::mem::size_of::<S>(), core::mem::size_of::<SchedConfig>());
    let _ = hprintln!("gust-bench,op,ntasks,iters,total_ticks,milliticks_per_op");
    let iters = 20_000u32;
    for &n in &[1u32, 2, 4, 8] {
        let dt = bench_poll(n, iters);
        let _ = hprintln!("gust-bench,poll_round,{},{},{},{}", n, iters, dt, (dt as u64 * 1000 / iters as u64));
    }
    // spawn cost (Spawned->Ready FSM + table insert + ready push)
    {
        let mut s = S::new(SchedConfig::DEFAULT);
        let v0 = now();
        for _ in 0..8 { let _ = s.spawn(); }
        let dt = delta(v0, now());
        let _ = hprintln!("gust-bench,spawn,8,8,{},{}", dt, (dt as u64 * 1000 / 8));
    }
    let _ = hprintln!("gust-bench: done");
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

//! gust minimal boot image — Cortex-M3. Proves the TCB-contract superloop:
//! native shim (vector table + SysTick + stub MMIO) drives kiln-async poll() with a
//! fixed-point-mixer failsafe task. Boots in qemu lm3s6965evb (M3); same ELF -> Renode F100.
#![no_std]
#![no_main]
use cortex_m_rt::{entry, exception};
use cortex_m_semihosting::{hprintln, debug};
use cortex_m::peripheral::syst::SystClkSource;
use core::sync::atomic::{AtomicU32, Ordering};
use kiln_async::{Scheduler, SchedConfig, TaskOutcome, PollRound};
use panic_halt as _;

// SysTick-driven monotonic tick (the native shim's time source)
static NOW_TICKS: AtomicU32 = AtomicU32::new(0);
// stub MMIO: "PWM register"
static LAST_PWM: AtomicU32 = AtomicU32::new(0);

// kiln#338: stub global allocator so the no_std image links (kiln-error/recovery.rs pulls alloc).
struct Noop;
unsafe impl core::alloc::GlobalAlloc for Noop {
    unsafe fn alloc(&self, _: core::alloc::Layout) -> *mut u8 { core::ptr::null_mut() }
    unsafe fn dealloc(&self, _: *mut u8, _: core::alloc::Layout) {}
}
#[global_allocator] static GA: Noop = Noop;

#[inline] fn pwm_write(chan: u32, pulse_us: u32) { if chan == 0 { LAST_PWM.store(pulse_us, Ordering::Relaxed); } }
// fixed-point Q8 mixer (integer -> clears all four synth gaps)
#[inline] fn mix(ch: u16) -> u16 {
    let v = 1500i32 + ((256i32 * (ch as i32 - 1024)) >> 8);
    (if v < 1000 { 1000 } else if v > 2000 { 2000 } else { v }) as u16
}

#[exception] fn SysTick() { NOW_TICKS.fetch_add(1, Ordering::Relaxed); }

#[entry]
fn main() -> ! {
    let cp = cortex_m::Peripherals::take().unwrap();
    let mut syst = cp.SYST;
    syst.set_clock_source(SystClkSource::Core);
    syst.set_reload(2_000);
    syst.clear_current();
    syst.enable_counter();
    syst.enable_interrupt();

    let _ = hprintln!("gust boot: kiln-async scheduler on Cortex-M3 (TCB superloop)");
    let _ = hprintln!("mem: Scheduler<6,6,4,2,2>={}B  SchedConfig={}B  static.bss=20B",
        core::mem::size_of::<Scheduler<6,6,4,2,2>>(), core::mem::size_of::<SchedConfig>());

    // boot(): scheduler + spawn the failsafe task
    let mut sched: Scheduler<6, 6, 4, 2, 2> = Scheduler::new(SchedConfig::DEFAULT);
    let _t = sched.spawn().unwrap();

    let mut rounds: u32 = 0;
    let mut rc: u16 = 1024;
    loop {
        let now = NOW_TICKS.load(Ordering::Relaxed);
        let r = sched.poll_round(|_s, _id, _fuel| {
            let pwm = mix(rc);            // failsafe task body: mix RC -> PWM
            pwm_write(0, pwm as u32);
            Ok(TaskOutcome::Yielded)      // periodic failsafe: stays runnable
        });
        match r {
            Ok(PollRound::Polled(_)) => rounds += 1,
            Ok(PollRound::Idle) => {}
            Err(_) => { let _ = hprintln!("poll error"); break; }
        }
        rc = 1024u16.wrapping_add((now & 0x1FF) as u16);
        if rounds % 1000 == 0 {
            let _ = hprintln!("poll round={} now_ticks={} pwm={}", rounds, now, LAST_PWM.load(Ordering::Relaxed));
        }
        if rounds >= 5000 {
            let _ = hprintln!("gust: {} poll rounds, scheduler stable, pwm={}", rounds, LAST_PWM.load(Ordering::Relaxed));
            debug::exit(debug::EXIT_SUCCESS);
        }
    }
    loop {}
}

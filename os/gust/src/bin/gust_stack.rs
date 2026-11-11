//! gust-stack — the north star, end to end: a Component-Model composition driven
//! ON the gust stack. The kiln-async scheduler (gust's executor) drives the
//! MELD-fused, synth-dissolved composition (gale-app-demo imports gale:kernel;
//! gale-kiln provides it over the verified gale::* deciders) as a scheduled task,
//! bare-metal, no wasm runtime.
//!
//! gust_fused proved the *dissolve* (CM → fuse → synth → run-demo()=53 once).
//! This proves the other half — "running on our stack": the same dissolved
//! composition is the body of a kiln task, re-polled every scheduler round, so
//! the verified components execute as scheduled work on the kiln/gust executor.
//! Components on top → meld fuse → one module → dissolve → driven by kiln on gust.
//!
//! Boot: cargo run --release --bin gust_stack  (qemu lm3s6965evb / Cortex-M3)
#![no_std]
#![no_main]
use core::sync::atomic::{AtomicU32, Ordering};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use kiln_async::{SchedConfig, Scheduler, TaskOutcome};
use panic_halt as _;

extern "C" {
    // The dissolved CM composition's entry (gale-app-demo + gale-kiln, meld-fused
    // → synth → native). synth keeps the wasm export name verbatim.
    #[link_name = "run-demo"]
    fn run_demo() -> u32;
}

type S = Scheduler<8, 8, 4, 2, 2>;
static LAST: AtomicU32 = AtomicU32::new(0);
static MISMATCH: AtomicU32 = AtomicU32::new(0);

#[entry]
fn main() -> ! {
    let _ = hprintln!(
        "gust-stack: kiln-async scheduler driving the dissolved CM composition (app + gale-kiln) as a task"
    );
    let mut s = S::new(SchedConfig::DEFAULT);
    let _ = s.spawn();

    const ROUNDS: u32 = 5000;
    let mut completed = 0u32;
    for _ in 0..ROUNDS {
        // Each scheduler round, the task body IS the dissolved CM composition.
        let r = s.poll_round(|_sched, _id, _fuel| {
            let v = unsafe { run_demo() };
            LAST.store(v, Ordering::Relaxed);
            if v != 53 {
                MISMATCH.fetch_add(1, Ordering::Relaxed);
            }
            Ok(TaskOutcome::Yielded)
        });
        if r.is_ok() {
            completed += 1;
        }
    }

    let last = LAST.load(Ordering::Relaxed);
    let bad = MISMATCH.load(Ordering::Relaxed);
    let _ = hprintln!(
        "gust-stack: {} poll rounds; dissolved run-demo() = {} each round; mismatches = {}",
        completed, last, bad
    );
    let _ = hprintln!(
        "gust-stack: the verified gale CM composition ran on the kiln/gust stack (dissolved to native, no runtime)"
    );
    debug::exit(if last == 53 && bad == 0 {
        debug::EXIT_SUCCESS
    } else {
        debug::EXIT_FAILURE
    });
    loop {}
}

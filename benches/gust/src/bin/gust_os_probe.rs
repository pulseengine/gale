//! gust-os-probe — LOCAL qemu-semihosting liveness probe of the gust:os v0.4.0
//! STEP-1 node (drivers/os-node/os-time-cm3.o): an app that imports ONLY gust:os/time,
//! wac-plugged with a time provider (backed by gust:hal/mmio), meld-fused + dissolved
//! to one 0-SRAM object exporting `run` and importing only `read32`. Proves the
//! syscall-seam compose is functionally live end-to-end: we provide the read32 TCB
//! atom, call the app's `run`, and check its OS-time logic returns the ok code.
#![no_std]
#![no_main]
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// The whole TCB the gust:os/time node needs: one mmio read atom (the timer CNT).
#[no_mangle]
pub extern "C" fn read32(_addr: u32) -> u32 { 1000 }

extern "C" { fn run() -> u32; }

#[entry]
fn main() -> ! {
    let r = unsafe { run() };
    if r == 0xA {
        hprintln!("gust-os-probe OK: app ran on gust:os/time (run()={:#x})", r);
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        hprintln!("gust-os-probe FAIL: run()={:#x} (want 0xA)", r);
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}

//! gust-spi-probe — the LOCAL qemu-semihosting probe of the DISSOLVED spi-thin .o,
//! run BEFORE the Renode content-gate to catch table/linmem no-op bugs fast.
//!
//! Points the dissolved driver at a plain `[u32; 8]` RAM window (real mapped SRAM
//! on lm3s6965evb) and checks the register effects + the transfer FSM via
//! semihosting — so a dissolved primitive that silently no-ops (e.g. a `.rodata`
//! linmem lookup that reads 0 under `--relocatable`) fails HERE, on `cargo run`,
//! not three CI minutes later in Renode. Reproduces the source-vs-dissolve bisect
//! discipline from the GPIO no-op bug.
#![no_std]
#![no_main]
use core::ptr::{addr_of_mut, read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// The mmio seam the dissolved driver imports — here it reads/writes the RAM window.
#[no_mangle]
pub extern "C" fn mmio_read32(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[no_mangle]
pub extern "C" fn mmio_write32(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}

extern "C" {
    fn spi_configure(base: u32, mode: u32, br_idx: u32);
    fn spi_xfer_byte(base: u32, out: u32) -> u32;
    fn spi_begin(state: u32, count: u32) -> u32;
    fn spi_step(state: u32) -> u32;
    fn spi_is_complete(state: u32) -> u32;
    fn spi_abort(state: u32) -> u32;
}

// Fake SPI register window in RAM: word i = byte offset i*4 (CR1=0, SR=2, DR=3).
static mut REG: [u32; 8] = [0; 8];
const CR1_EXPECT: u32 = 0x357; // mode=3, br_idx=2 (see gust_spi.rs)
const SPI_FAULT: u32 = 0xFFFF_FFFF;

#[entry]
fn main() -> ! {
    let base = addr_of_mut!(REG) as u32;
    let mut ok = true;

    // 1) config: driver must actually WRITE CR1 (a no-op leaves it 0).
    unsafe { spi_configure(base, 3, 2) };
    let cr1 = unsafe { read_volatile(addr_of_mut!(REG[0])) };
    if cr1 != CR1_EXPECT {
        hprintln!("spi-config FAIL: CR1={:#x} want {:#x}", cr1, CR1_EXPECT);
        ok = false;
    } else {
        hprintln!("spi-config ok: CR1={:#x}", cr1);
    }

    // 2) full-duplex byte: seed SR (TXE|RXNE) so the polled loops pass; the driver
    //    writes DR and reads it back on the RAM window.
    unsafe { write_volatile(addr_of_mut!(REG[2]), 0x3) };
    let rx = unsafe { spi_xfer_byte(base, 0xA5) };
    let dr = unsafe { read_volatile(addr_of_mut!(REG[3])) } & 0xFF;
    if rx != 0xA5 || dr != 0xA5 {
        hprintln!("spi-xfer FAIL: rx={:#x} dr={:#x}", rx, dr);
        ok = false;
    } else {
        hprintln!("spi-xfer ok: rx={:#x} dr={:#x}", rx, dr);
    }

    // 3) transfer FSM (SQE→CQE): exclusive bus + no lost byte.
    let s0 = unsafe { spi_begin(0, 3) };
    let busy = unsafe { spi_begin(s0, 2) };
    let s1 = unsafe { spi_step(s0) };
    let s2 = unsafe { spi_step(s1) };
    let s3 = unsafe { spi_step(s2) };
    let done = unsafe { spi_is_complete(s3) };
    let mid = unsafe { spi_is_complete(s0) };
    let idle = unsafe { spi_abort(s3) };
    if busy != SPI_FAULT || done != 1 || mid != 0 || idle != 0 {
        hprintln!(
            "spi-fsm FAIL: busy={:#x} done={} mid={} idle={:#x}",
            busy, done, mid, idle
        );
        ok = false;
    } else {
        hprintln!("spi-fsm ok: begin/step*3→complete, dup-begin faults, abort→idle");
    }

    if ok {
        hprintln!("spi-probe ALL OK");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}

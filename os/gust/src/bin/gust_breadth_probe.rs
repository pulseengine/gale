//! gust-breadth-probe — LOCAL qemu-semihosting liveness probe of the 4-driver
//! BREADTH node (REQ-DRV-BREADTH-001): gpio+timer+spi+uart, each a verified-wasm
//! gust:hal component, wac/meld-fused into ONE dissolved `.o` (drivers/breadth/
//! breadth-cm3.o, 0 SRAM, func_N collision gone). Proves all four drivers are
//! FUNCTIONALLY live from the single fused object — register effects + FSM — via a
//! RAM-backed mmio/irq bridge, before the Renode gate. The bridge is the whole TCB:
//! read32 / write32 / poll (3 atoms, within the four).
#![no_std]
#![no_main]
use core::ptr::addr_of_mut;
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

// RAM-backed peripheral store: every driver register access routes here, indexed
// by word. The used bases (gpio 0x1000, timer 0x2000, spi 0x3000, uart's fixed
// USART1 0x40013800) map to distinct clusters — no aliasing across the exercised
// registers. This IS the mmio bridge for the probe (Renode uses real windows).
static mut MEM: [u32; 0x1000] = [0; 0x1000];
#[inline]
fn idx(addr: u32) -> usize { ((addr >> 2) & 0xFFF) as usize }

#[no_mangle]
pub extern "C" fn read32(addr: u32) -> u32 { unsafe { MEM[idx(addr)] } }
#[no_mangle]
pub extern "C" fn write32(addr: u32, val: u32) { unsafe { MEM[idx(addr)] = val; } }
// irq.poll — report the line fired (1) so the split-phase uart_rx_fired path runs.
#[no_mangle]
pub extern "C" fn poll(_line: u32) -> u32 { 1 }

extern "C" {
    fn gpio_configure(base: u32, pin: u32, mode_idx: u32);
    fn timer_init(base: u32, psc: u32, arr: u32);
    fn timer_deadline(now: u32, ticks: u32) -> u32;
    fn timer_elapsed(now: u32, deadline: u32) -> u32;
    fn spi_configure(base: u32, mode: u32, br_idx: u32);
    fn spi_begin(state: u32, count: u32) -> u32;
    fn spi_step(state: u32) -> u32;
    fn spi_is_complete(state: u32) -> u32;
    fn uart_init(brr: u32);
    fn uart_rx() -> u32;
    fn uart_rx_fired() -> u32;
}

const G: u32 = 0x1000; // gpio base
const TM: u32 = 0x2000; // timer base
const SP: u32 = 0x3000; // spi base
const U: u32 = 0x4001_3800; // uart fixed USART1

fn rd(a: u32) -> u32 { unsafe { MEM[idx(a)] } }
fn wr(a: u32, v: u32) { unsafe { MEM[idx(a)] = v; } }

#[entry]
fn main() -> ! {
    let mut ok = true;

    // 1) GPIO — configure PC8 as output (mode idx 4 → nibble 0x3) → CRH[3:0]=0x3.
    unsafe { gpio_configure(G, 8, 4) };
    let crh = rd(G + 0x04);
    if crh & 0xF != 0x3 { hprintln!("gpio FAIL: CRH={:#x}", crh); ok = false; } else { hprintln!("gpio ok"); }

    // 2) TIMER — init writes PSC/ARR/CR1(CEN); deadline/elapsed math (Kani-proven).
    unsafe { timer_init(TM, 0x1234, 0xABCD) };
    let psc = rd(TM + 0x28); let arr = rd(TM + 0x2C); let cr1 = rd(TM + 0x00);
    let d = unsafe { timer_deadline(100, 50) };
    let e0 = unsafe { timer_elapsed(149, d) }; let e1 = unsafe { timer_elapsed(150, d) };
    if psc != 0x1234 || arr != 0xABCD || cr1 & 1 == 0 || d != 150 || e0 != 0 || e1 != 1 {
        hprintln!("timer FAIL: psc={:#x} arr={:#x} cr1={:#x} d={} e0={} e1={}", psc, arr, cr1, d, e0, e1); ok = false;
    } else { hprintln!("timer ok"); }

    // 3) SPI — configure (mode3/br2 → CR1=0x357) + FSM begin→step×3→complete.
    unsafe { spi_configure(SP, 3, 2) };
    let scr1 = rd(SP + 0x00);
    let s0 = unsafe { spi_begin(0, 3) };
    let s3 = unsafe { spi_step(spi_step(spi_step(s0))) };
    let done = unsafe { spi_is_complete(s3) };
    if scr1 != 0x357 || done != 1 {
        hprintln!("spi FAIL: CR1={:#x} done={}", scr1, done); ok = false;
    } else { hprintln!("spi ok"); }

    // 4) UART — seed SR (TXE|RXNE, no error) so the polled paths pass; init writes
    //    BRR/CR1; rx returns the seeded DR; rx_fired sees the (poll==1) line.
    wr(U + 0x00, (1 << 7) | (1 << 5)); // SR: TXE|RXNE
    wr(U + 0x04, 0xA5); // DR seed
    unsafe { uart_init(0x45) };
    let brr = rd(U + 0x08); let ucr1 = rd(U + 0x0C);
    let rx = unsafe { uart_rx() }; let fired = unsafe { uart_rx_fired() };
    if brr != 0x45 || ucr1 & (1 << 13) == 0 || rx != 0xA5 || fired != 1 {
        hprintln!("uart FAIL: brr={:#x} cr1={:#x} rx={:#x} fired={}", brr, ucr1, rx, fired); ok = false;
    } else { hprintln!("uart ok"); }

    // keep MEM live so the linker can't drop the bridge store.
    let _ = unsafe { addr_of_mut!(MEM) };
    if ok { hprintln!("breadth-probe ALL OK: 4 drivers live from one fused .o"); debug::exit(debug::EXIT_SUCCESS); }
    else { debug::exit(debug::EXIT_FAILURE); }
    loop {}
}

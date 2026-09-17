//! gust-breadth — the 4-driver BREADTH node (REQ-DRV-BREADTH-001) driven bare-metal
//! on gust, with a self-checking Renode content-gate. Links ONE dissolved object
//! (drivers/breadth/breadth-cm3.o) that is gpio+timer+spi+uart — four verified-wasm
//! gust:hal COMPONENTS, wac/meld-fused into a single relocatable module (0 SRAM, no
//! func_N collision). The whole TCB is the 3-atom bridge below: read32 / write32 /
//! poll. Asserts each driver's register effects / FSM on a real STM32 model and
//! emits breadth-*-ok over USART1 — the last line is TX'd BY the dissolved uart
//! driver itself, proving all four are live end-to-end from the one fused object.
#![no_std]
#![no_main]
use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::debug;
#[cfg(feature = "target-f100")]
use cortex_m_semihosting::hprintln;
use panic_halt as _;

// The gust:hal capability bridge the fused .o imports (gust:hal/mmio.{read32,
// write32} + gust:hal/irq.poll) — the entire trusted-native surface, 3 atoms.
#[no_mangle]
pub extern "C" fn read32(addr: u32) -> u32 { unsafe { read_volatile(addr as *const u32) } }
#[no_mangle]
pub extern "C" fn write32(addr: u32, val: u32) { unsafe { write_volatile(addr as *mut u32, val) } }
#[no_mangle]
pub extern "C" fn poll(_line: u32) -> u32 { 1 }

// Default (Renode) build: the object's exports are called directly, as before, so the
// gated image stays byte-identical.
#[cfg(not(feature = "target-f100"))]
extern "C" {
    fn gpio_configure(base: u32, pin: u32, mode_idx: u32);
    fn timer_init(base: u32, psc: u32, arr: u32);
    fn timer_deadline(now: u32, ticks: u32) -> u32;
    fn timer_elapsed(now: u32, deadline: u32) -> u32;
    fn spi_configure(base: u32, mode: u32, br_idx: u32);
    fn spi_begin(state: u32, count: u32) -> u32;
    fn spi_step(state: u32) -> u32;
    fn spi_is_complete(state: u32) -> u32;
    fn uart_tx_byte(b: u32);
}

// ON SILICON the fused object is NOT 0-SRAM. It reads and writes wasm data bytes at
// 0x0010_000C..0x0010_0011 as [r11 + off] (40 accesses; gale#398). With r11 unset the
// F100 took a precise BusFault at BFAR=0x0010_000D and the image hung — the same
// defect os-time-cm3.o had, hidden the same way under emulation. So every call goes
// through a trampoline that points r11 at BREADTH_DATA, zero-initialised SRAM
// standing in for wasm 0x10_0000..0x10_0020. A per-call trampoline, not one r11
// load in main: r11 is an ordinary callee-saved register to Rust, which may use it
// between calls.
#[cfg(feature = "target-f100")]
#[no_mangle]
static mut BREADTH_DATA: [u8; 32] = [0; 32];

#[cfg(feature = "target-f100")]
core::arch::global_asm!(
    ".section .text.breadth_r11",
    "    .global gpio_configure_r11", ".thumb_func", "gpio_configure_r11:",
    "    push {{r11, lr}}", "    ldr r11, =(BREADTH_DATA - 0x100000)", "    bl gpio_configure", "    pop {{r11, pc}}", "    .ltorg",
    "    .global timer_init_r11", ".thumb_func", "timer_init_r11:",
    "    push {{r11, lr}}", "    ldr r11, =(BREADTH_DATA - 0x100000)", "    bl timer_init", "    pop {{r11, pc}}", "    .ltorg",
    "    .global timer_deadline_r11", ".thumb_func", "timer_deadline_r11:",
    "    push {{r11, lr}}", "    ldr r11, =(BREADTH_DATA - 0x100000)", "    bl timer_deadline", "    pop {{r11, pc}}", "    .ltorg",
    "    .global timer_elapsed_r11", ".thumb_func", "timer_elapsed_r11:",
    "    push {{r11, lr}}", "    ldr r11, =(BREADTH_DATA - 0x100000)", "    bl timer_elapsed", "    pop {{r11, pc}}", "    .ltorg",
    "    .global spi_configure_r11", ".thumb_func", "spi_configure_r11:",
    "    push {{r11, lr}}", "    ldr r11, =(BREADTH_DATA - 0x100000)", "    bl spi_configure", "    pop {{r11, pc}}", "    .ltorg",
    "    .global spi_begin_r11", ".thumb_func", "spi_begin_r11:",
    "    push {{r11, lr}}", "    ldr r11, =(BREADTH_DATA - 0x100000)", "    bl spi_begin", "    pop {{r11, pc}}", "    .ltorg",
    "    .global spi_step_r11", ".thumb_func", "spi_step_r11:",
    "    push {{r11, lr}}", "    ldr r11, =(BREADTH_DATA - 0x100000)", "    bl spi_step", "    pop {{r11, pc}}", "    .ltorg",
    "    .global spi_is_complete_r11", ".thumb_func", "spi_is_complete_r11:",
    "    push {{r11, lr}}", "    ldr r11, =(BREADTH_DATA - 0x100000)", "    bl spi_is_complete", "    pop {{r11, pc}}", "    .ltorg",
    "    .global uart_tx_byte_r11", ".thumb_func", "uart_tx_byte_r11:",
    "    push {{r11, lr}}", "    ldr r11, =(BREADTH_DATA - 0x100000)", "    bl uart_tx_byte", "    pop {{r11, pc}}", "    .ltorg",
);

#[cfg(feature = "target-f100")]
extern "C" {
    #[link_name = "gpio_configure_r11"]
    fn gpio_configure(base: u32, pin: u32, mode_idx: u32);
    #[link_name = "timer_init_r11"]
    fn timer_init(base: u32, psc: u32, arr: u32);
    #[link_name = "timer_deadline_r11"]
    fn timer_deadline(now: u32, ticks: u32) -> u32;
    #[link_name = "timer_elapsed_r11"]
    fn timer_elapsed(now: u32, deadline: u32) -> u32;
    #[link_name = "spi_configure_r11"]
    fn spi_configure(base: u32, mode: u32, br_idx: u32);
    #[link_name = "spi_begin_r11"]
    fn spi_begin(state: u32, count: u32) -> u32;
    #[link_name = "spi_step_r11"]
    fn spi_step(state: u32) -> u32;
    #[link_name = "spi_is_complete_r11"]
    fn spi_is_complete(state: u32) -> u32;
    #[link_name = "uart_tx_byte_r11"]
    fn uart_tx_byte(b: u32);
}

const GPIOC: u32 = 0x4001_1000; // RAM-mapped window in the gate .repl
const TIM2: u32 = 0x4000_0000; // RAM-mapped window
const SPI1: u32 = 0x4001_3000; // RAM-mapped window
const USART1: u32 = 0x4001_3800; // real STM32_UART model
const USART_SR: u32 = 0x00;
const USART_DR: u32 = 0x04;
const USART_BRR: u32 = 0x08;
const USART_CR1: u32 = 0x0C;
const TXE: u32 = 1 << 7;

fn tx(s: &[u8]) {
    for &b in s {
        unsafe {
            while read_volatile((USART1 + USART_SR) as *const u32) & TXE == 0 {}
            write_volatile((USART1 + USART_DR) as *mut u32, (b as u32) & 0xFF);
        }
    }
}

#[entry]
fn main() -> ! {
    unsafe {
        const RCC_APB2ENR: u32 = 0x4002_1018;
        let e = read_volatile(RCC_APB2ENR as *const u32);
        write_volatile(RCC_APB2ENR as *mut u32, e | (1 << 0) | (1 << 2) | (1 << 14));
        // ON SILICON the other three peripherals need their clocks. Renode maps GPIOC,
        // TIM2 and SPI1 as plain RAM windows, so readback holds there unclocked; a real
        // F100 reads an unclocked peripheral as zero and every check below would fail
        // for a reason that has nothing to do with the drivers.
        #[cfg(feature = "target-f100")]
        {
            write_volatile(RCC_APB2ENR as *mut u32, read_volatile(RCC_APB2ENR as *const u32) | (1 << 4) | (1 << 12)); // IOPCEN, SPI1EN
            const RCC_APB1ENR: u32 = 0x4002_101C;
            write_volatile(RCC_APB1ENR as *mut u32, read_volatile(RCC_APB1ENR as *const u32) | (1 << 0)); // TIM2EN
        }
        let mut fails = 0u32;
        const GPIOA_CRH: u32 = 0x4001_0804;
        let c = read_volatile(GPIOA_CRH as *const u32);
        write_volatile(GPIOA_CRH as *mut u32, (c & !(0xF << 4)) | (0xB << 4));
        write_volatile((USART1 + USART_BRR) as *mut u32, 0x45);
        write_volatile((USART1 + USART_CR1) as *mut u32, (1 << 13) | (1 << 3));

        tx(b"breadth-gate begin\n");

        // GPIO — configure PC8 as output (mode idx 4 → CRH nibble 0x3).
        gpio_configure(GPIOC, 8, 4);
        let crh = read_volatile((GPIOC + 0x04) as *const u32);
        let gpio_ok = crh & 0xF == 0x3;
        fails += !gpio_ok as u32;
        tx(if gpio_ok { b"breadth-gpio-ok\n" } else { b"breadth-gpio-bad\n" });

        // TIMER — init writes PSC/ARR/CR1(CEN); wrap-safe deadline math (Kani-proven).
        timer_init(TIM2, 0x1234, 0xABCD);
        let psc = read_volatile((TIM2 + 0x28) as *const u32);
        let arr = read_volatile((TIM2 + 0x2C) as *const u32);
        let cr1 = read_volatile((TIM2 + 0x00) as *const u32);
        let d = timer_deadline(100, 50);
        let timer_ok = psc == 0x1234 && arr == 0xABCD && cr1 & 1 != 0
            && d == 150 && timer_elapsed(149, d) == 0 && timer_elapsed(150, d) == 1;
        fails += !timer_ok as u32;
        tx(if timer_ok { b"breadth-timer-ok\n" } else { b"breadth-timer-bad\n" });

        // SPI — configure (mode3/br2 → CR1=0x357) + transfer FSM begin→step×3→complete.
        spi_configure(SPI1, 3, 2);
        let scr1 = read_volatile((SPI1 + 0x00) as *const u32);
        let s3 = spi_step(spi_step(spi_step(spi_begin(0, 3))));
        let spi_ok = scr1 == 0x357 && spi_is_complete(s3) == 1;
        fails += !spi_ok as u32;
        tx(if spi_ok { b"breadth-spi-ok\n" } else { b"breadth-spi-bad\n" });

        // UART — the dissolved uart driver TXes the proof line itself over the real
        // USART1 (same peripheral this gate reports on): all four drivers live from
        // the one fused object.
        // The first byte is sampled: a real DR write CLEARS TXE until the byte moves on,
        // so TXE reading 0 immediately after the driver returns is the evidence that the
        // DISSOLVED driver wrote the register (TXE merely returning later is not — an
        // idle UART shows that too).
        #[allow(unused_mut, unused_variables, unused_assignments)]
        let mut txe_cleared_by_driver = false;
        for (i, &b) in b"breadth-uart-ok\n".iter().enumerate() {
            uart_tx_byte((b as u32) & 0xFF);
            #[cfg(feature = "target-f100")]
            if i == 0 {
                txe_cleared_by_driver = read_volatile((USART1 + USART_SR) as *const u32) & TXE == 0;
            }
            let _ = i;
        }

        tx(b"breadth-gate done\n");

        // ON SILICON the verdict goes out over semihosting: the VLDISCOVERY's ST-LINK/V1
        // has no virtual COM port, so nothing reads USART1. This used to exit SUCCESS
        // unconditionally — correct under Renode, whose robot reads the UART lines, and
        // vacuous anywhere the exit status is the verdict. The uart driver's own TX is
        // checked by register effect: TXE must come back after its last byte.
        #[cfg(feature = "target-f100")]
        {
            // TXE clears on every DR write and returns only when the byte leaves the
            // holding register, so it is sampled with a BOUNDED wait — reading it
            // immediately after the last byte reported the drivers' TX as failed.
            let mut uart_ok = false;
            for _ in 0..2_000_000u32 {
                if read_volatile((USART1 + USART_SR) as *const u32) & TXE != 0 { uart_ok = true; break; }
            }
            let uart_ok = uart_ok && txe_cleared_by_driver;
            fails += !uart_ok as u32;
            // The data window must have been USED, or the trampolines prove nothing.
            let window = read_volatile(core::ptr::addr_of!(BREADTH_DATA));
            let window_used = window.iter().any(|b| *b != 0);
            fails += !window_used as u32;
            let (c, p, a) = (crh & 0xF, psc, arr);
            if fails == 0 {
                hprintln!("gust-breadth OK: gpio CRH nibble={:#x}, tim2 PSC={:#x} ARR={:#x} CEN=1, spi1 CR1={:#x} FSM complete, uart TXE cleared by the dissolved write then returned, data window {:02x?} — 4 dissolved drivers on real STM32F100 registers", c, p, a, scr1, &window[12..18]);
            } else {
                hprintln!("gust-breadth FAIL: {} check(s) failed (gpio CRH nibble={:#x}, tim2 PSC={:#x} ARR={:#x} CR1={:#x}, spi1 CR1={:#x}, uart TXE-cleared-by-driver={} returned={}, data window used={})", fails, c, p, a, cr1, scr1, txe_cleared_by_driver, uart_ok, window_used);
                debug::exit(debug::EXIT_FAILURE);
                loop {}
            }
        }
        let _ = fails;
    }
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

//! gust-dac-probe — the LOCAL qemu-semihosting probe of the DISSOLVED dac-thin .o,
//! run BEFORE the Renode content-gate to catch table/linmem no-op bugs fast.
//!
//! Points the dissolved software-triggered DAC driver at a plain `[u32; 16]` RAM
//! window (real mapped SRAM on lm3s6965evb) laid out as the STM32F1 DAC register
//! block (CR/SWTRIGR/DHR/DOR) and checks the EXACT register effects the driver
//! writes (CR enable word on `enable`, DHR on `load`, SWTRIGR on `trigger`) plus the
//! FSM via semihosting — so a dissolved primitive that silently no-ops (e.g. a
//! `.rodata` linmem table read as 0 under `--relocatable`, or a dropped export)
//! fails HERE, on `cargo run`, not three CI minutes later in Renode.
//!
//! It DEMONSTRATES the driver's distinctive safety property, **glitch-free,
//! trigger-gated output**: `load` writes DHR but does NOT move the pin (DOR is
//! unchanged); the output publishes to DOR only when a `trigger` fires. The strong
//! non-vacuous demonstration stages a SECOND code while the first is on the pin and
//! shows the pin holds the OLD published value until the next trigger — no
//! half-updated code is ever driven.
//!
//! A plain RAM window has no DAC peripheral to latch DHR→DOR on the software
//! trigger, so — exactly as `gust_spi_probe` pre-seeds SR/DR — the probe EMULATES
//! that one hardware step (DOR := DHR right after it observes the driver's SWTRIGR
//! write) so `dac_output` (which the driver reads from DOR) can close the loop and
//! confirm the pin ends up at the loaded code. Everything else is the driver.
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
    fn dac_enable(base: u32, state: u32, channel: u32) -> u32;
    fn dac_load(base: u32, state: u32, v: u32) -> u32;
    fn dac_trigger(base: u32, state: u32) -> u32;
    fn dac_output(base: u32, state: u32) -> u32;
    fn dac_value(state: u32) -> u32;
    fn dac_is_output(state: u32) -> u32;
    fn dac_disable(base: u32, state: u32) -> u32;
}

// STM32F1 DAC register window in RAM (byte offset == word index * 4).
static mut REG: [u32; 16] = [0; 16];
const CR: usize = 0x00 / 4;
const SWTRIGR: usize = 0x04 / 4;
const DHR12R1: usize = 0x08 / 4;
const DOR1: usize = 0x2C / 4;

const DAC_FAULT: u32 = 0xFFFF_FFFF;
// CR software-triggered enable word for channel 1: EN(1<<0)|TEN(1<<2)|TSEL_SW(0x7<<3).
const CR_EN_SW: u32 = 0x3D;
const SWTRIG1: u32 = 1; // SWTRIGR bit for channel 1
const CODE_A: u32 = 0xABC; // first staged 12-bit code
const CODE_B: u32 = 0x555; // second staged 12-bit code (the atomic-publish demo)
const OVERRANGE: u32 = 0x1_2FFF; // > 12 bits; must be clamped to 0x2FF... see below

#[inline]
fn rd(i: usize) -> u32 {
    unsafe { read_volatile(addr_of_mut!(REG[i])) }
}

#[entry]
fn main() -> ! {
    let base = addr_of_mut!(REG) as u32;
    let mut ok = true;

    // 0) phase gating / glitch-guard from Off (state=0 → phase Off): a `load` before
    //    `enable` MUST be rejected and write NO DHR — a control loop cannot stage a
    //    code onto a disabled channel. A dissolve that no-ops load's phase check
    //    would let this through and dirty DHR.
    let load_off = unsafe { dac_load(base, 0, CODE_A) };
    let dhr_off = rd(DHR12R1);
    // and `trigger` from Off is rejected with no SWTRIGR write.
    let trig_off = unsafe { dac_trigger(base, 0) };
    let swtrig_off = rd(SWTRIGR);
    if load_off != DAC_FAULT || dhr_off != 0 || trig_off != DAC_FAULT || swtrig_off != 0 {
        hprintln!(
            "dac-phase-gate FAIL: load_off={:#x} DHR={:#x} trig_off={:#x} SWTRIGR={:#x}",
            load_off, dhr_off, trig_off, swtrig_off
        );
        ok = false;
    } else {
        hprintln!("dac-phase-gate ok: load/trigger from Off faulted, no DHR/SWTRIGR write");
    }

    // 1) enable ch1: driver must WRITE CR = EN|TEN|TSEL(sw) = 0x3D and land Ready
    //    (not Output — enable never publishes).
    let s1 = unsafe { dac_enable(base, 0, 0) };
    let cr1 = rd(CR);
    if s1 == DAC_FAULT || cr1 != CR_EN_SW || unsafe { dac_is_output(s1) } != 0 {
        hprintln!("dac-enable FAIL: s1={:#x} CR={:#x} want {:#x}", s1, cr1, CR_EN_SW);
        ok = false;
    } else {
        hprintln!("dac-enable ok: CR={:#x}, phase Ready (is_output=0)", cr1);
    }

    // 2) load CODE_A: driver WRITES DHR12R1 = 0xABC and lands Loaded — but the pin
    //    (DOR) does NOT move: glitch-free staging. is_output stays 0; dac_value ==
    //    the staged code.
    let s2 = unsafe { dac_load(base, s1, CODE_A) };
    let dhr2 = rd(DHR12R1);
    let dor2 = rd(DOR1); // still 0 — nothing has published
    if s2 == DAC_FAULT
        || dhr2 != CODE_A
        || dor2 != 0
        || unsafe { dac_is_output(s2) } != 0
        || unsafe { dac_value(s2) } != CODE_A
    {
        hprintln!(
            "dac-load FAIL: s2={:#x} DHR={:#x} want {:#x} DOR={:#x} (want 0) is_out={} val={:#x}",
            s2, dhr2, CODE_A, dor2, unsafe { dac_is_output(s2) }, unsafe { dac_value(s2) }
        );
        ok = false;
    } else {
        hprintln!(
            "dac-load ok: DHR={:#x} staged, DOR still 0 (pin unmoved), is_output=0",
            dhr2
        );
    }

    // 3) trigger: driver WRITES SWTRIGR = bit0 and lands Output. Emulate the DAC's
    //    hardware DHR→DOR latch (a RAM window has no peripheral to do it), then check
    //    dac_output reads the pin back == the loaded code: output-reflects-loaded.
    let s3 = unsafe { dac_trigger(base, s2) };
    let swtrig3 = rd(SWTRIGR);
    // hardware-model: the software trigger latches DHR into DOR on real silicon.
    unsafe { write_volatile(addr_of_mut!(REG[DOR1]), rd(DHR12R1)) };
    let out3 = unsafe { dac_output(base, s3) };
    if s3 == DAC_FAULT
        || swtrig3 != SWTRIG1
        || unsafe { dac_is_output(s3) } != 1
        || out3 != CODE_A
        || unsafe { dac_value(s3) } != CODE_A
    {
        hprintln!(
            "dac-trigger FAIL: s3={:#x} SWTRIGR={:#x} want {:#x} is_out={} DOR-read={:#x} want {:#x}",
            s3, swtrig3, SWTRIG1, unsafe { dac_is_output(s3) }, out3, CODE_A
        );
        ok = false;
    } else {
        hprintln!("dac-trigger ok: SWTRIGR={:#x}, DOR reads back {:#x} == loaded", swtrig3, out3);
    }

    // 4) GLITCH-FREE atomic publish (the distinctive property): stage CODE_B while
    //    CODE_A is on the pin. The driver rewrites DHR to 0x555, but DOR must STILL
    //    read 0xABC — the pin holds the previous published code, un-glitched, until
    //    the next trigger. is_output flips back to 0 (Loaded); dac_value == CODE_B.
    let s4 = unsafe { dac_load(base, s3, CODE_B) };
    let dhr4 = rd(DHR12R1);
    let dor4 = unsafe { dac_output(base, s4) }; // DOR untouched by load
    if s4 == DAC_FAULT
        || dhr4 != CODE_B
        || dor4 != CODE_A // pin STILL the old value — no glitch
        || unsafe { dac_is_output(s4) } != 0
        || unsafe { dac_value(s4) } != CODE_B
    {
        hprintln!(
            "dac-glitch-free FAIL: s4={:#x} DHR={:#x} want {:#x} DOR={:#x} want {:#x} (old) is_out={} val={:#x}",
            s4, dhr4, CODE_B, dor4, CODE_A, unsafe { dac_is_output(s4) }, unsafe { dac_value(s4) }
        );
        ok = false;
    } else {
        hprintln!(
            "dac-glitch-free ok: DHR restaged to {:#x} but DOR held old {:#x} — pin un-glitched",
            dhr4, dor4
        );
    }

    // 5) second trigger publishes CODE_B atomically: SWTRIGR fires again, latch
    //    DHR→DOR, and NOW the pin moves to 0x555 in one step.
    let s5 = unsafe { dac_trigger(base, s4) };
    let swtrig5 = rd(SWTRIGR);
    unsafe { write_volatile(addr_of_mut!(REG[DOR1]), rd(DHR12R1)) };
    let out5 = unsafe { dac_output(base, s5) };
    if s5 == DAC_FAULT || swtrig5 != SWTRIG1 || out5 != CODE_B || unsafe { dac_is_output(s5) } != 1 {
        hprintln!(
            "dac-publish2 FAIL: s5={:#x} SWTRIGR={:#x} DOR-read={:#x} want {:#x} is_out={}",
            s5, swtrig5, out5, CODE_B, unsafe { dac_is_output(s5) }
        );
        ok = false;
    } else {
        hprintln!("dac-publish2 ok: second trigger moved pin atomically to {:#x}", out5);
    }

    // 6) range clamp (Kani p1): a commanded value wider than 12 bits is masked to
    //    0xFFF before it reaches DHR — never an overrange code on the actuator line.
    let s6 = unsafe { dac_load(base, s5, OVERRANGE) };
    let dhr6 = rd(DHR12R1);
    let want = OVERRANGE & 0x0FFF;
    if s6 == DAC_FAULT || dhr6 != want || unsafe { dac_value(s6) } != want {
        hprintln!(
            "dac-clamp FAIL: s6={:#x} DHR={:#x} want {:#x} (={:#x} & 0xFFF) val={:#x}",
            s6, dhr6, want, OVERRANGE, unsafe { dac_value(s6) }
        );
        ok = false;
    } else {
        hprintln!("dac-clamp ok: {:#x} masked to DHR={:#x}", OVERRANGE, dhr6);
    }

    // 7) disable is total: driver WRITES CR=0 and returns to Off from any state.
    let s7 = unsafe { dac_disable(base, s6) };
    let cr7 = rd(CR);
    if cr7 != 0 || unsafe { dac_is_output(s7) } != 0 || unsafe { dac_value(s7) } != 0 {
        hprintln!("dac-disable FAIL: CR={:#x} is_out={} val={:#x}", cr7, unsafe {
            dac_is_output(s7)
        }, unsafe { dac_value(s7) });
        ok = false;
    } else {
        hprintln!("dac-disable ok: CR=0, back to Off");
    }

    if ok {
        hprintln!("dac-probe ALL OK");
        debug::exit(debug::EXIT_SUCCESS);
    } else {
        debug::exit(debug::EXIT_FAILURE);
    }
    loop {}
}

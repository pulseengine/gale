//! gust_switch_renode — core-0 image of the v0.6.0 MULTI-CORE Renode
//! placement demo (renode-test/gust_switch_2core.repl + .robot): the VERIFIED
//! partition-switch FSM (gale src/partition_switch.rs, Verus + Kani) composed
//! with the VERIFIED I-ISO region programmer (gale src/mpu_switch.rs) driving
//! a 3-partition ARINC-653-style major frame — flight (P0), mission (P1),
//! payload (P2) sharing THIS core, an estimator partition pinned to core 1
//! (gust_estimator_part.rs) — with every window boundary crossing the proven
//! ctx_save -> region_swap -> ctx_resume order and the MPU programmed ONLY
//! via the verified `switch_to_partition`.
//!
//! HONEST SCOPE (decided by the committed spike, src/bin/mpu_spike_renode.rs,
//! run on Renode 1.16.1 cortex-m3): Renode holds the v7-M MPU registers as
//! readable/writable STATE (MPU_TYPE.DREGION == 8, RBAR/RASR read back
//! exactly) but does NOT enforce them — a store into an ungranted region
//! falls through with no MemManage (spike RESULT=0xBADF0011). So THIS image
//! demonstrates, and the robot gate asserts:
//!
//!   (a) the major frame ADVANCING window by window (P0 -> P1 -> P2 -> P0,
//!       wrapping to window 0) under the verified Switcher, off-boundary
//!       ticks never preempting, boundary ticks always preempting;
//!   (b) region-swap-before-resume OBSERVED at every switch: seam sequence
//!       numbers prove save < swap < resume, and ctx_resume reads the REAL
//!       MPU register state back (RNR := 3, RBAR) to confirm the incoming
//!       partition's scratch region is ALREADY programmed at resume time;
//!   (c) spatial-map correctness at the VERIFIED-QUERY level: each window's
//!       partition covers its own scratch word and does NOT cover its
//!       neighbour's (RegionTable::covers_addr, the Verus-proven exec mirror
//!       of the grant spec) — labelled "covers-denied" in the output because
//!       it is a table query, NOT a hardware fault.
//!
//! Map-ENFORCEMENT (real MemManage on cross-partition writes, CFSR=0x82,
//! MMFAR exact) is NOT claimable on this platform; that evidence lives in the
//! merged qemu demonstrator (src/bin/gust_switch_probe.rs, lm3s6965evb) and
//! mpu_spike.rs. This image adds the multi-core PLACEMENT + window-sequence +
//! register-level swap-observability dimension on the Renode 2-core model.
//!
//! Output: USART1-class STM32 UART at 0x4001_3800 (per-core registered in the
//! .repl — this core's own uart0), asserted line-by-line by the robot content
//! gate. No semihosting anywhere (not capturable headless on all Renode
//! portables). Ends in a quiet WFI loop after the final OK/FAIL line.
#![no_std]
#![no_main]

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use cortex_m_rt::{entry, exception};
use gale::mpu_switch::{RegionTable, MPU_CTRL_ENABLE, REQUIRED_DREGION};
use gale::partition_switch::{MajorFrame, Switcher, MAX_WINDOWS};
use panic_halt as _;

// ---------------------------------------------------------------------------
// ARMv7-M System Control Space (PPB).
// ---------------------------------------------------------------------------
const MPU_TYPE: *mut u32 = 0xE000_ED90 as *mut u32;
const MPU_CTRL: *mut u32 = 0xE000_ED94 as *mut u32;
const MPU_RNR: *mut u32 = 0xE000_ED98 as *mut u32;
const MPU_RBAR: *mut u32 = 0xE000_ED9C as *mut u32;
const MPU_RASR: *mut u32 = 0xE000_EDA0 as *mut u32;
const SHCSR: *mut u32 = 0xE000_ED24 as *mut u32;

/// Sentinel the verified core uses for MPU_CTRL writes (mpu_switch::MPU_CTRL_ID).
const MPU_CTRL_ID: u32 = 0xFFFF_FFFF;

/// Number of partitions this core schedules (flight, mission, payload).
const N_PARTS: usize = 3;

/// Per-partition 2 KiB scratch region bases (slot 3 of each partition's map)
/// — same mid-SRAM layout as the qemu demonstrator (gust_switch_probe.rs).
const SCRATCH_BASE: [u32; N_PARTS] = [0x2000_8000, 0x2000_8800, 0x2000_9000];
const SCRATCH_SIZE: u32 = 0x800;
const SCRATCH_ADDR: [u32; N_PARTS] = [0x2000_8010, 0x2000_8810, 0x2000_9010];

/// Common grants every partition needs to keep this image running.
const FLASH_BASE: u32 = 0x0000_0000;
const FLASH_SIZE: u32 = 0x0004_0000; // 256K RO: code + rodata + vectors
const DATA_BASE: u32 = 0x2000_0000;
const DATA_SIZE: u32 = 0x0000_8000; // 32K RW: .data/.bss + statics
const STACK_BASE: u32 = 0x2000_C000;
const STACK_SIZE: u32 = 0x0000_4000; // 16K RW: stack window (MSP 0x2001_0000)
/// The UART window (0x4001_3800, 256 B): partitions report over their own
/// UART, so the grant is part of every partition's map — on ENFORCING
/// hardware this image's prints would otherwise fault. Slot 4 of each map.
const UART_BASE: u32 = 0x4001_3800;
const UART_WIN: u32 = 0x100;

// ---------------------------------------------------------------------------
// UART (STM32 F1 USART register layout; Renode UART.STM32_UART).
// ---------------------------------------------------------------------------
const UART_SR: *mut u32 = 0x4001_3800 as *mut u32;
const UART_DR: *mut u32 = 0x4001_3804 as *mut u32;
const UART_BRR: *mut u32 = 0x4001_3808 as *mut u32;
const UART_CR1: *mut u32 = 0x4001_380C as *mut u32;

fn uart_init() {
    unsafe {
        write_volatile(UART_BRR, 0x45); // 8 MHz / 115200 (as drivers/uart-thin)
        write_volatile(UART_CR1, (1 << 13) | (1 << 3)); // UE | TE
    }
}

fn putb(b: u8) {
    unsafe {
        while read_volatile(UART_SR) & (1 << 7) == 0 {} // TXE
        write_volatile(UART_DR, b as u32);
    }
}

fn puts(s: &str) {
    for b in s.bytes() {
        putb(b);
    }
}

fn put_hex(v: u32) {
    puts("0x");
    let mut i = 0;
    while i < 8 {
        let nib = (v >> (28 - 4 * i)) & 0xF;
        putb(if nib < 10 { b'0' + nib as u8 } else { b'a' + (nib - 10) as u8 });
        i += 1;
    }
}

fn put_dec_small(v: u32) {
    // 0..=99 is all this demo prints in decimal.
    if v >= 10 {
        putb(b'0' + (v / 10) as u8);
    }
    putb(b'0' + (v % 10) as u8);
}

fn putln(s: &str) {
    puts(s);
    putb(b'\n');
}

// ---------------------------------------------------------------------------
// Demo state (all inside the common 32K data grant).
// ---------------------------------------------------------------------------
static mut THE_TABLE: Option<RegionTable> = None;
static mut CURRENT_PART: u32 = 0xFFFF_FFFF;
static mut SEAM_SEQ: u32 = 0;
static mut LAST_SAVE_SEQ: u32 = 0;
static mut LAST_SWAP_SEQ: u32 = 0;
static mut LAST_RESUME_SEQ: u32 = 0;
static mut LAST_SAVED_PART: u32 = 0xFFFF_FFFF;
/// RBAR readback (slot 3) captured by ctx_resume at the LAST resume.
static mut LAST_RESUME_RBAR: u32 = 0;
/// Sticky: every ctx_resume so far found the incoming map already live.
static mut RESUME_MAP_OK: u32 = 1;

macro_rules! fail {
    ($msg:expr) => {{
        puts("gust-switch-2core core0 FAIL: ");
        putln($msg);
        loop {
            cortex_m::asm::wfi();
        }
    }};
}

// ---------------------------------------------------------------------------
// The `mpu_write` trusted-seam bridge (same contract as gust_switch_probe).
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn mpu_write(rnr: u32, rbar: u32, rasr: u32) {
    unsafe {
        if rnr == MPU_CTRL_ID {
            write_volatile(MPU_CTRL, rasr);
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
        } else {
            write_volatile(MPU_RNR, rnr);
            write_volatile(MPU_RBAR, rbar);
            write_volatile(MPU_RASR, rasr);
        }
    }
}

// ---------------------------------------------------------------------------
// The partition_switch trusted seams, stamped for observable ordering.
// ---------------------------------------------------------------------------
unsafe fn bump_seq() -> u32 {
    let s = read_volatile(addr_of!(SEAM_SEQ)) + 1;
    write_volatile(addr_of_mut!(SEAM_SEQ), s);
    s
}

#[no_mangle]
pub extern "C" fn ctx_save(part: u32) -> u32 {
    unsafe {
        let s = bump_seq();
        write_volatile(addr_of_mut!(LAST_SAVE_SEQ), s);
        write_volatile(addr_of_mut!(LAST_SAVED_PART), part);
    }
    0
}

#[no_mangle]
pub extern "C" fn region_swap(part: u32) -> u32 {
    unsafe {
        let s = bump_seq();
        write_volatile(addr_of_mut!(LAST_SWAP_SEQ), s);
        match (*addr_of!(THE_TABLE)).as_ref() {
            Some(t) => {
                t.switch_to_partition(part);
                0
            }
            None => {
                fail!("region_swap before table built");
            }
        }
    }
}

/// ctx_resume: the observable half of region-swap-before-resume — read the
/// REAL MPU register state back (RNR := 3, the scratch slot; RBAR) and
/// require the INCOMING partition's scratch region already programmed and
/// the MPU-enable register state set. Register STATE is spike-proven to hold
/// written values on this platform (mpu_spike_renode REG_RBAR/REG_RASR).
#[no_mangle]
pub extern "C" fn ctx_resume(part: u32) -> u32 {
    unsafe {
        let s = bump_seq();
        write_volatile(addr_of_mut!(LAST_RESUME_SEQ), s);
        let mut ok = read_volatile(MPU_CTRL) == MPU_CTRL_ENABLE;
        if (part as usize) < N_PARTS {
            write_volatile(MPU_RNR, 3);
            let rbar = read_volatile(MPU_RBAR);
            write_volatile(addr_of_mut!(LAST_RESUME_RBAR), rbar & !0x1F);
            ok = ok && (rbar & !0x1F) == SCRATCH_BASE[part as usize];
        } else {
            ok = false;
        }
        if !ok {
            write_volatile(addr_of_mut!(RESUME_MAP_OK), 0);
        }
        write_volatile(addr_of_mut!(CURRENT_PART), part);
    }
    0
}

// ---------------------------------------------------------------------------
// Faults: NOTHING may fault on this platform (no MPU enforcement) — any
// exception is a demo failure, reported over the UART.
// ---------------------------------------------------------------------------
#[exception]
fn MemoryManagement() {
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    fail!("unexpected MemManage (platform was spike-proven non-enforcing)");
}

#[exception]
unsafe fn HardFault(_ef: &cortex_m_rt::ExceptionFrame) -> ! {
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    fail!("HardFault");
}

// ---------------------------------------------------------------------------
// Setup helpers.
// ---------------------------------------------------------------------------
fn grant(t: &mut RegionTable, part: u32, base: u32, size: u32, writable: bool) {
    if !t.try_add_region(part, base, size, writable) {
        fail!("try_add_region rejected a grant");
    }
}

#[entry]
fn main() -> ! {
    uart_init();
    // Platform contract: exactly 8 MPU region slots (verified core's DREGION
    // requirement) — Renode's model reads back 8 (spike-confirmed).
    let dregion = (unsafe { read_volatile(MPU_TYPE) } >> 8) & 0xFF;
    if dregion != REQUIRED_DREGION {
        fail!("MPU_TYPE.DREGION != 8");
    }
    unsafe {
        // Enable the MemManage fault line (inert here, but keep the shape).
        write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16));
    }
    putln("gust-switch-2core core0 begin dregion 8");

    // ---- The major frame: 4 windows over partitions [0, 1, 2, 0] ----------
    let frame = MajorFrame {
        partition_id: [0, 1, 2, 0],
        offset: [0, 10, 20, 30],
        budget: [10, 10, 10, 10],
        frame_len: 40,
    };
    if !frame.check() {
        fail!("frame.check() rejected the major frame");
    }

    // ---- The region table: built ONLY via the verified builder ------------
    let mut t = RegionTable::new();
    for p in 0..N_PARTS as u32 {
        grant(&mut t, p, FLASH_BASE, FLASH_SIZE, false); // slot 0
        grant(&mut t, p, DATA_BASE, DATA_SIZE, true); // slot 1
        grant(&mut t, p, STACK_BASE, STACK_SIZE, true); // slot 2
        grant(&mut t, p, SCRATCH_BASE[p as usize], SCRATCH_SIZE, true); // slot 3
        grant(&mut t, p, UART_BASE, UART_WIN, true); // slot 4 (reporting)
    }
    unsafe {
        *addr_of_mut!(THE_TABLE) = Some(t);
    }

    // ---- Bring up window 0 through the same seams --------------------------
    let _ = region_swap(0);
    let _ = ctx_resume(0);
    if unsafe { read_volatile(MPU_CTRL) } != MPU_CTRL_ENABLE {
        fail!("MPU_CTRL not enabled after verified switch");
    }
    if unsafe { read_volatile(addr_of!(RESUME_MAP_OK)) } != 1 {
        fail!("P0 map not live at initial resume (RBAR readback)");
    }

    // ---- Drive the VERIFIED Switcher across the major frame ---------------
    let mut sw = Switcher::new(frame);
    for w in 0..MAX_WINDOWS {
        let p = frame.partition_id[w];
        if unsafe { read_volatile(addr_of!(CURRENT_PART)) } != p {
            fail!("window owner mismatch (seam ledger)");
        }
        let mid = frame.offset[w] + 1;
        if frame.current_window(mid) != w as u32 {
            fail!("current_window(mid) mismatch");
        }

        // Own-scratch write lands and reads back (register-level; on this
        // platform a cross write would also land — enforcement is qemu's).
        let own = SCRATCH_ADDR[p as usize];
        unsafe {
            write_volatile(own as *mut u32, 0xA5A5_0000 | w as u32);
            if read_volatile(own as *const u32) != 0xA5A5_0000 | w as u32 {
                fail!("own-scratch write did not land");
            }
        }
        // Spatial-map correctness at the VERIFIED-QUERY level: this window's
        // partition covers its own scratch word, NOT the neighbour's (the
        // Verus-proven covers_addr — a table query, not a hardware fault).
        let neighbor = ((p + 1) % N_PARTS as u32) as usize;
        let covers_ok = unsafe {
            match (*addr_of!(THE_TABLE)).as_ref() {
                Some(t) => {
                    t.covers_addr(p, own) && !t.covers_addr(p, SCRATCH_ADDR[neighbor])
                }
                None => false,
            }
        };
        if !covers_ok {
            fail!("covers_addr query wrong (own not covered or neighbour covered)");
        }
        puts("core0 win");
        put_dec_small(w as u32);
        puts(" P");
        put_dec_small(p);
        putln(" own-scratch-ok covers-denied-cross");

        // The boundary: off-boundary ticks never preempt; the window-end tick
        // ALWAYS does, then the verified switch crosses save->swap->resume.
        if sw.tick(mid) {
            fail!("off-boundary tick preempted");
        }
        let end_t = frame.offset[w] + frame.budget[w] - 1;
        if !sw.tick(end_t) {
            fail!("boundary tick did not preempt");
        }
        let seq_before = unsafe { read_volatile(addr_of!(SEAM_SEQ)) };
        sw.run_switch();
        let (s_save, s_swap, s_resume, saved_part, rbar) = unsafe {
            (
                read_volatile(addr_of!(LAST_SAVE_SEQ)),
                read_volatile(addr_of!(LAST_SWAP_SEQ)),
                read_volatile(addr_of!(LAST_RESUME_SEQ)),
                read_volatile(addr_of!(LAST_SAVED_PART)),
                read_volatile(addr_of!(LAST_RESUME_RBAR)),
            )
        };
        if !(s_save > seq_before && s_swap > s_save && s_resume > s_swap) {
            fail!("seam order broken (save->swap->resume)");
        }
        if saved_part != p {
            fail!("ctx_save saved the wrong outgoing partition");
        }
        if unsafe { read_volatile(addr_of!(RESUME_MAP_OK)) } != 1 {
            fail!("incoming map NOT live at resume (RBAR readback)");
        }
        let expect_cur = ((w + 1) % MAX_WINDOWS) as u32;
        if sw.cur != expect_cur {
            fail!("switch did not advance the window by exactly one");
        }
        let incoming = frame.partition_id[expect_cur as usize];
        puts("core0 switch");
        put_dec_small(w as u32);
        puts(" -> P");
        put_dec_small(incoming);
        puts(" seam-order-ok map-live rbar ");
        put_hex(rbar);
        putb(b'\n');
    }

    // ---- Final oracle ------------------------------------------------------
    let cur_part = unsafe { read_volatile(addr_of!(CURRENT_PART)) };
    if sw.cur != 0 || cur_part != frame.partition_id[0] {
        fail!("frame did not wrap to window 0 / P0");
    }
    putln(
        "gust-switch-2core core0 OK: frame wrapped P0->P1->P2->P0, 4 verified switches save->swap->resume, map-live-at-resume via RBAR readback 4/4, covers-query denies cross-partition (enforcement evidence: qemu gust_switch_probe)",
    );
    loop {
        cortex_m::asm::wfi();
    }
}

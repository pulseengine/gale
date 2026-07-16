//! gust-switch-probe — the v0.6.0 outer-partition-switch demonstrator: the
//! VERIFIED partition-switch FSM (gale src/partition_switch.rs, Verus 1095/0 +
//! Kani 4/4) composed with the VERIFIED I-ISO region programmer (gale
//! src/mpu_switch.rs, Verus 1159/0 + Kani 7/7) and driven end-to-end on qemu's
//! real v7-M PMSA MPU (lm3s6965evb, cortex-m3 — enforcement pre-verified by
//! src/bin/mpu_spike.rs and gust_iso_fault_probe.rs). Part (2) of
//! VER-OS-SWITCH-001: the end-to-end evidence for the merged FSM proofs.
//!
//! WHAT IS DEMONSTRATED: a 3-partition ARINC-653-style major frame
//! (P0 -> P1 -> P2 -> P0 across the 4 windows of MAX_WINDOWS, wrapping back to
//! window 0) where at every window boundary the verified `Switcher` preempts
//! (`tick` at the boundary tick) and `run_switch` crosses the three trusted
//! seams in the proven order — ctx_save -> region_swap -> ctx_resume — with
//! `region_swap` wired to `RegionTable::switch_to_partition` (the verified MPU
//! programmer, P1–P4 proven). Inside each window the probe then proves, with
//! REAL MemManage hardware exceptions (handler-flag-gated, not Rust bounds
//! checks), that the running partition is confined to its OWN memory map:
//!
//!   1. own-access OK      — a write to the partition's own scratch word lands
//!                           (0 faults, read-back verified);
//!   2. cross-access DENIED — a write to a NEIGHBOR partition's scratch word
//!                           MemManage-faults with MMFAR == the exact denied
//!                           address and CFSR DACCVIOL+MMARVALID (0x82);
//!   3. non-vacuity control — in P1's window, P0's scratch word (writable one
//!                           window earlier) is now DENIED: the map genuinely
//!                           changed at the switch. This is the IN-PROBE
//!                           control ("don't switch and the old grant would
//!                           still be live") — a static/un-swapped map would
//!                           let this write through and FAIL the probe, so the
//!                           confinement is attributable to the switch, not
//!                           the layout. No separate control bin is needed.
//!
//! Region-swap-before-resume is made OBSERVABLE (not just proven): each seam
//! records a monotonic sequence number, and `ctx_resume` reads the hardware
//! MPU back (RNR := scratch slot, RBAR) to confirm the incoming partition's
//! map is ALREADY live at resume time. The oracle requires
//! save-seq < swap-seq < resume-seq and the readback match at every switch.
//!
//! MEMORY MAP (lm3s6965evb: 256K flash @0x0, 64K SRAM @0x2000_0000), built
//! exclusively through the verified builder `RegionTable::new()` +
//! `try_add_region` (B1–B4 proven), programmed exclusively through the
//! verified `switch_to_partition` path. Every partition gets three COMMON
//! regions (the probe itself must run in every window: code, statics, stack)
//! plus ONE per-partition 2 KiB scratch region:
//!
//!   slot 0: flash 0x0000_0000 256K RO   (code + rodata + vectors)  common
//!   slot 1: SRAM  0x2000_0000 32K  RW   (.data/.bss + probe statics) common
//!   slot 2: SRAM  0x2000_C000 16K  RW   (stack window; MSP 0x2001_0000) common
//!   slot 3: scratch, per partition:
//!             P0 [0x2000_8000, 0x2000_8800)  scratch word @ 0x2000_8010
//!             P1 [0x2000_8800, 0x2000_9000)  scratch word @ 0x2000_8810
//!             P2 [0x2000_9000, 0x2000_9800)  scratch word @ 0x2000_9010
//!   slots 4–7: emitted DISABLED by the verified core (P2 deny-by-default)
//!
//! The scratch regions live in the mid-SRAM hole ABOVE the common data region
//! (not at 0x2000_0000, where cortex-m-rt places .data/.bss — the probe's own
//! statics must be writable in EVERY window). [0x2000_9800, 0x2000_C000)
//! stays granted to nobody, preserving the v0.5.0 deny-by-default posture
//! (MPU_CTRL_ENABLE has PRIVDEFENA clear: even privileged code gets nothing
//! the table does not grant).
//!
//! SCOPE (honest): single-core qemu, fault-containment. The probe plays all
//! three partitions itself (ctx_save/ctx_resume are recording stubs — there
//! is no real register-file save/restore); the timer tick is driven in
//! software (the tick source is a trusted seam in the verified model too).
//! Multi-core placement on Renode is a separate follow-on.
//!
//! Fault recovery: a naked MemoryManagement handler verifies the fault matches
//! the armed expectation (EXPECT_MMFAR — any other MemManage FAILs the probe
//! from the handler), records MMFAR/CFSR, and advances the stacked PC past the
//! faulting store (16/32-bit Thumb width decoded from the instruction), so the
//! probe keeps running through all windows and all expected faults. Exit
//! codes: semihosting EXIT_SUCCESS / EXIT_FAILURE — no fall-through OK path.
#![no_std]
#![no_main]

use core::ptr::{addr_of, addr_of_mut, read_volatile, write_volatile};
use core::sync::atomic::{compiler_fence, Ordering};
use cortex_m_rt::{entry, exception};
use cortex_m_semihosting::{debug, hprintln};
use gale::mpu_switch::{RegionTable, MPU_CTRL_ENABLE, REQUIRED_DREGION};
use gale::partition_switch::{MajorFrame, Switcher, MAX_WINDOWS};
use panic_halt as _;

// ---------------------------------------------------------------------------
// ARMv7-M System Control Space (PPB — always accessible, never MPU-checked).
// ---------------------------------------------------------------------------
const MPU_TYPE: *mut u32 = 0xE000_ED90 as *mut u32;
const MPU_CTRL: *mut u32 = 0xE000_ED94 as *mut u32;
const MPU_RNR: *mut u32 = 0xE000_ED98 as *mut u32;
const MPU_RBAR: *mut u32 = 0xE000_ED9C as *mut u32;
const MPU_RASR: *mut u32 = 0xE000_EDA0 as *mut u32;
const SHCSR: *mut u32 = 0xE000_ED24 as *mut u32;
const CFSR: *mut u32 = 0xE000_ED28 as *mut u32;
const MMFAR: *mut u32 = 0xE000_ED34 as *mut u32;

/// Sentinel the verified core uses for MPU_CTRL writes (mpu_switch::MPU_CTRL_ID).
const MPU_CTRL_ID: u32 = 0xFFFF_FFFF;

/// Number of partitions the demo schedules (windows map P0->P1->P2->P0).
const N_PARTS: usize = 3;

/// Per-partition 2 KiB scratch region bases (slot 3 of each partition's map).
const SCRATCH_BASE: [u32; N_PARTS] = [0x2000_8000, 0x2000_8800, 0x2000_9000];
/// Scratch region size: 2 KiB (power of 2, bases size-aligned).
const SCRATCH_SIZE: u32 = 0x800;
/// The scratch WORD inside each partition's scratch region (base + 0x10).
const SCRATCH_ADDR: [u32; N_PARTS] = [0x2000_8010, 0x2000_8810, 0x2000_9010];

/// Common grants every partition needs to keep the probe itself running.
const FLASH_BASE: u32 = 0x0000_0000;
const FLASH_SIZE: u32 = 0x0004_0000; // 256K RO: code + rodata + vectors
const DATA_BASE: u32 = 0x2000_0000;
const DATA_SIZE: u32 = 0x0000_8000; // 32K RW: .data/.bss + probe statics
const STACK_BASE: u32 = 0x2000_C000;
const STACK_SIZE: u32 = 0x0000_4000; // 16K RW: stack window (MSP 0x2001_0000)

/// Mid-SRAM word granted to NOBODY (above P2's scratch): deny-by-default probe.
const NOBODY_ADDR: u32 = 0x2000_9800;

// ---------------------------------------------------------------------------
// Probe state (all in the common 32K data grant — writable in every window).
// ---------------------------------------------------------------------------

/// The verified region table, built in main() via new()+try_add_region and
/// then programmed ONLY through switch_to_partition (the region_swap seam).
static mut THE_TABLE: Option<RegionTable> = None;

/// Total MemManage faults recorded (each one handler-verified against the
/// armed expectation — an unexpected fault FAILs from the handler).
static mut FAULT_COUNT: u32 = 0;
/// MMFAR / CFSR of the last (expected) fault.
static mut LAST_MMFAR: u32 = 0;
static mut LAST_CFSR: u32 = 0;
/// The ONE address the next MemManage is allowed to report (0 = none armed:
/// any fault is unexpected and FAILs). One-shot: consumed by the handler.
static mut EXPECT_MMFAR: u32 = 0;

/// Which partition the seams last resumed (the "who is running" ledger).
static mut CURRENT_PART: u32 = 0xFFFF_FFFF;
/// Monotonic seam sequence counter + per-seam last-call stamps: the
/// observable form of save -> swap -> resume ordering.
static mut SEAM_SEQ: u32 = 0;
static mut LAST_SAVE_SEQ: u32 = 0;
static mut LAST_SWAP_SEQ: u32 = 0;
static mut LAST_RESUME_SEQ: u32 = 0;
/// Which partition ctx_save last saved (must equal the outgoing window owner).
static mut LAST_SAVED_PART: u32 = 0xFFFF_FFFF;
/// Sticky flag: 1 while EVERY ctx_resume so far found the incoming
/// partition's scratch region ALREADY programmed in the hardware (RNR/RBAR
/// readback) with the MPU enabled — region-swap-before-resume, observed.
static mut RESUME_MAP_OK: u32 = 1;

macro_rules! fail {
    ($($t:tt)*) => {{
        hprintln!($($t)*);
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }};
}

// ---------------------------------------------------------------------------
// The `mpu_write` trusted-seam bridge — the ONLY register-store path, exactly
// per the seam's platform contract in src/mpu_switch.rs.
// ---------------------------------------------------------------------------

/// Platform implementation of the verified core's trusted extern seam.
/// `rnr == MPU_CTRL_ID` → MPU_CTRL := rasr, then DSB+ISB (contract item 1);
/// otherwise RNR := rnr, RBAR := rbar, RASR := rasr.
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
// The partition_switch trusted seams (ctx_save / region_swap / ctx_resume) —
// resolved here for the verified Switcher::run_switch. Each stamps a seam
// sequence number so the proven ordering is also OBSERVED at runtime.
// ---------------------------------------------------------------------------

/// Bump and return the monotonic seam sequence counter.
unsafe fn bump_seq() -> u32 {
    let s = read_volatile(addr_of!(SEAM_SEQ)) + 1;
    write_volatile(addr_of_mut!(SEAM_SEQ), s);
    s
}

/// ctx_save seam: minimal recording stub (no real register-file save on this
/// single-core demonstrator — the FSM ordering is what is under test).
#[no_mangle]
pub extern "C" fn ctx_save(part: u32) -> u32 {
    unsafe {
        let s = bump_seq();
        write_volatile(addr_of_mut!(LAST_SAVE_SEQ), s);
        write_volatile(addr_of_mut!(LAST_SAVED_PART), part);
    }
    0
}

/// region_swap seam: THE verified MPU program for the incoming partition —
/// program_partition (P1–P4 proven) → apply_program → mpu_write. No
/// hand-programming anywhere in this probe.
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
                fail!("gust-switch-probe FAIL: region_swap({}) before table built", part);
            }
        }
    }
}

/// ctx_resume seam: record CURRENT_PART = part, and — the observable half of
/// region-swap-before-resume — read the hardware MPU back (RNR := 3, the
/// scratch slot; RBAR) to confirm the INCOMING partition's scratch region is
/// ALREADY programmed and the MPU is enabled at the moment of resume. RBAR
/// reads back as base | current-RNR in its low bits, hence the !0x1F mask.
#[no_mangle]
pub extern "C" fn ctx_resume(part: u32) -> u32 {
    unsafe {
        let s = bump_seq();
        write_volatile(addr_of_mut!(LAST_RESUME_SEQ), s);
        let mut ok = read_volatile(MPU_CTRL) == MPU_CTRL_ENABLE;
        if (part as usize) < N_PARTS {
            write_volatile(MPU_RNR, 3);
            let rbar = read_volatile(MPU_RBAR);
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
// MemManage: expectation-gated recording + skip-past-the-store recovery.
// ---------------------------------------------------------------------------

// Naked MemoryManagement handler: on exception entry MSP points at the
// hardware-stacked frame {r0,r1,r2,r3,r12,lr,pc,xpsr}; hand that pointer to
// the Rust recorder, which returns through the untouched EXC_RETURN in lr.
core::arch::global_asm!(
    ".section .text.MemoryManagement",
    ".global MemoryManagement",
    ".thumb_func",
    "MemoryManagement:",
    "    mrs r0, msp",
    "    b   switch_memmanage",
);

/// The re-armable expected-fault recorder: a MemManage is legitimate ONLY if
/// an expectation is armed (EXPECT_MMFAR != 0) and MMFAR matches it exactly —
/// anything else (own-window writes, stray accesses) FAILs the probe from the
/// handler. On a match: record CFSR/MMFAR, count, consume the expectation,
/// clear the sticky status bits, and advance the stacked PC PAST the faulting
/// store (Thumb 16/32-bit width decoded from the instruction's first
/// halfword), clearing the stacked ICI/IT bits and forcing Thumb state so the
/// exception return is architecturally clean. The faulting store itself is a
/// dedicated #[inline(never)] plain-str helper (probe_store), so the skip is
/// well-defined (never inside an IT block, no write-back side effects).
#[no_mangle]
extern "C" fn switch_memmanage(frame: *mut u32) {
    unsafe {
        let cfsr = read_volatile(CFSR);
        let mmfar = read_volatile(MMFAR);
        // Clear the sticky MemManage status byte (write-1-to-clear).
        write_volatile(CFSR, 0xFF);
        let expect = read_volatile(addr_of!(EXPECT_MMFAR));
        if expect == 0 || mmfar != expect {
            // Unexpected fault: not part of the oracle. MPU off (direct
            // write — the oracle already failed), report, exit.
            write_volatile(MPU_CTRL, 0);
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
            fail!(
                "gust-switch-probe FAIL: unexpected MemManage pc={:#010x} CFSR={:#010x} MMFAR={:#010x} (armed expectation {:#010x})",
                read_volatile(frame.add(6)),
                cfsr,
                mmfar,
                expect
            );
        }
        write_volatile(addr_of_mut!(LAST_CFSR), cfsr);
        write_volatile(addr_of_mut!(LAST_MMFAR), mmfar);
        let n = read_volatile(addr_of!(FAULT_COUNT));
        write_volatile(addr_of_mut!(FAULT_COUNT), n + 1);
        // One-shot: the expectation is consumed by exactly one fault.
        write_volatile(addr_of_mut!(EXPECT_MMFAR), 0);
        // Skip the faulting store: stacked PC += Thumb instruction width
        // (32-bit iff bits[15:11] of the first halfword are 0b11101/0b11110/
        // 0b11111). Stacked xPSR: clear ICI/IT (bits 26:25, 15:10), force T.
        let pc = read_volatile(frame.add(6));
        let first_hw = read_volatile(pc as *const u16);
        let width: u32 = if (first_hw & 0xE000) == 0xE000 && (first_hw & 0x1800) != 0 {
            4
        } else {
            2
        };
        write_volatile(frame.add(6), pc + width);
        let xpsr = read_volatile(frame.add(7));
        write_volatile(frame.add(7), (xpsr & !0x0600_FC00) | (1 << 24));
    }
}

#[exception]
unsafe fn HardFault(ef: &cortex_m_rt::ExceptionFrame) -> ! {
    unsafe {
        write_volatile(MPU_CTRL, 0);
    }
    cortex_m::asm::dsb();
    cortex_m::asm::isb();
    hprintln!(
        "gust-switch-probe FAIL: HardFault (MemManage escalated?) pc={:#010x} CFSR={:#010x} MMFAR={:#010x}",
        ef.pc(),
        unsafe { read_volatile(CFSR) },
        unsafe { read_volatile(MMFAR) }
    );
    debug::exit(debug::EXIT_FAILURE);
    loop {}
}

// ---------------------------------------------------------------------------
// Guarded access helpers.
// ---------------------------------------------------------------------------

/// The ONE store the handler may skip: a plain volatile str in a dedicated
/// non-inlined frame (never in an IT block, no write-back addressing), so
/// advancing the stacked PC past it resumes cleanly at this function's return.
#[inline(never)]
fn probe_store(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) };
}

/// Volatile read-back (a fault here is unexpected → handler FAILs the probe).
#[inline(never)]
fn probe_load(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}

/// Oracle step: `addr` must be DENIED to the currently-running partition —
/// arm the one-shot expectation, issue the store, then require EXACTLY one
/// new MemManage with MMFAR == addr and CFSR DACCVIOL+MMARVALID. A store
/// that falls through (no fault) or a wrong-shaped fault FAILs.
fn expect_denied(addr: u32, ctx: &str) {
    unsafe {
        let before = read_volatile(addr_of!(FAULT_COUNT));
        write_volatile(addr_of_mut!(EXPECT_MMFAR), addr);
        compiler_fence(Ordering::SeqCst);
        probe_store(addr, 0xDEAD_BEEF);
        compiler_fence(Ordering::SeqCst);
        let after = read_volatile(addr_of!(FAULT_COUNT));
        if after != before + 1 {
            write_volatile(MPU_CTRL, 0);
            cortex_m::asm::dsb();
            cortex_m::asm::isb();
            fail!(
                "gust-switch-probe FAIL: {} — write to denied {:#010x} fell through (faults {} -> {}) — MPU not enforcing",
                ctx,
                addr,
                before,
                after
            );
        }
        let mmfar = read_volatile(addr_of!(LAST_MMFAR));
        let cfsr = read_volatile(addr_of!(LAST_CFSR));
        // DACCVIOL (bit 1) + MMARVALID (bit 7): a data-access violation with
        // a valid faulting-address register.
        if mmfar != addr || cfsr & 0x82 != 0x82 {
            fail!(
                "gust-switch-probe FAIL: {} — fault recorded but wrong shape (MMFAR={:#010x} want {:#010x} CFSR={:#010x})",
                ctx,
                mmfar,
                addr,
                cfsr
            );
        }
        if read_volatile(addr_of!(EXPECT_MMFAR)) != 0 {
            fail!("gust-switch-probe FAIL: {} — expectation not consumed", ctx);
        }
    }
}

/// Oracle step: `addr` must be GRANTED to the currently-running partition —
/// the write lands with ZERO new faults and reads back exactly.
fn expect_granted(addr: u32, val: u32, ctx: &str) {
    unsafe {
        let before = read_volatile(addr_of!(FAULT_COUNT));
        compiler_fence(Ordering::SeqCst);
        probe_store(addr, val);
        compiler_fence(Ordering::SeqCst);
        let after = read_volatile(addr_of!(FAULT_COUNT));
        if after != before {
            fail!(
                "gust-switch-probe FAIL: {} — own-window write to {:#010x} faulted (faults {} -> {})",
                ctx,
                addr,
                before,
                after
            );
        }
        let got = probe_load(addr);
        if got != val {
            fail!(
                "gust-switch-probe FAIL: {} — own-window write to {:#010x} did not land (read {:#010x} want {:#010x})",
                ctx,
                addr,
                got,
                val
            );
        }
    }
}

// ---------------------------------------------------------------------------
// Setup helpers.
// ---------------------------------------------------------------------------

/// Init-time platform check (mpu_write contract item 2): refuse to start
/// unless `MPU_TYPE.DREGION == REQUIRED_DREGION` — on a 16-region part the
/// verified sequence would leave slots 8..=15 STALE, defeating P2.
fn mpu_bridge_init_check() {
    let dregion = (unsafe { read_volatile(MPU_TYPE) } >> 8) & 0xFF;
    if dregion != REQUIRED_DREGION {
        fail!(
            "gust-switch-probe FAIL: platform contract violated — MPU_TYPE.DREGION={} (require {}); refusing to start",
            dregion,
            REQUIRED_DREGION
        );
    }
}

/// One grant through the VERIFIED builder; a rejection is a probe failure.
fn grant(t: &mut RegionTable, part: u32, base: u32, size: u32, writable: bool, what: &str) {
    if !t.try_add_region(part, base, size, writable) {
        fail!(
            "gust-switch-probe FAIL: try_add_region rejected {} for P{} (base={:#010x} size={:#x})",
            what,
            part,
            base,
            size
        );
    }
}

#[entry]
fn main() -> ! {
    // Platform contract, item 2 — refuse to start on a non-8-region part.
    mpu_bridge_init_check();
    unsafe {
        // Enable the MemManage fault so a violation does not escalate.
        write_volatile(SHCSR, read_volatile(SHCSR) | (1 << 16));
    }

    // ---- The major frame: 4 windows over partitions [0, 1, 2, 0] ----------
    // (MAX_WINDOWS == 4 > 3 partitions: window 3 revisits P0, per frame_inv's
    // all-budgets-positive contiguity — the wrap itself becomes evidence.)
    let frame = MajorFrame {
        partition_id: [0, 1, 2, 0],
        offset: [0, 10, 20, 30],
        budget: [10, 10, 10, 10],
        frame_len: 40,
    };
    // The verified validator: check() == frame_inv (Verus-ensured).
    if !frame.check() {
        fail!("gust-switch-probe FAIL: frame.check() rejected the major frame");
    }

    // ---- The region table: built ONLY via the verified builder ------------
    let mut t = RegionTable::new();
    for p in 0..N_PARTS as u32 {
        grant(&mut t, p, FLASH_BASE, FLASH_SIZE, false, "flash RO");
        grant(&mut t, p, DATA_BASE, DATA_SIZE, true, "data RW");
        grant(&mut t, p, STACK_BASE, STACK_SIZE, true, "stack RW");
        grant(&mut t, p, SCRATCH_BASE[p as usize], SCRATCH_SIZE, true, "scratch RW");
    }
    // Grant-shape sanity through the VERIFIED covers query: each partition
    // covers its own scratch word, no other partition's, and nobody covers
    // the ungranted mid-SRAM hole.
    for p in 0..N_PARTS as u32 {
        for q in 0..N_PARTS {
            let want = p as usize == q;
            if t.covers_addr(p, SCRATCH_ADDR[q]) != want {
                fail!(
                    "gust-switch-probe FAIL: covers_addr(P{}, {:#010x}) != {}",
                    p,
                    SCRATCH_ADDR[q],
                    want
                );
            }
        }
        if t.covers_addr(p, NOBODY_ADDR) {
            fail!(
                "gust-switch-probe FAIL: P{} covers the granted-to-nobody word {:#010x}",
                p,
                NOBODY_ADDR
            );
        }
    }
    unsafe {
        *addr_of_mut!(THE_TABLE) = Some(t);
    }

    // ---- Bring up window 0: program P0's map through the same seams -------
    let _ = region_swap(0);
    let _ = ctx_resume(0);
    let ctrl = unsafe { read_volatile(MPU_CTRL) };
    if ctrl != MPU_CTRL_ENABLE {
        fail!(
            "gust-switch-probe FAIL: MPU_CTRL={:#x} after verified switch (want {:#x})",
            ctrl,
            MPU_CTRL_ENABLE
        );
    }
    if unsafe { read_volatile(addr_of!(RESUME_MAP_OK)) } != 1 {
        fail!("gust-switch-probe FAIL: P0 map not live at initial resume (RBAR readback)");
    }

    // ---- Drive the VERIFIED Switcher across the major frame ---------------
    let mut sw = Switcher::new(frame);
    let mut denied: [u32; 8] = [0; 8];
    let mut denied_n: usize = 0;

    for w in 0..MAX_WINDOWS {
        let p = frame.partition_id[w];
        // The seams' ledger says window w's owner is running.
        let cur_part = unsafe { read_volatile(addr_of!(CURRENT_PART)) };
        if cur_part != p {
            fail!(
                "gust-switch-probe FAIL: window {} owner P{} but CURRENT_PART={}",
                w,
                p,
                cur_part
            );
        }
        // Temporal cross-check via the verified window lookup.
        let mid = frame.offset[w] + 1;
        if frame.current_window(mid) != w as u32 {
            fail!("gust-switch-probe FAIL: current_window({}) != {}", mid, w);
        }

        // 1. Own-access OK: P{p}'s scratch word is writable in its window.
        expect_granted(SCRATCH_ADDR[p as usize], 0xA5A5_0000 | w as u32, "own scratch");

        // 2. Cross-access DENIED: the neighbor's scratch word is not granted
        //    to P{p} — spatial isolation under P{p}'s live map.
        let neighbor = ((p + 1) % N_PARTS as u32) as usize;
        expect_denied(SCRATCH_ADDR[neighbor], "cross-partition scratch");
        denied[denied_n] = SCRATCH_ADDR[neighbor];
        denied_n += 1;

        // 3. Non-vacuity control (in P1's window): P0's scratch — writable
        //    one window earlier — must NOW be denied. A static/un-swapped
        //    map would let this through; the fault proves switch_to_partition
        //    genuinely swapped the map at the boundary.
        if w == 1 {
            expect_denied(SCRATCH_ADDR[0], "P0 scratch in P1 window (map-actually-changed)");
            denied[denied_n] = SCRATCH_ADDR[0];
            denied_n += 1;
        }

        // 4. The boundary: off-boundary ticks never preempt; the window-end
        //    tick ALWAYS does (S1, non-maskable), then the verified switch
        //    crosses save -> swap -> resume (S2 ordering) and advances the
        //    window by exactly one (S3 no-skip).
        if sw.tick(mid) {
            fail!("gust-switch-probe FAIL: off-boundary tick({}) preempted", mid);
        }
        let end_t = frame.offset[w] + frame.budget[w] - 1;
        if !sw.tick(end_t) {
            fail!("gust-switch-probe FAIL: boundary tick({}) did not preempt", end_t);
        }
        let seq_before = unsafe { read_volatile(addr_of!(SEAM_SEQ)) };
        sw.run_switch();
        let (s_save, s_swap, s_resume, saved_part) = unsafe {
            (
                read_volatile(addr_of!(LAST_SAVE_SEQ)),
                read_volatile(addr_of!(LAST_SWAP_SEQ)),
                read_volatile(addr_of!(LAST_RESUME_SEQ)),
                read_volatile(addr_of!(LAST_SAVED_PART)),
            )
        };
        if !(s_save > seq_before && s_swap > s_save && s_resume > s_swap) {
            fail!(
                "gust-switch-probe FAIL: seam order broken at window {} (save={} swap={} resume={} base={})",
                w,
                s_save,
                s_swap,
                s_resume,
                seq_before
            );
        }
        if saved_part != p {
            fail!(
                "gust-switch-probe FAIL: ctx_save saved P{} (outgoing was P{})",
                saved_part,
                p
            );
        }
        if unsafe { read_volatile(addr_of!(RESUME_MAP_OK)) } != 1 {
            fail!(
                "gust-switch-probe FAIL: incoming map NOT live at resume after window {} (RBAR readback)",
                w
            );
        }
        let expect_cur = ((w + 1) % MAX_WINDOWS) as u32;
        if sw.cur != expect_cur {
            fail!(
                "gust-switch-probe FAIL: cur={} after window {} switch (want {})",
                sw.cur,
                w,
                expect_cur
            );
        }
    }

    // ---- Final oracle ------------------------------------------------------
    let (faults, cur_part) = unsafe {
        (
            read_volatile(addr_of!(FAULT_COUNT)),
            read_volatile(addr_of!(CURRENT_PART)),
        )
    };
    // 4 windows x 1 cross fault + 1 map-actually-changed fault in P1's window.
    let expected_faults = MAX_WINDOWS as u32 + 1;
    if faults != expected_faults {
        fail!(
            "gust-switch-probe FAIL: fault count {} (want exactly {})",
            faults,
            expected_faults
        );
    }
    if sw.cur != 0 || cur_part != frame.partition_id[0] {
        fail!(
            "gust-switch-probe FAIL: frame did not wrap to window 0 / P0 (cur={} CURRENT_PART={})",
            sw.cur,
            cur_part
        );
    }
    hprintln!(
        "gust-switch-probe OK: 3 partitions across the major frame (windows P0->P1->P2->P0, wrapped to window 0) — each confined to its time window AND its MPU map, region-swap-before-resume observed at every switch (seam order + RBAR readback), {} expected cross-partition faults denied (CFSR DACCVIOL+MMARVALID) @ {:#010x} {:#010x} {:#010x} {:#010x} {:#010x}",
        faults,
        denied[0],
        denied[1],
        denied[2],
        denied[3],
        denied[4]
    );
    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

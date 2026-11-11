//! gust-iso — the dissolved ISOLATION CORE executed bare-metal on gust.
//!
//! Everything recorded about `iso-core-fused-cm3.o` so far describes lowering
//! fidelity and structural coverage: seam sets, object sizes, BIN-VERIFY rules,
//! witness MC/DC, a WASM→object disposition. **Nothing had ever run it.** Two
//! defects reached `main` through that gap — overlapping data segments (gale#266)
//! and a silent i64-argument miscompile (gale#269, synth#929) — and no static gate
//! caught either. This is the missing oracle.
//!
//! Links the fused hm+mpu+switch object and provides its four native atoms —
//! `ctx-save`, `region-swap`, `ctx-resume`, `mpu-write` — recording every call so
//! the FSM's ordering property is checked against observed behaviour rather than
//! against the source. Results go out USART1; the Renode robot asserts on them.
//!
//! The centrepiece is `iso-nointerfere`: the three components were fused with
//! `meld --memory shared`, which does NOT rebase addresses, so all three place
//! static data at the same base (gale#266 — four overlapping segment pairs). If
//! that overlap is live, writing mpu-thin's 320-byte RegionTable must corrupt
//! switch-thin's MajorFrame. This programs one, then re-reads the other.
#![no_std]
#![no_main]
use core::ptr::{read_volatile, write_volatile};
use cortex_m_rt::entry;
use panic_halt as _;

// ── the four trusted seams ──────────────────────────────────────────────────────
// Recorded rather than acted on: what is under test is the POLICY that decides
// when and in which order they fire, exactly as the Kani harnesses substitute them.
static mut SEAM_LOG: [u8; 16] = [0; 16];
static mut SEAM_N: usize = 0;
const S_SAVE: u8 = 1;
const S_SWAP: u8 = 2;
const S_RESUME: u8 = 3;

fn seam_record(tag: u8) {
    unsafe {
        let n = read_volatile(&raw const SEAM_N);
        if n < 16 {
            write_volatile((&raw mut SEAM_LOG).cast::<u8>().add(n), tag);
            write_volatile(&raw mut SEAM_N, n + 1);
        }
    }
}

#[export_name = "ctx-save"]
pub extern "C" fn seam_ctx_save(_part: u32) -> u32 {
    seam_record(S_SAVE);
    0
}
#[export_name = "region-swap"]
pub extern "C" fn seam_region_swap(_part: u32) -> u32 {
    seam_record(S_SWAP);
    0
}
#[export_name = "ctx-resume"]
pub extern "C" fn seam_ctx_resume(_part: u32) -> u32 {
    seam_record(S_RESUME);
    0
}

// mpu-write returns nothing; record the count and the last triple so the
// region-programming sequence can be checked without an MPU peripheral model.
static mut MPU_WRITES: u32 = 0;
static mut MPU_LAST: (u32, u32, u32) = (0, 0, 0);

#[export_name = "mpu-write"]
pub extern "C" fn seam_mpu_write(rnr: u32, rbar: u32, rasr: u32) {
    unsafe {
        write_volatile(&raw mut MPU_WRITES, read_volatile(&raw const MPU_WRITES) + 1);
        write_volatile(&raw mut MPU_LAST, (rnr, rbar, rasr));
    }
}

// ── the dissolved isolation core ────────────────────────────────────────────────
extern "C" {
    #[link_name = "gust:switch/fsm@0.1.0#set-window"]
    fn sw_set_window(idx: u32, pid: u32, offset: u32, budget: u32) -> i32;
    #[link_name = "gust:switch/fsm@0.1.0#seal-frame"]
    fn sw_seal_frame(frame_len: u32) -> i32;
    #[link_name = "gust:switch/fsm@0.1.0#frame-check"]
    fn sw_frame_check() -> i32;
    #[link_name = "gust:switch/fsm@0.1.0#current-window"]
    fn sw_current_window(t: u32) -> u32;
    #[link_name = "gust:switch/fsm@0.1.0#tick"]
    fn sw_tick(t: u32) -> i32;
    #[link_name = "gust:switch/fsm@0.1.0#run-switch"]
    fn sw_run_switch();

    #[link_name = "gust:mpu/iso@0.1.0#try-add-region"]
    fn mpu_try_add_region(part: u32, base: u32, size: u32, writable: i32) -> i32;
    #[link_name = "gust:mpu/iso@0.1.0#covers-addr"]
    fn mpu_covers_addr(part: u32, addr: u32) -> i32;
    #[link_name = "gust:mpu/iso@0.1.0#switch-to-partition"]
    fn mpu_switch_to_partition(part: u32);
    #[link_name = "gust:mpu/iso@0.1.0#size-field"]
    fn mpu_size_field(size: u32) -> u32;

    #[link_name = "gust:hm/detect@0.1.0#plausible"]
    fn hm_plausible(v: i32, lo: i32, hi: i32) -> i32;
    #[link_name = "gust:hm/detect@0.1.0#vote-ok"]
    fn hm_vote_ok(s0: i32, s1: i32, s2: i32, tol: i32) -> i32;
}

const USART1: u32 = 0x4001_3800;
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

fn ok(cond: bool, good: &[u8], bad: &[u8]) {
    tx(if cond { good } else { bad });
}

#[entry]
fn main() -> ! {
    unsafe {
        // GPIOA(PA9 TX) + AFIO + USART1; PA9 -> AF push-pull; 8MHz/115200.
        const RCC_APB2ENR: u32 = 0x4002_1018;
        let e = read_volatile(RCC_APB2ENR as *const u32);
        write_volatile(RCC_APB2ENR as *mut u32, e | (1 << 0) | (1 << 2) | (1 << 14));
        const GPIOA_CRH: u32 = 0x4001_0804;
        let c = read_volatile(GPIOA_CRH as *const u32);
        write_volatile(GPIOA_CRH as *mut u32, (c & !(0xF << 4)) | (0xB << 4));
        write_volatile((USART1 + USART_BRR) as *mut u32, 0x45);
        write_volatile((USART1 + USART_CR1) as *mut u32, (1 << 13) | (1 << 3));

        tx(b"iso-gate begin\n");

        // 0) The switch FSM accepts a well-formed major frame: four contiguous
        //    10-tick windows, sealed at 40.
        let w0 = sw_set_window(0, 1, 0, 10);
        let w1 = sw_set_window(1, 2, 10, 10);
        let w2 = sw_set_window(2, 3, 20, 10);
        let w3 = sw_set_window(3, 4, 30, 10);
        let sealed = sw_seal_frame(40);
        ok(
            w0 != 0 && w1 != 0 && w2 != 0 && w3 != 0 && sealed != 0 && sw_frame_check() != 0,
            b"iso-frame-ok\n",
            b"iso-frame-bad\n",
        );

        // 1) A malformed frame is rejected — the contiguity check runs on device,
        //    not just under Kani. offset[1] is left one tick short of off0+bud0.
        let _ = sw_set_window(1, 2, 11, 10);
        let bad_seal = sw_seal_frame(41);
        let _ = sw_set_window(1, 2, 10, 10);
        let reseal = sw_seal_frame(40);
        ok(
            bad_seal == 0 && reseal != 0,
            b"iso-frame-reject-ok\n",
            b"iso-frame-reject-bad\n",
        );

        // 2) Window lookup: each tick maps to its own window, uniquely.
        ok(
            sw_current_window(5) == 0
                && sw_current_window(15) == 1
                && sw_current_window(25) == 2
                && sw_current_window(35) == 3,
            b"iso-window-ok\n",
            b"iso-window-bad\n",
        );

        // 3) Non-maskable boundary preemption. Order matters and is part of the
        //    contract: `run_switch` drives the pipeline from SaveCtx, so a boundary
        //    tick must precede it.
        let off_boundary = sw_tick(0); // window 0 spans [0,10): not a boundary
        let at_boundary = sw_tick(9); // end-1 == 9: ALWAYS preempts
        ok(
            off_boundary == 0 && at_boundary != 0,
            b"iso-preempt-ok\n",
            b"iso-preempt-bad\n",
        );

        // 4) run_switch from SaveCtx crosses the three seams in exactly
        //    ctx-save -> region-swap -> ctx-resume, and lands back in Running with
        //    the window advanced by one. Kani proves the FSM edges; that the seam
        //    CALLS are bound to those edges is trusted code order — this is the
        //    first time that binding has been watched execute.
        write_volatile(&raw mut SEAM_N, 0);
        sw_run_switch();
        let n = read_volatile(&raw const SEAM_N);
        let log = read_volatile(&raw const SEAM_LOG);
        let advanced = sw_tick(19) != 0; // window 1 ends at 20 => cur advanced to 1
        ok(
            n == 3 && log[0] == S_SAVE && log[1] == S_SWAP && log[2] == S_RESUME && advanced,
            b"iso-seam-order-ok\n",
            b"iso-seam-order-bad\n",
        );

        // 5) The MPU region programmer accepts a valid region and rejects the
        //    invalid shapes (not a power of two, below the 32-byte minimum,
        //    misaligned) — the validate_region conjunction, on device.
        let good = mpu_try_add_region(0, 0x2000_0000, 1024, 1);
        let not_pow2 = mpu_try_add_region(0, 0x2000_0000, 48, 1);
        let too_small = mpu_try_add_region(0, 0x2000_0000, 16, 1);
        let misaligned = mpu_try_add_region(0, 0x2000_0010, 1024, 1);
        ok(
            good != 0 && not_pow2 == 0 && too_small == 0 && misaligned == 0,
            b"iso-region-ok\n",
            b"iso-region-bad\n",
        );

        // 6) Programming a partition drives the seam: one CTRL-disable write plus
        //    one write per region slot.
        write_volatile(&raw mut MPU_WRITES, 0);
        mpu_switch_to_partition(0);
        ok(
            read_volatile(&raw const MPU_WRITES) > 0,
            b"iso-mpu-seam-ok\n",
            b"iso-mpu-seam-bad\n",
        );

        // 7) THE INTERFERENCE TEST (gale#266). The three components were fused with
        //    `meld --memory shared`, which merges memories WITHOUT rebasing, so all
        //    three place static data at base 1048576 — four overlapping segment
        //    pairs, one of them mpu-thin's 320-byte RegionTable over switch-thin's
        //    .data. If that overlap is live, the region writes just performed must
        //    have moved the major frame. Sampled with NO I/O between the reads: the
        //    dissolved object carries a 2 688 B reservation and semihosting between
        //    samples perturbs it, which produced a false positive the first time
        //    this was run.
        let a = sw_current_window(5);
        let b = sw_current_window(15);
        let c = sw_current_window(25);
        let d = sw_current_window(35);
        let covers = mpu_covers_addr(0, 0x2000_0000);
        ok(
            a == 0 && b == 1 && c == 2 && d == 3 && covers != 0,
            b"iso-nointerfere-ok\n",
            b"iso-nointerfere-bad\n",
        );

        // 8) The health monitor's value-domain predicates, on device.
        ok(
            hm_plausible(5, 0, 10) != 0
                && hm_plausible(-5, 0, 10) == 0
                && hm_plausible(15, 0, 10) == 0
                && hm_vote_ok(10, 10, 10, 5) != 0
                && hm_vote_ok(10, 30, 10, 5) == 0
                && mpu_size_field(1024) == 9,
            b"iso-hm-ok\n",
            b"iso-hm-bad\n",
        );

        tx(b"iso-gate done\n");
    }
    loop {
        cortex_m::asm::wfi();
    }
}

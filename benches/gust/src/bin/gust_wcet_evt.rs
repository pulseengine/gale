//! gust-wcet-evt — DWT cycle sampling of two DISSOLVED functions, for the
//! measurement side of `VER-OS-WCET-001` (v0.6.0 track T4, workstream D1).
//!
//! `REQ-OS-WCET-001` gets its static per-function cycle bounds from synth itself
//! (`--emit-wcet`, schema `synth-wcet-v1` — see `drivers/emit-wcet.sh`). Those
//! bounds are only *evidence* if something can falsify them. `VER-OS-WCET-001`
//! says how: the static bound SHALL dominate both a measurement-based statistical
//! (EVT) bound and every observed raw DWT sample, and its kill-criterion is
//! exactly the negation of that. This firmware is the measurement half; the gate
//! half is `drivers/check-wcet-evt.py`, which consumes the lines printed below.
//!
//! DIRECTION OF EVIDENCE (this is not a budget source): DWT numbers here may only
//! ever FALSIFY the static model. No partition budget is, or may be, sized from a
//! raw DWT high-water-mark — see the standing rule in `drivers/emit-wcet.sh`.
//!
//! ## What is measured
//! The frozen dissolved node `drivers/os-node/repro-757/os-tl-fixed.o` (synth
//! 0.45.1 dissolve of the archived `repro-757/loom.wasm`; the same object the
//! I-ISO no-fault control links) exports two functions the sidecar reports as
//! `status: bounded`:
//!
//!   gust:os/time@0.1.0#deadline  (now: u64, ticks: u64) -> u64     now + ticks
//!   gust:os/time@0.1.0#elapsed   (now: u64, deadline: u64) -> u32  now >= deadline
//!
//! Both are straight-line and branch-free *in their arithmetic*, so the sample
//! distribution is near-degenerate by construction — this is a cross-check of the
//! static model against silicon, NOT an independent statistical bound. The checker
//! says so in its own output too.
//!
//! ## The one data-dependent branch, and why the harness pokes .data
//! Each function opens with a once-init guard the dissolve emitted:
//!
//!     ldr.w r5, [pc, #..]      @ literal = R_ARM_ABS32 __synth_wasm_seg_2 + 0xc
//!     ldrb  r5, [r6, #0]
//!     cmp   r5, #0
//!     bne.n <skip>             @ taken  -> SHORT path (11 instrs)
//!     ...  strb.w r8, [r4]     @ untaken-> STORE path (17-18 instrs)
//!
//! In the frozen `.data` image that guard byte (`__synth_wasm_seg_2 + 12`, i.e.
//! `.data + 0x24`) holds `0x3a` (it overlaps the `"gust:os up\n"` literal), so on
//! a stock run the branch is ALWAYS taken and the store path is never executed.
//! The sidecar's `instr_count: 18` shows the static bound covers the STORE path —
//! so sampling only the short path would leave the worst path unmeasured, i.e.
//! would make the cross-check weaker than the artifact asks for.
//!
//! So this harness samples BOTH: for the `cold` half of the samples it zeroes that
//! guard byte first (making the branch fall through into the store), for the `warm`
//! half it leaves it alone. `preflight_guard()` PROVES the decode above on the real
//! object at runtime (byte reads 0x3a; after zeroing it and calling `deadline` it
//! reads 0x01) before any of this is relied on — if that fails the run degrades to
//! warm-only sampling and says so, rather than silently claiming worst-path
//! coverage. The byte is restored to `0x3a` at the end. Nothing else in the object
//! is touched, and neither `#now`/`#line`/`run` (the functions that read that
//! string) are called.
//!
//! ## Measurement overhead
//! `read_read` = the cycle delta of two back-to-back CYCCNT reads with NOTHING
//! between them; it is subtracted from every sample, so a reported sample is the
//! call's own cycles. `call_shim` = the delta across a `bl` to a bare `bx lr` —
//! reported but NOT subtracted, so every reported sample stays CONSERVATIVE (it
//! still contains the caller's `bl` and argument marshalling). If a sample lands
//! above its static bound by less than `call_shim`, the checker annotates that
//! attribution instead of hiding it.
//!
//! ## Output contract (parsed by drivers/check-wcet-evt.py)
//!   WCET-EVT CAL read_read=<c> call_shim=<c> path_variants=<cold+warm|warm-only>
//!   WCET-EVT-SAMPLE <fn> i=<idx> cyc=<c> path=<cold|warm>
//!   WCET-EVT <fn> n=<N> min=<c> max=<c> mean=<c> overhead=<read_read>
//!   WCET-EVT DONE
//!
//! ## Build
//! F100 / thumbv7m only — the object is a synth `--target cortex-m3` .o and linking
//! it into a thumbv7em image silently yields an empty ELF, so the bin carries
//! `required-features = ["target-f100"]` and the default g474 build skips it:
//!   cd benches/gust && cp targets/generated/memory-stm32f100.x memory.x &&
//!   cargo build --release --bin gust_wcet_evt --no-default-features \
//!     --features target-f100 --target thumbv7m-none-eabi
//! Flash + capture is the coordinator's job; this file never assumes a number.
//! Model the flash/capture step on `silicon/run-adc.sh` (same board, same
//! ST-LINK/V1-over-Pi path), then pipe the capture to
//! `drivers/check-wcet-evt.py -`.
//!
//! ## Pre-silicon smoke test (no board needed)
//! `main` runs the two OBJECT-level gates BEFORE touching the DWT, so a plain
//! `cargo run` on the committed (qemu lm3s6965evb) `memory.x` exercises everything
//! except the timing itself. qemu models no DWT, so it stops at step 3 — which is
//! the correct behaviour, not a bug:
//!   # correctness: deadline==now+ticks, elapsed==now>=deadline over 20 pairs — IDENTICAL ok
//!   # worst-path: once-init guard __synth_wasm_seg_2+12 @ 0x20000024 — store path REACHABLE (decode proven)
//!   gust-wcet-evt FAIL: DWT cycle counter unavailable or not advancing (DEMCR=0x00000000 ...)
#![no_std]
#![no_main]

use core::hint::black_box;
use core::ptr::{addr_of, read_volatile, write_volatile};
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;

#[cfg(not(feature = "target-f100"))]
compile_error!(
    "gust_wcet_evt: build with --no-default-features --features target-f100 --target \
     thumbv7m-none-eabi (os-tl-fixed.o is a synth --target cortex-m3 relocatable object; \
     linking it into a thumbv7em/M4 image silently produces an empty ELF)"
);
#[cfg(feature = "target-f100")]
#[path = "../../targets/generated/gust_target_stm32f100.rs"]
#[allow(dead_code)]
mod target;
use target::BOARD;

// ---------------------------------------------------------------------------
// The dissolved node's imports. `read32`/`write32` are the ONLY undefined symbols
// in os-tl-fixed.o; they are reached from `#now` / `#line` / `run`, none of which
// this firmware calls. They exist so the object links, and are deliberately inert.
// ---------------------------------------------------------------------------
#[no_mangle]
pub extern "C" fn read32(addr: u32) -> u32 {
    unsafe { read_volatile(addr as *const u32) }
}
#[no_mangle]
pub extern "C" fn write32(addr: u32, val: u32) {
    unsafe { write_volatile(addr as *mut u32, val) }
}

extern "C" {
    // Exact mangled WIT export names, verbatim from `arm-none-eabi-nm` on the object.
    #[link_name = "gust:os/time@0.1.0#deadline"]
    fn wcet_deadline(now: u64, ticks: u64) -> u64;
    #[link_name = "gust:os/time@0.1.0#elapsed"]
    fn wcet_elapsed(now: u64, deadline: u64) -> u32;
    // Start of the object's third data segment — the once-init guard byte lives at
    // +12 (see the module comment; proven at runtime by preflight_guard()).
    static __synth_wasm_seg_2: u8;
    // Bare `bl`-able `bx lr`, for the call-overhead calibration.
    fn wcet_call_shim();
}

core::arch::global_asm!(
    ".section .text.wcet_call_shim",
    ".global wcet_call_shim",
    ".thumb_func",
    "wcet_call_shim:",
    "    bx lr",
);

// ---------------------------------------------------------------------------
// DWT cycle counter (Cortex-M3, ARMv7-M ARM C1.8). Raw registers rather than the
// cortex-m PAC wrapper so the enable sequence and the CYCCNT zeroing are explicit.
// ---------------------------------------------------------------------------
const DEMCR: *mut u32 = 0xE000_EDFC as *mut u32;
const DEMCR_TRCENA: u32 = 1 << 24;
const DWT_CTRL: *mut u32 = 0xE000_1000 as *mut u32;
const DWT_CTRL_CYCCNTENA: u32 = 1 << 0;
const DWT_CTRL_NOCYCCNT: u32 = 1 << 25; // RO: 1 = no cycle counter implemented
const DWT_CYCCNT: *mut u32 = 0xE000_1004 as *mut u32;

#[inline(always)]
fn cyc() -> u32 {
    unsafe { read_volatile(DWT_CYCCNT as *const u32) }
}

/// Enable TRCENA, then CYCCNTENA, then zero CYCCNT. Returns false if the part has
/// no cycle counter or the counter does not actually advance.
fn dwt_enable() -> bool {
    unsafe {
        write_volatile(DEMCR, read_volatile(DEMCR) | DEMCR_TRCENA);
        if read_volatile(DWT_CTRL) & DWT_CTRL_NOCYCCNT != 0 {
            return false;
        }
        write_volatile(DWT_CYCCNT, 0);
        write_volatile(DWT_CTRL, read_volatile(DWT_CTRL) | DWT_CTRL_CYCCNTENA);
    }
    // Liveness: the counter must advance.
    let a = cyc();
    for _ in 0..16 {
        cortex_m::asm::nop();
    }
    cyc() != a
}

// ---------------------------------------------------------------------------
// Inputs. Wrap-boundary heavy, per SWREQ-MEAS-MS02's "varied input set": both
// functions are 64-bit and their only interesting silicon behaviour is the
// adds/adc and cmp/sbcs carry chain, so the set is built around 0, 1, 2^63,
// 2^63-1, 2^32, u64::MAX and mixed small/large pairs.
// `static` + read_volatile so nothing is constant-folded into the call site.
// ---------------------------------------------------------------------------
const NIN: usize = 20;
static INPUTS: [(u64, u64); NIN] = [
    (0, 0),
    (0, 1),
    (1, 0),
    (1, 1),
    (u64::MAX, 1),
    (1, u64::MAX),
    (u64::MAX, u64::MAX),
    (u64::MAX - 1, 1),
    (1 << 63, 1 << 63),
    ((1u64 << 63) - 1, 1),
    (1, (1u64 << 63) - 1),
    ((1u64 << 63) - 1, (1u64 << 63) - 1),
    (0, u64::MAX),
    (u64::MAX, 0),
    (0x0000_0000_FFFF_FFFF, 1),
    (1, 0x0000_0000_FFFF_FFFF),
    (0xFFFF_FFFF_0000_0000, 0x0000_0001_0000_0000),
    (12_345, 67_890),
    (0xDEAD_BEEF_CAFE_BABE, 0x0123_4567_89AB_CDEF),
    (u64::MAX - 1, u64::MAX - 1),
];

/// N = 2 passes over the input set (pass 0 = cold/worst path, pass 1 = warm).
/// 40 >= the N>=30 the measurement framework requires, and gives the checker's
/// block-maxima estimator 6 blocks of ~6 (it needs >= 5).
const NSAMP: usize = 2 * NIN;

static mut CYC_DEADLINE: [u32; NSAMP] = [0; NSAMP];
static mut CYC_ELAPSED: [u32; NSAMP] = [0; NSAMP];

// The once-init guard byte inside the object's data (see module comment).
const GUARD_OFF: usize = 12;
const GUARD_FROZEN: u8 = 0x3a; // ':' of "gust:os up\n" in the frozen .data image

#[inline(always)]
fn guard_ptr() -> *mut u8 {
    unsafe { (addr_of!(__synth_wasm_seg_2) as *mut u8).add(GUARD_OFF) }
}

// ---------------------------------------------------------------------------
// Measurement primitives. `#[inline(never)]` so every sample of a given function
// goes through ONE identical instruction sequence; the arguments are forced into
// registers with black_box before the first CYCCNT read so argument marshalling
// is not charged to the callee.
// ---------------------------------------------------------------------------
// The result is consumed with `black_box` (a zero-instruction compiler barrier)
// rather than a volatile store: a store would need its destination address
// materialised from the literal pool AFTER the call, i.e. INSIDE the measurement
// window, charging 2-3 phantom cycles to the callee. Verified in the linked
// disassembly: the only instructions between the two `ldr rN, [DWT_CYCCNT]` are
// the `bl` itself.
#[inline(never)]
fn measure_deadline(now: u64, ticks: u64) -> u32 {
    let now = black_box(now);
    let ticks = black_box(ticks);
    let t0 = cyc();
    let r = unsafe { wcet_deadline(now, ticks) };
    let t1 = cyc();
    let _ = black_box(r);
    t1.wrapping_sub(t0)
}

#[inline(never)]
fn measure_elapsed(now: u64, deadline: u64) -> u32 {
    let now = black_box(now);
    let deadline = black_box(deadline);
    let t0 = cyc();
    let r = unsafe { wcet_elapsed(now, deadline) };
    let t1 = cyc();
    let _ = black_box(r);
    t1.wrapping_sub(t0)
}

#[inline(never)]
fn measure_call_shim() -> u32 {
    let t0 = cyc();
    unsafe { wcet_call_shim() };
    let t1 = cyc();
    t1.wrapping_sub(t0)
}

/// Two back-to-back CYCCNT reads with nothing between: the irreducible cost of
/// the measurement itself. Minimum over repeats (the M3 is in-order and this is
/// deterministic; the min is the un-perturbed value).
fn calibrate_read_read() -> u32 {
    let mut best = u32::MAX;
    for _ in 0..64 {
        let a = cyc();
        let b = cyc();
        let d = b.wrapping_sub(a);
        if d < best {
            best = d;
        }
    }
    best
}

fn calibrate_call_shim(read_read: u32) -> u32 {
    let mut best = u32::MAX;
    for _ in 0..64 {
        let d = measure_call_shim();
        if d < best {
            best = d;
        }
    }
    best.saturating_sub(read_read)
}

// ---------------------------------------------------------------------------
// Correctness gate: the thing being timed must be the thing that is specified.
// deadline(now, ticks) == now.wrapping_add(ticks); elapsed(now, dl) == now >= dl.
// ---------------------------------------------------------------------------
fn correctness_gate() -> u32 {
    let mut bad = 0u32;
    for i in 0..NIN {
        let (now, arg) = INPUTS[i];
        let got_d = unsafe { wcet_deadline(black_box(now), black_box(arg)) };
        if got_d != now.wrapping_add(arg) {
            bad += 1;
        }
        let got_e = unsafe { wcet_elapsed(black_box(now), black_box(arg)) };
        if got_e != u32::from(now >= arg) {
            bad += 1;
        }
    }
    bad
}

/// Prove the once-init guard decode on the real object before relying on it:
/// the byte must read `0x3a` in the frozen image, and after zeroing it a call to
/// `#deadline` must set it to `0x01` (the `strb` in the store path). Returns true
/// only if BOTH hold, i.e. only if zeroing really does select the worst path.
fn preflight_guard() -> bool {
    let p = guard_ptr();
    unsafe {
        if read_volatile(p) != GUARD_FROZEN {
            return false;
        }
        write_volatile(p, 0);
        let _ = wcet_deadline(black_box(7), black_box(9));
        read_volatile(p) == 1
    }
}

fn stats(s: &[u32]) -> (u32, u32, u32) {
    let mut mn = u32::MAX;
    let mut mx = 0u32;
    let mut sum = 0u64;
    for &v in s {
        if v < mn {
            mn = v;
        }
        if v > mx {
            mx = v;
        }
        sum += v as u64;
    }
    (mn, mx, (sum / s.len() as u64) as u32)
}

#[entry]
fn main() -> ! {
    let _ = hprintln!(
        "gust-wcet-evt: DWT cycle sampling of the DISSOLVED gust:os/time exports on real \
         {} silicon (VER-OS-WCET-001 measurement half; static bounds come from \
         drivers/os-node/repro-757/os-tl.wcet.json, checked by drivers/check-wcet-evt.py)",
        BOARD
    );

    // ORDER MATTERS. The two OBJECT-level gates below need no cycle counter, so
    // they run FIRST: that makes a plain `cargo run` under qemu lm3s6965evb a real
    // pre-silicon smoke test of everything except the timing (does the object link
    // and run? does it still compute the right answers? does the worst-path poke
    // still decode?), and only the DWT step is silicon-gated.

    // --- 1. correctness gate: time the thing the bound is actually about ----
    let bad = correctness_gate();
    if bad != 0 {
        let _ = hprintln!(
            "gust-wcet-evt FAIL: {} functional mismatch(es) over the {} input pairs — refusing \
             to report timings for code that does not compute what the bound is about",
            bad,
            NIN
        );
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }
    let _ = hprintln!("# correctness: deadline==now+ticks, elapsed==now>=deadline over {} pairs — IDENTICAL ok", NIN);

    // --- 2. worst-path availability ----------------------------------------
    // NOTE: correctness_gate() above already called #deadline; with the frozen
    // guard byte the store path is skipped, but restore it anyway so the decode
    // proof below starts from the exact frozen image.
    unsafe { write_volatile(guard_ptr(), GUARD_FROZEN) };
    let cold_ok = preflight_guard();
    let _ = hprintln!(
        "# worst-path: once-init guard __synth_wasm_seg_2+{} @ {:?} — store path {}",
        GUARD_OFF,
        guard_ptr(),
        if cold_ok { "REACHABLE (decode proven)" } else { "NOT reachable" }
    );

    // --- 3. DWT: the only silicon-gated step --------------------------------
    if !dwt_enable() {
        // Loud + diagnosable: this is the guard that stops the harness inventing
        // zeros on a part (or an emulator) with no working cycle counter. qemu's
        // lm3s6965evb hits this path, which is correct — icount is not silicon.
        let _ = hprintln!(
            "gust-wcet-evt FAIL: DWT cycle counter unavailable or not advancing \
             (DEMCR=0x{:08x} DWT_CTRL=0x{:08x} CYCCNT=0x{:08x}) — refusing to report \
             timings; VER-OS-WCET-001 needs real silicon, not an emulated counter",
            unsafe { read_volatile(DEMCR as *const u32) },
            unsafe { read_volatile(DWT_CTRL as *const u32) },
            cyc()
        );
        debug::exit(debug::EXIT_FAILURE);
        loop {}
    }

    // --- 4. calibration -----------------------------------------------------
    let read_read = calibrate_read_read();
    let call_shim = calibrate_call_shim(read_read);
    let _ = hprintln!(
        "WCET-EVT CAL read_read={} call_shim={} path_variants={}",
        read_read,
        call_shim,
        if cold_ok { "cold+warm" } else { "warm-only" }
    );
    if !cold_ok {
        let _ = hprintln!(
            "# NOTE: once-init guard decode did NOT hold on this object (expected \
             __synth_wasm_seg_2+{}==0x{:02x}, and 0x01 after a zeroed call) — sampling the \
             SHORT path only; the store path the static bound covers is NOT exercised.",
            GUARD_OFF,
            GUARD_FROZEN
        );
    }

    // --- 5. sampling --------------------------------------------------------
    // pass 0 = cold (guard zeroed before each call -> store path taken, the path
    // the sidecar's instr_count=18 corresponds to); pass 1 = warm (stock guard).
    for pass in 0..2usize {
        for i in 0..NIN {
            let (now, arg) = INPUTS[i];
            let idx = pass * NIN + i;

            if pass == 0 && cold_ok {
                unsafe { write_volatile(guard_ptr(), 0) };
            }
            let d = measure_deadline(now, arg);
            unsafe { CYC_DEADLINE[idx] = d.saturating_sub(read_read) };

            if pass == 0 && cold_ok {
                unsafe { write_volatile(guard_ptr(), 0) };
            }
            let e = measure_elapsed(now, arg);
            unsafe { CYC_ELAPSED[idx] = e.saturating_sub(read_read) };
        }
    }
    // Leave the object's data exactly as the frozen image has it.
    unsafe { write_volatile(guard_ptr(), GUARD_FROZEN) };

    // --- 6. report ----------------------------------------------------------
    for (name, arr) in [
        ("deadline", unsafe { &*addr_of!(CYC_DEADLINE) }),
        ("elapsed", unsafe { &*addr_of!(CYC_ELAPSED) }),
    ] {
        for i in 0..NSAMP {
            let path = if i < NIN && cold_ok { "cold" } else { "warm" };
            let _ = hprintln!("WCET-EVT-SAMPLE {} i={} cyc={} path={}", name, i, arr[i], path);
        }
        let (mn, mx, mean) = stats(arr);
        let _ = hprintln!(
            "WCET-EVT {} n={} min={} max={} mean={} overhead={}",
            name,
            NSAMP,
            mn,
            mx,
            mean,
            read_read
        );
    }
    let _ = hprintln!("WCET-EVT DONE");

    debug::exit(debug::EXIT_SUCCESS);
    loop {}
}

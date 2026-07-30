//! gust:hal **thin-seam** GPIO driver — the maximal-wasm extreme, v0.3.0 driver
//! breadth (the pattern-setter after UART/DMA).
//!
//! The ENTIRE STM32F1 (F100 value-line) GPIO protocol — per-pin mode encoding,
//! the CRL/CRH config-field placement, atomic set/reset via BSRR, and input read
//! from IDR — lives here, in verified wasm. It imports ONLY `gust:hal/mmio`
//! (read32/write32) — a strict SUBSET of what uart-thin needs (no irq), so it adds
//! **zero new TCB atoms**. No host GPIO driver exists; this *is* the driver,
//! dissolved to native.
//!
//! REQ-DRV-COMPONENT-001: this is a wasm **component** — `world gpio-driver`
//! (`../wit/gust-hal.wit`), importing `gust:hal/mmio` and exporting `gust:hal/gpio`.
//! The capability is therefore checked against a typed contract at composition
//! time, instead of being an untyped `env` extern that only had to match by name
//! at native link.
//!
//! Build:   cargo build --release --target wasm32-unknown-unknown
//!          wasm-tools component new <wasm> -o gpio.component.wasm
//! Dissolve: loom optimize --passes inline | synth compile --target cortex-m3
//!           --all-exports --relocatable
//! Verify:  cargo kani   (the pure pin-config core: total, injective, in-range)
#![cfg_attr(not(kani), no_std)]

#[cfg(not(kani))]
#[panic_handler]
fn panic(_: &core::panic::PanicInfo) -> ! {
    loop {}
}

// wit-bindgen's canonical-ABI glue must LINK against a global allocator; this world
// is scalar-only (u32 in, u32 out), so nothing ever calls it and a zero-state
// trapping allocator keeps the driver's 0-SRAM property intact.
#[cfg(not(kani))]
use core::alloc::{GlobalAlloc, Layout};
#[cfg(not(kani))]
struct NoAlloc;
#[cfg(not(kani))]
unsafe impl GlobalAlloc for NoAlloc {
    unsafe fn alloc(&self, _: Layout) -> *mut u8 {
        core::ptr::null_mut()
    }
    unsafe fn dealloc(&self, _: *mut u8, _: Layout) {}
}
#[cfg(not(kani))]
#[global_allocator]
static ALLOC: NoAlloc = NoAlloc;

// gust:hal mmio capability — now a WIT-typed component import (still the SAME two
// primitives the ~10-line TCB bridge uart-thin uses; no irq atom).
#[cfg(not(kani))]
wit_bindgen::generate!({ world: "gpio-driver", path: "../wit", generate_all });
#[cfg(not(kani))]
use crate::gust::hal::mmio::{read32, write32};

// Kani model-checks the pure pin-config core on the host, where no wasm bindings
// exist. No harness reaches an mmio call (they were undefined `extern` symbols
// before this seam change, equally unreachable), so a stub stands in for the import.
#[cfg(kani)]
fn read32(_addr: u32) -> u32 {
    0
}
#[cfg(kani)]
fn write32(_addr: u32, _val: u32) {}

// STM32F1 GPIO port register map (offsets from a port base, e.g. GPIOC=0x4001_1000).
// Device knowledge as *data* (offsets/bit math), not trusted code.
const CRL: u32 = 0x00; // config, pins 0..=7   (4 bits/pin)
const CRH: u32 = 0x04; // config, pins 8..=15  (4 bits/pin)
const IDR: u32 = 0x08; // input data register
const ODR: u32 = 0x0C; // output data register
const BSRR: u32 = 0x10; // bit set (0..15) / reset (16..31), atomic

/// STM32F1 pin configuration — the driver's pure, verifiable core (gale `_decide`
/// style). Each pin's config is a 4-bit field `(CNF<<2)|MODE`:
///   MODE  00=input · 10=output 2MHz · 11=output 50MHz
///   CNF   (in) 00=analog 01=floating 10=pull · (out) 00=push-pull 01=open-drain 10=alt-pp
/// The encoding is proven total, injective, and — with `pin_slot` — always in range.
/// CRITICAL — TABLE-FREE by construction. A `match`/array from mode-index to nibble
/// compiles to a **linear-memory lookup table** (`.rodata` → wasm data segment); a
/// thin-seam driver dissolved `--relocatable` (no `--native-pointer-abi`, no data
/// segment, 0 SRAM, 0 TCB atoms) has no linmem base, so that load silently returns 0
/// and the config no-ops (caught by the Renode content-gate). So the mode→nibble map
/// is a **packed-constant shift/mask** — pure arithmetic, no table, no linmem.
/// The 7 nibbles for idx 0..=6 are [0x0,0x4,0x8,0x2,0x3,0x6,0xB], packed 4 bits each:
const NIBBLE_LUT: u32 = 0x0B63_2840; // idx i → (NIBBLE_LUT >> (i*4)) & 0xF

/// The 4-bit CRL/CRH nibble for a mode index. Total; result always ≤ 0xF. Unknown
/// indices (>6) map to 0x0 (high-impedance analog input) — an out-of-range request
/// can never leave a pin as an unintended output. Table-free (shift+mask only).
#[inline]
pub fn nibble_for_idx(i: u32) -> u32 {
    if i > 6 {
        0
    } else {
        (NIBBLE_LUT >> (i * 4)) & 0xF
    }
}

/// A nibble drives the pin (MODE bits nonzero) iff it is an output/alt mode.
#[inline]
pub fn is_output(nibble: u32) -> bool {
    nibble & 0x3 != 0
}

/// Which config register and bit-shift hold a pin's 4-bit field. `pin` is masked to
/// 0..=15, so the returned shift is always ∈ {0,4,…,28} and the field stays inside
/// the 32-bit register — the placement can never index out of range.
#[inline]
pub fn pin_slot(pin: u32) -> (u32, u32) {
    let p = pin & 0xF; // 0..=15 by construction
    if p < 8 {
        (CRL, p * 4)
    } else {
        (CRH, (p - 8) * 4)
    }
}

#[inline(always)]
fn rd(a: u32) -> u32 {
    read32(a)
}
#[inline(always)]
fn wr(a: u32, v: u32) {
    write32(a, v)
}

// ---- exported protocol primitives (the driver's gust:hal-facing surface) ----
// Scalar ABI, no linmem/data segment → 0 SRAM, no native-pointer-abi dependency.

/// Configure `pin` (0..=15) on the port at `port_base` to `mode_idx` (see
/// `nibble_for_idx`). Read-modify-write of the 4-bit CRL/CRH field — leaves the
/// other 15 pins untouched.
#[no_mangle]
pub extern "C" fn gpio_configure(port_base: u32, pin: u32, mode_idx: u32) {
    let (reg, shift) = pin_slot(pin);
    let nib = nibble_for_idx(mode_idx);
    let cur = rd(port_base + reg);
    let cleared = cur & !(0xF << shift);
    wr(port_base + reg, cleared | (nib << shift));
}

/// Drive `pin` high — atomic set via BSRR (no read-modify-write race with an ISR).
#[no_mangle]
pub extern "C" fn gpio_set(port_base: u32, pin: u32) {
    wr(port_base + BSRR, 1 << (pin & 0xF));
}

/// Drive `pin` low — atomic reset via BSRR (upper half-word).
#[no_mangle]
pub extern "C" fn gpio_clear(port_base: u32, pin: u32) {
    wr(port_base + BSRR, 1 << ((pin & 0xF) + 16));
}

/// Read `pin`'s input level (0/1) from IDR.
#[no_mangle]
pub extern "C" fn gpio_read(port_base: u32, pin: u32) -> u32 {
    (rd(port_base + IDR) >> (pin & 0xF)) & 1
}

/// Flip `pin`'s output level — reads its current ODR level and drives the opposite
/// via the atomic BSRR path.
#[no_mangle]
pub extern "C" fn gpio_toggle(port_base: u32, pin: u32) {
    let p = pin & 0xF;
    if (rd(port_base + ODR) >> p) & 1 != 0 {
        wr(port_base + BSRR, 1 << (p + 16));
    } else {
        wr(port_base + BSRR, 1 << p);
    }
}

// `gust:hal/gpio` exported over the SAME bodies as the C-ABI symbols above, not a
// second implementation: the component's exports and the dissolved object's
// `gpio_*` entry points (benches/gust/build.rs, gust_gpio probe) then cannot
// diverge in behaviour. Same shape exec-provider used in v0.6.0.
#[cfg(not(kani))]
struct Driver;
#[cfg(not(kani))]
impl exports::gust::hal::gpio::Guest for Driver {
    fn configure(port_base: u32, pin: u32, mode_idx: u32) {
        gpio_configure(port_base, pin, mode_idx)
    }
    fn set(port_base: u32, pin: u32) {
        gpio_set(port_base, pin)
    }
    fn clear(port_base: u32, pin: u32) {
        gpio_clear(port_base, pin)
    }
    fn read(port_base: u32, pin: u32) -> u32 {
        gpio_read(port_base, pin)
    }
    fn toggle(port_base: u32, pin: u32) {
        gpio_toggle(port_base, pin)
    }
}
#[cfg(not(kani))]
export!(Driver);

/// Kani proofs for the verifiable core (`cargo kani`): the pin-config encoding is
/// total, bounded, injective, mode-consistent, and always placed in range.
#[cfg(kani)]
mod kani_proofs {
    use super::*;

    const N: u32 = 7; // valid mode indices 0..=6

    /// Every valid mode index encodes to a bounded 4-bit nibble, and the packed LUT
    /// reproduces the intended table exactly (regression guard on the bit-packing).
    #[kani::proof]
    fn nibble_bounded_and_correct() {
        let i: u32 = kani::any();
        kani::assume(i < N);
        let nib = nibble_for_idx(i);
        assert!(nib <= 0xF);
        // the intended nibbles for idx 0..=6
        let want = match i {
            0 => 0x0,
            1 => 0x4,
            2 => 0x8,
            3 => 0x2,
            4 => 0x3,
            5 => 0x6,
            _ => 0xB,
        };
        assert_eq!(nib, want);
    }

    /// The encoding is injective: distinct valid indices never collide to the same
    /// nibble (no two configs are silently aliased).
    #[kani::proof]
    fn nibble_injective() {
        let i: u32 = kani::any();
        let j: u32 = kani::any();
        kani::assume(i < N && j < N);
        if nibble_for_idx(i) == nibble_for_idx(j) {
            assert_eq!(i, j);
        }
    }

    /// For any pin, the config-field placement stays inside the 32-bit register:
    /// shift ∈ {0,4,…,28}, the register is CRL or CRH, and shift+4 ≤ 32.
    #[kani::proof]
    fn slot_in_range() {
        let pin: u32 = kani::any();
        let (reg, shift) = pin_slot(pin);
        assert!(reg == CRL || reg == CRH);
        assert!(shift <= 28);
        assert!(shift % 4 == 0);
        assert!(shift + 4 <= 32);
    }

    /// Out-of-range mode indices are safe: they never produce an output nibble
    /// (an invalid request cannot silently turn a pin into a driver).
    #[kani::proof]
    fn unknown_mode_is_safe_input() {
        let i: u32 = kani::any();
        kani::assume(i > 6);
        assert!(!is_output(nibble_for_idx(i)));
    }
}

# gust:hal thin-seam hardware-timer driver — results (gust-OS v0.3.0 driver breadth)

The **second v0.3.0 driver-breadth module** (after GPIO): a hardware timer as a
verified thin-seam driver, turning the raw counter into a usable time capability. The
STM32 timer config (PSC/ARR/CR1) **and** the wrap-safe deadline arithmetic live in
verified wasm; the driver imports only `gust:hal/mmio` (like gpio-thin) — **0 new TCB
atoms**. Written **table-free from the start** (the gpio-thin lesson): all logic is
arithmetic, no `match`→`.rodata` linmem lookup, so it dissolves `--relocatable` clean.

| | dissolved (loom 1.1.18 + synth 0.33.0, cortex-m3) |
|---|---|
| `.text` (flash) | **212 B** |
| SRAM (`.bss`+`.data`) | **0 B** |
| TCB | **2 relocations — `mmio_read32`, `mmio_write32`** — subset of the existing 4-item TCB → **0 new atoms** |
| verified | Kani **3/3, 0 failures** (wrap-safe deadline: no missed/early fire ∀ interval,elapsed < 2³¹ incl. across the wrap; reflexive-fires; 0/1 boolean export) + the `gust-timer-renode` register-effect content-gate |
| linmem | **0 loads** (`wasm-tools print | grep 'i32.load offset='` = 0 — table-free) |

## Componentization (world `timer-driver`, REQ-DRV-COMPONENT-001)

The driver no longer reaches the bridge through raw `env.mmio_read32`/
`env.mmio_write32` externs but through `wit_bindgen::generate!` against
`world timer-driver { import mmio; export timer; }` (`../wit/gust-hal.wit` — the
contract predates this change and is used unedited). The five exported primitives
already matched `interface timer` field-for-field, so the `Guest` impl just forwards
to the same bodies the `#[no_mangle] extern "C"` symbols export — component and
dissolved object cannot diverge. No register logic, constant or arithmetic changed;
Kani is still 3/3.

    cargo build --release --target wasm32-unknown-unknown   # 3148 B core module
    wasm-tools component new .../gust_timer_thin.wasm -o timer.component.wasm
    wasm-tools validate timer.component.wasm                # OK
    wasm-tools component wit timer.component.wasm           # 4287 B component
      import gust:hal/mmio@0.1.0;
      export gust:hal/timer@0.1.0;

One import, one export — what `drivers/check-driver-components.sh` gates.
`.cargo/config.toml` is **gone**: `-C link-arg=--allow-undefined` existed only
because the raw externs were undefined wasm symbols; a WIT-typed import is a real
wasm import, so rust-lld needs no override (verified by a clean rebuild after
`rm -rf target/wasm32-unknown-unknown`).

### Re-pinned symbol contract — the dissolved object changes shape

WIT-typing the seam renames the undefined symbols to the WIT field names, exactly as
exec-provider found in v0.6.0 and gpio-thin repeated. Both objects below were
dissolved to a scratch path with the SAME toolchain (loom 1.2.0 + synth 0.49.0), so
the delta is the seam change alone:

| | pre-change (`env` externs) | componentized |
|---|---|---|
| `.text` | 212 B | **982 B** (+770) |
| `.data`+`.bss` | 0 B | **0 B** (0-SRAM preserved) |
| undefined | `mmio_read32`, `mmio_write32` | **`read32`, `write32`** |
| defined | `timer_{init,now,deadline,elapsed,ack}` | the same five, **plus** `gust:hal/timer@0.1.0#{init,now,deadline,elapsed,ack}` and `cabi_realloc{,_wit_bindgen_0_52_0}` |

The `.text` growth is canonical-ABI export glue plus an unreachable `cabi_realloc`
emitted twice — the native path never enters it (it calls `timer_*` directly). Known
and filed as loom#303; not optimised here.

**`timer-thin-cm3.o` is deliberately NOT regenerated**, so today's `gust_timer` link
and the Renode content-gate are untouched. Whoever regenerates it must, in the same
change, rename the bridge exports in `benches/gust/src/bin/gust_timer.rs` to
`#[export_name = "read32"]` / `#[export_name = "write32"]`; otherwise the link fails
on undefined `read32`/`write32`. There is no duplicate-symbol hazard — the new object
does not mention `mmio_read32`/`mmio_write32` at all.

## The verifiable core (`cargo kani`)

`has_elapsed(now, deadline) = (now.wrapping_sub(deadline) as i32) >= 0` — the
monotonic-within-half-range deadline test. The main proof `no_wrap_induced_misfire`:
for a deadline set as `start + interval` (interval < 2³¹), as `now` advances `elapsed`
ticks (elapsed < 2³¹), the timer fires **exactly** when `elapsed >= interval`,
including across the u32 wrap — so a naive `now >= deadline` misfire at the boundary is
proven impossible. Kill-criterion: any (start, interval, elapsed) triple in range
mis-decides has-elapsed, or the Renode gate observes a `timer-*-bad` line.

## Verified end-to-end (local qemu probe, then Renode gate)

A local qemu-semihosting probe of the **dissolved .o** (catching table/linmem bugs
before CI, per the gpio lesson) confirmed: `timer_init` writes PSC=0x1234 / ARR=0xABCD
/ CR1 CEN=1; deadline(100,50)=150 with elapsed@149=0, @150=1; and the wrap case
deadline(0xFFFFFFF0,0x20)=0x10 with elapsed@0x0F=0, @0x10=1. The `gust-timer-renode`
robot asserts the same as `timer-init-ok` / `timer-deadline-ok` / `timer-wrap-ok` on
USART1.

## Reproduce

```sh
cd benches/gust/drivers/timer-thin
cargo kani                                             # 3/3 verified
cargo build --release --target wasm32-unknown-unknown
wasm-tools print target/.../gust_timer_thin.wasm | grep -c 'i32.load offset='   # 0 (table-free)
loom optimize <wasm> --passes inline --attestation false -o t.wasm
synth compile t.wasm --target cortex-m3 --all-exports --relocatable -o timer-thin-cm3.o  # 212 B, 0 SRAM
```

---

_Toolchain note: current pins are synth 0.52.0 / loom 1.2.0 (#208, re-pinned from 0.49.0). The 0.49 regen
confirmed this driver's dissolved size is unchanged from the synth 0.33.0 measurement
above; register effects unchanged, 0-SRAM preserved._

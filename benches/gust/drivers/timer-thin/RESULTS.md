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

_Toolchain note: current pins are synth 0.49.0 / loom 1.2.0 (#208). The 0.49 regen
confirmed this driver's dissolved size is unchanged from the synth 0.33.0 measurement
above; register effects unchanged, 0-SRAM preserved._

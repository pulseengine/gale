# Thin-seam UART driver — dissolve measurement

The **entire STM32 USART protocol** (init, baud, TXE/RXNE polling, RX drain)
implemented in verified wasm (`src/lib.rs`), importing only the generic
`gust:hal` capabilities `mmio.{read32,write32}` + `irq.poll`. Dissolved with
**loom 1.1.16 + synth 0.15.0** (`--target cortex-m3 --native-pointer-abi
--shadow-stack-size 1024 --all-exports --relocatable`).

| metric | value |
|---|---|
| dissolved `.text` (flash) | **326 B** |
| `.data` (SRAM) | **0 B** |
| `.bss` (SRAM) | **0 B** |
| **SRAM total** | **0 B** (poll-drain RX; no ring buffer) |
| TCB (import relocations) | **3** — `mmio_read32`, `mmio_write32`, `irq_poll` |
| export | `driver_step` |

The TCB is the ~10-line generic register-poke + IRQ-flag bridge, shared by every
driver. The whole driver is verified wasm; nothing peripheral-specific is in the
trusted code.

**Honest caveat:** this poll-drain form allocates no RX buffer, so SRAM = 0. A
*buffered* RX (needed for the gale#65 CCSDS-over-USART stream) puts its ring
buffer in linear memory → that buffer is the SRAM cost; the protocol logic stays
free. The mid/fat seam objects and the buffered variant are measured next.

Reproduce:
```sh
cd benches/gust/drivers/uart-thin
cargo build --release --target wasm32-unknown-unknown
loom optimize target/wasm32-unknown-unknown/release/gust_uart_thin.wasm --passes inline | \
  synth compile - --target cortex-m3 --native-pointer-abi --shadow-stack-size 1024 \
  --all-exports --relocatable -o uart-thin-cm3.o
llvm-size uart-thin-cm3.o
```

## synth 0.15.0 perf test (the new version) — levers help compute, not I/O

Dissolved the driver with synth 0.15.0's four ARM levers **on vs off**:

| | `.text` |
|---|---|
| levers OFF | 382 B |
| levers ON (0.15.0 default) | 382 B |
| **delta** | **0 B (0%)** |

cm3 == cm4 == 382 B (string-driver) / 254 B (primitives-driver). **The levers
give nothing here** — but the disassembly (below) shows *why*, and it is not the
import dispatch (an earlier guess — corrected): it is the **synth#428 prologue +
regalloc residuals**, which dominate tiny driver primitives.

### Grounded finding (from the dissolved disasm — synth#428, still in v0.15.0)

Every primitive (even `uart_rx_fired`, which just calls `irq_poll`) emits:
1. a **6-register leaf prologue** `stmdb sp!, {r4,r5,r6,r7,r8,lr}` + `sub sp,#24`
   — pure overhead for functions that touch 1–2 regs (synth#428 "shrink leaf
   prologue" / VCR-RA-002);
2. **redundant stack spill/reload round-trips** — e.g. `str.w r0,[sp,#20]`
   immediately followed by `ldr.w r2,[sp,#20]`;
3. a **materialised-boolean-then-test** — `ite ne; mov #1/#0; cmp #0; bne` instead
   of a direct conditional branch.

These are the *same* synth#428 items, but they hit **driver primitives harder
than gust_mix**: a tiny hot function (TX one byte) pays a 6-register push/pop +
24-byte frame per call. The v0.13–0.15 arithmetic levers (cmp→select fusion,
local promotion, immediate-shift) don't touch them. → the real perf-loop
recommendation for driver-class code: **the leaf-prologue shrink + spill
elimination (synth#428 VCR-RA-002)**, reported with this disasm as evidence.
(The decision logic itself lowered *well*: `usart_rx_decide` became a single
`(sr & 0x2a) == 0x20` mask-compare — error-priority fused, as Kani proves.)

## Renode end-to-end — WORKING

`gust_uart` (demonstrator + ~10-line thin bridge) drives the dissolved driver on
a hermetic Renode Cortex-M3 with a **real STM32 USART model** (usart1 =
UART.STM32_UART @ 0x40013800). The driver TXes via `uart_tx_byte` over MMIO and
the USART emits — captured output: **`gust-uart-thin`** (614 instr, no fault).

**Design that made it work (and fixed the earlier placement issue):** a driver
provides *protocol primitives* (`uart_init` / `uart_tx_byte` / `uart_rx` /
`uart_rx_fired`); the **app owns the payload**. So the driver carries **no data
segment** — the earlier failure was an embedded TX string landing in the wasm
1 MB linmem at a VMA the linker didn't map (native-pointer-abi). With the string
moved to the demonstrator (normal flash), the driver is 0-data/0-bss, needs no
r11 trampoline, and places cleanly.

Bonus: a **real USART** file-backend *is* capturable headless on the macOS Renode
portable (unlike the SemihostingUart) — so the content-based `Wait For Line`
correctness gate works locally *and* in CI.

| metric (primitive driver) | value |
|---|---|
| dissolved `.text` (flash) | **254 B** |
| SRAM (`.data` + `.bss`) | **0 B** |
| exports | uart_init, uart_tx_byte, uart_rx, uart_rx_fired |
| TCB (import relocations) | mmio_read32, mmio_write32, irq_poll |
| verified | usart_rx_decide — Kani SUCCESSFUL (error-priority, all 2³² SR) |

## Componentization (world `uart-driver`, REQ-DRV-COMPONENT-001)

The driver no longer reaches the bridge through raw `env.mmio_read32` /
`env.mmio_write32` / `env.irq_poll` externs but through `wit_bindgen::generate!`
against `world uart-driver { import mmio; import irq; export uart; }`
(`../wit/gust-hal.wit` — the contract predates this change and is used unedited).
This is the first thin driver to carry **two** typed imports. The four exported
primitives already matched `interface uart` field-for-field, so the `Guest` impl just
forwards to the same bodies the `#[no_mangle] extern "C"` symbols export. No register
logic, bitmask or decision changed; Kani is still 1/1.

    cargo build --release --target wasm32-unknown-unknown   # 2958 B core module
    wasm-tools component new .../gust_uart_thin.wasm -o uart.component.wasm
    wasm-tools validate uart.component.wasm                 # OK
    wasm-tools component wit uart.component.wasm            # 3984 B component
      import gust:hal/mmio@0.1.0;
      import gust:hal/irq@0.1.0;
      export gust:hal/uart@0.1.0;

`.cargo/config.toml` is **gone**: `-C link-arg=--allow-undefined` existed only
because the raw externs were undefined wasm symbols; a WIT-typed import is a real
wasm import, so rust-lld needs no override (verified by a clean rebuild after
`rm -rf target/wasm32-unknown-unknown`).

**One value-domain narrowing, forced by the contract:** `gust:hal/irq.poll` is
declared `-> bool`, so `uart_rx_fired` now returns 0/1 where it previously forwarded
the bridge's raw `u32` verbatim. Every in-tree bridge already answers 0 or 1
(`gust_uart.rs` returns 0, `gust_breadth_probe.rs` returns 1) and the interface doc
has always read "nonzero if the line fired", so no caller observes the difference —
but it is a contract-imposed narrowing, not a silent one.

### Re-pinned symbol contract — the dissolved object changes shape

Both objects below were dissolved to a scratch path with the SAME toolchain (loom
1.2.0 + synth 0.49.0) and the same `--native-pointer-abi --all-exports
--relocatable` recipe, so the delta is the seam change alone. (`--shadow-stack-size
1024` had to be dropped from the pre-change run: synth 0.49 refuses it when no
relocation reaches the native-pointer region, which was the case before
componentization.)

| | pre-change (`env` externs) | componentized |
|---|---|---|
| `.text` | 254 B | **986 B** (+732) |
| `.data` | 0 B | **20 B** |
| `.bss` | 0 B | **1048576 B** (1048 B with `--shadow-stack-size 1024`) |
| undefined | `mmio_read32`, `mmio_write32`, `irq_poll` | **`read32`, `write32`, `poll`** |
| defined | `uart_{init,tx_byte,rx,rx_fired}` | the same four, **plus** `gust:hal/uart@0.1.0#{init,tx-byte,rx,rx-fired}`, `cabi_realloc{,_wit_bindgen_0_52_0}` and `__synth_{wasm_data,globals,wasm_seg_0}` |

**The 0-SRAM property does not survive this driver's `--native-pointer-abi` recipe.**
wit-bindgen emits a 16-byte `.rodata` segment (the `cabi_realloc` alignment
constants), and `--native-pointer-abi` materialises the linear-memory region that
segment lives in. The segment is never read (`i32.load offset=` is still **0** — the
driver stays table-free), so dissolving *without* `--native-pointer-abi` gives 1014 B
`.text` / **0 SRAM**; timer-thin and spi-thin, which never used the flag, keep 0 SRAM
unchanged. Recorded, not optimised (loom#303 covers the `.text` half); which recipe a
regenerated object should use is a follow-up decision.

**`uart-thin-cm3.o` is deliberately NOT regenerated**, so today's `gust_uart` link
and the Renode content-gate are untouched. Whoever regenerates it must, in the same
change, rename the bridge exports in `benches/gust/src/bin/gust_uart.rs` to
`#[export_name = "read32"]` / `#[export_name = "write32"]` / `#[export_name =
"poll"]`; otherwise the link fails on undefined `read32`/`write32`/`poll`. There is
no duplicate-symbol hazard — the new object does not mention the old names at all.

---

_Toolchain note: current pins are synth 0.52.0 / loom 1.2.0 (#208, re-pinned from 0.49.0), not the synth
0.15.0 used for the measurements above. The 0.49 regen (10-driver byte-check)
confirmed this driver's dissolved size is unchanged; register effects unchanged,
0-SRAM preserved._

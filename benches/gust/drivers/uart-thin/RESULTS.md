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

---

_Toolchain note: current pins are synth 0.49.0 / loom 1.2.0 (#208), not the synth
0.15.0 used for the measurements above. The 0.49 regen (10-driver byte-check)
confirmed this driver's dissolved size is unchanged; register effects unchanged,
0-SRAM preserved._

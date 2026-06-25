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

cm3 == cm4 == 382 B. **The levers give nothing here** — the UART driver is
I/O-bound (memory-mapped register loads/stores + a poll loop via meld-dispatched
imports), not the arithmetic-dense clamp/select the levers target (which gave
gust_mix **−31%**). Honest perf-loop finding: *the arithmetic levers optimise
compute; the optimisation opportunity for driver code is the **meld-dispatch
import-call overhead** (synth logs "Meld dispatch enabled" for the 3 mmio/irq
imports), not the ARM peephole levers.* → a recommendation for meld/synth.

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

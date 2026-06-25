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

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

## Renode end-to-end status (honest)

`gust_uart` (demonstrator + ~10-line thin bridge + r11=0 trampoline) **runs on
the M3** (762 instr, no boot fault; USART1 SR reads 0xC0 so TXE is set — the TX
poll won't spin). The content-based TX gate is **pending a data-placement detail**:
the wasm declares 17 pages (1 MB) linmem and the TX string sits at ~0x10000D
inside it; native-pointer-abi places it at a VMA the gust linker doesn't map
(reads hit "non existing peripheral"). control_step avoided this because its table
data lived in the reserved linmem `.bss`; this 0-bss driver's string does not.
Resolution = the synth#383 / native-pointer-abi linmem-placement work (map the
data section into SRAM, or avoid the string constant). The driver LOGIC is proven
(Kani) and dissolves clean; only the demonstrator's string placement is pending.

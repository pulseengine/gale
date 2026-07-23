# spi-thin — dissolved thin-seam SPI driver (gust-OS v0.3.0 driver breadth)

The STM32F1 SPI master protocol as verified wasm: CR1 mode/baud config, the
full-duplex byte shift over SR/DR, and a Kani-proven **transfer FSM** presenting
the RTIO iodev SQE→CQE shape (FIND-DRV-RTIO-001). Imports ONLY `gust:hal/mmio`
(read32/write32) — the same subset gpio-thin uses — so it adds **zero new TCB
atoms**. No host SPI driver exists; this *is* the driver, dissolved to native.

## Verification (the pure core — `cargo kani`, 6/6, 0 failures)

| Harness | Property |
|---|---|
| `p1_exclusive_bus` | `begin` succeeds IFF Idle ∧ count>0 — never onto a busy bus |
| `p2_no_lost_byte` | each Active `step` does `remaining -= 1`; Complete IFF it hit 0 |
| `p2b_step_requires_active` | `step` rejected unless Active (no negative/resurrected count) |
| `p3_abort_frees_bus` | `abort` total; always returns the bus to Idle |
| `p4_pack_roundtrip` | the scalar dissolve ABI carries every FSM state losslessly |
| `p5_cr1_well_formed` | CR1 = master+SW-NSS+enabled, mode in [1:0], baud in [5:3], no stray bit |

## Dissolve (loom 1.1.18 + synth 0.33.1, cortex-m3)

```
cargo build --release --target wasm32-unknown-unknown
wasm-tools print target/.../gust_spi_thin.wasm | grep -c 'i32.load offset='   # 0 (table-free)
loom optimize <wasm> --passes inline --attestation false -o t.wasm
synth compile t.wasm --target cortex-m3 --all-exports --relocatable -o spi-thin-cm3.o
```

| metric | value |
|---|---|
| `.text` | **494 B** |
| `.data` / `.bss` (SRAM) | **0 / 0** |
| linmem loads | **0** (`i32.load offset=` = 0 — table-free, no `.rodata` LUT) |
| new TCB atoms | **0** (mmio_read32 / mmio_write32 only) |

Table-free by construction (the gpio-thin lesson): the mode/baud encoding is pure
bit arithmetic (`mode & 0b11`, `(br & 0b111) << 3`), not a `match`/array — so no
`.rodata` lookup that a `--relocatable` dissolve would read as 0 and silently
no-op.

## End-to-end gates

- **Local qemu-semihosting probe** (`cargo run --bin gust_spi_probe`) of the
  DISSOLVED `.o` against a RAM register window — ran FIRST, before CI: `CR1=0x357`,
  byte shift `0xA5`, FSM begin→step×3→complete / dup-begin faults / abort→idle — ALL OK.
- **Renode content-gate** (`gust-spi-renode`, CI): the dissolved driver (linked
  native, mmio bridge only) writes CR1 on a RAM-mapped SPI1 window, shifts a byte
  over a pre-seeded SR/DR, and runs the FSM on a real STM32 model — asserts
  `spi-config-ok` / `spi-xfer-ok` / `spi-fsm-ok` over USART1.

Source-level proof + native register-effect check; the wasm→native dissolve is
differentially trusted, not proven equivalent (docs/safety/verification-honesty.md).

---

_Toolchain note: current pins are synth 0.49.0 / loom 1.2.0 (#208). The 0.49 regen
measured this driver's dissolved `.text` at **454 B** (was 494 B on synth 0.33.1,
above); register effects unchanged, 0-SRAM preserved._

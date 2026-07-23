# gust:hal thin-seam GPIO driver — results (gust-OS v0.3.0 driver breadth)

The **first v0.3.0 driver-breadth module**, and the pattern-setter: proves the
`gust:hal` thin-seam model generalizes past UART/DMA to a third peripheral class
(digital I/O) with **zero new TCB atoms**. The entire STM32F1 (F100) GPIO protocol
— per-pin mode encoding, CRL/CRH config-field placement, atomic BSRR set/reset,
IDR read — is verified wasm dissolved to native; the driver imports only
`gust:hal/mmio` (a strict subset of what uart-thin needs — no `irq`).

| | dissolved (loom 1.1.18 + synth 0.31.0, cortex-m3) |
|---|---|
| `.text` (flash) | **490 B** — `gpio_configure` 232 / `gpio_toggle` 110 / `gpio_clear` 56 / `gpio_read` 54 / `gpio_set` 52 |
| SRAM (`.bss`+`.data`) | **0 B** (scalar ABI, no linmem/data segment) |
| TCB | **2 relocations — `mmio_read32`, `mmio_write32`** — a **subset** of the existing 4-item TCB, so **0 new atoms** |
| verified | Kani **4/4 harnesses, 0 failures** — pin-config encode is total + bounded (≤0xF) + mode-consistent (`is_output` ⇔ MODE≠00), **injective** (no two modes alias), slot placement always in range (shift∈{0,4,…,28}, field ⊂ 32-bit reg), and unknown mode-index is safe (never an output) |

## The verifiable core (`cargo kani`)

The driver's pure decision logic — the pin-config encoder and the pin→(register,
shift) placement — is Kani-proven over its whole input domain, gale `_decide`-style:

- **`nibble_bounded_and_mode_consistent`** — every `PinMode` encodes to a valid
  4-bit `(CNF<<2)|MODE` nibble, and the pin is driven (MODE≠00) *exactly* when the
  mode is an output/alt mode. An input mode never drives the pin; an output mode is
  never left floating.
- **`nibble_injective`** — distinct modes never collide to the same nibble (no
  silently-aliased config).
- **`slot_in_range`** — for *any* `pin`, the config field lands inside the 32-bit
  CRL/CRH register (masked to 0..=15 by construction; shift ≤ 28, 4-aligned).
- **`unknown_mode_is_safe_input`** — an out-of-range mode index maps to
  high-impedance analog input, so a bad request can never turn a pin into an
  unintended output. Kill-criterion: any of these fails, or the driver reads/writes
  outside the port register window.

## Reproduce

```sh
cd benches/gust/drivers/gpio-thin
cargo kani                                             # 4/4 verified
cargo build --release --target wasm32-unknown-unknown  # 849 B wasm
loom optimize target/wasm32-unknown-unknown/release/gust_gpio_thin.wasm \
  --passes inline --attestation false -o gpio_inl.wasm
synth compile gpio_inl.wasm --target cortex-m3 --all-exports --relocatable \
  -o gpio-thin-cm3.o                                   # 490 B .text, 0 SRAM
arm-zephyr-eabi-nm -u gpio-thin-cm3.o                  # only mmio_read32/write32
```

## Remaining gate (before v0.3.0 REQ-DRV-GPIO-001 V-closes)

- **Renode F100 content-gate:** drive a pin, read it back byte-exact on the
  STM32VLDISCOVERY model (the mechanical oracle, mirroring `gust_uart`'s USART
  gate). This is the last gate; when it's green, `VER-DRV-GPIO-001` is added to
  rivet and `rivet release status v0.3.0` drops from `NOT cuttable (4)` → `(3)`.

---

_Toolchain note: current pins are synth 0.49.0 / loom 1.2.0 (#208). The 0.49 regen
measured this driver's dissolved `.text` at **534 B** (was 490 B on synth 0.31.0,
above) — a +9% regression, the one outlier in the 10-driver byte-check; filed as a
synth note. Register effects unchanged, 0-SRAM preserved._

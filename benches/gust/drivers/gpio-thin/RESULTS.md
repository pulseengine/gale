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

## Componentization (world `gpio-driver`, REQ-DRV-COMPONENT-001)

This driver is the **axis-A pilot**: it no longer reaches the bridge through raw
`env.mmio_read32`/`env.mmio_write32` externs but through `wit_bindgen::generate!`
against `world gpio-driver { import mmio; export gpio; }` (`../wit/gust-hal.wit` —
the contract predates this change and is used unedited). The five exported
primitives already matched `interface gpio` field-for-field, so the `Guest` impl
just forwards to the same bodies the `#[no_mangle] extern "C"` symbols export —
component and dissolved object cannot diverge.

    cargo build --release --target wasm32-unknown-unknown   # 3174 B core module
    wasm-tools component new .../gust_gpio_thin.wasm -o gpio.component.wasm
    wasm-tools validate gpio.component.wasm                 # OK
    wasm-tools component wit gpio.component.wasm            # 4273 B component
      import gust:hal/mmio@0.1.0;
      export gust:hal/gpio@0.1.0;

One import, one export — what `drivers/check-driver-components.sh` gates.
`.cargo/config.toml` is **gone**: `-C link-arg=--allow-undefined` existed only
because the raw externs were undefined wasm symbols; a WIT-typed import is a real
wasm import, so rust-lld needs no override (verified by a clean rebuild without it,
byte-identical output).

### Re-pinned symbol contract — the dissolved object changes shape

WIT-typing the seam renames the undefined symbols to the WIT field names, exactly as
exec-provider found in v0.6.0. Both objects below were dissolved with the SAME
toolchain (loom 1.2.0 + synth 0.49.0) so the delta is the seam change alone:

| | pre-change (`env` externs) | componentized |
|---|---|---|
| `.text` | 534 B | **1410 B** (+876) |
| `.data`+`.bss` | 0 B | **0 B** (0-SRAM preserved) |
| undefined | `mmio_read32`, `mmio_write32` | **`read32`, `write32`** |
| defined | `gpio_{configure,set,clear,read,toggle}` | the same five, **plus** `gust:hal/gpio@0.1.0#{configure,set,clear,read,toggle}` and `cabi_realloc{,_wit_bindgen_0_52_0}` |

The `.text` growth is the canonical-ABI export glue + `cabi_realloc`, which the
native path never enters (it calls `gpio_*` directly) — dead weight in the dissolved
object, worth pruning before eight drivers pay it.

**`gpio-thin-cm3.o` is deliberately NOT regenerated here**, so today's
`gust_gpio` link and the Renode content-gate are untouched. Whoever regenerates it
must, in the same change, rename the probe's bridge exports in
`benches/gust/src/bin/gust_gpio.rs` to `#[export_name = "read32"]` /
`#[export_name = "write32"]`; otherwise the link fails on undefined `read32`/
`write32`. Unlike exec-provider there is no duplicate-symbol hazard — the new object
does not mention `mmio_read32`/`mmio_write32` at all.

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

_Toolchain note: current pins are synth 0.52.0 / loom 1.2.0 (#208, re-pinned from 0.49.0). The 0.49 regen
measured this driver's dissolved `.text` at **534 B** (was 490 B on synth 0.31.0,
above) — a +9% regression, the one outlier in the 10-driver byte-check; filed as a
synth note. Register effects unchanged, 0-SRAM preserved._

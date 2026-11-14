# sen66-thin: the SEN66 air-quality sensor over gust:bus/i2c-master

Device driver for wohl's air node (gale#511, seam `gale-wohl/air-sensor-read`).

- **Imports** only `gust:bus/i2c-master@0.1.0` (`begin`, `xfer`, `end`).
- **Exports** `gust:sen66/air@0.1.0` (`wit/sensor/gust-sen66.wit`): `start`, `read`, `stop`.
- **`read` returns one packed u64** per sample, the shape the owner chose on #511 on 2026-11-14:

| Bits | Content |
|---|---|
| 0..=15 | CO2 ppm |
| 16..=31 | PM2.5 ×10 |
| 32..=47 | VOC index ×10, as an int16 bit pattern |
| 48 / 49 / 50 | valid bit for CO2 / PM2.5 / VOC |
| 63 | error, with the code in bits 0..=7 |

- **Not-ready, CRC failure and unknown fields** (0xFFFF / 0x7FFF) never appear as values. The first two are errors; an unknown field is a cleared valid bit with value 0.

Facts come from the SEN6x datasheet, "D1 Version 1.0 – October 2026"
(sha256 34b8e5b69c3686c76955abe7781b52841aaff7bd67beed012c4655b4566bbeeb). Every
constant in `src/lib.rs` names its section or table.

## How it talks to the sensor

- **`read` sequence:** Get Data Ready (0x0202), then a 3-byte read; if the flag is true, Read Measured Values (0x0300), then a 27-byte read.
- **No buffer:** the 27 bytes stream through a pure accumulator (`rx_step`), which checks each word's CRC as its CRC byte arrives and keeps only words 1, 6 and 8.
- **Busy handling:** while it is processing a command, the sensor NACKs its address (§4.5). Every transaction is retried on NACK, up to `RETRY_MAX` = 20000 attempts.
  - A NACKed attempt is about 11 bit times, about 0.11 ms at 100 kbit/s. So the 1400 ms Stop Measurement time needs about 12700 attempts.
  - That estimate is arithmetic, not a measurement.
  - The bound is an iteration count, not a time.

## Measured (local, 2026-11-14)

- **Unit tests: 9 of 9 pass.** They run against a byte-level sensor model behind gust:bus:
  - a read with address NACKs before each transaction;
  - not-ready;
  - unknown fields;
  - a CRC failure;
  - a negative VOC;
  - a sensor that never answers;
  - plus the three CRC and framing tests.
- **Kani 0.67.0: 9 of 9 harnesses verified.**

| Harness | What it checks |
|---|---|
| c1 | CRC(0xBEEF) = 0x92, the datasheet example |
| c2, c3 | every 1- and 2-bit error in a received word is rejected |
| c4 | `check_word` accepts exactly the framed words |
| c5 | the unknown values follow Table 34 |
| c6 | `pack` follows the seam encoding for all inputs |
| c7 | the accumulator agrees with a direct decode of every 27-byte frame |
| c8 | the seam's kill criterion, below |
| c9 | `start` and `stop` return 0 or a well-formed error |

- **c8 in detail.** The sensor model serves arbitrary bytes, any bus call may fail with any gust:bus error, and each transaction may see up to one address NACK. Under that model, `read` returns a sample only when Get Data Ready was CRC-valid and true and all 9 words passed their CRC. The sample is then exactly `pack` of words 8, 1 and 6. Every other outcome is a well-formed error.
- **Negative controls, each caught (exit 1):**

| Planted fault | Caught by |
|---|---|
| not-ready treated as ready | c8 |
| CRC failure ignored at finish | c8 |
| CO2 taken from word 5 | c7 |
| unknown VOC kept | c6 |
| polynomial 0x07 | c1 |
| init 0x00 | c1 |
| CRC over the low byte only | c2 |
| RH treated as uint16 | c5 |
| `assert!(v & ERR64 != 0)` planted in c8 | c8 |

  The last one fails, so c8 reaches its success branch: samples are reachable, not ruled out by the harness.
- **Dissolve, pinned loom 1.4.1 and synth 0.89.0:**
  - `loom optimize --passes inline`, then `synth compile --target cortex-m3 --all-exports --relocatable --embedder-data-init --embedder-global-init`.
  - text 3714 / data 0 / bss 0 (3898 before the `pack` change below).
  - Undefined symbols: exactly `begin`, `end`, `xfer`.
  - 0 r9 references and 0 `[fp, …]` (linear-memory) accesses (`arm-none-eabi-objdump -d`).
- **No linear-memory access.** The wasm has no load, store or global access. Three things were needed:
  - `generate!` sets `disable_run_ctors_once_workaround: true`. The default guard touches a flag at linear address 0x10000C at every export entry (gale#516). This crate has no constructors.
  - `read_values` returns the u64, not an aggregate. An aggregate return goes through a linear-memory stack slot.
  - The bus shell is `#[inline(always)]`, so `&mut Hw` never needs an address.

  The 12-byte `.rodata` segment wit-bindgen emits is still in the wasm; no code reads it.
- **`drivers/check-cross-arch.py`:** exit 0.
  - ARM: the undefined set equals the imports.
  - RV32: synth 0.89.0 emits an object whose undefined set is `begin`, `end`, `xfer`. This is the first thin driver that crosses to RV32 on this ledger. The emitted RV32 code has not been inspected or run.
- **`cargo xtask check wit-resolve`** resolves `wit/sensor/gust-sen66.wit` against gust:hal plus gust:bus. A planted import typo makes it fail (exit 1).

## The dissolved object is wrong on synth 0.89.0

**The wasm is correct. The dissolved ARM object returns wrong samples. Do not deploy it until synth#1710 and synth#1711 are fixed and `gust-sen66-renode` passes.**

Found by running the dissolved driver on Renode's model of the STM32 I2C v2 block, against a SEN66 slave model (`os/gust/renode-test/gust_sen66.robot`, branch `feat/air-sensor-renode`). Six reads, with expected values computed independently from the encoding:

- **The u32 calls are correct:** `start`, `stop`, the bus driver's `configure`, and both not-ready reads.
- **The VOC bytes cross the bus correctly,** so the register path works.
- **The sample values are wrong.**

The same wasm, raw and loom-inlined, gives every expected value under wasmtime against the same sensor model and the same six steps. So the defect is in the wasm-to-ARM step.

Minimized under qemu (Cortex-M3), with wasmtime as the oracle, there are two open synth defects:

- **synth#1711.** An i64 `select` whose condition is a compare computed after the operands gets garbled halves. That is the shape rustc emits for `if known(x) { v |= … }` over u64. `pack` now builds two u32 halves and joins them in a non-inlined function. Kani c6 and c8 re-verify it, the tests pass, and the wasm has no i64 `select` left.
- **synth#1710.** An explicit `return` of an i64, even a constant, moves only the low half into r0, and r1 keeps a stale value. rustc emits such a `return` for every early exit of `read`. There is no reliable source-level workaround, so the object stays wrong until synth fixes it. After the `pack` change, the Renode run shows correct low words, high words of 0, and the CRC error returned as `0x0000001100000011`.

`synth compile --verify` exits 0 on this wasm. It validates the i32 selection rules and lists the i64 ops only as `I64Const × 23: register-operation`, so this class is outside what it checks.

## What is not shown

- Nothing has run against a real SEN66, on Renode or on silicon.
- The sensor model is mine, written from the datasheet. c8 holds against that model, not against the device.
- c8 bounds the retry loop at `RETRY_MAX` = 2 under Kani. The property does not depend on the count, but harnesses only show it for the bound they run.
- Device status (0xD206) is not read. Its bit definitions are in a separate Sensirion document, which has not been extracted.
- Start is valid only in idle mode (Table 27). What the sensor does with Start while it is already measuring is not stated in the datasheet sections read, and `start` does not guard it.
- The driver Kani harnesses do not run in CI (gale#509).

## Build and check

    cargo build --release --target wasm32-unknown-unknown
    cargo test --lib
    cargo kani

## Regenerate the committed object

    cargo build --release --target wasm32-unknown-unknown
    loom optimize --passes inline <wasm> -o opt.wasm
    synth compile opt.wasm --target cortex-m3 --all-exports --relocatable \
        --embedder-data-init --embedder-global-init -o sen66-thin-cm3.o
    arm-none-eabi-objcopy --localize-symbol=cabi_realloc \
        --localize-symbol=cabi_realloc_wit_bindgen_0_52_0 sen66-thin-cm3.o

loom 1.4.1 and synth 0.89.0 from the pinned layer. The `objcopy` step makes the two
wit-bindgen `cabi_realloc` exports local so this object can be linked next to another
wit-bindgen driver (gust_sen66 links two); nothing calls them, because every function
crosses the seam as a scalar. synth has no option to keep an export local (synth#1726).

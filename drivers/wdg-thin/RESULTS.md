# wdg-thin — verified thin-seam IWDG (independent watchdog) driver (gust:hal)

The 8th verified thin-seam iodev (after GPIO/timer/SPI/UART/I2C/ADC/DAC). The whole
STM32F1 IWDG key-sequence + lifecycle — 0x5555 unlock / PR+RLR config / 0xCCCC start /
0xAAAA refresh — in verified wasm, importing ONLY `gust:hal/mmio` (read32/write32):
**zero new TCB atoms**.

This is the **module-level hardware backstop** the partition-scheduler Health Monitor
design (gale#63) names: if the verified HM/switch core itself hangs, it stops servicing
the IWDG and the hardware forces a reset → fail-to-safe. So this driver's own
correctness is part of that safety argument.

## Distinctive property: cannot-un-start (Kani-proven)

Once the watchdog is started (0xCCCC) it can **never be disabled in software** — only a
system reset stops it. A watchdog you can accidentally turn off is worthless. The FSM
provides **no** disable transition, and `p2_cannot_un_start` proves that applied to a
Running watchdog *every* provided transition either keeps it Running (refresh) or is
rejected without mutating — there is no software path out of Running. Companion
invariants: config registers are **write-protected** until the 0x5555 key unlocks them
(`p1`/`p7`), start is Configured-only and one-way (`p4`), and a refresh only has effect
once Running (`p3`).

## Measured

- **Dissolve (loom 1.2.0 inline → synth 0.52.0 --target cortex-m3 --all-exports
  --relocatable): `wdg-thin-cm3.o` = text 1718 / data 0 / bss 0 → 0 SRAM.** Scalar
  packed-u32 FSM (phase[31:30] · prescaler[14:12] · reload[11:0]) crosses the seam with
  no pointer; table-free config (pure bit arithmetic, no `.rodata` linmem). Imports are
  exactly `gust:hal/mmio`'s `read32` / `write32` — the only undefined symbols. (Pre-
  componentization the same recipe gave text 638 with imports named `mmio_read32` /
  `mmio_write32`; see the componentization section for the +1080 B glue breakdown.)
- **Kani: 7/7 harnesses verified, 0 failures** — p1 write-protection · p2 cannot-un-start ·
  p3 refresh-only-running · p4 start-once · p5 config-bounds · p6 pack-roundtrip ·
  p7 unlock-gates-config. `cargo kani` (kani 0.67.0).

## Componentization (world `wdg-driver`, REQ-DRV-COMPONENT-001)

The driver no longer reaches the bridge through raw `env.mmio_read32` /
`env.mmio_write32` externs but through `wit_bindgen::generate!` against
`world wdg-driver { import mmio; export wdg; }`. Unlike the four drivers migrated
before it (gpio/timer/spi/uart), **no contract existed** — `interface wdg` was
authored for this driver, derived from what `src/lib.rs` actually exports, in the
style of `interface spi` (the closest precedent: same state-threaded scalar FSM,
same `is-*: func(state: u32) -> u32` predicate shape). The six exported primitives
map field-for-field, so the `Guest` impl forwards to the same bodies the
`#[no_mangle] extern "C"` symbols export — component and dissolved object cannot
diverge. No register logic, key value, mask or decision changed; Kani is still 7/7.

The interface deliberately has **no `stop`/`disable`**: the FSM's absence of that
transition is the cannot-un-start property, so the *contract itself* cannot express
the one operation `p2` proves impossible. `is-running` stays `u32` (not `bool`) to
match `spi.is-complete` / `uart.rx-fired` and keep the ABI flat.

    cargo build --release --target wasm32-unknown-unknown   # 3675 B core module
    wasm-tools component new .../gust_wdg_thin.wasm -o wdg.component.wasm
    wasm-tools validate wdg.component.wasm                  # OK
    wasm-tools component wit wdg.component.wasm             # 4912 B component
      import gust:hal/mmio@0.1.0;
      export gust:hal/wdg@0.1.0;

One import, one export. `.cargo/config.toml` is **gone**: `-C
link-arg=--allow-undefined` existed only because the raw externs were undefined wasm
symbols; a WIT-typed import is a real wasm import, so rust-lld needs no override
(verified by a clean rebuild after `rm -rf target/wasm32-unknown-unknown`).

### The dissolved object IS regenerated here — and the bridge was re-pinned with it

Unlike the previous four drivers, `wdg-thin-cm3.o` **is** regenerated, because this
is the first componentized driver with hardware runners and a human needs to flash
it. Both objects below were dissolved with the SAME toolchain (loom 1.2.0 + synth
0.52.0), so the delta is the seam change alone; the pre-change rebuild came out
**byte-identical** to the previously committed object, so the baseline is exact.

| | pre-change (`env` externs) | componentized |
|---|---|---|
| `.text` | 638 B | **1718 B** (+1080) |
| `.data` + `.bss` | 0 B | **0 B** (0-SRAM preserved) |
| undefined | `mmio_read32`, `mmio_write32` | **`read32`, `write32`** |
| defined | `wdg_{unlock,configure,lock,start,refresh,is_running}` | the same six, **plus** `gust:hal/wdg@0.1.0#{unlock,configure,lock,start,refresh,is-running}` and `cabi_realloc{,_wit_bindgen_0_52_0}` |

The +1080 B is canonical-ABI export glue (six wrappers, 24–148 B each) plus **two
identical 166 B copies of `cabi_realloc`** (332 B) that nothing can reach — this
world is scalar-only, so no lowering ever allocates. The native path never enters
any of it (it calls `wdg_*` directly). Known and **filed as loom#303**; recorded
here, deliberately **not** optimised in this change.

`--native-pointer-abi` is **not** used: on uart-thin it materialised wit-bindgen's
`.rodata` into linear memory and cost 1 MB of `.bss` for 8 bytes of text. 0-SRAM is
a headline property of these drivers, so the flat scalar recipe stays.

Because the object's imports are renamed, the three firmwares that link it —
`gust_wdg` (Renode gate), `gust_wdg_probe` (qemu) and `gust_wdg_silicon` (real
board) — export their mmio bridge as `#[export_name = "read32"]` /
`#[export_name = "write32"]` in the same commit; otherwise the link fails on
undefined `read32`/`write32`. All four bins link, and `gust_wdg_probe` on qemu
lm3s6965evb still reports the full key sequence against the regenerated object:

    wdg-protect ok: config-from-Idle faulted, no register write
    wdg-unlock ok: KR=0x5555
    wdg-config ok: PR=0x5 RLR=0x123
    wdg-lock ok: s3=0x80005123, KR unchanged=0x5555, running=0
    wdg-start ok: KR=0xcccc running=1
    wdg-cannot-un-start ok: unlock/config/lock/restart all faulted while Running, ...
    wdg-refresh ok: KR=0xaaaa running stays 1 across repeated refresh
    wdg-probe ALL OK

**Not run here:** the real-silicon two-boot proof (`gust_wdg_silicon` on a
NUCLEO-G474RE / STM32VLDISCOVERY). Both target builds link; flashing is a human step.

## Build

    cargo build --release --target wasm32-unknown-unknown
    loom optimize <wasm> --passes inline | synth compile --target cortex-m3 --all-exports --relocatable

## Follow-on (not in this PR)

A `gust_wdg` demonstrator + Renode content-gate (assert the KR key sequence + that no
software write clears the running state) and a rivet `VER-DRV-WDG-001` artifact. Ties
into the partition-scheduler Health Monitor (gale#63) as the HW fail-to-safe backstop.

---

_Toolchain note: current pins are synth 0.52.0 / loom 1.2.0 (#208, re-pinned from 0.49.0). The 0.49 regen
measured this driver's dissolved `.text` at **648 B** (was 660 B on synth 0.40.0); the
0.52.0 rebuild of that same pre-componentization source gives **638 B** and reproduces
the previously committed object byte-for-byte. Register effects unchanged, 0-SRAM
preserved throughout._

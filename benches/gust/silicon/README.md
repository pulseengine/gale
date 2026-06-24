# On-silicon cycle bench — one TCB, two boards

True hardware cycle counts (DWT CYCCNT, not qemu `-icount`) for native (LLVM→thumb)
vs dissolved (wasm→loom→synth) `gust_mix`, on **both** PulseEngine silicon targets:

| board | MCU | target | dissolved `.o` | RAM | probe-rs chip |
|---|---|---|---|---|---|
| NUCLEO-G474RE | Cortex-M4 | `thumbv7em-none-eabi` | `gust_mix-cm4.o` | 96 KB used of 128 | `STM32G474RETx` |
| STM32VLDISCOVERY | Cortex-M3 / STM32F100 | `thumbv7m-none-eabi` | `gust_mix-cm3.o` | **8 KB** | `STM32F100RBTx` |

`silicon_bench` is one bare-metal TCB (`src/bin/silicon_bench.rs`); the board only
changes the memory map (`silicon/memory-*.x`), the target triple, and the dissolved
object. The DWT cycle counter is identical on M3 and M4, so the two boards' numbers
are directly comparable.

## Run

```sh
brew install probe-rs-tools          # one-time
cd benches/gust
silicon/run.sh g474re                # board you have now (Cortex-M4)
silicon/run.sh f100                  # the STM32F100 (Cortex-M3), once connected
```

`run.sh` swaps in the board memory map, builds, then `probe-rs run` flashes +
streams semihosting (look for `silicon_bench,ratio_x1000,...`), and restores
`memory.x` on exit.

## qemu note

`silicon_bench` runs on qemu lm3s6965 too (harness self-check — correctness gate +
output structure), but qemu's Cortex-M3 model does **not** implement the DWT cycle
counter, so it reports `0` cycles there. The numbers are only meaningful on real
silicon. (For deterministic instruction-proportional numbers without hardware, use
`gust_codegen_bench` under qemu `-icount` instead.)

## Why two objects (the M3-into-M4 gotcha)

The dissolved object's ARMv7-M (`cortex-m3`) build attributes make `rust-lld`
silently emit an **empty ELF** when linked into a thumbv7em (M4) image — so the
G474RE needs a `synth --target cortex-m4` object and the F100 the `cortex-m3` one.
`build.rs` selects by the cargo `TARGET`. (The thumbv7em target also needs its own
`-Tlink.x` rustflag in `.cargo/config.toml`, else the link produces an empty ELF.)
The cortex-m4 object is also the correct artifact for the M4-vs-LLVM-thumbv7em
codegen comparison (synth#428).

## Fusion comparison (synth#428 precondition #2: the G474RE DWT no-regression)

To measure the `SYNTH_CMP_SELECT_FUSE` cmp→select fusion on real M4 silicon, build
a flag-on object and point `run.sh` at it:

```sh
# regenerate a flag-on cortex-m4 gust_mix (loom inline → synth flag-on):
SYNTH_CMP_SELECT_FUSE=1 synth compile <stripped gust_mix>.wasm \
  --target cortex-m4 --all-exports --relocatable -o /tmp/gust_mix-cm4-on.o
GUST_MIX_O=/tmp/gust_mix-cm4-on.o silicon/run.sh g474re
```

Compare the `ratio_x1000` flag-off vs flag-on on the G474RE — that's the on-silicon
DWT signal the synth default-on flip is gated on.

# On-silicon reflash plan — dissolved gust on STM32VLDISCOVERY (STM32F100RB, 8 KB)

The literal-silicon confirmation of the synth#383 shrink (closes the
"physical F100 reflash pending hardware" caveat). qemu gave the functional
result; Renode gives the real-M3 model; **this is the actual chip.**

## Board (one-time)
**STM32VLDISCOVERY** — STM32F100RBT6B: Cortex-M3, 128 KB flash @ `0x08000000`,
**8 KB SRAM** @ `0x20000000`, **onboard ST-LINK** (USB; no separate probe).
~€15. The exact part Renode's `stm32vldiscovery.repl` models and gust targets.

## Host tools (one-time)
```sh
# macOS
brew install probe-rs-tools           # probe-rs (flash + RTT/semihosting), or:
brew install stlink open-ocd          # st-flash / OpenOCD alternative
rustup target add thumbv7m-none-eabi  # already present
```

## 1. Build the dissolved gust with the F100 memory map + the shrink
The committed `memory.x` is the lm3s/qemu layout (`FLASH 0x0`). For the F100, flash
is at `0x08000000`. Use the F100 variant:
```sh
cat > memory.x.f100 <<'MX'
MEMORY {
  FLASH : ORIGIN = 0x08000000, LENGTH = 128K
  RAM   : ORIGIN = 0x20000000, LENGTH = 8K
}
MX
# re-dissolve the kernel with synth >= v0.11.51 (#409) + the shrink, then rename:
SYNTH=synth   # or build-from-source @main
"$SYNTH" compile wasm-kernel/gust_kernel.wasm --target cortex-m3 --native-pointer-abi \
    --shadow-stack-size 4096 --all-exports --relocatable -o wasm-kernel/gust_kernel-cortex-m3.o
llvm-objcopy --redefine-sym gust_poll=gust_poll_body wasm-kernel/gust_kernel-cortex-m3.o
cp memory.x.f100 memory.x
cargo build --release --bin gust_wasm     # links bss 4256 into the 8 KB RAM
```

## 2. Flash + run with semihosting (the heartbeat capture)
gust prints via **semihosting** (`hprintln`), so run under a debugger that forwards it:
```sh
# probe-rs (simplest — semihosting to console):
probe-rs run --chip STM32F100RBTx \
    target/thumbv7m-none-eabi/release/gust_wasm
# --- OR OpenOCD + arm-none-eabi-gdb with `monitor arm semihosting enable` ---
```

## 3. Expected output (must match qemu/Renode)
```
gust-wasm boot: kiln-async kernel DISSOLVED (wasm->loom->synth cortex-m3), native Rust TCB
gust-wasm: dissolved gust_mix(1024)=1500 (expect 1500)
...
gust-wasm: 5000 DISSOLVED poll rounds, scheduler stable, pwm=<n>
```
Kill-criterion: a HardFault, a `gust_mix != 1500`, or no heartbeat = the shrink
mis-addressed on real silicon (a refuse-geometry/contract gap — exactly what
synth#383 wanted surfaced *before* tagging).

## 4. Confirm
If it boots clean, post the on-silicon line on **synth#383** (and reference it in
the v0.11.51 / VCR-MEM-001 notes — upgrading "qemu-8KB confirmed" → "silicon
confirmed on STM32F100RB"). Capture the deterministic cycle count too (DWT
`CYCCNT`) for the WCET record. Restore `memory.x` afterwards (keep the F100
variant as `memory.x.f100`).

# LLVM-LTO + Gale build crashes on NUCLEO-G474RE silicon

**Status:** known-broken on real silicon as of gale@`418c6b8c`. Renode CI passes the same build.
**Discovered:** 2026-05-10, during Phase B of the silicon-anchor protocol.
**Action:** silicon-anchor protocol intentionally **does not** capture an LTO variant for this board until this is fixed.

## What runs

- The build itself succeeds (FLASH 26,592 B, 1 surviving `gale_` symbol vs 3 in the rustc-direct gale build — LTO is doing meaningful inlining).
- `west flash --runner openocd` writes the ELF cleanly.
- The chip starts executing; a few bytes of the bench's `print_csv_header` reach the VCP (`# DWT rows: ...`, partial `# health rows:...`), then the chip resets / re-boots / faults.

## Crash signature

Halting the chip via `openocd ... halt` once it's in the fault state:

```
PC:    0x08004d9a   →   arch_system_halt at zephyr/kernel/fatal.c:30
xPSR:  0x21000022   →   IPSR ISR_NUMBER = 0x22 = 34 = External IRQ 18
CFSR:  0x00000000   (no Cortex-M fault flags — not a hardfault)
HFSR:  0x00000000
SHCSR: 0x00070000   (USGFAULTENA + BUSFAULTENA + MEMFAULTENA — all enabled, none active)
ICSR:  0x0400f822   (VECTACTIVE = 34 = ExtIRQ 18; pending: USG, MEM, BUS, SVCALL faults)
VTOR:  0x08000000   (vector table at start of flash, normal)
```

**External IRQ 18 on STM32G474 = ADC1_2 (ADC1 / ADC2 global interrupt)**, per RM0440 §11.3 (Interrupt and exception vectors).

The chip is permanently sitting in `arch_system_halt`, which is reached via Zephyr's `z_irq_spurious` → fatal handler chain when an IRQ fires without a registered handler.

## Hypothesis

Smart-data emission's `smart_mcu_g4.c` uses Zephyr's ADC API (`DEVICE_DT_GET(DT_NODELABEL(adc1))` + `adc_channel_setup` + `adc_read`). Under the LLVM cross-language LTO link step, one of the following is happening:

1. **Init-order race:** the ADC driver's `Z_DEVICE_DT_INST` static initializer registers the ADC IRQ handler in the IRQ table, but the LLVM linker is reordering the initializer such that an ADC interrupt fires (perhaps from an ADC self-calibration sequence in `smart_mcu_init`) before the handler entry is in place.
2. **IRQ table eviction:** LTO's aggressive inlining is folding the ADC IRQ handler symbol into the calling code, but the IRQ-table-vs-handler symbol matching logic in `gen_isr_tables.py` doesn't see the inlined version, leaving the table slot pointing at `z_irq_spurious`.
3. **Calibration state corruption:** STM32G4 ADC requires explicit calibration (`LL_ADC_StartCalibration`) before first conversion. If LTO reorders init-fn pointers vs the device-driver auto-init, calibration may be skipped and the first ADC operation triggers an internal error → ADC IRQ 18.

Renode's `stm32f4_disco` simulation lane passes because Renode does not model the ADC peripheral with the same interrupt fidelity — the published Renode "LLVM+LTO+Gale = same as rustc-direct" claim is consequently silicon-untested.

## Reproduction

Toolchain matched at LLVM 21.1.8 (rustc 1.94.1 + brew `llvm@21` + brew `lld@21`):

```sh
SDK_DIR=/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1
ARM_BIN="${SDK_DIR}/gnu/arm-zephyr-eabi"
GCC_LIB_DIR=$(dirname $("${ARM_BIN}/bin/arm-zephyr-eabi-gcc" -print-libgcc-file-name -mcpu=cortex-m4 -mthumb -mfloat-abi=soft))
PICOLIBC_DIR="${ARM_BIN}/arm-zephyr-eabi/lib/thumb/v7e-m/nofp"
LLD_LIBS="-L${GCC_LIB_DIR} -L${PICOLIBC_DIR}"

export PATH=/opt/homebrew/opt/llvm@21/bin:/opt/homebrew/opt/lld@21/bin:${ARM_BIN}/bin:$PATH
export ZEPHYR_BASE=/Volumes/Home/git/pulseengine/zephyr
export ZEPHYR_SDK_INSTALL_DIR=${SDK_DIR}
GALE_ROOT=/Volumes/Home/git/pulseengine/gale-smart-data

west build -b nucleo_g474re -d /tmp/silicon-lto -s "$GALE_ROOT/benches/engine_control" -- \
    -DZEPHYR_TOOLCHAIN_VARIANT=llvm \
    -DCMAKE_TRY_COMPILE_TARGET_TYPE=STATIC_LIBRARY \
    -DZEPHYR_EXTRA_MODULES="$GALE_ROOT" \
    -DOVERLAY_CONFIG="$GALE_ROOT/benches/engine_control/prj-gale.conf;$GALE_ROOT/zephyr/gale_lto_overlay.conf" \
    -DCMAKE_EXE_LINKER_FLAGS="${LLD_LIBS}" \
    -DENGINE_BENCH_SWEEP=long
```

## Discriminating tests for next session

1. Build LTO **without smart-data** (drop the `boards/nucleo_g474re.conf` `CONFIG_ADC=y` and conditionally compile-out `smart_mcu_g4.c`). If it boots, ADC interaction is confirmed.
2. Build LTO **without `CONFIG_ISR_TABLES_LOCAL_DECLARATION`** (remove from `gale_lto_overlay.conf`). Tests whether the local ISR-table machinery is the trigger.
3. Build with the `arm-zephyr-eabi-gcc` toolchain + `CONFIG_LTO=y` (GCC-side LTO, no clang). If GCC-LTO works and clang-LTO doesn't, narrows the bug to the LLVM linker plugin / lld.
4. Capture the chip's pre-fault `printk` output via openocd `tracesetup` + ITM trace, to see the last function called before the IRQ 18 fires.

## Why this matters for the publication

The "Three Quiet Barriers" blog post claims LLVM-LTO+Gale produces handoff timing equivalent to rustc-direct+Gale (both at 347 cyc on Renode `stm32f4_disco`). On real `nucleo_g474re` silicon, the LTO build does not run at all. Until this crash is root-caused, **no headline "LLVM cross-language LTO works on this MCU family" claim can be sustained for the G4** — only for the F4 (which the published Renode CI uses).

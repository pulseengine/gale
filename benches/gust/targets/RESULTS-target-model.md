# Target model — silicon validation (IWDG full-V slice)

The AADL target model + generated constants, validated on **real hardware on both
SoC families** it targets. The same `gust_wdg_silicon` source, retargeted across
STM32F1 ↔ STM32G4 by selecting a cargo feature (which selects the generated
constants module — nothing hand-edited), armed the real IWDG on each board and the
hardware watchdog fired the reset. This closes the loop opened by Task 9's build
parity: not only do both firmwares build, they behave identically on silicon.

## What is proven

- The generated constants are the ones baked into the firmware: the G4 ELF embeds
  `RCC_CSR = 0x40021094` (`.word 0x40021094` at the CSR load site); the F1 ELF
  embeds `RCC_CSR = 0x40021024`. Confirmed with `arm-none-eabi-objdump -d`.
- Each firmware read RCC_CSR at its own family's offset and detected `IWDGRSTF`
  (bit 29) after the watchdog reset — i.e. the F1/G4 register-map difference (offset
  0x24/0x94, RMVF bit 24/23), which used to be hand-scattered magic numbers, is now
  supplied by the AADL model and is correct on both parts.

## STM32G474 (Cortex-M4, thumbv7em) — onboard ST-LINK V3, `probe-rs`

Build: `cargo build --release --bin gust_wdg_silicon --target thumbv7em-none-eabi`
(default feature `target-g474re`; `benches/gust/silicon/run-wdg.sh`).

```
boot 1 on STM32G474 (RCC_CSR=0x14000000, no prior WDG reset). Arming the REAL IWDG
  @0x40003000 via the dissolved wdg-thin driver (PR=5, RLR=0x123 ≈ 1.2 s)...
armed (last KR write=0x0000, is_running=1). NOT refreshing — expect a HARDWARE
  reset in ~1.2 s ...
  [~1.2 s later the hardware IWDG resets the chip; probe-rs re-run for boot 2]
gust-wdg-silicon OK: IWDG watchdog reset CONFIRMED on real STM32G474 silicon
  (RCC_CSR=0x34000000, IWDGRSTF=1) — the dissolved wdg-thin driver armed the
  hardware watchdog and it fired the reset.
```

`0x34000000` has bit 29 set → `IWDGRSTF=1`, read at the **G4** RCC_CSR (0x40021094).

## STM32F100 (Cortex-M3, thumbv7m) — ST-LINK/V1 on a Linux flash host, openocd

Build: `cargo build --release --bin gust_wdg_silicon --no-default-features
--features target-f100 --target thumbv7m-none-eabi`. Flash + capture (single
session rides through the reset — captures both boots):
`openocd -f interface/stlink-hla.cfg -f target/stm32f1x.cfg -c "init; halt; program
<elf> verify; arm semihosting enable; resume"`.

```
device id = 0x10016420       # STM32F1 value-line
** Verified OK **
boot 1 on STM32F100 (RCC_CSR=0x14000000, no prior WDG reset). Arming the REAL IWDG
  @0x40003000 via the dissolved wdg-thin driver (PR=5, RLR=0x123 ≈ 1.2 s)...
armed (last KR write=0x0000, is_running=1). NOT refreshing ...
gust-wdg-silicon OK: IWDG watchdog reset CONFIRMED on real STM32F100 silicon
  (RCC_CSR=0x34000000, IWDGRSTF=1) — the dissolved wdg-thin driver armed the
  hardware watchdog and it fired the reset.
```

`0x34000000` has bit 29 set → `IWDGRSTF=1`, read at the **F1** RCC_CSR (0x40021024).

## Scope / honesty

- This validates that the **generated constants are correct on real silicon** for
  both families, and that model-swap retargeting preserves behaviour. It is the
  same evidence class as the original wdg silicon anchor (a real hardware watchdog
  reset that a silently-no-op'd driver could never produce), now driven by the
  AADL-generated constants rather than hand-written ones.
- The wasm→native dissolve remains differentially trusted, not proven equivalent
  (see `docs/safety/verification-honesty.md`). Kani proves the driver FSM; the
  generator is golden- + parity-tested; silicon confirms the whole chain end to end.
- Probe behaviour note: on the watchdog reset, `probe-rs` reports an "Exception" and
  drops (re-run lands on boot 2, since `IWDGRSTF` persists until `RMVF`); openocd's
  semihosting session rides through in one run. Do **not** pass
  `probe-rs --catch-hardfault` — it flags the legitimate reset as a fault.

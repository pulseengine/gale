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

## Peripheral breadth — where every modelled base address comes from

The F100 model was grown from `Iwdg`/`Rcc`/`Adc` to cover the peripherals gale
already has drivers for. Every base was **lifted from an existing in-tree use**,
never sourced from a datasheet by hand — the model became the single source of
truth for values the tree was already asserting. The evidence is tiered, and the
tier matters: a gate that maps a *plain memory window* at an address pins the
address the firmware uses, but does not independently confirm it against silicon.

| Device | Base | Evidence | Tier |
|---|---|---|---|
| `Rcc` `Apb2enr_Offset` | `0x40021018` | `src/bin/gust_adc_silicon.rs:83` sets ADC1EN here before each Vrefint read; real STM32VLDISCOVERY returned 1645/1646 raw (`silicon/RESULTS-f100.md`). An unclocked ADC cannot convert. | **silicon** |
| `Adc` | `0x40012400` | same silicon run (`gust-adc-silicon OK … ADC1 @0x40012400`) | **silicon** |
| `Iwdg` | `0x40003000` | hardware watchdog reset on this board (above) | **silicon** |
| `Usart1` | `0x40013800` | `drivers/uart-thin/src/lib.rs:33`; `//:gust-uart-renode` drives Renode's **real** `UART.STM32_UART @ <0x40013800,+0x100>` register model and content-gates the emitted bytes. F100-linked image also flashed + ran to completion on the physical board (`drivers/uart-thin/SILICON.md`); on-wire byte capture still pending an external USB-serial on PA9. | real peripheral model + silicon execution |
| `Gpioa` | `0x40010800` | `GPIOA_CRH = 0x40010804` (base + `Crh_Offset`) configures PA9 as USART1 TX in `src/bin/gust_uart.rs:51` and every gate demonstrator; `renode-test/f100_silicon.repl` maps `gpioa @ 0x40010800`. Executed by the image that ran on real silicon. | silicon execution (no observable readback) |
| `Gpioc` | `0x40011000` | `src/bin/gust_gpio.rs:46`, `src/bin/gust_breadth.rs:37`, `drivers/gpio-thin/src/lib.rs:30`; gated by `//:gust-gpio-renode` + `//:gust-breadth-renode` | RAM-window gate |
| `Tim2` | `0x40000000` | `src/bin/gust_timer.rs:30`, `src/bin/gust_breadth.rs:38`, `drivers/timer-thin/src/lib.rs:30`; `TIM2_CNT = 0x40000024` in `drivers/time-provider/src/lib.rs:13`; gated by `//:gust-timer-renode` + `//:gust-breadth-renode` | RAM-window gate |
| `Spi1` | `0x40013000` | `src/bin/gust_spi.rs:35`, `src/bin/gust_breadth.rs:39`, `drivers/spi-thin/src/lib.rs:34`; gated by `//:gust-spi-renode` + `//:gust-breadth-renode` | RAM-window gate |

Register offsets are **properties of a device, never devices**: `0x40013804` is
`Usart1` + `Dr_Offset`, and the generator derives `USART1_DR` as `Base + offset`.

### Found in-tree but deliberately NOT modelled

These addresses are exercised by passing Renode gates, but those gates run on a
**generic Cortex-M3 / STM32F103RE-class** platform, not on `Board.vldiscovery`.
Putting them on the F100 board would assert board facts the tree does not
establish, so they wait for a datasheet source (RM0041) plus an F100 gate or
silicon run:

| Address | Peripheral | In-tree use | Why not modelled |
|---|---|---|---|
| `0x40006400` | bxCAN1 | `src/bin/gust_can.rs:38`, `//:gust-can-renode` | Strongest reason to stay out: the generator's own model already treats "bxCAN on a value-line part" as the canonical **absent** peripheral (`tools/gust-target-gen/src/model.rs`, the `Present => false` comment), and this board *is* the value line (`device id = 0x10016420`, above). Presence needs an RM0041 source; if it is confirmed absent it belongs in the model as `Present => false` with **no** `Base`, not as a base address. |
| `0x40012C00` | TIM1 (advanced timer) | `src/bin/gust_pwm.rs:40`, `//:gust-pwm-renode` | Gate platform is not the F100 board; no F100 evidence. |
| `0x40007400` | DAC | `src/bin/gust_dac.rs:42`, `//:gust-dac-renode` | idem |
| `0x40005400` | I2C1 | `src/bin/gust_i2c.rs:37`, `//:gust-i2c-renode` | idem |
| `0x40020000` | DMA1 | `src/bin/gust_dma.rs:27`, `//:gust-dma-renode` | idem |

### STM32G474 — still deliberately empty beyond `Iwdg`/`Rcc`

**No F100 peripheral base may be copied to the G474.** This very model already
holds the counter-example: the *same* register, `RCC_CSR`, sits at offset `0x24`
on the F1 and `0x94` on the G4 — the two maps are not interchangeable, and the
watchdog only retargeted because `Iwdg` and the `Rcc` base happen to coincide,
each independently silicon-validated on a real G474RE (above).

Every gust driver gate runs on Cortex-M3 / STM32F1-class Renode platforms and the
only G4 silicon run in the tree is the IWDG one, so the tree holds **zero** G4
evidence for USART/GPIO/TIM/SPI/ADC bases — nor for an `Apb2enr_Offset` (the F1
value `0x18` is anchored only by an F100 silicon run and must not be assumed to
hold on the G4). What the G474 model needs, and what this task could not supply:

1. An **RM0440** peripheral memory-map source for each base to be added.
2. A **G474 gate or silicon run** exercising it — a `.repl` for the G4 map plus a
   `renode_test` target, or a `benches/gust/silicon/` run on the NUCLEO-G474RE in
   the shape of `gust_adc_silicon` (self-checking, no external wiring).
3. Then the same `emit_wit::interfaces_for` / `emit_rs::PERIPHERALS` entries the
   F100 devices already use — the generator side needs no further work.

An empty G474 peripheral set is the honest state, not an oversight.

## Assurance assessment — witness (MC/DC) and sigil (attestation)

Per the feature-loop methodology, these conditional steps are assessed, not
silently skipped:

- **witness (MC/DC on the wasm component): N/A for this slice.** The target-model
  work introduces no new decision/branch. The wdg-thin driver FSM is unchanged
  (Kani-proven, VER-DRV-WDG-001) and the firmware's only branch — boot 1 vs boot 2
  on `IWDGRSTF` — is untouched; the change moved compile-time constants from
  hand-written to generated code (identical values, verified by the parity test and
  on silicon). No new condition means no new truth-table row, so there is nothing
  for witness to cover here. If a future target adds a *conditionally-present*
  peripheral that the firmware branches on, that branch gets a witness pass.
- **sigil (attestation): deferred, tracked as FIND-TARGET-SIGIL-001.** The generator
  is a new build stage that emits committed artifacts (`benches/gust/targets/
  generated/`). Signing that tree (so a consumer can verify the generated constants
  were produced by the trusted generator from the reviewed model, not hand-edited)
  is a reasonable build-integrity step but is not v1-blocking: the committed tree is
  already guarded by the CI drift gate (regenerate + `git diff --exit-code`), which
  gives tamper-evidence within the repo. Recorded as a named follow-on rather than
  dropped, so a recurring N/A does not hide a real attestation gap.

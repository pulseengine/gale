# STM32G474RE — sourced constants for the target model, and what silicon confirms

Research input for populating `stm32g474.aadl`, which today declares only `Iwdg` and
`Rcc` because the repo held no G4 evidence for anything else. **Nothing here has entered
the model yet.** This file is the evidence; the AADL change is a separate, reviewable step.

The F100 model was populated by lifting addresses already proven by passing gates. No
such tree evidence exists for G4 — every driver gate runs on Cortex-M3/STM32F1-class
platforms — so these come from documentation, and the riskiest were then checked against
the physical NUCLEO-G474RE.

## Sources, and why they count as independent

| tag | source | class |
|---|---|---|
| **SVD** | ST CMSIS-SVD `STM32G474xx.svd` v1.9 | ST vendor, machine-readable — primary |
| **CMSIS-H** | ST `cmsis-device-g4`, `stm32g474xx.h` | ST vendor, *different artifact and pipeline* from the SVD |
| **METAPAC** | embassy-rs `stm32-data-generated` | **non-ST**, own extraction pipeline |
| **ZEPHYR** | `zephyr` `dts/arm/st/g4/*.dtsi`, `nucleo_g474re.dts` | **non-ST**, hand-maintained from RM0440 |
| **G4HAL-RS** | `stm32-rs/stm32g4xx-hal` | **non-ST, hand-transcribed** — genuinely independent for pin/AF |
| **ST-BSP** | `STM32G4xx-Nucleo-BSP` (`BOARD_ID "MB1367"`) | ST vendor, board level |

**RM0440 and DS12288 could not be retrieved** — `st.com` PDF fetches failed. Every
reference manual citation in the source research is second-hand and was treated as weaker
than the machine-readable triple. The bench checks below exist precisely to compensate.

Note `PINCTRL` and `MBED` are both CubeMX-DB derived and do **not** corroborate each other.

## Confirmed on the physical board (2026-07-30)

Read over SWD with probe-rs on the attached NUCLEO-G474RE. These convert SVD-only reset
values into observed facts:

| check | address | expected | read | verdict |
|---|---|---|---|---|
| `DBGMCU_IDCODE` | `0xE004_2000` | low 12 bits `0x469` | `20036469` | **confirms STM32G47x, category 3** |
| `FLASH_SIZE` | `0x1FFF_75E0` | `0x0200` = 512 KiB | `0200` | **confirms the RE part** |
| `RCC_AHB1ENR` | `0x4002_1048` | `0x0000_0100` (FLASHEN) | `00000100` | **confirms AHB1ENR at offset 0x48** |
| `RCC_APB1ENR1` | `0x4002_1058` | `0x0000_0400` (RTCAPBEN) | `00000400` | **confirms APB1ENR1 at offset 0x58** |
| `GPIOA_MODER` | `0x4800_0000` | `0xABFF_FFFF` | `abffffff` | **confirms GPIOA base + MODER@0x00** |
| `GPIOB_MODER` | `0x4800_0400` | `0xFFFF_FEBF` | `fffffebf` | **confirms GPIOB base** |
| `GPIOC_MODER` | `0x4800_0800` | `0xFFFF_FFFF` | `ffffffff` | **confirms GPIOC base** |

The GPIO reads required first writing `0x7` to `RCC_AHB2ENR` (`0x4002_104C`), which is
itself the confirmation of that register's offset and of the `GPIOAEN`/`BEN`/`CEN` bit
positions — an unclocked peripheral reads zero on this part.

Three *different* distinctive MODER values across three bases is not a result reachable
by accident. Together these pin the single most dangerous F1→G4 difference: GPIO moved
from APB2 at `0x4001_0800` to **AHB2 at `0x4800_0000`**, and the enable bit moved from
F1's one `APB2ENR` to G4's `AHB2ENR`.

**Read as zero because unclocked, not because wrong:** `TIM2_ARR` and `ADC1_CR` both read
`00000000` before their clocks were enabled — consistent with `RCC_APB1ENR1` showing
`TIM2EN` clear. Their reset values (`0xFFFF_FFFF`, `0x2000_0000`) remain SVD-only.

## Still single-source — do NOT model without settling

| item | status | settled by |
|---|---|---|
| `Vrefint = ADC channel 18` (F1 is **17**) | ST-only | convert channel 18 with `VREFEN` set; expect ~1500 counts, rock-steady |
| `ADC12_COMMON_CCR.VREFEN` = bit 22 | SVD only | same conversion |
| `RCC_APB1ENR2.LPUART1EN` = bit 0 | METAPAC only | enable it and read an LPUART register |
| DMAMUX `CxCR` field layout; `DMAREQ_ID` values | SVD only / not researched | a DMA driver needs the request IDs |
| UM2505 solder-bridge numbers | second-hand | read the real UM2505 before touching a soldering iron |
| Factory `FLASH_OPTR.DBANK` mode | undetermined | read `0x4002_2020` bit 22 |

## Two corrections this research produced

**The VCP is LPUART1, not USART2.** On NUCLEO-G474RE the ST-LINK virtual COM port is
`LPUART1` (`0x4000_8000`) on PA2/PA3 at **AF12** — corroborated by ST-BSP, ZEPHYR, MBED
and G4HAL-RS. **LPUART's BRR is a 256x oversampled divider** (`BRR = 256 * f_ck / baud`),
unlike USART's plain divider; a driver ported without that runs at 256x the intended rate.
USART2 does exist on the same pins at AF7, and *might* reach the same physical VCP nets,
but no document says so — that is inference and is marked as such.

**Our RAM figure is short by 32 KiB.** `stm32g474.aadl` declares `Length => 98304`
(96 KiB = SRAM1+SRAM2). The RE part has CCM SRAM aliased immediately after SRAM2 at
`0x2001_8000`, DMA-reachable at that alias, giving a contiguous **128 KiB**. Safe as-is,
but 32 KiB is being left on the floor. Caveats before changing it: `FLASH_OPTR.CCMSRAM_RST`
can erase CCM on reset, and `SYSCFG_SWPR` offers per-page write protection.

## F1 -> G4 porting traps — the highest-value section

Every existing thin driver hardcodes F1 offsets. Ordered by how silently each fails.

| driver | what happens if pointed at G4 unchanged |
|---|---|
| **uart-thin** | **Worst case.** USART1 is at `0x4001_3800` on *both* families, so there is no bus fault and no obvious symptom — but the register map is entirely different (`CR1 0x00`/`BRR 0x0C`/`ISR 0x1C`/`RDR 0x24`/`TDR 0x28`, and `UE` is bit **0**, not 13). The driver's `SR` read hits `CR1`; its `CR1` write lands in `BRR` as a nonsense divider. Trap-within-a-trap: `TXE@7`/`RXNE@5` keep the same bit numbers, so the masks look correct in review. |
| **i2c-thin** | Same base `0x4000_5400`, so again no fault — but G4 is I2C **v2**: `TIMINGR 0x10`, `ISR 0x18`, `RXDR 0x24`, `TXDR 0x28`. Beyond offsets the protocol model changed (`CR2.NBYTES`+`AUTOEND`, no `START`/`ACK` in `CR1`). **The verified ACK-all-but-last FSM does not map onto v2 — it needs a fresh proof, not a port.** |
| **dac-thin** | `0x4000_7400` on G4 is inside the **PWR** region. Writing DAC codes there pokes power-control registers. Rebase to `0x5000_0800` and all six offsets carry over unchanged. |
| **gpio-thin** | Wrong base *and* wrong layout. `BSRR` moved `0x10 -> 0x18`, `IDR` `0x08 -> 0x10`; the 4-bit-per-pin `CRL`/`CRH` model became 2-bit `MODER` + separate `OTYPER`/`OSPEEDR`/`PUPDR`. The `NIBBLE_LUT` encoding table is F1-only. Needs a rewrite, not a re-base. |
| **adc-thin** | Different base (`0x5000_0000`), different map, **and a new mandatory startup**: `ADC_CR` resets with `DEEPPWD` set — clear it, set `ADVREGEN`, wait, calibrate, `ADEN`, poll `ADRDY`. The F1 `ADON`-twice idiom does not exist. Vrefint channel 17 -> **18**. |
| **spi-thin** | Base and `CR1`/`SR`/`DR` offsets identical. **But** G4 SPI has a FIFO: with `CR2.FRXTH` at its reset value, `RXNE` only asserts at 16 bits, so a byte-at-a-time driver hangs. Set `FRXTH=1` and access `DR` 8-bit. |
| **timer-thin** | Cleanest port — all offsets identical. Only delta: **TIM2 is 32-bit on G4** (`ARR` resets `0xFFFF_FFFF`) versus 16-bit on F1. A Kani bound asserting `ARR <= 0xFFFF` would need widening. |
| **pwm-thin** | TIM1 sits at `0x4001_2C00` on both, all eight offsets identical. Only the RCC gate moves and the pin needs G4-style AF selection. |
| **wdg-thin** | Identical (plus a new `WINR` at `0x10`). This is why the G474 model already works for the watchdog. |
| **dma-own** | Base and channel stride match, but **DMAMUX is the killer**: on G4 the peripheral request must be *routed* via `DMAMUX1_CxCR`. Without it the channel arms and never fires — the ownership FSM completes its handoff and no bytes move. Also, DMA reaches CCM SRAM only via the `0x2001_8000` alias. |

**Cross-cutting.** F1 gates most peripherals from one `APB2ENR` at RCC+`0x18`. G4 splits it
across `AHB1ENR 0x48` (DMA/DMAMUX), `AHB2ENR 0x4C` (GPIO/ADC/DAC), `APB1ENR1 0x58`
(TIM2/USART2/I2C1) and `APB2ENR 0x60` (TIM1/SPI1/USART1). A helper writing "the enable
register at RCC+0x18" hits G4's `CIER` instead: no fault, no clock, and every subsequent
peripheral read returns zero. And G4 needs explicit alternate-function selection
(`MODER`=AF plus an `AFRL`/`AFRH` nibble) that F1 did not — so even the "clean" ports need
a pin-setup step they have never had.

## Also worth knowing

- **LD2 is PA5, active-high** (ST-BSP + ZEPHYR + MBED). Blinking it is a complete
  end-to-end check of GPIO base, `MODER`, `BSRR` and the board mapping, with no wiring.
- **PA5 is also SPI1_SCK (AF5)** — an LED test and a SPI1 test cannot share default pins.
- **Renode has no STM32G4 platform** (`platforms/cpus/` has f0/f1/f4/f7/g0/h7/l0/l1/l5/wba
  but no g4). A G4 target cannot be CI-gated in emulation without writing one; the
  physical board is the only execution path.

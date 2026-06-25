# gust generic driver model — design (UART seam spike)

**Status:** design / approved-to-spec · **Date:** 2026-06-25 · **Branch:** `feat/gust-driver-model`

## Problem

Adding a driver to gust today is hand-wired: per-bin `build.rs` link lines, a
hand-rolled kiln `poll_round` loop, manual export-stripping and the r11=0
trampoline, and — for any I/O driver — a bespoke hand-written bridge resolving the
dissolved object's import-call relocations. Compute "drivers" (import
`gale:kernel`, fuse, dissolve, drive as a kiln task — e.g. `gust_control`) are a
clean recipe; **hardware/I/O drivers are architecturally supported but not
ergonomic, and the verified/TCB boundary for them has never been measured.**

Jess will bring an arbitrary driver ask. Before that lands we want (a) the
**seam** nailed — the WIT contract and the verified-wasm / trusted-TCB split — and
(b) an **empirical answer** to "how much of a driver can move into verified
wasm," because that decides both the architecture and how much we depend on a
host HAL at all.

## Goal & non-goals

**Goal (this spec):** define the `gust:hal` capability seam and prove it with
**one UART driver end-to-end**, with the UART *protocol implemented in verified
wasm*, built at **three seam granularities** so we can **measure the trusted-code
(TCB) bytes** each costs. The measurement is the primary deliverable.

**Non-goals (deferred — YAGNI until there is real duplication to factor out):**
- The manifest + generator that auto-emits bridge stubs / build wiring /
  trampoline. Extract it *after* Jess's ask makes driver #2.
- The full peripheral set (gpio / spi / i2c / adc / timer). Only UART here.
- embassy on roomy parts as a product path. embassy appears here only as the
  *fat-seam backend*, and primarily to measure that it does **not** fit the
  constrained target.

## Key prior decisions (from brainstorming)

1. **embassy HAL-only; kiln stays the one executor.** embassy's executor is not
   adopted; embassy is (at most) a host-side peripheral backend.
2. **Two capability shapes:** *sync* caps for fast register touches; *split-phase*
   (start + poll/yield, kiln-wakeable) caps for interrupt-driven streaming. No
   embassy `Future` crosses the wasm boundary.
3. **Eventual wiring mechanism = manifest + generator**, but deferred.
4. **First bite = one UART driver, measure TCB at thin/mid/fat seams.**

## Architecture — the `gust:hal` seam

A new WIT package `gust:hal@0.1.0`. A dissolved **driver component** *imports*
capability interfaces; the **TCB bridge** *implements* them. Both sides compile
against the same WIT, so the seam is a typed contract, not a convention.

The irreducible TCB for any peripheral driver is small:
- the raw MMIO read/write primitive (sandboxed wasm cannot touch MMIO),
- the interrupt vector entry + "buffer the byte, wake the task."

Everything else — protocol, framing, baud math, ring buffers, DMA-descriptor
building — is pure compute and can be **verified wasm**. The seam *granularity* is
the lever that trades TCB size against how much is verified:

| seam | imported capability | TCB owns | wasm owns | embassy used? |
|---|---|---|---|---|
| **thin** | `mmio.{read8,write8}` + `irq.wait` (split-phase) | ~10 lines, generic, shared by every driver | **everything**: register sequencing, baud, framing, ring buffer | no (init/clocks only) |
| **mid** | `uart.{put-byte, rx-poll}` | byte-level register access + RX IRQ→buffer | framing, buffering, protocol above the byte | optional |
| **fat** | `uart.{write, read}` | the whole driver (embassy `uart.write().await`) | nothing but calls | yes (embassy-stm32) |

Thesis link: **the thinner the seam, the more is verified and the less embassy is
needed** — and the thin seam is the only one that can fit a tiny node.

## The UART driver

Same observable behavior at all three seams: TX a known byte sequence, RX it back
(loopback), assert identical. What moves across the seam:

- **thin:** the wasm driver writes the USART control/baud registers via
  `mmio.write8`, polls TXE/RXNE via `mmio.read8`, manages its own ring buffer;
  RX uses `irq.wait` (split-phase) so it yields to kiln between bytes.
- **mid:** the wasm driver calls `put-byte` per byte and `rx-poll` to drain;
  framing/buffering still in wasm.
- **fat:** the wasm driver calls `write`/`read`; embassy owns the rest.

### kiln integration

- **sync caps** (`put-byte`, `mmio.write8`): called inline; the bridge does a
  brief blocking register touch and returns — same call pattern as `control_step`.
- **split-phase caps** (`irq.wait`, empty `rx-poll`): the driver returns
  `TaskOutcome::Yielded`; the TCB IRQ handler buffers the byte and wakes the
  task; the next `poll_round` resumes. Reuses kiln's cooperative model and the
  repo's `crank-stream` async-WIT precedent.
- **wiring (hand-done for the spike):** r11=0 trampoline (per `gust_control.rs`)
  for the dissolved object; bridge `.o` linked via `build.rs`; driven from a
  `gust_uart` demonstrator's `poll_round` loop.

## Target & measurement — STM32VLDISCOVERY / STM32F100 in Renode

The spike is pinned to the **STM32VLDISCOVERY (STM32F100RB, Cortex-M3, 8 KB SRAM /
128 KB flash)** modelled in Renode — the constrained device we need anyway (the
physical board is in the silicon plan; the F100 Renode platform already exists in
`benches/gust/renode-test/`).

- **Real USART model:** add `usart1: UART.STM32_UART @ sysbus 0x40013800` to
  `stm32f100.repl` (the F100 value line is register-compatible with the F103 USART
  Renode already models). The dissolved driver's `mmio.write8(0x40013804, b)`
  hits a real STM32 USART register model.
- **Correctness gate (content-based, in CI):** a Renode `Create Terminal Tester`
  on `sysbus.usart1` + `Wait For Line On Uart` asserts the echoed bytes. Unlike
  the SemihostingUart (uncapturable headless on the macOS portable), a **real
  USART** *is* capturable — proven by the existing stm32f4 sem robots. So the
  UART spike gets a genuine content assertion, not just no-fault. Becomes a 4th
  `renode_test` target in the CI module (Bazel 8 pin etc. already in place).
- **TCB metric:** `llvm-size`/`nm` on the bridge `.o` (+ any linked embassy code)
  at thin / mid / fat, plus the dissolved-driver `.text` each — a table like the
  synth 0.15.0 one.
- **Fat/embassy on F100 is build-only and expected to NOT fit 8 KB SRAM** (a full
  embassy-stm32 HAL is well over the budget; F100 value-line support is also
  thin). That negative result *is* a finding: it shows the tiny node requires the
  thin seam. The embassy contrast number, if wanted, is taken separately on the
  roomy G474 (where embassy fits) — out of scope for this spike's pass/fail.

## Success criteria

1. The UART driver passes the content-based correctness gate (TX/RX echo) on the
   F100 Renode target at the **thin** and **mid** seams.
2. A TCB-bytes table (thin vs mid vs fat) + dissolved `.text` per seam, committed
   alongside the spike — the empirical answer to "how much into wasm."
3. A documented verdict: the recommended default seam granularity, and a yes/no
   on "the thin seam fits the 8 KB F100 where the embassy fat seam does not."

## Risks & open questions

- **Renode STM32_UART fidelity:** confirm baud/TXE/RXNE/DR semantics suffice for a
  real driver loop (init register writes may need RCC/clock-enable modelling —
  add minimal RCC if required, or have the bridge stub clock-enable).
- **Split-phase IRQ→kiln-wake in Renode:** the USART RX interrupt must reach the
  TCB handler and wake the task; verify Renode delivers the USART IRQ to the NVIC
  line the bridge binds.
- **Thin-seam ergonomics:** writing the USART register sequence in wasm is more
  work than calling embassy; acceptable for the spike (the point is the TCB
  measurement), revisit for the generator.
- **F100 value-line specifics** vs the F103 USART model — register offsets used
  must be the common subset (DR @ +0x04, SR @ +0x00, BRR @ +0x08, CR1 @ +0x0C).

## What this unblocks

With the seam proven and the granularity chosen by measurement, Jess's arbitrary
driver becomes driver #2 at the chosen seam — and two real drivers are the
duplication from which the deferred manifest+generator gets *extracted* rather
than guessed.

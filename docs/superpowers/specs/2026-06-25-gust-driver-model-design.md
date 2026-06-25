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

**Requirements source — gale#65 (gale-nano / gust origin).** This is not
speculative: gale#65 specifies exactly this model — *"only the bare-minimum
hardware (registers/MMIO) is native; everything above that is wasm"* — on the
**STM32F100 (Cortex-M3, 24 MHz, 8 KB SRAM)** px4io-class failsafe. The real first
driver is the **USART @ 1.5 Mbaud carrying CCSDS Space Packets wrapped by
relay-sec** (counter + anti-replay + Ascon-AEAD) from the FMU, on the ASIL/DAL-A
path. Jess co-designs the `gust:hal` WIT and brings the stm32f103-based Renode HIL.
So: the thin-seam UART here *is* that IPC carrier's foundation, and relay-sec /
relay-ccsds (both `no_std`, M3-capable) can themselves dissolve into the wasm —
maximal-wasm all the way up the stack, with only the register poke native. (Note:
the F100 value line has no LPUART; gale#65's "LPUART" maps to **USART1** here,
which reaches 1.5 Mbaud at fCK/16 on a 24 MHz clock and is what Renode models.)

## Goal & non-goals

**Goal (this spec):** define the `gust:hal` capability seam and prove it with
**one UART driver end-to-end on the STM32F100 (Jess's device class)**, with the
UART *protocol implemented in verified wasm*, built at **three seam granularities**
so we can **measure the SRAM and TCB footprint** each costs against the 8 KB
budget. The measurement — *how much verified wasm fits the tiny node* — is the
primary deliverable.

**Non-goals (deferred — YAGNI until there is real duplication to factor out):**
- The manifest + generator that auto-emits bridge stubs / build wiring /
  trampoline. Extract it *after* Jess's ask makes driver #2.
- The full peripheral set (gpio / spi / i2c / adc / timer). Only UART here.
- A larger-device / embassy-fits arm. The target is the F100 throughout; embassy
  appears only as the *fat-seam overflow check* on F100, to quantify why it
  doesn't fit. **XIP** (external-flash execute-in-place) is a documented
  follow-on lever, not in scope (see "Staying small").

## Key prior decisions (from brainstorming)

1. **embassy HAL-only; kiln stays the one executor.** embassy's executor is not
   adopted; embassy is (at most) a host-side peripheral backend.
2. **Two capability shapes:** *sync* caps for fast register touches; *split-phase*
   (start + poll/yield, kiln-wakeable) caps for interrupt-driven streaming. No
   embassy `Future` crosses the wasm boundary.
3. **Eventual wiring mechanism = manifest + generator**, but deferred.
4. **First bite = one UART driver on the F100 (Jess's device class); measure
   *both* SRAM (.bss/.data) and TCB bytes at thin/mid/fat seams.** SRAM is the
   binding constraint on an 8 KB part — the real question is how much
   verified-wasm logic fits while staying small.

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

## Verification — Verus + Rocq + Kani (earned, not claimed)

"Verified wasm" is earned by proofs, not by being in wasm — the same bar as the
gale kernel primitives (CLAUDE.md: Verus + Rocq + Kani simultaneously, written to
the intersection). For a driver:

- **The pure decisions are verified** like gale `_decide` functions. The first is
  the **USART RX decision** (`usart_rx_decide`): errors take priority over
  data-ready, so the driver provably never reads `DR` on an overrun/framing error
  (which would desync the byte stream). **Kani-proven now** (`cargo kani --harness
  rx_decide_error_priority`, over all 2³² SR values — VERIFICATION SUCCESSFUL).
  Its Verus + Rocq tracks attach on promotion of the decision into the gale
  verified crate (src/ Verus → verus-strip → plain/ → wasm; `proofs/*.v` Rocq).
- **Buffering reuses already-proven logic.** A UART RX buffer is a ring; the
  gale `msgq` ring is already Verus + Rocq + Kani proven (`src/msgq.rs`,
  `proofs/msgq_proofs.v`). The buffered driver composes that rather than carry an
  unverified buffer — maximal-wasm *and* maximal-verified, reusing existing proofs.
- **Only the MMIO poke is unverified** — the irreducible volatile I/O shell, which
  no formal track covers (and which is the entire TCB).

This is REQ-DRV-VERIFY-001 / VER-DRV-KANI in rivet. It is what makes the driver
fit gale#65's ASIL/DAL-A px4io path, not just "small."

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
- **Metrics (two — SRAM is the one that binds):** per seam — (a) **SRAM** =
  the linked image's `.bss + .data` (the dissolved wasm's linear memory / buffers
  / shadow stack + the bridge's state) against the 8 KB budget; (b) **TCB bytes**
  = `.text` of the trusted bridge (+ any linked embassy code); plus the
  dissolved-driver `.text` (flash, cheap). A table like the synth 0.15.0 one,
  with **SRAM as the headline column**.
- **Fat/embassy on F100 is the overflow check** — build-only, expected to blow
  the 8 KB SRAM budget (a full embassy-stm32 HAL is well over it). That negative
  result *is* a finding: it quantifies how far over the tiny node a fat seam goes,
  i.e. why the F100 *requires* the thin/mid seam. There is no larger-device arm —
  the point is staying on Jess's F100 class and maximising verified wasm within
  its SRAM.

## Staying small: maximal-wasm vs SRAM, per-node composition, and XIP

The driving constraint is **SRAM on an 8 KB part**, and the goal is to move as
much driver logic as possible into *verified wasm* without blowing it. Three
points shape the model:

- **Flash is cheap, SRAM binds.** Dissolved `.text` already executes from flash
  (128 KB on F100 — ample); it is the wasm **linear memory** (buffers, state) +
  shadow stack that consume **SRAM**. So "more logic in wasm" is nearly free in
  flash and costs SRAM *only where it needs data* (e.g. a UART RX ring buffer).
  Minimising the linear-memory footprint — static-sized to actual need, no heap,
  the synth#383 shadow-stack shrink — is what makes maximal-wasm fit.
- **Per-node bespoke composition (the core extension model).** Each node's image
  is one wasm composed with *exactly the specific drivers that node needs* (each
  driver = a component importing only the `gust:hal` capabilities it uses) plus
  the kernel pieces it uses — then meld-fused and dissolved. Nothing unused is
  linked, so the SRAM cost is precisely what that node's driver set requires. This
  is how "compose a new wasm from components for each" stays small *and* maximises
  verified surface: the composition, not a fixed firmware, is the product.
- **XIP — follow-on lever, addresses flash not SRAM.** Execute-in-place from
  **external** flash lets a library of composed per-node images live and run
  without consuming internal flash — useful once there are many nodes/drivers. It
  does **not** relieve the SRAM/buffer pressure that binds the 8 KB part, so it is
  orthogonal to the maximal-wasm/SRAM question this spike answers. Scoped out;
  revisit when image count or size makes internal flash the constraint.

## Success criteria

1. The UART driver passes the content-based correctness gate (TX/RX echo) on the
   F100 Renode target at the **thin** and **mid** seams.
2. A per-seam table — **SRAM (.bss/.data) vs the 8 KB budget** (headline), TCB
   bytes, and dissolved `.text` — committed alongside the spike. The empirical
   answer to *how much verified wasm fits the tiny node*.
3. A documented verdict: the seam granularity that maximises verified-wasm logic
   while staying within F100 SRAM, and by how much the fat/embassy seam overflows
   it.

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
- **embassy-stm32 may not support the F100 value line.** If it won't build for
  F100, the fat-seam overflow measurement is taken on the nearest supported F1
  (e.g. F103) as the embassy-footprint proxy — still a valid "this is how big the
  fat seam is vs 8 KB" finding; the thin/mid arms remain the real F100 targets.

## What this unblocks

With the seam proven and the granularity chosen by measurement, Jess's arbitrary
driver becomes driver #2 at the chosen seam — and two real drivers are the
duplication from which the deferred manifest+generator gets *extracted* rather
than guessed.

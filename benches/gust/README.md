# gust — a maximal-wasm mini-RTOS for tiny bare-metal nodes

> **gust** = the smallest gale (Beaufort force 7, 28 kt) — `gale` at its minimum, for the smallest node.

Target: **STM32F100 (Cortex-M3)** — the px4io-class failsafe node (jess #65 / REQ-PIX-009).

## What it is
A mini-RTOS whose **kernel is wasm** (scheduler + primitives compiled to the target via
the verified `meld→loom→synth` chain), with only a **~4-item native shim** as the trusted
computing base (TCB). The TCB boundary contract (gale#65):

| native shim (TCB) | wasm OS (verified) |
|---|---|
| vector table + reset · SysTick (time/fuel) · 5 MMIO imports (pwm/sbus/ipc/fatal) · wake-from-ISR | kiln-async scheduler + sem/mutex/msgq/timeout/event + failsafe app |

## Status (this is a bring-up, honestly scoped)
- ✅ **Boots on Cortex-M3** (qemu `lm3s6965evb`): `boot()` → SysTick-driven superloop → `poll()`
  runs the kiln-async scheduler + a fixed-point-mixer failsafe task; 5000 stable poll rounds.
- ⚠️ Scheduler is compiled **native thumbv7m** here — NOT yet through `meld→loom→synth`
  (the maximal-wasm version is the next integration; this proves the OS logic on-target).
- ⚠️ `kiln#338` (`no_alloc` gating `kiln-error/recovery.rs`) is **stubbed** with a Noop
  allocator to link; the failsafe path never allocates. Must land for a clean image.
- SysTick needs qemu `-icount` (instruction-driven clock) to advance; on the real F100
  (Renode) it ticks natively.

## Build & run
- **Local (qemu, zero-install):** `./run-qemu.sh`
- **Renode (STM32F100, cycle-accurate):** `ELF=target/thumbv7m-none-eabi/release/gust renode-test renode/gust_f100.robot`
  — boots on the real F100 model and reads `ExecutedInstructions` = the **fuel→cycles WCET calibration**.

## Roadmap
1. SysTick/time source on the F100 (Renode). 2. Renode cycle calibration in CI.
3. Dissolve the scheduler `wasm→loom→synth` (maximal-wasm). 4. Real MMIO (PWM/SBUS/IPC) + wake-from-ISR.

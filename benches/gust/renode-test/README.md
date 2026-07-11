# gust on Renode (hermetic, via pulseengine/renode-bazel-rules)

Boots the **dissolved** gust artifacts (wasm → loom → synth) on hermetic Renode
Cortex-M3 device classes — the on-target (real M3 ISA model) cycle-class signal,
**CI-reproducible with no board**, stronger than qemu's lm3s stand-in. Each
target checks in its **own** `.repl` (no dependency on the pinned Renode's
bundled platform set), matching this module's self-contained design.

```sh
cd benches/gust/renode-test
bazel test //:gust-renode //:gust-control-renode //:gust-f100-renode   # Linux
```

Self-contained module (own `MODULE.bazel`, `git_override` on
`pulseengine/renode-bazel-rules`) — it does **not** touch gale's root bazel or
the verification tracks.

## The three M3 device-class targets

| Target | Platform (`.repl`) | ELF | What it proves |
|---|---|---|---|
| `gust-renode` | generic M3 + 8 KB SRAM | dissolved gust **kernel** (`gust_wasm.elf`, 4256 B .bss) | the synth#383 .bss shrink fits 8 KB |
| `gust-control-renode` | M3 + 64 KB SRAM (STM32F103RE-class) | dissolved **engine_control** on the kiln stack (`gust_control.elf`, 9408 B .bss) | north-star rung 1 boots + runs; deterministic cycle count |
| `gust-f100-renode` | **STM32F100RB** (STM32VLDISCOVERY: 128 KB / 8 KB) | dissolved gust **kernel** | the real silicon board's exact memory class |

`gust_control` (9408 B .bss) needs the 64 KB class — it does **not** fit the
F100's 8 KB SRAM, so the F100 target runs the kernel and the 64 KB F103RE-class
target runs the control loop. This is the honest device-fit constraint.

## Status (honest)
- **What's asserted (verified locally on the same Renode engine):** each
  dissolved ELF loads into its SRAM and the M3 executes the scheduler loop for
  ≥1 s with no early fault; the deterministic executed-instruction count is
  logged (the fuel→cycles WCET seed; M3 has no cache → instr ≈ cycles). Measured
  (synth 0.15.0 control_step): `gust_control` = **0x1605A4 = 1,442,724 instr** over
  RunFor 2 s, no fault, SP at the 64 KB top (was 1,453,238 on synth 0.12.0 — the
  three applicable levers; local-promotion is gated by synth#474 on this function);
  the kernel ≈ 200 M instr on the 8 KB class.
- **CI-first:** the hermetic Renode is the **linux** portable, so these run in
  CI / on Linux, not on a macOS host (the macOS targets are `incompatible` and
  skip). First green is in CI.
- **Correctness vs cycle-count split:** `gust_control`'s spark/fuel correctness
  (== C/wasmtime) is gated by the **qemu** run (exit-code on match) and
  `gale_decider_diff`; Renode's unique contribution here is the **real-M3-model
  deterministic cycle number**. They are complementary gates, not redundant.
- **Semihosting-line assertion — confirmed NOT capturable on the macOS portable.**
  A `Wait For Line On Uart` on the `SemihostingUart` would let Renode assert the
  heartbeat *content* directly (not just no-fault). The macOS-portable
  `CreateFileBackend` on `cpu.uartSemihosting` captures **nothing** headless
  (reproduced 2026-06-24); and the repo's proven `Wait For Line` tests assert on
  a **real USART** (Zephyr console), not a `SemihostingUart`. So this assertion is
  deferred until **CI** (linux hermetic) confirms `SemihostingUart` → terminal-
  tester routing — adding it blind would risk a 120 s timeout → CI-red. Tracked
  as the follow-up below.
- **Follow-up:** (1) genrule the ELFs from the `.wasm` via synth@main +
  `--shadow-stack-size` (today the ELFs are checked-in fixtures); (2) once CI
  confirms `SemihostingUart` routing, add the heartbeat `Wait For Line` content
  assertion to `gust_control.robot`.
- The **functional** confirmation (5000 rounds, exact spark/fuel) is the qemu
  run; the **literal-silicon** confirmation is the G474RE DWT bench (`../silicon/`)
  and the STM32VLDISCOVERY reflash (`../REFLASH.md`). Renode adds the
  **real-M3-model + deterministic-cycle** dimension between them.

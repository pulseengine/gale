# gust on Renode (hermetic, via pulseengine/renode-bazel-rules)

Boots the **dissolved** gust kernel (wasm → loom → synth, `--shadow-stack-size`)
on a hermetic Renode **Cortex-M3 + 8 KB SRAM** — the on-target (real M3 ISA
model) confirmation of the synth#383 shrink, stronger than qemu's lm3s stand-in.

```sh
cd benches/gust/renode-test
bazel test //:gust-renode      # Linux (the hermetic Renode portable is linux)
```

Self-contained module (own `MODULE.bazel`, `git_override` on
`pulseengine/renode-bazel-rules`) — it does **not** touch gale's root bazel or
the verification tracks.

## Status (honest)
- **What's asserted:** the dissolved ELF loads into the 8 KB SRAM and the M3
  executes the scheduler loop for 2 s with no early fault; the deterministic
  executed-instruction count is logged (the fuel→cycles WCET seed). Verified
  locally on the same engine (≈200 M instr, no fault, SP at the 8 KB top).
- **CI-first:** the hermetic Renode is the **linux** portable, so this runs in
  CI / on Linux, not on a macOS host. First green is in CI.
- **Follow-ups:** (1) genrule the ELF from `gust_kernel.wasm` via synth@main +
  `--shadow-stack-size` (today the ELF is a checked-in fixture); (2) add a
  semihosting `Wait For Line` heartbeat assertion once CI confirms the
  `SemihostingUart` routing (the macOS-portable build couldn't capture it
  headlessly); (3) an `stm32vldiscovery` (real F100) platform variant.
- The **functional** confirmation (5000 rounds, `gust_mix=1500`) is the qemu
  run; this adds the **real-M3-model + deterministic-cycle** dimension. The
  **literal-silicon** confirmation is the STM32VLDISCOVERY reflash (`../REFLASH.md`).

# wave060 — Renode MULTI-CORE placement demo for the outer partition switch

Branch: `feat/gust-switch-renode-multicore` (off origin/main d4ee4e9).
Target: the one item VER-OS-SWITCH-001 still lists as REMAINING — "the
multi-CORE Renode placement demo (a second core / estimator partition wired
into the Renode model)".

## Step 0 — THE SPIKE (decides the scope)

Question: does Renode's Cortex-M3 enforce the ARMv7-M PMSA MPU (as qemu's
lm3s6965evb does, pre-proven by `mpu_spike.rs`)?

Spike: `benches/gust/src/bin/mpu_spike_renode.rs` (committed) — the Renode
counterpart of `mpu_spike.rs`: same three hand-programmed regions
(flash 256K / RAM-low 32K / RAM-stack 16K, PRIVDEFENA=0 deny-by-default),
same denied store into the ungranted 16K SRAM hole at 0x2000_8000. Renode
reporting is semihosting-free: every step + the verdict are magic words at
fixed SRAM addresses, read back headless from the monitor.

Run (Renode 1.16.1, `m3_64k.repl`, CPU.CortexM cortex-m3 + IRQControllers.NVIC
— the same models the committed platforms use). Verbatim mailbox readback:

```
==== spike mailbox ====
RESULT:
0xBADF0011
STEP:
0x51AE0003
DREGION:
0x00000008
REG_RBAR:
0x20008003
REG_RASR:
0x03000015
CFSR:
0x00000000
MMFAR:
0x00000000
PC:
0x5ea
```

**Verdict: Renode does NOT enforce the v7-M MPU.** RESULT=0xBADF0011 =
the denied store FELL THROUGH (no MemManage, CFSR/MMFAR untouched, execution
reached the fell-through epilogue). BUT the MPU is present as REGISTER STATE:
MPU_TYPE.DREGION reads 8, and RBAR/RASR read back exactly what was written
(REG_RBAR=0x20008003 = base 0x20008000 | architectural REGION field 3;
REG_RASR=0x03000015 = the written rasr(11,0b011)). Corroborating structure:
`help sysbus.cpu` exposes no MPU-related property, and Renode logs
`nvic: Changing value of the SHCSR register...` — the SCB/MPU registers live
in the NVIC peripheral model as inert state; tlib never sees them. So
enforcement is structurally absent, not a configuration miss.

**→ HONEST FALLBACK scope** (per the plan): the Renode demo demonstrates
(a) the 3-partition major frame advancing under the verified Switcher,
(b) region-swap-before-resume OBSERVED via RBAR readback per window
(register state is spike-proven readable), (c) multi-CORE placement
(second core running the estimator partition concurrently). Map-ENFORCEMENT
evidence remains the merged qemu probes (`gust_switch_probe.rs`,
`mpu_spike.rs`). Nothing is faked: cross-partition "denial" on Renode is
asserted only at the VERIFIED-QUERY level (`covers_addr`, labelled
`covers-denied-cross` in the output) and the robot/rivet text says exactly
that.

## Construction

- **Platform** `benches/gust/renode-test/gust_switch_2core.repl`: two
  CPU.CortexM cortex-m3 (`cpu0`/`cpu1`, cpuId 0/1), each with its OWN NVIC
  (per-cpu `Bus.BusPointRegistration` @0xE000E000), OWN private 256K flash
  @0x0 + 64K SRAM @0x2000_0000 (per-cpu registrations — both cores boot
  their own image at identical addresses, the honest model of per-core
  private partition memory), and OWN STM32_UART @0x4001_3800 (per-cpu
  `Bus.BusRangeRegistration`; `uart0`/`uart1`). SemihostingUart is NOT used
  (not capturable headless on all portables — see renode-test/README.md);
  each image reports over its real UART model instead.
- **Core-0 image** `benches/gust/src/bin/gust_switch_renode.rs`: the
  gust_switch_probe construction re-used — same MajorFrame ([0,1,2,0] over
  4 windows, frame.check()), same RegionTable built ONLY via the verified
  builder (flash RO / data RW / stack RW / per-partition 2K scratch, plus a
  5th grant: the partition's UART window — required for honest maps, since
  partitions report over MMIO here, not semihosting), same trusted seams
  (mpu_write per the platform contract; ctx_save/region_swap/ctx_resume
  stamped with monotonic sequence numbers; region_swap =
  `switch_to_partition`, no hand-programming), same RBAR-readback-at-resume
  (RNR:=3, mask !0x1F). Differences from the qemu probe, all spike-forced:
  no expect_denied/fault-skip machinery (no enforcement to observe — any
  MemManage/HardFault is a straight FAIL), cross-partition isolation
  asserted per window via the Verus-proven `covers_addr` query, UART prints
  instead of hprintln, quiet WFI loop instead of semihosting exit.
- **Core-1 image** `benches/gust/src/bin/gust_estimator_part.rs`: the
  estimator partition pinned to core 1 — builds its OWN table through the
  verified builder, programs the core-1 MPU through the verified
  `switch_to_partition`, confirms map-live via MPU_CTRL + RBAR readback,
  then runs a deterministic fixed-point first-order low-pass (Q8,
  est += (meas-est)>>3, 32 steps/period) with in-image monotone-error +
  final-convergence checks, emitting 8 heartbeat lines.
- **Robot gate** `benches/gust/renode-test/gust_switch_2core.robot`:
  mach create → LoadPlatformDescription → `sysbus LoadELF @… cpu=cpu0` /
  `cpu=cpu1` → one terminal tester per UART → `start` → 15 exact
  content assertions: core0 begin, 4 window lines, 4 switch lines (each
  with the incoming partition's exact scratch RBAR), core0 final OK line,
  core1 begin/map-live/hb1/hb8 (exact deterministic est values)/final OK.
- **BUILD.bazel**: `renode_test(name = "gust-switch-2core-renode", …)` with
  `variables_with_label = {ELF0, ELF1, REPL}` (the rule passes arbitrary
  variable names — verified against the pinned rules_renode defs.bzl).
  ELFs committed like the existing fixtures (`cargo build --release --bin
  gust_switch_renode --bin gust_estimator_part`, copied; SHAs verified
  identical to the tree copies).
- **CI**: `//:gust-switch-2core-renode` appended to the existing bazel test
  list in `.github/workflows/gust-renode.yml` (no new workflow).

### Debug finding worth keeping (cost ~20 min of wall-clock)

`Create Terminal Tester … defaultPauseEmulation=true` (the pattern the
single-core robots use) HANGS on this 2-cpu platform — the wait neither
matches nor times out (reproduced twice on Renode 1.16.1; renode spins at
~200% CPU). Plain testers + explicit `Execute Command start` + the same
waits pass in ~1 s. The committed robot carries a NOTE comment; the
single-core robots are untouched.

## Runs (verbatim tails + exit codes)

1. **New robot gate, locally** (macOS Renode 1.16.1 via `renode-test`, same
   engine/robot/repl/ELFs the bazel target wires):

```
+++++ Starting test 'gust_switch_2core.Verified partition switch places 3+1 partitions across two M3 cores'
+++++ Finished test 'gust_switch_2core.Verified partition switch places 3+1 partitions across two M3 cores' in 1.06 seconds with status OK
Suite .../gust_switch_2core.robot finished successfully in 1.18 seconds.
Tests finished successfully :)
```

   The UART content the gate asserts (captured raw in a prior manual run,
   `CreateFileBackend`, identical images):

```
--- uart0 ---
gust-switch-2core core0 begin dregion 8
core0 win0 P0 own-scratch-ok covers-denied-cross
core0 switch0 -> P1 seam-order-ok map-live rbar 0x20008800
core0 win1 P1 own-scratch-ok covers-denied-cross
core0 switch1 -> P2 seam-order-ok map-live rbar 0x20009000
core0 win2 P2 own-scratch-ok covers-denied-cross
core0 switch2 -> P0 seam-order-ok map-live rbar 0x20008000
core0 win3 P0 own-scratch-ok covers-denied-cross
core0 switch3 -> P0 seam-order-ok map-live rbar 0x20008000
gust-switch-2core core0 OK: frame wrapped P0->P1->P2->P0, 4 verified switches save->swap->resume, map-live-at-resume via RBAR readback 4/4, covers-query denies cross-partition (enforcement evidence: qemu gust_switch_probe)
--- uart1 ---
gust-switch-2core core1 estimator begin dregion 8
core1 estimator map-live rbar 0x20008000 mpu-ctrl-enabled
core1 estimator-hb 1 est 0x0003da0d
core1 estimator-hb 2 est 0x0003e7cb
core1 estimator-hb 3 est 0x0003e7f9
[hb 4..7 identical est 0x0003e7f9]
core1 estimator-hb 8 est 0x0003e7f9
gust-switch-2core core1 OK: estimator partition on its own core, map programmed via verified switch_to_partition (RBAR readback), 8 heartbeats, converged
```

2. **`bazel test //benches/gust/renode-test:gust-switch-2core-renode` locally:
   NOT RUNNABLE on this host** — macOS: rules_renode's hermetic portable is
   linux-only, so toolchain resolution fails for EVERY renode_test target
   (pre-existing, documented in renode-test/README.md "CI-first"). Verbatim:

```
ERROR: .../BUILD.bazel:26:12: While resolving toolchains for target //:gust-control-renode (bfb1d0c): No matching toolchains found for types:
  @@rules_renode+//renode:toolchain_type
```

   `bazel query //:gust-switch-2core-renode` parses/loads the new target
   fine. The bazel-level first-green is CI (as it was for all 8 existing
   targets); the robot/repl/ELF content the target runs was proven green
   locally in (1).

3. **Regression — the existing 3 M3 device-class robots, same local engine**
   (bazel not runnable, see above; robots run directly):

```
+++++ Finished test 'gust_renode.Dissolved gust boots and runs on Cortex-M3 8K' in 7.87 seconds with status OK
+++++ Finished test 'gust_control.Dissolved engine_control runs on the kiln stack (Cortex-M3 64K)' in 2.79 seconds with status OK
+++++ Finished test 'gust_f100.Dissolved gust kernel boots and runs on STM32F100RB (8K SRAM)' in 7.95 seconds with status OK
```

   (gust_uart.robot was also run green as the infra baseline, 1.25 s OK.)

4. **Regression — the qemu switch demo**:

```
$ cd benches/gust && cargo run --release --bin gust_switch_probe
gust-switch-probe OK: 3 partitions across the major frame (windows P0->P1->P2->P0, wrapped to window 0) — each confined to its time window AND its MPU map, region-swap-before-resume observed at every switch (seam order + RBAR readback), 5 expected cross-partition faults denied (CFSR DACCVIOL+MMARVALID) @ 0x20008810 0x20009010 0x20008010 0x20008010 0x20008810
EXIT=0
```

5. **`rivet validate`**: `Result: PASS (332 warnings)` (warnings
   pre-existing), exit 0.

6. **`cargo build --release --bins`** (whole gust bench crate): exit 0.

## rivet

**No status flipped** (fallback scope — the controller decides). Appended an
honest ADDENDUM (2026-07-17) to VER-OS-SWITCH-001's description after the
existing DELIVERED (2026-07-16) paragraph: what the Renode demo shows
((a) frame advance, (b) swap-before-resume via seam order + RBAR readback,
(c) multi-core placement, (d) covers_addr-level spatial correctness), the
spike verdict verbatim (RESULT=0xBADF0011, DREGION=8, registers-as-state),
and the explicit statement that map-ENFORCEMENT evidence remains the qemu
demonstrator. REQ-OS-SWITCH-001 untouched.

## Honest gaps

- **Renode contributes NO map-enforcement evidence** — its M3 does not
  implement MPU checks (spike-proven). Everything the robot asserts is
  window sequencing, seam ordering, register-state readback, verified-query
  spatial correctness, and concurrent per-core placement. Enforcement =
  qemu only (merged gust_switch_probe + mpu_spike).
- **The bazel target has not executed anywhere yet** — macOS cannot run
  renode_test targets at all (linux-only hermetic portable; pre-existing
  for all 9 targets). First bazel-level green is CI. The exact
  robot+repl+ELFs it wires were run green locally via renode-test.
- **Version skew**: local runs are Renode 1.16.1; CI pins the
  1.15.3+20250128 linux portable. Per-cpu registrations / `LoadELF cpu=` /
  dual-M-class platforms predate 1.15.3 upstream, but the pin has not been
  exercised against THIS platform until CI runs.
- The core-0 demo still plays all three of its partitions itself
  (ctx_save/ctx_resume remain recording stubs — no register-file
  save/restore), and ticks are software-driven; same honest scope as the
  merged qemu probe.
- `defaultPauseEmulation=true` + 2-cpu platform hangs the terminal tester
  (Renode 1.16.1, reproduced) — worked around with explicit `start`;
  upstream-friction candidate for the renode/rules_renode trackers.

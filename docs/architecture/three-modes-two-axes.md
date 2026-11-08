---
id: DOC-ARCH-MODES
title: "gale: one verified core, three modes, two axes (host × hardware)"
type: documentation
status: approved
tags: [architecture, modes, library-os, ports, hardware-model, byo-os]
links:
  - type: related-to
    target: SYSREQ-ARCH-MODES-001
  - type: related-to
    target: FIND-ARCH-AXES-001
  - type: related-to
    target: DOC-BYOOS-ARCH
---

# gale: one verified core, three modes, two axes

## Context

gale started as verified Rust replacements for Zephyr kernel primitives. The repo still describes
itself that way (`Cargo.toml`: "formally verified Zephyr kernel primitives"; `src/sem.rs`: "a
formally verified port of zephyr/kernel/sem.c"). That is no longer what gale is:

- **The verified core is already host-free.** Of the 55 files in `src/`, only `systick.rs` touches
  the CPU. The rest is decision logic (what a give, take, schedule or switch should do) with no
  dependency on Zephyr.
- **The OS exists, in the wrong place.** gust — partition switch, health monitor, scheduler, 29
  driver/provider/app crates, the `gust:hal` / `gust:os` / `gust:sched` WIT seams — lives under `benches/gust/`. It is
  referenced from 9 of 29 workflows and 8 rivet artifact files (30 path mentions).
- **The host seam exists, unnamed.** gust on bare metal runs on a small trusted native shim (see
  DOC-BYOOS-ARCH, `SAC-BYOOS-SCAFFOLD`): boot/reset, scheduler start, device init, MMIO, IRQ entry,
  context save/resume, region swap, MPU write, time.
- **A host run already exists, as a test.** `benches/gust/drivers/gustos-hostrun` executes the
  fused `gust:os` component on a host wasm engine (REQ-OS-COMPOSITE-EXEC-001). Its only residual
  import is `gust:hal/mmio`, answered by a **fake** register file, which is right for a
  falsification harness. A hosted port is the same seam with real devices and a real clock.
- **Zephyr is one host, used one way.** `ffi/` + `zephyr/gale_*.c` drop the decision functions into
  Zephyr's kernel. That is the "primitives" mode, and it is the only mode with a host other than
  bare metal today.

## Decision

gale is **one verified core used in three modes**, on **several hosts**. Only the port and the
trusted base change between them.

| Mode | What gale is | The host provides | Trusted base |
|---|---|---|---|
| **Primitives** | verified decision functions inside someone else's kernel | the kernel | core + shim + host kernel |
| **Library OS** | gust running on a host OS | threads/tasks, time, events, protection | core + OS + port + host kernel |
| **OS** | gust is the kernel | the CPU | core + OS + native shim |

| Host | Mode(s) | Notes |
|---|---|---|
| Bare metal Cortex-M | OS | today's gust; Renode + silicon evidence |
| Zephyr | Primitives (today); Library OS (new) | library-OS = gust partitions on `k_thread`/`k_timer` |
| FreeRTOS | Library OS | gust partitions as FreeRTOS tasks, gust's scheduler decides. **Not** primitives mode: we keep our own semantics (2026-11-07 decision), and FreeRTOS internals are not split into replaceable files the way Zephyr's are |
| Linux, macOS | Library OS (hosted) | **not a simulator** — see below |

### Two axes: host and hardware

The host OS and the hardware are **orthogonal**. G474 can run bare metal, Zephyr or FreeRTOS; Linux
can run on x86-64 or AArch64. So gale does not put chips inside host ports (which would describe the
same chip three times), and it does not model hosts as boards (FreeRTOS-on-G474 has no slot in that tree).

| Axis | Levels | Form | Example |
|---|---|---|---|
| **Host** | bare metal · Zephyr · FreeRTOS · Linux · macOS | **code**: `ports/host/<host>` | boot, exec, time, events, fatal |
| **Hardware** | architecture → processor → SoC → board | **architecture is code**: `ports/arch/<arch>`; **processor, SoC and board are data**: AADL → generated | armv7m: context switch + PMSAv7 MPU; armv8m: PMSAv8; riscv32: PMP |

A build is **core + gust + host port + arch port + target data**. The arch port is needed only where
gale owns the CPU (bare metal), or where the host leaves protection to us. Processor, SoC and board
never become code. They stay in the AADL target model that already generates the consts, `memory.x`
and `gust:hal` worlds, and that was validated on two silicon families (STM32G474, STM32F100).

**Hardware truth on RTOS hosts:** AADL stays authoritative for every fact gale relies on.
On a host with its own board support (Zephyr devicetree, FreeRTOS vendor config), a gate
**cross-checks** the AADL facts against the host's and fails on disagreement. gale does not generate
the host's config, and does not defer to it.

### Hosted is not simulated

Zephyr's `native_sim` runs Zephyr's own arch layer on POSIX with simulated time and hardware. gale
hosted is a real port:

- **time** is the host's monotonic clock; **execution** is real host threads (or processes);
- **I/O** is real: `gust:hal` backed by host devices (stdio/pty for uart, sockets, files);
- **isolation is enforced**: `mprotect` (or one process per tenant) replaces the MPU, at page
  granularity (4 KiB Linux, 16 KiB Apple silicon) instead of bytes;
- **tenants are the same wasm components**, run by a host wasm runtime, or dissolved to native code
  where synth targets the host (synth already compiles AArch64; how much of our flag set it covers is
  untested).

What does **not** transfer: timing. WCET and deadline results from a hosted run are not evidence for
a device (evidence-transfer). The trusted-base JSON (gale#467) grows a per-target `platform` block
that says so: which host kernel is trusted, and what protection the port provides.

## The platform seam

A host port implements one contract. It is a Rust module selected at compile time (cargo feature / cfg),
**outside** the verified code — the verified core keeps its no-trait-object, no-closure rule (CLAUDE.md).

| Capability | Bare metal Cortex-M | POSIX (Linux/macOS) | FreeRTOS | Zephyr (library OS) |
|---|---|---|---|---|
| boot | reset vector, init order | `main`, init order | `vTaskStartScheduler` hook | `SYS_INIT` |
| exec: run partition until yield/preempt; save/resume | `ctx-save` / `ctx-resume` | one thread per partition, hand-off so exactly one runs | one task per partition, notify hand-off | one `k_thread` per partition |
| time: now, resolution, arm deadline | SysTick / LPTIM | `CLOCK_MONOTONIC` + timer thread | tick + software timer | `k_uptime`, `k_timer` |
| events: interrupt → gale | IRQ entry | signal / eventfd | ISR → task notify | ISR → `k_poll_signal` |
| protect: grant / revoke region | the arch port's MPU (`mpu-write`, `region-swap`) | `mprotect` / process boundary | MPU wrappers where the port has them, else none | Zephyr memory domains |
| io | MMIO | host-backed `gust:hal` | vendor HAL | Zephyr device API |
| fatal | HM → halt/reset | HM → abort/restart partition | HM → task delete/restart | HM → `k_fatal` path |

Every port **declares** its protection level: `none`, `fault-containment` (a stray write is caught)
or `authority-containment` (tenant code cannot even ask). That declaration goes into the trusted-base
JSON, and a gate checks it against a matched-pair test on that port.

## Repository layout (target)

```
src/              gale core (verified decision logic)             — unchanged, docs generalised
os/               gust runtime: scheduler, partition switch, HM   — moved out of the gust bench tree
drivers/          drivers, providers, apps                        — moved out of the gust bench tree
wit/              gale:kernel, gust:hal, gust:os, gust:sched      — today split over 3 dirs
ports/host/
  baremetal/      boot, IRQ entry, fatal                          — extracted from gust
  zephyr/         primitives mode (today's ffi/ + zephyr/), later library OS
  freertos/       library OS                                      — new
  posix/          Linux + macOS                                   — new
ports/arch/
  armv7m/         context switch, PMSAv7 MPU (M3, M4, M7)         — extracted from gust
  armv8m/         PMSAv8 (M33)                                    — later
  riscv32/        PMP (ESP32-C3)                                  — later
targets/          AADL: cores, SoCs, boards → generated/          — moved (step 2a)
benches/          benchmarks only
proofs/ artifacts/ tools/ docs/                                   — unchanged
```

Crate description becomes: *"verified OS components — primitives, library OS, OS"*.

## Sequencing

A one-shot move would break 29 workflows, Bazel, path-bearing rivet artifacts (source-provenance,
markers) and the committed Renode fixtures at once, under a 2-PR CI ceiling. So:

1. **Record it.** This doc in `docs/architecture/`, plus rivet: a system-req for the three modes,
   sw-reqs for the seam contract and the per-port protection declaration, and a finding citing
   this context. Ships alone.
2. **Move directories first**, one PR per move, so everything after this step is written in its
   final home rather than moved twice. Each move is a row in the `moved-paths` gate's ledger
   (`cargo xtask check moved-paths`): any tracked, non-historical file still naming an old path
   fails, and so does a recreated old directory. The `.text` sections of every native gust binary
   are compared before and after each move, and a pure move leaves them identical. Order:
   `targets/` (2a), then `drivers/`, `wit/`, and the gust runtime to `os/`.
3. **Name the seams, change no behaviour.** Today the bare-metal shim is copy-pasted, not a module:
   `mmio_read32`/`mmio_write32` are defined in 19 gust binaries, a UART `tx` in 13, `mpu_write` in 9,
   and 48 carry their own semihosting exit. This step moves that into `ports/host/baremetal` (boot,
   exit, fault entry) and `ports/arch/armv7m` (MMIO, barriers, MPU writes), and the binaries call it.
   Oracle: build all native gust binaries before and after and compare their `.text` sections; every
   difference is either zero or explained. The Renode cycle gates must keep their recorded values
   (e.g. control `0x162CB6`). The committed synth objects come from the wasm side, which this step
   does not touch, so their staying byte-identical proves nothing here; they are not the oracle.
4. **POSIX port.** Start from `gustos-hostrun`'s seam: swap the fake register file for host-backed
   `gust:hal` devices and the host clock. The harness stays as it is, as the falsification test.
   First milestone: `gust_two_tenant` on Linux and macOS CI runners. Its matched pair: tenant A
   writes into B's memory → the host faults → gust's HM reaches the **same verdict** it reaches on
   Renode. With protection off, the write lands. This proves the seam is real, and it gives a fast
   loop with no SDK download.
5. **FreeRTOS port**, same matched pair, on QEMU first and then silicon. Adds the first
   AADL-vs-host hardware cross-check (FreeRTOS config ↔ AADL).
6. **Zephyr library-OS mode**, after FreeRTOS, with the devicetree ↔ AADL cross-check. Primitives
   mode on Zephyr stays as it is throughout.

## Not decided here

- Changing any semantics. Every mode keeps gale's semantics (2026-11-07).
- Timing claims for hosted ports. There are none.
- The Zephyr 4.5 port (#480), which stays port-only work below the OS work.

## Record

Decided by the project owner on 2026-11-08, choosing between options for each point:
- **structure:** two axes rather than a Zephyr-style tree, host-only ports, or a full package matrix;
- **hardware truth:** AADL authoritative and cross-checked, rather than generating host config or deferring to it;
- **names:** gale = the project and the verified core, gust = the OS;
- **order:** the decision record and the seam extraction go before #478 (decided as "steps 1–2"; the directory moves were inserted between them on 2026-11-08, after the owner asked not to forget them, so they are now steps 1–3).

Requirements: SYSREQ-ARCH-MODES-001 and the REQ-ARCH-* set derived from it; context in FIND-ARCH-AXES-001.

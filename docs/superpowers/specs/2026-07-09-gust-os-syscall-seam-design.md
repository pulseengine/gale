# gust-OS v0.4.0 — the `gust:os` syscall seam (design)

Status: approved scope (2026-07-09). Milestone: v0.4.0 on the unified line
(v0.1 sem → v0.2 mutex/DMA → v0.3 driver breadth → **v0.4 syscall seam**).
Requirement: `REQ-OS-SYSCALL-001` (artifacts/gust_os_roadmap.yaml).

## Goal

The OS surface an app sees is a single WIT world **`gust:os`** exposing capabilities
as typed interfaces, replacing today's ad-hoc per-demo import lists. An app
component is **portable across nodes** by importing only `gust:os`; the node's TCB
+ composed providers (kiln, gale::msgq, the dissolved drivers) satisfy it. No host
`Future` crosses the boundary (REQ-DRV-ASYNC-001 holds).

## Scope (approved: "core four + RTIO io")

One `gust:os` world, five interfaces:

| interface | capability | backed by | ABI shape |
|---|---|---|---|
| `time` | monotonic ticks + deadline math | timer-comp driver (gust:hal) | scalar (`u64`/`u32`) |
| `log` | line/byte output | uart-comp driver or semihosting bridge | `list<u8>` / `string` |
| `spawn` | task handle, cooperative | kiln executor | scalar handle + poll |
| `channel` | bounded message passing | gale::msgq ring | `own<msg>` / `list<u8>` |
| `io` | RTIO submit/complete | composed iodevs (spi/dma) | `submit(sqe)` / `poll-completion() → cqe` over `own<buffer>` |

The `io` interface takes the io_uring/Zephyr-RTIO SQE→CQE shape (FIND-DRV-RTIO-001),
reusing the `own<buffer>` handoff from gust:hal/dma (dma-own's proven ownership FSM).

## Key design decision: bounded shared linmem (NOT 0-SRAM)

The leaf drivers are all-scalar (`u32↔u32`) → the CM canonical ABI stays flat, 0
SRAM. **The syscall seam is different**: `log`/`channel`/`io` carry *buffers*, so
the canonical ABI needs a real linear memory (realloc + a working segment). This is
expected and correct for an OS layer:

- The meld-fused node has **one shared linear memory** (app + providers share it,
  via `meld fuse --memory shared`), sized to a **declared budget that fits the F100
  8 KiB SRAM** (synth `--native-pointer-abi` + `#383` shadow-stack budget flag; see
  project_synth_015_perf_win VCR-MEM-001).
- Payload buffers are `own<buffer>` in that shared segment — ownership-tracked
  exactly like dma-own, so no aliasing/leak; the io path reuses the DMA handoff.
- The property that replaces "0 SRAM" here is **bounded SRAM**: the whole node
  (app working set + shim) fits the 8 KiB budget, asserted + measured, not 0.

So v0.4.0's SRAM story is a *budget* (fits 8 KiB), not zero — a deliberate,
honest shift from the leaf-driver 0-SRAM claim.

## Build pipeline (the proven dissolve path, one layer up)

1. Author `drivers/wit/gust-os.wit` (`package gust:os@0.1.0`; the 5 interfaces +
   a `node` world the app imports and the providers export). Reuse gust:hal types
   (`dma-buffer` resource) where the io path needs them.
2. wit-bindgen the app against `import gust:os` (guest); wit-bindgen the providers
   (kiln/msgq/driver components) to `export gust:os`.
3. `wac compose` app + providers → one composite; `meld fuse --memory shared`
   (bounded linmem) → `loom optimize --passes inline` → `synth compile --target
   cortex-m3 --all-exports --relocatable --native-pointer-abi <budget>` → one node
   `.o`.
4. `gust_syscall` demo bin: a real app that only imports `gust:os` (logs, spawns a
   task, sends on a channel, submits an io) — link the node `.o`, run on Renode.
5. **Local qemu probe FIRST** (RAM-backed bridge, like gust_breadth_probe), then
   Renode content-gate `gust-syscall-renode` + gust-renode.yml.

## Verification

- Kani on the decision cores: channel ring (bounded, no lost/dup msg — reuse
  gale::msgq proofs), spawn state machine, io SQE→CQE lifecycle (reuse dma-own FSM).
- Oracle gate: local qemu probe → Renode gate (app runs purely on gust:os) → `nm`
  TCB-atom count (must stay the 4-item shim + kiln, no new trusted atoms) → SRAM
  budget check (fits 8 KiB) → byte-identical dissolve.
- rivet: `VER-OS-SYSCALL-001` verifies `REQ-OS-SYSCALL-001`; add per-capability
  sub-reqs if the single req proves too coarse for the trace.

## Milestone shape (multi-tick)

This is a v0.3-breadth-sized effort. Rough tick plan: (1) WIT world + one interface
end-to-end (`time`, all-scalar, proves the gust:os compose) → (2) `log` + `channel`
(first buffers → shared-linmem budget wired) → (3) `spawn` (kiln) → (4) `io` (RTIO
over own<buffer>, reuse dma-own) → (5) `gust_syscall` demo + gates + rivet → cut
v0.4.0. Each tick is a green PR; the seam grows interface-by-interface.

## Anti-goals (deferred)

- MPU per-component isolation → v0.5.0 (gated on synth#404).
- Multi-node scheduling / IPC across nodes → beyond v0.4.0.

---
id: gale-release-plan
title: gale release plan
---
# gale release plan (rivet-driven)

Releases are scoped **in rivet** via the first-class **`release:` field** on the
artifacts in scope (rivet ≥ 0.21; the feature that resolved pulseengine/rivet#512 —
gale's earlier `release-vX.Y.Z` tag was the workaround and has been migrated to the
field).

**Readiness is a query, not an opinion.** A release is cuttable when every
requirement in its scope is verified and its V is closed (`rivet validate` green for
the scope).

## Readiness burn-down (run anytime)
```sh
rivet release status v0.1.0          # per-status burn-down + the not-yet-verified set
rivet list --release v0.1.0          # the full scoped set
```
`rivet release status` exits non-zero when not cuttable, so CI can gate on it.

> **Resolved (pulseengine/rivet#612, shipped rivet 0.23):** `release status`
> readiness is now configurable. gale sets `release: { require: coverage }` in
> `rivet.yaml` — an artifact is release-ready when its `validate` coverage rules
> pass (the V is closed), not when a status string flips. This is the correct mode
> for an ASPICE V-model project (verification is expressed via links, terminal req
> status stays `approved`), so `rivet release status` now gives a **meaningful**
> cuttability verdict for gale.

## v0.1.0 — semaphore (depth-first)
Scope = the semaphore primitive, taken end-to-end to verified before the next
primitive starts. Scoped via `release: v0.1.0`:

- **Requirements (11):** `SWREQ-SEM-P01..P10` (the P1–P10 invariants/behaviours) + `SYSREQ-SEM-001`.
- **Verification (V closed, per ASPICE):** the reqs P01–P10 are **formally
  verified** by `FV-SEM-001` (the schema-legal req-level verifier); the design
  (`SWDD-SEM-*`) is unit-verified (`UV-SEM-001..005`); the arch (`SWARCH-SEM-001`,
  `SWARCH-WQ-001`) is integration-verified (`IV-SEM-001/002`); `SYSREQ-SEM-001` is
  system-verified (`SV-SEM-001`). Plus the silicon result (`k_sem_give` 907 cyc,
  wasm-cross-LTO vs 471 LLVM-LTO).

**Status: CUTTABLE.** `rivet release status v0.1.0` (rivet 0.23,
`require: coverage`) = ✓ Cuttable, `rivet validate` = PASS. Note (2026-07-02): the
earlier "not cuttable / links missing" reading was a mis-modeled coverage rule —
unit/integration verifications attach to design/arch (ASPICE forbids them verifying
a req directly), so a req is verified directly only by `sw-verification` (formal).
`swe1-has-verification` was corrected to `from-types: [sw-verification]`
accordingly; the V was already closed transitively. Cutting the tag is a release
decision, not a traceability blocker.

## Cadence
Per-primitive depth-first: v0.1 sem → v0.2 mutex → … Each release ships when its
scope's V is closed; unassigned primitives are backlog, not commitments.

## The gust-OS track — one unified line (2026-07-08)

The versioned line carries **two kinds of scope on a single track** (decided
2026-07-08): the **gale verified primitives** (sem/mutex/msgq/… — the *supply
chain*) and the **gust operating system** they compose into (the *product*). gust
is not a separate version series; its milestones interleave into the same `vX.Y.Z`
line, scoped in `artifacts/gust_os_roadmap.yaml`.

**North-star:** a general **multi-tenant** verified OS — hosting mutually-distrusting
components, isolated by MPU, over the same 4-item Rust TCB shim — reached by
*breadth first*. What boots today (on `main`) is the v0.2 composition: app + kiln +
gale primitives dissolved to one native `.o`, `uart-thin` (254 B) + `dma-own`
(218 B) drivers, a ~77-line + 5-instruction + 10-line TCB, on qemu M3 / Renode F100 /
F100+G474RE silicon. The ladder from there:

| release | milestone | scope (rivet) | gate to cut |
|---|---|---|---|
| **v0.3.0** | **driver breadth** (next) | `REQ-DRV-{GPIO,TIMER,SPI,BREADTH}-001` + `VER-DRV-*` | GPIO+timer+SPI as verified thin-seam drivers, each Kani-proven + Renode content-gated; **zero new TCB atoms**; the 4-driver node fits F100 8 KiB |
| **v0.4.0** | **capability/syscall seam** | `REQ-OS-SYSCALL-001` (`gust:os` world) | apps import one typed `gust:os` world (time/log/spawn/channel) instead of ad-hoc imports; app portable across nodes |
| **v0.5.0** | **isolation / multi-tenancy** | `REQ-OS-{MPU,MULTITENANT}-001` | two mutually-distrusting components, MPU-per-region (unblocks **synth#404** multi-memory); a faulting tenant cannot corrupt a sibling or the TCB |
| **v1.0.0** | **the OS, cut** | `REQ-OS-RELEASE-001` | whole composition — scheduler + IPC + `gust:os` + GPIO/timer/SPI/UART/DMA + MPU multi-tenancy — sigil-signed, booting the SAME components on M3 **and** M4 silicon, published as a Pages showcase |

**Readiness is the `rivet release status` burn-down, not this table.** The table is
the *scope map*; whether a milestone is cuttable is a query. Live snapshot
(2026-07-08, `rivet release status`, `require: coverage`):

```
v0.1.0  ✓ Cuttable            (11 artifacts, V closed)         — semaphore, done
v0.2.0  ✗ NOT cuttable (3)    DD-DMA-ENFORCE/REGION, REQ-DMA-ASYNC — DMA, in progress
v0.3.0  ✗ NOT cuttable (4)    REQ-DRV-{GPIO,TIMER,SPI,BREADTH}     — driver breadth, not started
v0.4.0  ✗ NOT cuttable (1)    REQ-OS-SYSCALL                       — syscall seam
v0.5.0  ✗ NOT cuttable (2)    REQ-OS-{MPU,MULTITENANT}             — isolation
v1.0.0  ✗ NOT cuttable (1)    REQ-OS-RELEASE                       — the OS cut
```

A planned milestone deliberately carries **no verification artifacts** — under
`require: coverage` a req reads "ready" the instant a `verifies` link exists, so
pre-declaring the Kani verifiers would make v0.3.0 falsely report near-cuttable
while nothing is built. Each `VER-DRV-*` is added by the feature loop **when its
proof passes**; that incoming link is what drops the req off the not-ready list.
Re-target scope with `rivet release move <artifact> <version>` (a logged decision).

**Verification bar (every driver/component, unchanged from the driver model):**
protocol/decision logic is verified wasm (Kani on the pure decision; Verus/Rocq when
promoted into `gale/src`), dissolved to native; the TCB stays the irreducible
mmio+irq(+dma-barrier) sliver. **Oracle gate per milestone:** Renode F100 content
test (byte-exact I/O) + silicon boot + `nm` TCB-atom count + SRAM-budget check +
byte-identical dissolve — all mechanical, per `oracle-gate-a-change`.

**The perf track (0.7×) is orthogonal.** synth proof-carrying specialization
(synth#494, the 0.45× floor) makes the dissolved OS *faster*; it is not on the
OS-completeness critical path and does not gate any milestone above.

**Immediate next step (v0.3.0, driver breadth):** start with **GPIO** — the smallest
verified thin-seam driver — as the pattern-setter: `drivers/gpio-thin/` (wasm pin
logic + Kani `pin-config` proof), a Renode F100 read-back content-gate, and a
COMPARE.md row (dissolved size + confirming 0 new TCB atoms). Then timer, then SPI
(reusing `dma-own`). Ship v0.3.0 when all four `VER-DRV-*` are green.

## Wasm-module distribution (added 2026-06-11)

Each release ships the wasm-cross-LTO artifacts per `docs/wasm-module-distribution.md`:
the dissolved `.wasm` (verification artifact), per-target `.o`, and the sha256+toolchain
manifest (sigil-signed once the signing flow lands). v0.1.0 ships the **sem** module;
consumption is `CONFIG_GALE_WASM_LTO_SEM=y` + `-DGALE_WASM_LTO_OBJ_DIR=<assets>`.
Measured: the released object passes the kernel semaphore suite (mps2/an385 qemu) —
functional equivalence with the native-FFI build. Release-readiness for the wasm lane =
`release-wasm.yml` green + the manifest's falsifiable claims verified (see the
distribution doc §Falsifiable claims).

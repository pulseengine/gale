# gust AADL Target Model — Design Spec

**Status:** approved design, pending spec review
**Date:** 2026-07-22
**Topic:** Board / SoC / device modeling for gust, driven by spar/AADL, with a
build-time generator that bakes per-target constants into the dissolved objects.

## Problem

gust's verified drivers are already target-agnostic at the register level — the
peripheral base is a *parameter* (`wdg_configure(base, …)`, `wdg_start(base, …)`),
so the proven FSM does not change per target. What is **not** modeled is
everything *around* the base: the peripheral's address, the memory map, which
peripherals a given part even has, and the clock/reset tree. Today that knowledge
is scattered as magic constants inside each silicon firmware
(`RCC_CSR = 0x40021094 // G4; F1 is 0x40021024`) plus per-board `memory-*.x`
linker scripts.

That scatter is what blocks clean target swap/extend. To validate a driver on a
second board, or to add a new part (F103-class for bxCAN, or the NXP i.MX RT of
the Pixhawk 6X-RT), a human hand-writes new constants in new firmware bins. This
is the exact problem Zephyr's devicetree solves — board → SoC → CPU layering, with
peripheral nodes carrying `reg` addresses and a presence list.

**Concrete motivating case:** the F100 (VLDISCOVERY) has no bxCAN; an F103-class
part does. "Can I silicon-validate CAN?" is really "does this part have that
peripheral?" — a fact a board/SoC model captures and scattered constants cannot.

## Thesis constraint (why not just adopt Zephyr devicetree)

gust's whole premise is dissolve-to-native, minimal TCB, 0-SRAM. Any target model
**must stay build-time**: a declarative description that bakes constants into the
dissolved `.o` at compile time, adding zero runtime code and zero TCB atoms.
Zephyr's *pattern* (declarative HW → generated constants) is right; importing its
runtime devicetree machinery would betray the thesis. We therefore build the model
in gale's own verified toolchain (spar/AADL + rivet), not by pulling in an external
DT runtime.

## Decisions (locked)

1. **Target model lives in spar/AADL** as the single source of truth. spar parses
   and analyzes it (EMV2 fault propagation is the traceability payoff).
2. **A gale-side build-time generator** reads `spar items --format json` and emits
   the concrete artifacts. spar has **no codegen** — it parses/analyzes only — so
   the model→artifact bridge is owned by gale, not by an upstream spar `generate`
   command. This keeps each tool single-responsibility and adds no upstream
   dependency.
3. **First increment = full-V slice for one device (IWDG)** across **both** live
   boards (G474RE / G4 and F100 / F1), proving retarget-by-model-swap across the
   F1/G4 constant gap.
4. **Generated artifacts are committed** with a `GENERATED — DO NOT EDIT` header; a
   CI **drift oracle** re-runs spar + generator and asserts a zero diff. Normal
   builds stay spar-free; generation is auditable.
5. Extension to F103/CAN and i.MX RT is the **next** increment, not v1.

## Feasibility (verified 2026-07-22, not assumed)

- `spar parse` accepts hardware AADL: `processor` / `memory` / `device` / `system`
  categories, `extends` inheritance, and `system implementation` subcomponents all
  return `OK`.
- `spar items --format json` exposes each component's `category`, `extends` chain,
  and `properties` with a `typed_value` (integer/boolean) — everything a generator
  needs.
- **spar bug (filed as friction):** C-style `0x40003000` silently mis-parses to
  `0` (hex digits land in a string field). AADL-native `16#40003000#` parses
  correctly (→ `1073754112`). **Bases are written base-16 AADL-style.**

## Architecture

### Layer 1 — AADL source of truth (`benches/gust/targets/`)

Files:
- `gust_target_props.aadl` — property set: `Base`, `Length`, `Present`, plus
  register-detail properties (`Csr_Offset`, `Iwdgrstf_Bit`, `Rmvf_Bit`, IWDG
  key values / offsets).
- `cortex_m.aadl` — shared processor cores (`CortexM3`, `CortexM4` extends…).
- `stm32f100.aadl`, `stm32g474.aadl` — per-part SoC + board `system implementation`
  binding a processor + memories + device instances.

Each `device IWDG` carries `Base => 16#40003000#` (universal STM32 IWDG base) and
the part-specific `RCC_CSR` facts as properties (F1: offset `16#24#`, `Rmvf_Bit =>
24`; G4: offset `16#94#`, `Rmvf_Bit => 23`). The `memory` components carry flash /
sram `Base` + `Length`.

**EMV2 annex** on the board model expresses the safety story the IWDG mitigates:
`task-hang (no refresh) → watchdog timeout → system reset (safe state)`. This is
the per-device fault model rivet traces to a hazard.

**Oracle (this layer):** `spar parse` clean **and** `spar analyze --root
<Board>.impl` (EMV2) green.

### Layer 2 — gale-side generator (`tools/gust-target-gen/`)

A small Rust tool (no runtime footprint — it runs at authoring/CI time only). Given
one or more target `.aadl` files it:
1. shells `spar items --format json <files>`,
2. walks the JSON: for the named board `system implementation`, resolves its
   device + memory subcomponents and their (inherited) properties,
3. emits, per target `<board>`:
   - `gust_target_<board>.rs` — `pub const` module (IWDG_BASE, RCC_CSR, IWDGRSTF,
     RMVF, flash/sram bases + lengths). Replaces the hand-scattered magic numbers.
   - `memory-<board>.x` — linker script from the `memory` components.
   - a per-target **gust:hal WIT world** that binds the generic device driver
     import to this board's device instance (the swap mechanism).

**Oracle (this layer):** a generator unit test — a fixed AADL/JSON fixture in,
expected consts + memory.x out (golden-file compare).

### Layer 3 — rivet traceability

- `REQ-TARGET-MODEL-001` — the AADL target model is the single source of
  board/SoC/device facts; firmware and linker scripts derive from it, not from
  hand-written constants.
- `VER-TARGET-IWDG-001` — the generated IWDG binding for a board equals the
  validated constants and the driver still silicon-confirms on that board.
- Trace topology: AADL `device IWDG` → generated consts → `wdg-thin` driver →
  silicon result. EMV2 fault node ↔ rivet hazard (the task-hang→reset story).
- **Oracle:** `rivet validate` + `rivet check` + `rivet coverage` green over the
  new artifacts.

### Layer 4 — code + the keystone oracle + silicon

- **Retarget `gust_wdg_silicon`** to `include!` / consume the **generated**
  `gust_target_<board>.rs` consts, deleting the hand-written `IWDG_BASE` /
  `RCC_CSR` / `RMVF` constants. `memory-<board>.x` becomes the generated file.
- **Keystone parity oracle:** the generated values for G474RE and F100 equal
  today's hand-written values (checked in the generator golden test **and** by the
  firmware building byte-identically where applicable), *and* the watchdog still
  fires on silicon. This folds the safe "parity retrofit" into the full-V slice: we
  prove the pipeline while beginning to trust it.
- **Silicon confirmation:** G474RE via direct probe-rs (already validated); F100 via
  the proven Pi + openocd `stlink-hla.cfg` recipe. Same driver, same AADL device,
  two boards, swap-only.

### Layers assessed, not gold-plated (v1)

- **witness (MC/DC):** the target-binding introduces no new decision branch over
  the already-covered driver FSM. v1 assesses the truth table; if genuinely no new
  gap rows, that is stated explicitly (not silently skipped) and recorded — per the
  methodology's recurring-N/A caution.
- **sigil (attestation):** the generator is a new build stage. v1 assesses signing
  the generated artifacts; if deferred, it is recorded as a named follow-on, not
  dropped.

## Repo layout

```
benches/gust/targets/
  gust_target_props.aadl
  cortex_m.aadl
  stm32f100.aadl
  stm32g474.aadl
  generated/                      # committed, GENERATED — DO NOT EDIT
    gust_target_stm32f100.rs
    gust_target_stm32g474.rs
    memory-f100.x                 # supersedes the hand-written silicon/memory-*.x
    memory-g474re.x
    world-stm32f100.wit
    world-stm32g474.wit
tools/gust-target-gen/            # the gale-side generator (+ unit tests)
  src/main.rs
  tests/golden/
```

A `regen` entrypoint (Makefile target or `cargo run -p gust-target-gen`) runs spar
+ generator; CI asserts `git diff --exit-code` over `targets/generated/` (drift
oracle) and runs `spar parse` + `spar analyze` on the models.

## Non-goals (v1)

- No F103 / CAN, no i.MX RT — next increment.
- No runtime devicetree, no dynamic HW discovery — everything build-time.
- No clock-tree computation / no PLL modeling — only the facts drivers need
  (bases, memory map, the RCC_CSR reset-flag facts, peripheral presence).
- No re-authoring of the driver FSMs — they are unchanged; only their constant
  bindings move to generated code.

## Oracle summary (kill-criteria)

| Claim | Mechanical oracle | Kill-criterion |
|---|---|---|
| AADL model is well-formed + fault-analyzable | `spar parse` + `spar analyze` (EMV2) | spar reports a diagnostic / analysis fails |
| Generator is correct | golden unit test (JSON → consts/memory.x) | generated output ≠ golden fixture |
| Generated == validated constants | parity compare in generator test + firmware build | a generated const ≠ the hand-written value it replaces |
| No stale committed artifacts | CI `git diff --exit-code` over `generated/` | regen produces a diff |
| Driver still works on real HW | silicon run (G474RE probe-rs, F100 Pi/openocd) | watchdog does not fire / `IWDGRSTF` not set |
| Traceability closed | `rivet validate` + `check` + `coverage` | an uncovered predicate / broken link |

## Friction filed (spar repo)

1. **spar#337** — `0x…` hex integer literal silently mis-parses to `0` (works only
   with `16#…#`). Workaround adopted: base-16 AADL literals.
2. **spar#338** — no codegen path from an analyzed model to concrete artifacts
   (constants / linker map / WIT binding); motivates the gale-side generator and
   asks whether `items --json` is the intended downstream contract.

## Risks / open items

- **spar availability in CI** for the drift/analyze gate — the generator itself is
  not on the normal build path (artifacts are committed), so only the drift gate
  needs spar. Confirm spar is installable in the relevant CI job.
- **WIT world generation shape** — the driver's mmio WIT seam already exists; what
  is generated per-target is the *binding/world* that selects the board's device
  instance, not a new interface. The plan must pin the exact generated world syntax
  against the existing gust:hal WIT.
- **memory-*.x supersession** — the flash scripts (`run-wdg.sh`, `run.sh`) and
  build wiring currently reference `silicon/memory-*.x`; the plan repoints them at
  `targets/generated/memory-*.x` and removes the hand-written copies in the same
  task to avoid two sources of truth.

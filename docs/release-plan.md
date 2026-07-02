---
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

## Wasm-module distribution (added 2026-06-11)

Each release ships the wasm-cross-LTO artifacts per `docs/wasm-module-distribution.md`:
the dissolved `.wasm` (verification artifact), per-target `.o`, and the sha256+toolchain
manifest (sigil-signed once the signing flow lands). v0.1.0 ships the **sem** module;
consumption is `CONFIG_GALE_WASM_LTO_SEM=y` + `-DGALE_WASM_LTO_OBJ_DIR=<assets>`.
Measured: the released object passes the kernel semaphore suite (mps2/an385 qemu) —
functional equivalence with the native-FFI build. Release-readiness for the wasm lane =
`release-wasm.yml` green + the manifest's falsifiable claims verified (see the
distribution doc §Falsifiable claims).

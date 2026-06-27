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

> **Caveat (pulseengine/rivet#612):** `release status` currently judges cuttability
> by `status ∈ {verified, accepted}` only. gale is an ASPICE V-model project — it
> marks an artifact verified via `verified-by`/`verified-on` **links** (checked by
> `rivet validate` coverage), not a status value, and its terminal req status is
> `approved`. So the burn-down **verdict** reads "not cuttable" until #612 makes it
> link/schema-aware; the **counts** are accurate, and the real V-gate is
> `rivet validate`. CI runs the burn-down as an advisory step (compliance.yml).

## v0.1.0 — semaphore (depth-first)
Scope = the semaphore primitive, taken end-to-end to verified before the next
primitive starts. Scoped via `release: v0.1.0`:

- **Requirements (11):** `SWREQ-SEM-P01..P10` (the P1–P10 invariants/behaviours) + `SYSREQ-SEM-001`.
- **Evidence already authored (not yet linked):** 5 detail-designs (`SWDD-SEM-*`),
  1 arch (`SWARCH-SEM-001`), and **16 verifications** — `UV-SEM-001..005` (unit),
  `IV-SEM-001/002` (integration), `SV-SEM-001` (system), `FV-SEM-001..003` +
  `FV-WQ/PRI/THR/ERR-001` (formal), plus the silicon result (`k_sem_give` 907 cyc,
  wasm-cross-LTO vs 471 LLVM-LTO).

**Status: NOT cuttable yet.** All 11 reqs are `approved`, none `verified`. The
gap is *linkage, not work*: the 16 sem verifications exist but aren't wired to the
reqs via `verifies` links, so `rivet validate` reports the reqs unverified (part
of the 311 lifecycle-coverage gaps). **v0.1 burn-down = wire the sem
verifications → reqs, then drive status `approved → verified`** (a
`traceability-audit` pass), after which v0.1 is cuttable.

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

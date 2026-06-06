---
title: gale release plan
---
# gale release plan (rivet-driven)

Releases are scoped **in rivet** via a `release-vX.Y.Z` **tag** on the requirement
artifacts in scope (rivet 0.15.0's `sw-req`/`system-req` schemas have no native
`release:` field — see pulseengine/rivet issue on this; tags are the working
mechanism and are queryable via `(has-tag …)`).

**Readiness is a query, not an opinion.** A release is cuttable when every
requirement in its scope is `verified`/`accepted` and its V is closed
(`rivet validate` green for the scope).

## Readiness burn-down (run anytime)
```sh
REL=release-v0.1.0
total=$(rivet list --filter "(has-tag \"$REL\")" --format json | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')
done=$(rivet list  --filter "(and (has-tag \"$REL\") (or (= status \"verified\") (= status \"accepted\")))" --format json | python3 -c 'import json,sys;print(json.load(sys.stdin)["count"])')
echo "$REL: $done / $total verified"
```

## v0.1.0 — semaphore (depth-first)
Scope = the semaphore primitive, taken end-to-end to `verified` before the next
primitive starts. Tagged `release-v0.1.0`:

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

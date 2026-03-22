# STPA Analysis — Gale Kernel Replacement

**Date:** 2026-03-22
**Scope:** Systems-Theoretic Process Analysis for ASIL-D safety case

See full analysis in conversation history. Summary of critical gaps below.

## Critical Gaps (ASIL-D Blocking)

| Gap | Risk | Status |
|-----|------|--------|
| GAP-1: No FFI layout verification | Struct mismatch silently corrupts decisions | **OPEN** |
| GAP-2: Model-implementation divergence | Verus proofs don't cover FFI code | **OPEN** |
| GAP-3: SMP mutex unlock race | lock_count modified without spinlock | **OPEN** |

## High Gaps

| Gap | Risk | Status |
|-----|------|--------|
| GAP-4: No panic-freedom proof for FFI | Corrupted input → kernel abort | **OPEN** |
| GAP-5: Multi-arch coverage | Only M3 tested | **PARTIAL** (M4F/M33/R5 via Renode) |
| GAP-6: Error code sync | Manual convention, no automated check | **OPEN** |
| GAP-7: Pipe model/ring_buf consistency | Count divergence possible | **OPEN** |

## Medium Gaps

| Gap | Risk | Status |
|-----|------|--------|
| GAP-8: ISR blocking protection | Debug-only assert | **OPEN** |
| GAP-9: Wait queue model bounded at 64 | Exceeding limit unmodeled | **OPEN** |
| GAP-10: 64-bit pointer truncation | ptrdiff_t → uint32_t | **OPEN** |

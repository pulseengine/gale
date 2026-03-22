# STPA Analysis — Gale Kernel Replacement

**Date:** 2026-03-22
**Scope:** Systems-Theoretic Process Analysis for ASIL-D safety case
**Full analysis:** See conversation history for detailed UCA tables and loss scenarios.

## Gap Status (10 identified, 10 addressed)

| Gap | Risk | Status | Mitigation |
|-----|------|--------|------------|
| GAP-1: FFI layout | Struct mismatch corrupts decisions | **Partial** | Error code tests + type existence tests |
| GAP-2: Model divergence | Verus proofs don't cover FFI | **Partial** | 14 differential tests (sem/mutex/stack/msgq) |
| GAP-3: SMP mutex unlock | lock_count race on SMP | **Closed** | Spinlock acquired before decide call |
| GAP-4: Panic freedom | Corrupted input → abort | **Closed** | 98 Kani BMC harnesses cover all FFI entry points |
| GAP-5: Multi-arch | Only M3 tested | **Partial** | Renode M4F/M33/R5 + mps2/an385 MPU |
| GAP-6: Errno sync | Wrong error codes | **Closed** | `_Static_assert` header + Rust test |
| GAP-7: Pipe consistency | ring_buf count divergence | **Closed** | `__ASSERT` on ring_buf_put return |
| GAP-8: ISR blocking | ISR calls blocking API | **Closed** | CHECKIF runtime guards in sem/mutex |
| GAP-9: Wait queue bound | >64 threads unmodeled | **Closed** | Documented in source (MAX_WAITERS) |
| GAP-10: 64-bit truncation | ptrdiff_t → uint32_t | **Closed** | BUILD_ASSERT(sizeof(ptr) <= 4) |

## Remaining Work

- **GAP-1**: Add cbindgen or `_Static_assert(offsetof)` for all 47 `#[repr(C)]` structs
- **GAP-2**: Extend differential tests to cover pipe, timer, event, mem_slab
- **GAP-5**: Add RISC-V and 64-bit CI targets

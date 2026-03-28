# Next Verification Targets — ASIL-D Roadmap

**Date:** 2026-03-28

## Current State
- 23/51 kernel files replaced (61% by lines)
- 807 Verus verified properties, 0 errors
- All synchronization + IPC primitives covered

## Key Insight: Models Already Exist

Gale has Verus-verified models for the next wave of targets.
The verification is DONE. Only FFI shim + Zephyr test wiring remains.

## Priority Targets

### 1. userspace.c — Syscall Validation (CRITICAL)

| Metric | Value |
|--------|-------|
| C source | kernel/userspace.c (1,039 lines) |
| Rust model | src/userspace.rs (751 lines, US1-US8 verified) |
| Properties | Object permission bitmask, type checking, init flag |
| Zephyr test | tests/kernel/mem_protect/userspace |
| Prerequisite | CONFIG_USERSPACE (needs MPU — Cortex-M33/M4F) |
| Effort | Medium |

This is the enforcement mechanism for multi-partition isolation.
Certifiers care most about this for ASIL-D.

### 2. mem_domain.c — Spatial Isolation (HIGH)

| Metric | Value |
|--------|-------|
| C source | kernel/mem_domain.c (424 lines) |
| Rust model | src/mem_domain.rs (554 lines, MD1-MD6 verified) |
| Properties | Partition non-overlap, bounds checking, W^X |
| Zephyr test | tests/kernel/mem_protect/mem_domain |
| Effort | Easy-Medium |

Partition management — if this is wrong, spatial isolation fails.

### 3. heap.c — Heap Allocator (CRITICAL)

| Metric | Value |
|--------|-------|
| C source | lib/heap/heap.c (592 lines) |
| Rust model | src/heap.rs (755 lines, HP1-HP8 verified) |
| Properties | No double-free, bounds checking, chunk conservation |
| Zephyr test | tests/lib/heap |
| Effort | Medium (lib/ path, not kernel/) |

Heap corruption is the #1 CVE class in embedded systems.
Verus can prove at compile time what heap_validate.c checks at runtime.

### 4. MPU Configuration (CRITICAL, DEFERRED)

| Metric | Value |
|--------|-------|
| C source | arch/arm/core/mpu/ (2,390 lines) |
| Properties | Region overlap, alignment, permissions |
| Effort | Hard (architecture-specific) |

MPU misconfiguration = complete isolation failure. Highest-value
architecture-specific target. Defer until M33/M4F is primary.

## Performance Baseline

Binary size overhead (semaphore test, qemu_cortex_m3):
- FLASH: +2,944 bytes (+7.8%)
- RAM: +864 bytes (+6.8%)
- Gale code: 492 bytes (9 functions)
- LTO tested: no improvement (GCC can't cross-language LTO with Rust)

## What Certifiers Care About (from Zephyr Safety WG)

Zephyr targets IEC 61508 SIL 3 via Route 3s (pre-existing assessment).
Safety scope: KERNEL functionalities only.

**In safety perimeter:** synchronization, IPC, scheduling, memory isolation,
syscall validation, heap allocator, spinlock discipline.

**Out of scope:** monitoring, debug, dynamic features, SMP, drivers,
application utilities, boot infrastructure.

Gale currently covers the entire "in safety perimeter" set at the
Verus model level. The FFI wiring for userspace/mem_domain/heap
would complete the story for certification.

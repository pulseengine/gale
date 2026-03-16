# Top 10 Priorities for Gale Expansion

## Context

Targeting two ASIL levels:
- **ASIL-D** (kernel core): synchronization, scheduling, memory — must be formally verified
- **ASIL-B** (separation/userspace): memory protection, syscall validation, task isolation — Rust type safety + selective verification

## The List

### 1. Heap Allocator — `lib/heap/heap.c` (592 lines)
**ASIL-D | Unaddressed STPA hazard | Highest single-file ROI**

The sys_heap free-list uses raw pointer arithmetic with chunk headers embedded in allocated memory. A single corruption leads to use-after-free, double-free, or heap overflow — the #1 source of CVEs in embedded systems.

- **What to verify:** Free-list invariant (no cycles, no overlap), chunk header integrity, merge/split correctness, allocation size bounds
- **What Rust gives for free:** No null deref, no buffer overflow, ownership prevents use-after-free
- **Effort:** Medium-high (complex data structure, but well-bounded)
- **Impact:** Eliminates the largest class of memory safety vulnerabilities

### 2. Userspace Syscall Validation — `userspace.c` + `userspace_handler.c` (1,128 lines)
**ASIL-B | Task separation | Critical for multi-tenant safety**

Validates that userspace threads can only access permitted kernel objects. Every `z_vrfy_*` function checks object type, ownership, and permissions before allowing the syscall. A bug here means privilege escalation.

- **What to verify:** Permission check completeness (every syscall validated), object type discrimination, no TOCTOU between check and use
- **What Rust gives for free:** Exhaustive match on object types (no missing cases), Result types force error handling
- **Effort:** Medium (large but repetitive pattern)
- **Impact:** Enables ASIL-B userspace isolation — kernel objects can't be spoofed

### 3. Memory Domain / MPU Configuration — `mem_domain.c` (424 lines)
**ASIL-B | Hardware protection | 80% generic logic**

Manages memory partitions for userspace isolation. The partition arithmetic (base, size, alignment, overlap checks) is pure logic — only the final MPU register write is hardware-specific.

- **What to verify:** Partition non-overlap, alignment constraints, size bounds, permission propagation
- **What Rust gives for free:** Bounds checking on partition arrays, type-safe permission flags
- **Effort:** Medium (80% extractable as generic Rust)
- **Impact:** Verified MPU configuration = verified spatial isolation

### 4. Spinlock Model — `spinlock_validate.c` (52 lines) + spinlock semantics
**ASIL-D | Unaddressed STPA hazard | Foundation for all concurrency**

Every kernel primitive depends on spinlocks. Currently spinlocks are opaque — Gale verifies what happens INSIDE the lock but not the lock discipline itself. `spinlock_validate.c` is the debug validator (ownership tracking, double-acquire detection).

- **What to verify:** No double-acquisition, no release without acquisition, correct nesting order, IRQ state consistency
- **What Rust gives for free:** RAII lock guards (auto-release), ownership prevents lock leaks
- **Effort:** Small (52 lines + model)
- **Impact:** Closes the concurrency safety gap across ALL primitives

### 5. Wire FFI for 12 Existing Verus Models
**ASIL-D | Immediate coverage boost | Zero new verification needed**

These modules have Verus-verified models and tests but no C shim integration: timeout, poll, futex, timeslice, kheap, thread_lifecycle, work, fatal, mempool, dynamic, smp_state, sched.

- **What to do:** Write FFI exports, C headers, C shims, Kconfig entries, CMakeLists blocks — same pattern as the original 9 primitives
- **Effort:** Medium (mechanical, well-established pattern)
- **Impact:** Kernel coverage jumps from 36% to 67% of the minimal ASIL-D kernel

### 6. Ring Buffers — `lib/os/ring_buffer.c` + `mpsc_pbuf.c` + `spsc_pbuf.c` (1,047+ lines)
**ASIL-D | Lock-free algorithms | High complexity = high risk**

Lock-free MPSC and SPSC ring buffers used by logging, tracing, and IPC. Lock-free code is notoriously hard to get right — memory ordering, ABA problems, producer/consumer index arithmetic.

- **What to verify:** Producer/consumer index bounds, no data loss under concurrent access, linearizability
- **What Rust gives for free:** `AtomicU32` with explicit `Ordering` (no silent relaxed access), type-safe index wrappers
- **Effort:** High (lock-free verification is research-level)
- **Impact:** Verified lock-free data structures are a landmark achievement

### 7. Thread Stack Setup — `thread.c` stack portions (~400 lines of 1,367)
**ASIL-D | Stack overflow prevention | Partially covered by thread_lifecycle**

Thread creation involves computing stack sizes, setting up guard regions, and initializing the stack frame. Incorrect stack setup leads to stack overflow into adjacent memory — undetectable corruption.

- **What to verify:** Stack size alignment, guard region placement, initial frame correctness, watermark tracking
- **What Rust gives for free:** Bounds checking, no raw pointer arithmetic for stack layout
- **Effort:** Medium (arch-specific frame setup stays in C, but size/alignment is generic)
- **Impact:** Prevents the hardest-to-debug class of embedded bugs

### 8. Device Initialization — `device.c` (277 lines) + `init.c` (652 lines)
**ASIL-B | Boot safety | Initialization ordering**

Devices must be initialized in dependency order. A driver initializing before its bus controller is ready causes undefined behavior. The init system uses priority levels and dependency declarations.

- **What to verify:** Topological sort correctness, no circular dependencies, all required deps initialized before dependents
- **What Rust gives for free:** Type-state pattern for init phases (can't use device before init), Result types for init failures
- **Effort:** Medium
- **Impact:** Prevents a common class of hard-to-reproduce boot failures

### 9. Atomic Operations — `atomic_c.c` (414 lines)
**ASIL-D | 95% generic | Software atomic fallback**

The software atomic implementation (for CPUs without hardware atomics) uses IRQ-lock-protected read-modify-write sequences. 95% is pure arithmetic under an IRQ lock — the IRQ lock/unlock is the only hardware part.

- **What to verify:** Each atomic operation (add, sub, or, and, xor, cas) produces the correct result, compare-and-swap linearizability
- **What Rust gives for free:** `core::sync::atomic` types with correct memory ordering
- **Effort:** Small-medium (operations are simple, many of them)
- **Impact:** Verified atomics = verified foundation for all concurrent data structures

### 10. Fault Handler Model — `fatal.c` expanded + ARM fault decode
**ASIL-D | Partially covered | Safety response**

Gale has a basic fatal classification model. Expanding it to cover the ARM Cortex-M fault register decode (HardFault, MemManage, BusFault, UsageFault status registers) would verify that the correct fault type is identified and the appropriate recovery action is taken.

- **What to verify:** CFSR bit decode correctness, fault address extraction, stacked register recovery, appropriate action per fault type
- **What Rust gives for free:** Exhaustive enum matching over fault types, no missing cases
- **Effort:** Medium (register bit decode is tedious but straightforward)
- **Impact:** Verified fault handling = verified safety response to hardware errors

---

## Summary Table

| # | Target | Lines | ASIL | Effort | Impact |
|---|--------|-------|------|--------|--------|
| 1 | Heap allocator | 592 | D | Medium-high | Eliminates #1 CVE class |
| 2 | Userspace syscall validation | 1,128 | B | Medium | Enables task isolation |
| 3 | Memory domain / MPU | 424 | B | Medium | Verified spatial isolation |
| 4 | Spinlock model | 52+ | D | Small | Closes concurrency gap |
| 5 | Wire FFI for 12 models | — | D | Medium | 36%→67% coverage instantly |
| 6 | Ring buffers (lock-free) | 1,047 | D | High | Landmark verification |
| 7 | Thread stack setup | ~400 | D | Medium | Prevents stack overflow |
| 8 | Device init ordering | 929 | B | Medium | Prevents boot failures |
| 9 | Atomic operations | 414 | D | Small-med | Verified concurrency foundation |
| 10 | Fault handler expanded | ~200 | D | Medium | Verified safety response |

**Total new code to verify:** ~5,186 lines
**Result:** ASIL-D kernel coverage → 90%+, ASIL-B userspace isolation established

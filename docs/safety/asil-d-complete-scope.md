# ASIL-D Complete Scope — Beyond "Safety Path Only"

**Date:** 2026-03-28

## The Problem with "Excluded from Safety Path"

Labeling monitoring/diagnostics as "excluded" is ASIL-B thinking.
For ASIL-D, ISO 26262 requires **freedom from interference** (FFI) —
ANY code that shares memory, CPU time, or kernel objects with the
safety function must be proven to not interfere. This includes:

- Thread monitor iterating the thread list (can corrupt it)
- Spinlock validator writing to lock metadata (can cause deadlock)
- Usage statistics reading cycle counters in ISR (can cause jitter)
- Obj_core registration modifying linked lists (can corrupt kernel objects)

**For ASIL-D, these must either be:**
1. **Proven non-interfering** (formal or systematic analysis)
2. **Disabled at compile time** (Kconfig exclusion with verified absence)
3. **Isolated by hardware** (MPU/MMU partition)

## ASIL-D Scope: What Actually Needs Coverage

### Tier 1: Must Be Formally Verified (current Gale scope)
- Synchronization: sem, mutex, condvar — **DONE (Verus)**
- IPC: msgq, stack, pipe, queue, mbox, fifo, lifo — **DONE (Verus)**
- Scheduling decisions: should_preempt, next_up — **DONE (Verus)**
- Timeout arithmetic: deadline tracking — **DONE (Verus)**
- Memory tracking: mem_slab counts — **DONE (Verus)**

### Tier 2: Must Be Formally Verified (next wave — models exist)
- Syscall validation: userspace.c — **MODEL EXISTS (US1-US8)**
- Spatial isolation: mem_domain.c — **MODEL EXISTS (MD1-MD6)**
- Heap allocator: heap.c — **MODEL EXISTS (HP1-HP8)**
- Spinlock discipline: spinlock.c — **MODEL EXISTS (SL1-SL5)**

### Tier 3: Must Be Proven Non-Interfering (currently "excluded")
- Thread monitor: shares thread list with scheduler — **NEEDS ANALYSIS**
- Usage statistics: reads cycle counters, accesses per-thread data — **NEEDS ANALYSIS**
- Obj_core: linked list operations on kernel objects — **NEEDS ANALYSIS**
- Spinlock validate: writes to lock metadata — **NEEDS ANALYSIS**

For Tier 3, the ASIL-D approach is: **prove that disabling them
(via Kconfig) removes ALL their code.** This is a compile-time
property verifiable by:
- `#ifdef CONFIG_THREAD_MONITOR` guarding all code paths
- Link-time verification that no symbols from excluded modules
  appear in the final binary
- Static analysis confirming no conditional code paths remain

### Tier 4: Out of Scope (not shared with safety functions)
- banner.c, version.c, main_weak.c — one-time boot, no shared state
- boot_args.c — pre-kernel, no runtime impact
- nothread.c — mutually exclusive with MULTITHREADING

## LLVM Cross-Language LTO

### Current State (GCC)
- FLASH overhead: +7.8% (2,944 bytes)
- Gale code: 492 bytes
- GCC cannot do cross-language LTO with Rust

### LLVM Path (zero-overhead target)
Zephyr supports LLVM/Clang (`-DZEPHYR_TOOLCHAIN_VARIANT=llvm`).
Rust shares the LLVM backend. Cross-language LTO enables:

```
Rust FFI function (LLVM IR)  ──┐
                                ├── LLD linker ──→ single optimized binary
C shim function (LLVM IR)    ──┘
```

With `-C linker-plugin-lto` in Rust and `-flto=thin` in Clang:
1. Rust emits LLVM bitcode instead of machine code
2. C is compiled to LLVM bitcode
3. LLD optimizes across both → can inline Rust into C callers
4. The 492 bytes of Rust could be inlined to ZERO overhead

### Implementation
```toml
# ffi/Cargo.toml — for LLVM cross-language LTO
[profile.release]
lto = true
linker-plugin-lto = true  # emit LLVM bitcode
```

```cmake
# Zephyr build — enable LLVM LTO
set(ZEPHYR_TOOLCHAIN_VARIANT llvm)
set(CONFIG_LTO y)
```

### Expected Result
- Gale FFI calls inlined into C shim → zero call overhead
- Dead code elimination across Rust/C boundary
- Potential for SMALLER binary than baseline (better optimization)

## ASIL-D Freedom From Interference Matrix

| Module | Shares State With | FFI Risk | ASIL-D Treatment |
|--------|------------------|----------|-----------------|
| thread_monitor | Thread list (scheduler) | Can corrupt list during iteration | Kconfig exclusion verified |
| usage | Per-thread cycle counters | ISR jitter from reads | Kconfig exclusion verified |
| obj_core | Kernel object registry | Linked list corruption | Kconfig exclusion verified |
| spinlock_validate | Lock metadata | False deadlock detection | Kconfig exclusion verified |
| init.c | Thread creation | One-time, sequential | Static analysis |
| idle.c | Scheduler ready queue | Lowest priority, minimal | Review + test |
| device.c | Device init flags | One-time, sequential | Static analysis |

## Action Items

1. **Verify Kconfig exclusion completeness** — for each Tier 3 module,
   prove that `CONFIG_X=n` removes ALL code (no residual conditionals)
2. **Add LLVM LTO CI job** — build with Clang, measure size delta
3. **Wire FFI shims for Tier 2** — userspace, mem_domain, heap
4. **Document FFI analysis for Tier 3** — formal argument that
   disabled modules cannot interfere

# Performance Benchmarks — Regression Tracking

**Purpose:** Track performance metrics on every commit. Benchmarks are gates, not just measurements — a regression fails CI.

## Metrics Tracked

### 1. Binary Size (FLASH + RAM)
Build the semaphore test with and without Gale overlay, compare:
- FLASH: must not increase >10% vs baseline
- RAM: must not increase >5% vs baseline

### 2. Stack Usage per FFI Function
Each `gale_k_*_decide` function must use <256 bytes of stack.
Measured via `-fstack-usage` (GCC) during cross-compilation.

### 3. No Heap Allocation (Structural)
`plain/src/` uses no `alloc` crate, no `Box`, no `Vec` (only fixed arrays).
This is enforced by `#![no_std]` + `unsafe_code = "deny"`.
No runtime check needed — it's a compile-time guarantee.

### 4. Cargo Benchmarks (Host-Side)
`cargo bench` via criterion measures decision function throughput:
- `sem_give_decide`: expected <10ns per call
- `mutex_lock_decide`: expected <10ns per call
- These are pure scalar functions, no allocation

### 5. Zephyr Test Timing
The CI already measures test duration. A test that previously took 8s
and now takes 20s indicates a performance regression (e.g., busy loop,
missed optimization).

## CI Integration

Performance benchmarks should be:
1. **Measured** on every push (artifact upload)
2. **Compared** against baseline (previous main)
3. **Failed** if regression exceeds threshold

The `cargo bench` results can be tracked via GitHub Actions benchmark action
(e.g., `benchmark-action/github-action-benchmark`) which stores results
and detects regressions automatically.

## What Memory Leaks Mean for Gale

Gale's verified Rust layer has **zero heap allocation by construction**:
- `#![no_std]` — no standard library allocator
- `unsafe_code = "deny"` — no raw pointers or manual allocation
- All data structures are fixed-size arrays on the stack
- The `WaitQueue` is `[Option<Thread>; 64]` — stack-allocated, bounded

Memory leaks can only occur in:
- The C shim layer (uses Zephyr's existing allocation patterns)
- Zephyr's own kernel code (unchanged by Gale)

Both are covered by Zephyr's existing leak detection tools:
- `CONFIG_OBJECT_TRACING` — tracks kernel object lifecycle
- `CONFIG_THREAD_ANALYZER` — reports thread stack usage
- Valgrind/ASan in POSIX builds

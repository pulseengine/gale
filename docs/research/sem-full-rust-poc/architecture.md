# Full-Rust Semaphore Kernel Implementation: Architecture

## What this POC proves

### 1. Rust-to-Rust for verified logic is possible and better

The current architecture has an unnecessary FFI hop in the critical path:

```
Zephyr C (gale_sem.c)  ──C-to-Rust FFI──▶  gale_sem_count_give(count, limit)
                                             │
                                             ▼
                                           Gale Rust (verified arithmetic)
```

The C shim `gale_sem.c` calls `gale_sem_count_give()` via `extern "C"` — a
C-to-Rust FFI call. This works, but:

- The compiler cannot inline across the FFI boundary (no cross-language LTO
  between Zephyr's C compiler and rustc, unless both use LLVM and LTO is
  configured end-to-end).
- Type safety is lost at the boundary — `uint32_t` and `u32` are ABI-compatible
  but the compiler cannot verify the semaphore invariant (0 <= count <= limit)
  across the FFI.
- The C shim is ~235 lines of unverified C code that can harbor bugs.

The full-Rust approach eliminates this:

```
Gale Rust (sem_kernel.rs)  ──direct Rust call──▶  gale::sem::Semaphore::init()
         │                                          (verified, can be inlined)
         │
         ──extern "C"──▶  Zephyr C scheduler ops
                           (z_pend_curr, z_unpend_first_thread, etc.)
```

Benefits:
- **Cross-crate inlining**: With LTO, `Semaphore::init()` validation is inlined
  into `z_impl_k_sem_init()`. Zero overhead vs. the C shim's FFI call.
- **Type safety across the boundary**: The Rust compiler enforces that
  `Semaphore::init()` returns a valid `Semaphore` with count <= limit.
  No way to accidentally write `sem->count = initial_count` without validation.
- **No unverified C glue**: The 235-line `gale_sem.c` is replaced by ~160 lines
  of Rust that are structurally identical but with stronger type guarantees.

### 2. Binary compatibility with Zephyr is maintained

The `#[no_mangle] pub unsafe extern "C"` functions produce symbols with the
exact same names and calling conventions as the C functions they replace:

- `z_impl_k_sem_init` (replaces the C version in gale_sem.c)
- `z_impl_k_sem_give` (replaces the C version in gale_sem.c)
- `z_impl_k_sem_take` (replaces the C version in gale_sem.c)
- `z_impl_k_sem_reset` (replaces the C version in gale_sem.c)
- `z_impl_k_sem_count_get` (replaces the inline in kernel.h)

Zephyr's syscall dispatch (`z_vrfy_*` wrappers, marshaling code) calls these
symbols. No changes needed to Zephyr's build system or plumbing.

### 3. The FFI boundary is in the right place

The verified/unverified boundary should be:

```
VERIFIED (proven correct)     │  UNVERIFIED (trusted platform)
──────────────────────────────┼────────────────────────────────
Count arithmetic              │  Thread scheduling
  - init validation           │    - z_unpend_first_thread
  - give increment            │    - z_pend_curr
  - take decrement            │    - z_ready_thread
  - bounds checking           │    - z_reschedule
                              │  Spinlock operations
                              │    - k_spin_lock/unlock
                              │  Architecture operations
                              │    - arch_thread_return_value_set
```

In the C shim, both sides are written in different languages but the FFI
boundary cuts through the VERIFIED side (C calls Rust for arithmetic). In the
full-Rust approach, the FFI boundary aligns with the verified/unverified
boundary — verified logic is all Rust, unverified platform ops are all C.

## Challenges and open questions

### 1. Bindgen for Zephyr kernel-internal headers

Zephyr's kernel-internal headers (`ksched.h`, `wait_q.h`, `kernel_internal.h`)
are heavily macro-laden and config-dependent. Running bindgen requires:

- A complete Zephyr build context (`.config`, selected arch, toolchain)
- Preprocessing with the correct `-D` flags and include paths
- Handling of `ALWAYS_INLINE` functions (see below)

**Mitigation**: The POC uses manual bindings. For production:
1. Create a `wrapper.h` that includes the needed headers.
2. Run bindgen as a Zephyr CMake build step, using the build's include paths.
3. Output goes to a generated `kernel_sys.rs` that is `include!`-ed.

This is the same approach used by `zephyr-rust` and other Rust-on-Zephyr projects.

### 2. Inline function trampolines

Several critical Zephyr functions are `static ALWAYS_INLINE`:

| Function | Header | Notes |
|---|---|---|
| `z_unpend_first_thread` | ksched.h:162 | Performance-critical |
| `z_waitq_init` | wait_q.h:40 | Init-time only |
| `arch_thread_return_value_set` | kernel_arch_interface.h:160 | Arch-specific |
| `k_spin_lock` | spinlock.h | Interrupt control |
| `k_spin_unlock` | spinlock.h | Interrupt control |

These have no linkable symbol — they are inlined at every call site. Options:

a. **C trampoline file** (recommended for POC): A small C file that wraps each
   inline in a non-inline function. ~30 lines of trivial C. This is the standard
   approach for Rust calling C inlines.

b. **Rewrite in Rust**: For `k_spin_lock`, this means calling `arch_irq_lock()`
   directly. For `z_unpend_first_thread`, this means calling `_priq_wait_best()`
   and `unpend_thread_no_timeout()`. More Rust code, but eliminates all C.

c. **LLVM cross-language LTO**: If both Zephyr and Rust use LLVM, inlines can
   potentially be resolved at link time. Experimental, not reliable.

For production, option (a) is pragmatic. Option (b) is the long-term goal
(eventually replacing the scheduler too).

### 3. `struct k_sem` layout compatibility

The Zephyr `k_sem` struct has config-dependent fields:

```c
struct k_sem {
    _wait_q_t wait_q;           // Always present
    unsigned int count;         // Always present
    unsigned int limit;         // Always present
    Z_DECL_POLL_EVENT           // Only with CONFIG_POLL
    SYS_PORT_TRACING_TRACKING_FIELD(k_sem)  // Only with tracing
    struct k_obj_core obj_core; // Only with CONFIG_OBJ_CORE_SEM
};
```

The Rust code must access `count` and `limit` at the correct offsets.

**Solution**: Bindgen generates the correct `k_sem` layout for the active config.
The POC uses a minimal layout (just wait_q, count, limit) which matches the
qemu_cortex_m3 config used for testing.

### 4. Spinlock semantics and safety

The global spinlock pattern (`static struct k_spinlock lock`) is inherently
unsafe in Rust terms — it's a mutable static accessed from multiple contexts.
This is correct in Zephyr because:

- On uniprocessor: `k_spin_lock` disables interrupts, providing exclusion.
- On SMP: The spinlock also uses an atomic, providing inter-CPU exclusion.

In Rust, we use `static mut` which requires `unsafe` at every access. This is
acceptable for kernel code but means the spinlock protocol (lock before access,
unlock after) is not compiler-enforced.

**Future work**: A Rust spinlock wrapper that uses RAII (lock guard) to enforce
the protocol. This would make it impossible to forget to unlock.

### 5. Tracing and poll events

The C shim includes `SYS_PORT_TRACING_*` macros and `handle_poll_events()`.
These are not safety-critical but are needed for feature parity.

- **Tracing**: Can be implemented as Rust macros that expand to nothing when
  tracing is disabled, or to `extern "C"` calls when enabled.
- **Poll events**: Requires accessing the `poll_events` field of `k_sem`,
  which is config-dependent. Bindgen handles this; the POC uses a cfg feature.

## Comparison with current C shim approach

| Aspect | C shim (gale_sem.c) | Full Rust (this POC) |
|---|---|---|
| Lines of code | ~235 C + ~30 Rust FFI | ~160 Rust + ~30 C trampolines |
| Verified call path | C → Rust FFI (extern "C") | Rust → Rust (direct call) |
| Inlining across boundary | No (unless cross-lang LTO) | Yes (with LTO) |
| Type safety | Lost at FFI boundary | Preserved across crates |
| Unverified C code | 235 lines (full logic) | ~30 lines (inline trampolines only) |
| Zephyr diff required | Replace kernel/sem.c | Replace kernel/sem.c |
| Build system changes | Link Rust staticlib | Link Rust staticlib (same) |
| Bindgen needed | No (C calls Rust) | Yes (Rust calls C kernel internals) |
| Maturity | Proven (24/24 tests pass) | POC (architecture validated) |

## Migration path

### Phase 1 (current): C shim calls Rust for arithmetic
- **Status**: Complete, all 24 tests pass.
- **Architecture**: gale_sem.c → gale FFI (count ops only).
- **Risk**: Low — minimal change to Zephyr.

### Phase 2 (this POC): Full Rust replaces C shim
- **Prerequisite**: Bindgen integration for Zephyr kernel headers.
- **Prerequisite**: C trampoline file for inline functions.
- **Work items**:
  1. Create `zephyr_kernel_sys` crate with bindgen-generated bindings.
  2. Create `inline_trampolines.c` (~30 lines) for inline functions.
  3. Port `gale_sem.c` → `sem_kernel.rs` (this POC is the template).
  4. Update CMakeLists.txt to link the new staticlib instead of gale_sem.c.
  5. Run all 24 semaphore tests — must pass identically.

### Phase 3 (future): Extend to all primitives
- Apply the same pattern to mutex, condvar, msgq, stack, pipe.
- Each C shim (gale_mutex.c, etc.) becomes a Rust file.
- Share the `zephyr_kernel_sys` bindings crate.

### Phase 4 (long-term): Replace scheduler calls
- Rewrite `z_unpend_first_thread`, `z_pend_curr` etc. in Rust.
- Verify the scheduling logic with Verus.
- Eliminate all C from the critical path.

## Files in this POC

| File | Purpose |
|---|---|
| `kernel_sys.rs` | Manual Zephyr kernel bindings (bindgen substitute) |
| `sem_kernel.rs` | Full Rust semaphore implementation |
| `Cargo.toml` | Crate configuration showing the gale dependency |
| `architecture.md` | This document |

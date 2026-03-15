# Research: Rust-to-Rust Kernel Linking for Gale + Zephyr

**Date**: 2026-03-15
**Status**: Research / Proposal
**Scope**: In-kernel Rust linking -- NOT the Kiln/WASM userspace path
**Related**: `docs/research/kiln-direct-rust-linking.md` (Phase 2 / WASM path)

## Motivation

> "For Rust we prefer Rust-to-Rust linking instead of Rust-to-C-to-Rust.
> Looks different for sure once we are in userspace but in the kernel
> definitely."

The current Phase 1 architecture has a double language crossing for every
verified operation:

```
Zephyr C kernel  -->  C shim (gale_sem.c)  -->  extern "C" FFI  -->  Rust (gale)
```

When Zephyr's kernel itself could be calling Rust, the C-to-Rust-to-C-to-Rust
round-trip is wasteful:

- Two ABI boundary crossings per call (C -> Rust FFI, and eventually back)
- `unsafe` raw pointer manipulation at the FFI boundary (`*mut u32`)
- C header files (`gale_sem.h`) that duplicate the Rust API surface
- `#[no_mangle] pub extern "C"` boilerplate in `ffi/src/lib.rs`
- Loss of Rust type safety at every boundary (everything becomes `u32`/`i32`)

This document explores eliminating the C intermediary for **in-kernel** use,
where Rust code can call Gale's verified Rust directly.

This is distinct from the Kiln/WASM path (documented in
`kiln-direct-rust-linking.md`), which targets **userspace** components via the
WebAssembly Component Model.

## Current Architecture (Phase 1)

### Call flow

```
z_impl_k_sem_give()           [Zephyr C, kernel/sem.c or gale_sem.c]
  |
  +-- k_spin_lock(&lock)      [C: arch-specific spinlock]
  +-- z_unpend_first_thread()  [C: wait queue dequeue]
  +-- gale_sem_count_give()    [C FFI -> Rust -> return u32]
  +-- z_reschedule()           [C: scheduler]
  +-- k_spin_unlock()          [C: arch-specific spinlock]
```

### What Gale verifies vs. what stays in C

| Concern | Implementation | Language |
|---------|---------------|----------|
| Count arithmetic | Gale (Verus-verified) | Rust |
| State machine validation | Gale (Verus-verified) | Rust |
| Ring buffer index math | Gale (Verus-verified) | Rust |
| Wait queue management | Zephyr `_wait_q_t` | C |
| Thread scheduling | Zephyr `z_reschedule` | C |
| Spinlocks | Zephyr `k_spinlock` | C |
| Tracing/diagnostics | Zephyr `SYS_PORT_TRACING_*` | C |
| Poll events | Zephyr `z_handle_obj_poll_events` | C |
| Userspace validation | Zephyr `z_vrfy_*` | C |
| Object core (debug) | Zephyr `k_obj_core_*` | C |

The FFI boundary is thin: only pure arithmetic/validation crosses it.
All kernel infrastructure (wait queues, scheduling, spinlocks) stays in C.

### Key files

| Layer | Path |
|-------|------|
| Verified Rust (Verus) | `src/sem.rs`, `src/mutex.rs`, etc. |
| Plain Rust (testable) | `plain/src/sem.rs`, etc. |
| FFI staticlib | `ffi/src/lib.rs` (crate-type = staticlib) |
| C headers | `ffi/include/gale_sem.h`, etc. |
| C shims | `zephyr/gale_sem.c`, etc. |
| Zephyr build glue | `zephyr/CMakeLists.txt`, `zephyr/Kconfig` |

## Zephyr's Rust Support Today

### Official status (Zephyr 4.1+, zephyr-lang-rust)

Zephyr has official Rust support via the `zephyr-lang-rust` module:

- **Scope**: Application-level Rust, NOT kernel-internal Rust
- **Mechanism**: `rust_cargo_application()` CMake macro compiles Rust
  application code as a `staticlib`, linked into the Zephyr image
- **API**: `zephyr::sys` module wraps C kernel APIs via bindgen-generated FFI
- **Direction**: Rust application code calls **down** into C kernel via FFI
- **No kernel-internal Rust**: The kernel itself (scheduler, wait queues,
  kernel objects) remains 100% C

Key limitation: `zephyr-lang-rust` provides Rust-calls-C, not C-calls-Rust or
Rust-calls-Rust-inside-kernel. It cannot replace kernel `.c` files with `.rs`.

There is **no** `CONFIG_RUST` that enables Rust-in-kernel. There are **no**
`.rs` files in the Zephyr kernel tree. There is no CMake infrastructure for
compiling Rust as part of the kernel build (only as applications).

### Relevant Zephyr issues

- [#65837](https://github.com/zephyrproject-rtos/zephyr/issues/65837):
  Rust Support in Zephyr (application focus)
- [#75900](https://github.com/zephyrproject-rtos/zephyr/issues/75900):
  Rust on Zephyr -- cmake and code organization

Neither proposes kernel-internal Rust.

## How Other Projects Handle Rust Kernels

### Rust for Linux

- **Approach**: Rust modules are compiled as `.o` files and linked into the
  kernel image alongside C `.o` files
- **FFI**: Rust calls C kernel APIs via bindgen-generated bindings. C calls
  Rust via `extern "C"` + `#[no_mangle]` on module entry points
- **Abstractions**: The `kernel` crate provides safe Rust wrappers around
  C kernel primitives (`Mutex`, `SpinLock`, `Task`, etc.)
- **Key insight**: Rust modules call other Rust modules **directly** (normal
  Rust crate dependencies). FFI is only at the Rust-C boundary, not
  Rust-Rust boundaries
- **Cross-language LTO**: When both Clang (for C) and rustc use the same LLVM
  version, linker-plugin LTO can inline across the C-Rust boundary, making
  `extern "C"` calls zero-cost

### Hubris (Oxide Computer)

- **Approach**: Pure Rust kernel (~2000 lines), no C at all
- **Architecture**: Microkernel with static task set defined at build time
- **Linking**: All tasks compiled as separate Rust crates, linked as separate
  binaries with MPU isolation between them
- **Key insight**: By eliminating C entirely, there is no FFI. Kernel
  primitives are Rust traits/structs called directly

### Tock OS

- **Approach**: Rust kernel with tiered trust model
- **Architecture**: Core kernel (trusted) + capsules (semi-trusted Rust
  modules in kernel address space) + userland processes (untrusted, any
  language, MPU-isolated)
- **Kernel primitives**: Implemented directly in Rust. Capsules call kernel
  APIs as normal Rust function calls
- **Key insight**: Capsule isolation is via Rust's type system and module
  visibility, not FFI boundaries. Kernel-internal code is Rust-to-Rust

### Embassy

- **Approach**: Pure Rust async embedded framework, replaces traditional RTOS
- **Primitives**: `embassy-sync` provides `Mutex`, `Channel`, `Signal`,
  `Semaphore` -- all in pure `no_std` Rust
- **Key insight**: No kernel/userspace split. Everything is Rust, everything
  is direct function calls. Zero FFI

### Summary of approaches

| Project | Kernel language | Rust-to-Rust? | FFI role |
|---------|----------------|---------------|----------|
| Rust for Linux | C + Rust modules | Yes (between Rust modules) | Rust-C boundary only |
| Hubris | Pure Rust | Yes (all calls) | None |
| Tock | Pure Rust kernel | Yes (capsules) | Userspace syscall ABI only |
| Embassy | Pure Rust | Yes (all calls) | None |
| Zephyr + Gale (today) | C kernel | No | Every Gale call crosses FFI |

## Three Approaches for Gale + Zephyr

### Approach A: Full Rust Kernel Primitives

Replace `kernel/sem.c` (and other primitives) entirely with Rust
implementations that directly call Zephyr's C scheduler/wait-queue APIs.

#### Architecture

```
z_impl_k_sem_give()        [Rust, replaces gale_sem.c entirely]
  |
  +-- k_spin_lock(&lock)   [Rust FFI call to C]
  +-- z_unpend_first()     [Rust FFI call to C]
  +-- Semaphore::give()    [Rust-to-Rust, direct, no FFI]
  +-- z_reschedule()       [Rust FFI call to C]
  +-- k_spin_unlock()      [Rust FFI call to C]
```

The Rust implementation owns the entire `z_impl_k_sem_give` function. It calls
Gale's verified `Semaphore::give()` directly (Rust-to-Rust), and calls Zephyr's
C scheduler APIs via bindgen-generated FFI.

#### What this requires

1. **Rust bindgen for Zephyr kernel internals**: Generate Rust bindings for
   `_wait_q_t`, `k_spinlock`, `k_spinlock_key_t`, `z_unpend_first_thread()`,
   `z_reschedule()`, `z_pend_curr()`, `z_ready_thread()`,
   `arch_thread_return_value_set()`, `k_object_init()`, `SYS_PORT_TRACING_*`
   macros, `sys_dlist_init()`, etc.

2. **CMake integration**: Extend Zephyr's build system to compile Rust crates
   as kernel libraries (not just applications). This means:
   - `zephyr_rust_library()` CMake macro (does not exist today)
   - Passing kernel-internal include paths to bindgen
   - Linking the Rust staticlib into the kernel (not application) link group
   - Handling `CONFIG_*` feature flags in Rust via `cfg` attributes

3. **Struct layout compatibility**: Zephyr's `struct k_sem` is defined in C
   headers and used by both C kernel code and C applications. The Rust
   implementation must operate on the same struct layout. Options:
   - Use bindgen to import `struct k_sem` as a Rust type
   - Use `#[repr(C)]` Rust struct with identical layout
   - Use opaque pointers and field accessors

4. **Macro expansion**: Many Zephyr kernel APIs are macros or inline functions
   (`SYS_PORT_TRACING_*`, `CHECKIF`, `K_TIMEOUT_EQ`). Bindgen cannot generate
   bindings for these. They need C wrapper functions or must be reimplemented
   in Rust.

#### Pros

- **True Rust-to-Rust**: Gale's verified code is called directly from the
  kernel primitive implementation. No `extern "C"`, no `#[no_mangle]`, no C
  header duplication
- **Full type safety**: The Rust compiler enforces correct usage of Gale's
  API (e.g., `GiveResult` enum vs. raw `u32` return)
- **Incremental**: Can replace one primitive at a time (sem, then mutex, etc.)
- **Cross-language LTO**: With Clang + rustc on same LLVM version, the FFI
  calls to Zephyr's C scheduler can be inlined away entirely
- **Wider verification scope**: The Rust code owns the full primitive
  implementation, including the integration with wait queues. More of the
  code path is in a memory-safe language

#### Cons

- **Massive bindgen surface**: Zephyr's kernel-internal API is large, macro-
  heavy, and config-dependent. Generating stable Rust bindings for it is a
  significant ongoing maintenance burden
- **Struct layout fragility**: `struct k_sem`, `struct k_thread`, etc. change
  across Zephyr versions. Bindgen must be re-run on every Zephyr update
- **Zephyr upstream friction**: This approach requires changes to Zephyr's
  build system (`zephyr_rust_library()` does not exist). Upstream acceptance
  is uncertain
- **Config combinatorial explosion**: Zephyr has hundreds of `CONFIG_*`
  options that affect kernel struct layouts and API availability. The Rust
  bindings must handle all combinations
- **ISR/thread context**: Rust code running in the kernel must handle ISR
  context correctly. `panic!` in ISR context is fatal. `alloc` is forbidden.
  Stack usage must be bounded
- **Still has FFI**: The FFI does not disappear -- it moves. Instead of
  Rust-to-C FFI for Gale's arithmetic, there is Rust-to-C FFI for Zephyr's
  scheduler. The total number of FFI calls per operation may increase

### Approach B: Optimized Current Architecture (Thin C Wrappers)

Keep the current C shim + Rust staticlib architecture, but optimize the FFI
boundary to minimize overhead and maximize the verified Rust surface.

#### Architecture

```
z_impl_k_sem_give()        [C, gale_sem.c -- minimal orchestration]
  |
  +-- k_spin_lock(&lock)   [C: one line]
  +-- z_unpend_first()     [C: one line]
  +-- gale_sem_give()      [C FFI -> Rust (but more than count arithmetic)]
  +-- z_reschedule()       [C: one line]
  +-- k_spin_unlock()      [C: one line]
```

#### Optimizations over current state

1. **Move more logic into Rust**: Instead of exporting individual arithmetic
   functions (`gale_sem_count_give`, `gale_sem_count_take`), export higher-
   level operations that bundle multiple verified checks:

   ```rust
   // Current FFI (fine-grained):
   pub extern "C" fn gale_sem_count_give(count: u32, limit: u32) -> u32;
   pub extern "C" fn gale_sem_count_take(count: *mut u32) -> i32;

   // Optimized FFI (coarse-grained):
   pub extern "C" fn gale_sem_give(state: *mut GaleSemState) -> GaleSemGiveResult;
   pub extern "C" fn gale_sem_take(state: *mut GaleSemState) -> GaleSemTakeResult;
   ```

   This reduces FFI calls per operation and keeps more invariant checking
   inside Rust.

2. **Cross-language LTO**: Enable `-C linker-plugin-lto` in the Rust
   compilation and `-flto=thin` in the C compilation. When both use the
   same LLVM version (Clang for C, rustc for Rust), the linker can inline
   the `extern "C"` Rust functions directly into the C call sites,
   eliminating the ABI boundary entirely at the binary level.

   ```cmake
   # In CMakeLists.txt:
   set(GALE_CARGO_RUSTFLAGS "-Clinker-plugin-lto")
   # Zephyr C compilation (if using Clang):
   zephyr_compile_options(-flto=thin)
   ```

   With LTO, `gale_sem_count_give()` is inlined directly into
   `z_impl_k_sem_give()` in the final binary. The ABI boundary exists
   in source code but vanishes in the binary.

3. **`#[repr(C)]` state structs**: Instead of passing individual `u32`
   values, pass a pointer to a `#[repr(C)]` struct that mirrors the
   verified-state portion of the kernel object:

   ```rust
   #[repr(C)]
   pub struct GaleSemState {
       pub count: u32,
       pub limit: u32,
   }
   ```

   This reduces the number of FFI parameters and makes the Rust side own
   a typed view of the state.

#### Pros

- **No Zephyr build system changes**: Works with existing `west build` +
  `ZEPHYR_EXTRA_MODULES` mechanism
- **No bindgen dependency**: Does not need to generate bindings for
  Zephyr kernel internals
- **No struct layout coupling**: Gale defines its own state structs, not
  tied to `struct k_sem` layout changes
- **Cross-language LTO eliminates ABI overhead**: With Clang + same-LLVM
  rustc, the FFI boundary is zero-cost in the final binary
- **Proven approach**: Already working, all tests passing
- **Minimal maintenance burden**: C shims are thin (10-20 lines each) and
  rarely change

#### Cons

- **Still C source files**: `gale_sem.c` etc. exist as source code even if
  they're minimal. Each primitive needs a `.c` file
- **FFI boilerplate**: `ffi/src/lib.rs` still has `#[no_mangle] extern "C"`
  per function, and `ffi/include/` still has `.h` files
- **Source-level FFI**: Even with LTO, the source code has an explicit FFI
  boundary. Code review must cross languages
- **Type safety gap**: At the C call site, Gale functions take/return
  primitive types (`u32`, `i32`), not Rust enums or Result types
- **Cross-language LTO requires Clang**: Zephyr's default compiler is GCC.
  LTO only works when both C and Rust use LLVM (Clang + rustc). GCC-based
  builds retain the ABI overhead

### Approach C: Zephyr-Native Rust Module System

Build a Rust module system within Zephyr that allows kernel subsystems to be
implemented in Rust with first-class build system support.

#### Architecture

```
# In a hypothetical future Zephyr with native Rust support:

zephyr_rust_kernel_library(gale_sem)
zephyr_rust_kernel_sources(
  gale_sem_impl.rs    # Rust implementation of z_impl_k_sem_*
)
zephyr_rust_kernel_dependencies(
  gale                # Direct Rust dependency, no FFI
  zephyr_kernel_sys   # Auto-generated bindings for kernel internals
)
```

The Rust file directly implements `z_impl_k_sem_give` etc., exported as
`#[no_mangle] pub extern "C"` so Zephyr's existing C callers (syscall
dispatch, inline wrappers in `kernel.h`) can call it. Internally, it calls
Gale's Rust API directly (Rust-to-Rust).

This is essentially Approach A but with official Zephyr build system support.

#### What needs to exist

1. **`zephyr_rust_kernel_library()` CMake macro**: Like `zephyr_library()`
   but invokes `cargo build` and links the result into the kernel link group

2. **`zephyr_kernel_sys` crate**: Auto-generated by Zephyr's build system
   (similar to how `zephyr-lang-rust` generates `zephyr-sys` for apps).
   Contains bindings for kernel-internal APIs:
   - `z_unpend_first_thread()`, `z_pend_curr()`, `z_reschedule()`, etc.
   - `k_spinlock`, `k_spinlock_key_t`
   - `struct k_thread` (opaque or field-accessible)
   - Tracing macros (as Rust functions or macros)

3. **Kernel Rust policy**: Rules for Rust code in the kernel:
   - `#![no_std]`, no allocation, bounded stack usage
   - `panic = "abort"` (kernel cannot unwind)
   - ISR-safety annotations
   - Review process for `unsafe` blocks

4. **Zephyr upstream acceptance**: This requires RFC, TSC approval, and
   sustained maintenance commitment from the Zephyr community

#### Pros

- **First-class Rust in kernel**: Officially supported, maintained, tested
- **Shared tooling**: Zephyr's CI builds and tests Rust kernel modules
- **Community**: Other projects could use the same infrastructure
- **Clean separation**: `zephyr_kernel_sys` provides a stable Rust API for
  kernel internals, maintained alongside the kernel

#### Cons

- **Does not exist today**: This is a multi-year effort requiring Zephyr
  upstream buy-in
- **Community alignment uncertain**: Zephyr's current Rust effort
  (`zephyr-lang-rust`) is application-focused. Kernel-internal Rust is a
  much larger architectural commitment
- **Linux parallel is cautionary**: Rust for Linux has been in progress
  since 2021 and still faces significant resistance and maintenance
  challenges. Zephyr's community is smaller
- **Config complexity**: Zephyr's `Kconfig` system generates hundreds of
  configuration-dependent defines. Exposing all of these to Rust via `cfg`
  attributes is a significant engineering effort
- **Blocks on upstream**: Gale cannot ship this approach until Zephyr
  accepts it. Timeline is unpredictable

## What Component Model / WASI Covers vs. What's Kernel-Specific

### WASI coverage

| Concern | WASI status | Notes |
|---------|-------------|-------|
| Thread creation | `shared-everything-threads` (in progress) | Replaces withdrawn `wasi-threads` |
| Mutexes, semaphores | No WASI proposal | Implementable with `atomic.wait`/`notify` from core wasm threads |
| Condition variables | No WASI proposal | Same as above |
| Message queues | No WASI proposal | Application-level concern |
| Memory allocation | `wasi-allocator` (proposed) | Block allocators not covered |
| Timers | `wasi-clocks` | Wall clock and monotonic, not RTOS timer callbacks |
| Events/signals | No WASI proposal | Bitmask event flags are RTOS-specific |
| Pipes | `wasi-io` streams | Byte streams exist but semantics differ from RTOS pipes |

### What's kernel-specific (not covered by WASI)

- **ISR context execution**: WASI has no concept of interrupt service routines
- **Priority-based scheduling**: WASI threading is cooperative, not preemptive
  with priority inheritance
- **Spinlocks**: Hardware-level spin-wait is below WASI's abstraction level
- **Wait queue ordering**: RTOS wait queues are priority-ordered; this is not
  a WASI concern
- **Zero-copy IPC**: RTOS message queues and pipes operate on shared memory
  with known buffer layouts; WASI uses the canonical ABI with serialization
- **Deterministic timing**: ASIL-D requires bounded worst-case execution
  time, which WASI does not guarantee

### Implication for Gale

WASI/Component Model is the right abstraction for **userspace** components
that import kernel services. It is NOT suitable for implementing the kernel
primitives themselves. The kernel-internal path must remain native Rust (or
C), compiled directly to the target architecture.

The two paths are complementary:
- **In-kernel**: Rust-to-Rust (this document) or Rust-to-C-FFI (current)
- **Userspace**: WIT/Component Model via Kiln (see `kiln-direct-rust-linking.md`)

## Recommended Path: Pragmatic Layering

### Short term: Approach B with cross-language LTO

The current architecture is sound and working. The pragmatic optimization is:

1. **Enable cross-language LTO** for Clang-based builds. This eliminates the
   FFI ABI overhead in the binary without changing the source architecture.
   The `extern "C"` boundary becomes a source-level documentation marker,
   not a runtime cost.

2. **Coarsen the FFI boundary**: Move from fine-grained functions
   (`gale_sem_count_give`) to coarser operations that keep more logic in
   Rust. This reduces the number of FFI calls per kernel operation.

3. **Add `#[repr(C)]` state structs** to pass structured data across the
   boundary instead of individual `u32` values.

This gets 90% of the benefit of Rust-to-Rust linking with zero risk and
no Zephyr upstream dependencies.

### Medium term: Approach A for a single primitive (proof of concept)

Build a proof-of-concept that replaces `gale_sem.c` entirely with Rust:

1. Use bindgen to generate bindings for the specific Zephyr kernel-internal
   APIs that `gale_sem.c` uses:
   - `k_spinlock`, `k_spin_lock()`, `k_spin_unlock()`
   - `z_unpend_first_thread()`, `z_pend_curr()`
   - `z_ready_thread()`, `z_reschedule()`
   - `arch_thread_return_value_set()`
   - `SYS_PORT_TRACING_*` (as C wrapper functions)
   - `handle_poll_events()` (as C wrapper function)

2. Write `gale_sem_kernel.rs` that implements `z_impl_k_sem_give`,
   `z_impl_k_sem_take`, etc. as `#[no_mangle] pub extern "C"` functions.
   Internally, these call `gale::sem::Semaphore::give()` directly.

3. The `extern "C"` on `z_impl_k_sem_give` is required because Zephyr's C
   code calls it (syscall dispatch, `kernel.h` inline wrappers). But the
   *internal* call from `z_impl_k_sem_give` to `Semaphore::give()` is pure
   Rust-to-Rust -- no FFI boundary.

4. Test with the existing Zephyr semaphore test suite on qemu_cortex_m3.

This demonstrates the approach without committing to the full bindgen
maintenance burden. If it works, it can be extended to other primitives.

### Long term: Engage Zephyr community (Approach C)

If the proof of concept succeeds, propose Rust kernel module support to the
Zephyr project:

1. Present the proof of concept (one verified kernel primitive in Rust)
2. Propose `zephyr_rust_kernel_library()` CMake infrastructure
3. Propose `zephyr_kernel_sys` auto-generated bindings crate
4. Align with the existing `zephyr-lang-rust` effort
5. Seek TSC sponsorship

This is a long-term community effort. Gale should not block on it.

## Concrete Next Steps

### Step 1: Cross-language LTO evaluation (1-2 days)

- Determine if Zephyr can be built with Clang for qemu_cortex_m3
  (`-DZEPHYR_TOOLCHAIN_VARIANT=llvm`)
- Add `-C linker-plugin-lto` to the Gale FFI cargo build
- Verify that the `gale_sem_count_give` FFI function is inlined into
  `z_impl_k_sem_give` in the final ELF (inspect with `objdump -d`)
- Measure binary size difference
- **Deliverable**: Documentation of LTO results, proof that FFI boundary
  is zero-cost in practice

### Step 2: Coarsen FFI boundary (2-3 days)

- Refactor `ffi/src/lib.rs` to export higher-level operations
- Add `#[repr(C)]` state structs for semaphore, mutex, etc.
- Update C shims to use the coarser API
- Run all Zephyr test suites to confirm no regressions
- **Deliverable**: Fewer FFI functions, more logic in Rust, all tests pass

### Step 3: Bindgen proof of concept for semaphore (3-5 days)

- Create `gale-kernel/` crate (new, separate from `ffi/`)
- Use bindgen to generate Rust bindings for the ~10 Zephyr kernel-internal
  APIs that `gale_sem.c` uses
- Write `gale_sem_kernel.rs` implementing `z_impl_k_sem_give` etc.
  - Internally calls `gale::sem::Semaphore::give()` directly (Rust-to-Rust)
  - Externally exports `#[no_mangle] pub extern "C" fn z_impl_k_sem_give()`
    for Zephyr's C callers
- Wire into CMake as a replacement for `gale_sem.c`
- Test on qemu_cortex_m3 with Zephyr semaphore test suite
- **Deliverable**: Working semaphore with Rust-to-Rust internal linking,
  C-compatible external interface, all 24 tests passing

### Step 4: Evaluate and decide (1 day)

After Step 3, evaluate:
- How many Zephyr-internal APIs did bindgen need? How stable are they?
- How much `unsafe` code is in the new kernel crate vs. the old C shim?
- Is the maintenance burden acceptable?
- Does cross-language LTO make Approach B "good enough"?

If the bindgen surface is small and stable: proceed to extend to other
primitives. If it is large and fragile: stick with Approach B + LTO.

## Appendix: Why FFI Is Not Rust-to-C-to-Rust

A common misconception: "the FFI is Rust calling C calling Rust."

The actual flow is:

```
C (Zephyr kernel) -> C FFI call -> Rust (Gale)
```

There is only one language crossing. Zephyr's kernel is C. It calls Rust
via `extern "C"` functions. The Rust functions return. There is no
"C calling Rust calling C" -- the Rust functions are leaf functions that do
pure computation and return.

The concern motivating this research is not the ABI overhead (which LTO
eliminates) but the **type safety gap**: at the C call site, everything is
`u32`/`i32`. Rust's `Result`, `enum`, ownership, and borrow checking are
invisible to the C caller. By moving the call site into Rust, we get Rust's
full type system at the integration point where Gale's verified logic meets
Zephyr's kernel infrastructure.

## Appendix: Comparison with Kiln Path

| Aspect | In-kernel (this doc) | Kiln/WASM (`kiln-direct-rust-linking.md`) |
|--------|---------------------|------------------------------------------|
| Target | Kernel-internal code | Userspace application components |
| Language boundary | Rust-to-Rust (+ Rust-to-C for scheduler) | Rust-to-Rust (via Kiln host dispatch) |
| Interface contract | Rust crate dependency (types) | WIT interface definition (language-agnostic) |
| Isolation | None (kernel address space) | Component Model (capability-based) |
| Verification chain | Verus/Rocq (source only) | Verus -> Meld -> Loom -> Synth (full pipeline) |
| Runtime overhead | Zero (static linking) | Zero with Synth (compiled to direct calls) |
| Zephyr dependency | Requires Zephyr kernel headers | Independent (SWREQ-KILN-006) |
| Applicability | Zephyr-specific | Any Kiln-supported platform |

Both paths are valuable and complementary. The in-kernel path optimizes the
Zephyr deployment. The Kiln path enables platform-independent deployment with
end-to-end verified compilation.

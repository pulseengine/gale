# Research: Kiln Direct Rust Linking for Gale Kernel Primitives

**Date**: 2026-03-15
**Status**: Research / Proposal
**Related**: `artifacts/phase2_kiln_integration.yaml` (STKH-002, SWREQ-KILN-001..006)

## Current Architecture (Phase 1)

Gale's verified kernel primitives are deployed into Zephyr via a three-layer
indirection:

```
  Rust caller (Zephyr app or WASM component)
       |
       v
  Zephyr C API  (z_impl_k_sem_give, etc.)
       |
       v
  C shim        (gale_sem.c calls gale_sem_count_give)
       |
       v
  extern "C"    (ffi/src/lib.rs  #[no_mangle] functions)
       |
       v
  gale crate    (src/sem.rs  Semaphore::give — Verus-verified)
```

Key files:

| Layer | Path |
|-------|------|
| Verified Rust | `src/sem.rs`, `src/mutex.rs`, `src/condvar.rs`, etc. (Verus) |
| Plain Rust (no Verus) | `plain/src/sem.rs`, etc. (identical logic, `cargo test`-able) |
| FFI staticlib | `ffi/src/lib.rs` + `ffi/Cargo.toml` (crate-type = staticlib) |
| C headers | `ffi/include/gale_sem.h`, `gale_mutex.h`, etc. |
| C shims | `zephyr/gale_sem.c`, `gale_mutex.c`, etc. |
| Zephyr glue | `zephyr/CMakeLists.txt`, `zephyr/Kconfig` |

The FFI layer exposes pure functions (`gale_sem_count_give`, `gale_mutex_lock_validate`,
etc.) that compute the next state. The C shim integrates these into Zephyr's
wait queue, spinlock, scheduling, tracing, and poll infrastructure.

### What crosses the FFI boundary

Only **pure arithmetic/validation** crosses the boundary. For example, the
semaphore FFI exports three functions:

- `gale_sem_count_init(initial_count, limit) -> int32_t` -- parameter validation
- `gale_sem_count_give(count, limit) -> uint32_t` -- increment capped at limit
- `gale_sem_count_take(*count) -> int32_t` -- decrement or return EBUSY

Wait queues, scheduling, spinlocks, tracing, and poll events remain in Zephyr C.

## Proposed Architecture (Phase 2 -- Dual Mode)

```
                              +------------------+
                              |  Gale crate      |
                              |  (Verus-verified)|
                              +--------+---------+
                                       |
                        +--------------+--------------+
                        |                             |
              +---------v----------+       +----------v---------+
              |  Mode A: C FFI     |       |  Mode B: WIT/Kiln  |
              |  (Zephyr compat)   |       |  (direct Rust)     |
              +--------------------+       +--------------------+
              |  ffi/src/lib.rs    |       |  wit/gale.wit      |
              |  #[no_mangle]      |       |  WIT interface def |
              |  extern "C"        |       |                    |
              +--------+-----------+       +----------+---------+
                       |                              |
              +--------v-----------+       +----------v---------+
              |  C shim            |       |  Kiln host module  |
              |  gale_sem.c etc.   |       |  (Rust, no FFI)    |
              +--------+-----------+       +----------+---------+
                       |                              |
              +--------v-----------+       +----------v---------+
              |  Zephyr kernel     |       |  WASM component    |
              |  (native C)        |       |  (via Kiln runtime)|
              +--------------------+       +----------+---------+
                                                      |
                                           +----------v---------+
                                           |  Synth ARM codegen |
                                           |  (WASM -> ARM ELF) |
                                           +--------------------+
```

**Mode A** is the existing Phase 1 path. No changes.

**Mode B** is the new Kiln path:
1. Define WIT interfaces for each kernel primitive
2. Gale implements the WIT world as a Kiln host module
3. WASM components import the kernel interface
4. Kiln dispatches imports directly to Gale Rust methods
5. Synth compiles the WASM component to ARM, turning host calls into direct function calls

## PulseEngine Ecosystem Analysis

### Kiln (WebAssembly Runtime)

- Full Component Model and WASI 0.2 support
- `no_std` compatible, runs on Zephyr, Linux, macOS, QNX, VxWorks, Tock
- `kiln-component` crate has full WIT integration (`wit_integration.rs`, `wit_component_integration.rs`)
- `kiln-host` crate provides `HostIntegrationManager` with function registry (up to 256 host functions in `no_std`)
- `kiln-format` includes a WIT parser (`wit_parser.rs`, `wit_parser_bounded.rs`, `wit_parser_enhanced.rs`)
- `kiln-platform` already has `zephyr_sync.rs` and `zephyr_memory.rs` -- Zephyr is a first-class target
- Host functions are registered by name and called through the canonical ABI

### Meld (Static Component Fusion)

- Merges multiple WASM components into a single module at build time
- Eliminates inter-component overhead for known-at-build-time compositions
- Relevant for Gale: Meld can fuse the kernel host module with application components, making the call path zero-overhead

### Synth (WASM-to-ARM Compiler)

- Compiles WASM to bare-metal ARM Cortex-M ELF
- Has `synth-abi` crate for Component Model lift/lower
- Rocq proofs establish codegen correctness
- Host function calls in WASM become direct ARM function calls in the generated binary
- This means: WASM import of `kernel:sem/give` compiles to a direct `bl gale_sem_give` in ARM

### Loom (WASM Optimizer)

- Z3-backed translation validation for WASM optimizations
- Ensures optimized WASM preserves semantics of the original

### rules_wasm_component (Bazel Rules)

- `wit_library()` rule for defining WIT packages
- `wit_bindgen()` rule for generating Rust/Go/C++/Python bindings from WIT
- `symmetric_wit_bindgen()` for symmetric (p2) bindings
- Already integrated with Kiln's build system

## Approach Evaluation

### Option A: Conditional Compilation (Feature Flags)

```rust
// In gale/src/sem.rs -- no changes needed (already pure Rust)

// In a new gale-kiln/ crate:
impl KilnHostModule for GaleKernel {
    fn register(host: &mut HostIntegrationManager) {
        host.register("kernel:sem/give", |args| {
            let sem = args.get_resource::<Semaphore>(0)?;
            Ok(sem.give())
        });
    }
}

// ffi/src/lib.rs stays as-is for Mode A
```

**Pros**:
- Simplest implementation path
- Verified core code (`src/sem.rs`) unchanged
- Both modes share the same verified logic

**Cons**:
- No standard contract between caller and implementation
- Kiln integration is ad-hoc Rust code, not a formal interface
- Other languages cannot use the Kiln path (only Rust callers)
- No component isolation -- caller and kernel share address space directly

### Option B: WIT Component Model (Recommended)

Define kernel primitives as WIT interfaces. Applications import them. Kiln
implements them as host functions backed by Gale's Rust code.

**Pros**:
- Standard, language-agnostic contract (WIT)
- Component isolation via the Component Model
- Synth compiles host calls to direct ARM calls (zero runtime overhead in final binary)
- Meld can fuse components at build time (zero inter-component overhead)
- Existing tooling: `rules_wasm_component` has `wit_library`, `wit_bindgen`
- Kiln already has WIT parser and host function registry
- Aligns perfectly with SWREQ-KILN-001 through SWREQ-KILN-006
- Enables capability-based hardware access (SWREQ-KILN-005)

**Cons**:
- WIT interface design requires careful thought for RTOS semantics (timeouts, ISR context)
- Canonical ABI has overhead in the interpreted path (but Synth eliminates it)
- More upfront design work than Option A

### Option C: Trait-Based Abstraction

```rust
pub trait KernelSemaphore {
    fn init(&mut self, initial: u32, limit: u32) -> Result<(), KernelError>;
    fn give(&mut self) -> GiveResult;
    fn take(&mut self, wait: bool) -> TakeResult;
    fn reset(&mut self);
    fn count(&self) -> u32;
}
```

**Pros**:
- Pure Rust, no external tooling
- Trait can be used directly by Rust callers and wrapped for C FFI

**Cons**:
- Not a standard interface -- only useful within Rust
- Dynamic dispatch overhead (`dyn KernelSemaphore`) or monomorphization bloat
- Doesn't integrate with the Component Model pipeline
- Doesn't support capability-based isolation
- Doesn't enable the Meld/Loom/Synth verified compilation pipeline

## Recommendation: Option B (WIT Component Model)

Option B is the clear winner because it:

1. **Aligns with the existing requirements** (SWREQ-KILN-001 specifies WIT interfaces)
2. **Leverages the full PulseEngine pipeline**: WIT -> Kiln host -> Meld fusion -> Loom optimization -> Synth ARM codegen
3. **Eliminates overhead**: Synth turns WASM host imports into direct ARM function calls; Meld fuses components at build time
4. **Maintains backwards compatibility**: C shims remain for Zephyr kernel use; WIT path is additive
5. **Is language-agnostic**: Any language with a WIT bindgen (Rust, C, Go, Python via rules_wasm_component) can use Gale
6. **Has tooling ready**: `rules_wasm_component` already provides `wit_library()` and `wit_bindgen()` Bazel rules

Option A (traits + feature flags) can serve as a short-term stepping stone
for Rust-to-Rust callers before the full WIT pipeline is ready.

## WIT Interface Design

### Package Structure

```wit
// wit/gale.wit
//
// Package: pulseengine:gale@0.1.0
// World:   kernel-primitives

package pulseengine:gale@0.1.0;

/// Error codes matching Zephyr kernel conventions.
interface types {
    /// Kernel error type.
    enum kernel-error {
        /// Invalid argument (EINVAL).
        invalid-argument,
        /// Resource busy (EBUSY).
        busy,
        /// Operation cancelled (EAGAIN).
        again,
        /// No memory (ENOMEM).
        no-memory,
        /// Permission denied (EPERM).
        permission-denied,
        /// Broken pipe (EPIPE).
        broken-pipe,
        /// Overflow (EOVERFLOW).
        overflow,
        /// No message (ENOMSG).
        no-message,
    }

    /// Timeout specification for blocking operations.
    variant timeout {
        /// Do not wait; return immediately.
        no-wait,
        /// Wait indefinitely.
        forever,
        /// Wait for at most the given number of milliseconds.
        millis(u32),
    }
}

/// Counting semaphore.
///
/// Formally verified properties (ASIL-D):
///   P1: 0 <= count <= limit
///   P2: limit > 0
///   P3: give increments count, capped at limit
///   P5: take decrements count by 1
///   P6: take returns busy when count == 0 and no-wait
///   P9: no arithmetic overflow
interface semaphore {
    use types.{kernel-error, timeout};

    /// Opaque semaphore handle.
    resource sem {
        /// Create a new semaphore.
        constructor(initial-count: u32, limit: u32);

        /// Signal (give) the semaphore.
        give: func() -> result<_, kernel-error>;

        /// Wait (take) on the semaphore.
        take: func(timeout: timeout) -> result<_, kernel-error>;

        /// Reset the semaphore count to zero.
        reset: func();

        /// Get the current count.
        count: func() -> u32;
    }
}

/// Reentrant mutex with priority inheritance.
///
/// Formally verified: ownership, reentrancy bounds, lock-count arithmetic.
interface mutex {
    use types.{kernel-error, timeout};

    resource mtx {
        constructor();

        /// Lock the mutex (blocks if held by another thread).
        lock: func(timeout: timeout) -> result<_, kernel-error>;

        /// Unlock the mutex.
        unlock: func() -> result<_, kernel-error>;
    }
}

/// Condition variable.
interface condvar {
    use types.{kernel-error, timeout};

    resource cv {
        constructor();

        /// Wait on the condition variable (must hold associated mutex).
        wait: func(timeout: timeout) -> result<_, kernel-error>;

        /// Signal one waiting thread.
        signal: func();

        /// Broadcast to all waiting threads.
        broadcast: func();
    }
}

/// Fixed-size message queue.
interface message-queue {
    use types.{kernel-error, timeout};

    resource msgq {
        /// Create a message queue.
        /// msg-size: size of each message in bytes.
        /// max-msgs: maximum number of messages in the queue.
        constructor(msg-size: u32, max-msgs: u32);

        /// Put a message at the back of the queue.
        put: func(data: list<u8>, timeout: timeout) -> result<_, kernel-error>;

        /// Get a message from the front of the queue.
        get: func(timeout: timeout) -> result<list<u8>, kernel-error>;

        /// Peek at a message without removing it.
        peek: func() -> result<list<u8>, kernel-error>;

        /// Get the number of messages currently in the queue.
        num-used: func() -> u32;
    }
}

/// LIFO stack of pointer-sized values.
interface stack {
    use types.{kernel-error, timeout};

    resource stk {
        constructor(max-entries: u32);

        push: func(value: u64) -> result<_, kernel-error>;
        pop: func(timeout: timeout) -> result<u64, kernel-error>;
    }
}

/// Byte-stream pipe.
interface pipe {
    use types.{kernel-error, timeout};

    resource pip {
        constructor(size: u32);

        write: func(data: list<u8>, timeout: timeout) -> result<u32, kernel-error>;
        read: func(max-bytes: u32, timeout: timeout) -> result<list<u8>, kernel-error>;
        flush: func() -> result<_, kernel-error>;
        close: func();
    }
}

/// Event flags (bitmask signaling).
interface event {
    use types.{kernel-error, timeout};

    resource evt {
        constructor();

        post: func(bits: u32);
        set: func(bits: u32);
        clear: func(bits: u32);

        /// Wait for any of the specified bits.
        wait-any: func(bits: u32, timeout: timeout) -> result<u32, kernel-error>;
        /// Wait for all of the specified bits.
        wait-all: func(bits: u32, timeout: timeout) -> result<u32, kernel-error>;
    }
}

/// Timer (periodic or one-shot).
interface timer {
    use types.{kernel-error};

    resource tmr {
        constructor(period-ms: u32);

        start: func();
        stop: func();
        status-get: func() -> u32;
    }
}

/// Memory slab allocator (fixed-size block pool).
interface mem-slab {
    use types.{kernel-error, timeout};

    resource slab {
        constructor(block-size: u32, num-blocks: u32);

        alloc: func(timeout: timeout) -> result<u32, kernel-error>;
        free: func(block: u32) -> result<_, kernel-error>;
        num-used: func() -> u32;
        num-free: func() -> u32;
    }
}

/// World that applications import to use kernel primitives.
world kernel-primitives {
    import types;
    import semaphore;
    import mutex;
    import condvar;
    import message-queue;
    import stack;
    import pipe;
    import event;
    import timer;
    import mem-slab;
}
```

### Bazel Integration

```python
# wit/BUILD.bazel
load("@rules_wasm_component//wit:defs.bzl", "wit_library", "wit_bindgen")

wit_library(
    name = "gale_wit",
    srcs = ["gale.wit"],
    package = "pulseengine:gale@0.1.0",
    visibility = ["//visibility:public"],
)

wit_bindgen(
    name = "gale_host_bindings",
    wit = ":gale_wit",
    language = "rust",
    world = "kernel-primitives",
    direction = "import",  # Host implements these
)

wit_bindgen(
    name = "gale_guest_bindings",
    wit = ":gale_wit",
    language = "rust",
    world = "kernel-primitives",
    direction = "export",  # Guest imports these
)
```

## Kiln Host Module Implementation Sketch

```rust
// gale-kiln/src/lib.rs
//
// Kiln host module: registers Gale primitives as WASM host functions.
// No C FFI, no extern "C", no #[no_mangle].

#![no_std]

use gale::sem::Semaphore;
use gale::mutex::Mutex;
// ... other primitives

use kiln_host::HostIntegrationManager;
use kiln_component::types::Value;

pub fn register_gale_host_functions(host: &mut HostIntegrationManager) {
    // Semaphore
    host.register_resource("pulseengine:gale/semaphore", "sem", |args| {
        let initial = args[0].as_u32();
        let limit = args[1].as_u32();
        let sem = Semaphore::init(initial, limit)?;
        Ok(ResourceHandle::new(sem))
    });

    host.register_method("pulseengine:gale/semaphore", "sem", "give", |sem: &mut Semaphore, _args| {
        sem.give();
        Ok(vec![])
    });

    host.register_method("pulseengine:gale/semaphore", "sem", "take", |sem: &mut Semaphore, args| {
        let timeout = args[0].as_timeout();
        let result = sem.take(timeout);
        Ok(vec![Value::from(result)])
    });

    // ... mutex, condvar, msgq, stack, pipe, event, timer, mem_slab
}
```

## End-to-End Verified Pipeline

With WIT interfaces in place, the full verification chain becomes:

```
Source verification:
  Gale Rust code  ---[Verus/Rocq]---> functional correctness proven

Interface contract:
  gale.wit        ---[wit-bindgen]---> type-safe bindings (no marshaling bugs)

Component fusion:
  app.wasm + gale host  ---[Meld]---> single fused component

Optimization:
  fused.wasm  ---[Loom + Z3]---> optimized.wasm (semantics preserved)

Codegen:
  optimized.wasm  ---[Synth + Rocq]---> ARM ELF (codegen correctness proven)

Final binary:
  ARM ELF with direct bl calls to Gale functions
  (no WASM interpreter, no canonical ABI overhead, no C shim)
```

Every arrow in this chain has a mechanized proof or translation validation.
This closes SYSREQ-KILN-002 (end-to-end verified compilation pipeline).

## Concrete Next Steps

### Step 1: Write the WIT interface (1-2 days)

- Create `wit/gale.wit` based on the design above
- Add `wit/BUILD.bazel` with `wit_library` + `wit_bindgen` rules
- Validate with `wit-bindgen` that bindings compile
- Deliverable: WIT package `pulseengine:gale@0.1.0`

### Step 2: Create gale-kiln crate (3-5 days)

- New crate `gale-kiln/` that depends on `gale` (plain, no Verus)
- Implements `register_gale_host_functions()` for Kiln's `HostIntegrationManager`
- Maps WIT resource types to Gale structs
- Unit tests using Kiln's test infrastructure
- Deliverable: `gale-kiln` crate that registers all kernel primitives as Kiln host functions

### Step 3: Integration test with Kiln (2-3 days)

- Write a minimal WASM component that imports `pulseengine:gale/semaphore`
- Instantiate in Kiln with Gale host module registered
- Verify semaphore give/take/reset work through the WASM import path
- Deliverable: passing integration test in Kiln

### Step 4: Synth codegen test (2-3 days)

- Compile the test WASM component with Synth to ARM ELF
- Run on Renode Cortex-M4 emulator
- Verify host imports become direct `bl` calls (no interpreter)
- Deliverable: ARM binary executing Gale kernel primitives natively

### Step 5: Meld fusion test (1-2 days)

- Use Meld to fuse the test component with Gale host
- Verify the fused binary eliminates inter-component overhead
- Run Loom translation validation on the fused module
- Deliverable: zero-overhead fused binary with Z3 validation

### Step 6: Requirements traceability (1 day)

- Update `artifacts/phase2_kiln_integration.yaml` status from `draft` to `implemented`
- Add verification evidence links (test results, proof artifacts)
- Deliverable: SWREQ-KILN-001..006 satisfied

## Open Questions

1. **Timeout semantics in WIT**: WIT does not have a native "block the caller"
   concept. For the interpreted path, Kiln must handle blocking (e.g., via
   async/await or cooperative yield). For Synth-compiled code, blocking maps
   directly to Zephyr's `z_pend_curr`. How should the WIT interface express
   "this call may block the calling thread"?

2. **ISR context**: Zephyr kernel primitives behave differently in ISR vs
   thread context (e.g., `k_sem_take` with timeout is invalid in ISR). Should
   the WIT interface expose ISR-safety constraints, or should Kiln enforce
   this at the host level?

3. **Resource ownership**: WIT resources have ownership semantics (handles are
   linear). This maps well to Gale's `&mut self` pattern. But Zephyr kernel
   objects are typically shared (multiple threads access the same semaphore).
   The WIT interface may need `borrow<sem>` handles rather than owned handles.

4. **Wait queue integration**: In Phase 1, the wait queue stays in Zephyr C.
   For Phase 2 (SWREQ-KILN-006: no Zephyr dependency), Gale needs its own
   wait queue backed by Kiln's threading primitives (`kiln-platform` threading
   layer). This is a significant implementation step beyond the WIT interface.

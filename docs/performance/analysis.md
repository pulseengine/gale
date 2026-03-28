# Gale Performance and Memory Analysis

Gale replaces safety-critical arithmetic and state-machine logic in 25 Zephyr
kernel modules with formally verified Rust, linked via a C FFI static library
(`libgale_ffi.a`).  This document characterises the cost of that replacement
in binary size, RAM, stack usage, and call overhead.

## 1. Binary Size (FLASH / RAM)

### Measurement Method

Zephyr prints a "Memory region" summary at link time.  The CI size-comparison
job (`size-comparison` in `zephyr-tests.yml`) builds the semaphore test suite
twice -- once with the Gale overlay and once without -- and reports the
difference.

To measure locally:

```bash
# Baseline (stock Zephyr)
west build -b qemu_cortex_m3 -s zephyr/tests/kernel/semaphore/semaphore -d build-baseline
arm-zephyr-eabi-size build-baseline/zephyr/zephyr.elf

# With Gale
west build -b qemu_cortex_m3 -s zephyr/tests/kernel/semaphore/semaphore -d build-gale \
  -- -DZEPHYR_EXTRA_MODULES=/path/to/gale -DOVERLAY_CONFIG=/path/to/gale/zephyr/gale_overlay.conf
arm-zephyr-eabi-size build-gale/zephyr/zephyr.elf
```

### What contributes to size

| Component | Location | Notes |
|-----------|----------|-------|
| `libgale_ffi.a` | `ffi/target/<target>/release/` | Static lib, `opt-level = "z"`, fat LTO, `panic = "abort"` |
| C shim files | `zephyr/gale_*.c` | 20 files, each ~50-200 lines, replaces upstream `kernel/*.c` |
| Upstream exclusions | Zephyr `kernel/CMakeLists.txt` | sem.c, mutex.c, msg_q.c, etc. excluded when Gale is active |

The Rust library is compiled with aggressive size optimisation:
- `opt-level = "z"` (optimise for size)
- `lto = true` (fat LTO -- dead code elimination across the entire crate)
- `codegen-units = 1` (maximum cross-function optimisation)
- `panic = "abort"` (no unwinding tables)

Since Gale's FFI functions are pure scalar arithmetic (no heap, no alloc, no
format strings, no panic messages), the compiled code is very small.  The
120 exported `#[no_mangle]` functions are all leaf functions operating on
scalars and small `#[repr(C)]` structs.

### Expected impact

The Gale replacement is expected to be **roughly size-neutral** because:
- Rust functions replace equivalent C functions (same logic, same complexity)
- No Rust runtime, no alloc, no format infrastructure
- Fat LTO strips all unused code from the static library
- The upstream C files that Gale replaces are excluded from the build

Any measurable delta comes from:
- ABI differences (Rust function prologues may differ slightly from C)
- `overflow-checks = true` adds a few bytes per checked operation
- Minor differences in register allocation between LLVM (Rust) and GCC (Zephyr)


## 2. Stack Usage per FFI Function

### Design: no stack growth

Gale's FFI functions are pure -- they take scalar arguments and return scalar
results or small `#[repr(C)]` structs.  They do not:
- Allocate on the heap (no `alloc` crate)
- Use recursion
- Create large stack buffers
- Call back into C (no re-entrant FFI)

The largest return type is `GaleSemTakeDecision` (8 bytes: `i32` + `u32` + `u8`
padded to alignment).  Decision structs for other modules are similarly small.

### Measuring stack usage

**Compile-time** (GCC `-fstack-usage`):
The C shim files (`gale_*.c`) are compiled by Zephyr's GCC toolchain, which
supports `-fstack-usage`.  Add to the overlay:

```
CONFIG_COMPILER_STACK_USAGE=y
```

This generates `.su` files alongside each `.o` file, showing per-function
stack consumption.  Note: this only covers the C shim side, not the Rust
FFI functions themselves.

**Runtime** (Zephyr thread analyzer):

```
CONFIG_THREAD_ANALYZER=y
CONFIG_THREAD_ANALYZER_AUTO=y
CONFIG_THREAD_ANALYZER_AUTO_INTERVAL=5
CONFIG_INIT_STACKS=y
```

This reports per-thread stack high-water marks, which includes the full
call chain through C shim -> Rust FFI.

**Rust-side static analysis**:
LLVM does not emit `.su` files, but `cargo-call-stack` can produce call
graphs with stack annotations for `no_std` targets.  Since all Gale FFI
functions are leaf functions with no dynamic dispatch, the stack usage
is deterministic and trivially bounded.


## 3. Call Overhead (C -> Rust FFI)

### Theoretical overhead: zero

The Rust FFI functions use `extern "C"` ABI, which is identical to a normal
C function call.  There is no:
- Marshalling or serialisation
- Runtime type checking
- Trampoline or thunk
- Thread-local storage access
- Lock acquisition

The call is a direct `BL` (branch-and-link) instruction to the Rust function
symbol, same as calling any other C function.

### Practical overhead

The only measurable difference vs. inline C code:
- **Function call itself**: ~2-4 cycles for `BL` + `BX LR` (Cortex-M3)
- **Register save/restore**: the Rust function may save/restore a few
  callee-saved registers, same as any non-inlined function
- **Overflow checks**: `overflow-checks = true` adds a branch per checked
  arithmetic operation (~1 cycle, predictable)

For context, a Zephyr `z_pend_curr()` (the blocking path in sem/mutex take)
takes hundreds of cycles for context switching.  The FFI overhead is
negligible compared to the kernel operations it protects.

### Could we inline across the boundary?

Cross-language LTO (Clang + LLVM for both C and Rust) could theoretically
inline the Rust functions into the C call site, eliminating the call
overhead entirely.  See `docs/research/cross-language-lto-poc.md` for the
feasibility study.  This is not pursued because:
1. Zephyr's CI uses GCC, not Clang
2. The overhead is already negligible
3. Keeping the FFI boundary explicit aids auditability


## 4. Memory Safety Properties

### No dynamic allocation

Gale's verified code has **zero** heap usage:
- No `alloc` crate dependency
- No `Box`, `Vec`, `String`, or any heap type
- No `malloc`/`free` calls
- All state is passed as scalars or small stack-allocated structs

This means:
- **No memory leaks** -- there is nothing to leak
- **No use-after-free** -- no pointers to freed memory
- **No double-free** -- no free calls at all
- **No heap fragmentation** -- no heap

### No raw pointer dereference in verified code

The verified pure logic (`gale` crate, `src/*.rs`) is `#![deny(unsafe_code)]`.
The only `unsafe` is in the FFI boundary (`ffi/src/lib.rs`) for:
- `#[no_mangle]` attribute (required for C linkage)
- Pointer dereference in legacy v1 API (`gale_sem_count_take` takes `*mut u32`)

The v2 decision API eliminates even these pointer dereferences -- it returns
structs by value, and the C shim applies the result.


## 5. Zephyr Built-in Profiling Options

These Kconfig options can be added to `gale_overlay.conf` for runtime analysis:

### Thread stack analysis

```
CONFIG_THREAD_ANALYZER=y
CONFIG_THREAD_ANALYZER_AUTO=y
CONFIG_THREAD_ANALYZER_AUTO_INTERVAL=5
CONFIG_THREAD_ANALYZER_USE_PRINTK=y
CONFIG_INIT_STACKS=y
```

Reports per-thread: name, stack size, stack used (high-water mark), unused
bytes.  Useful for validating that Gale's FFI calls do not increase stack
pressure beyond what stock Zephyr uses.

### Thread runtime statistics

```
CONFIG_SCHED_THREAD_USAGE=y
CONFIG_SCHED_THREAD_USAGE_ANALYSIS=y
CONFIG_SCHED_THREAD_USAGE_ALL=y
```

Collects per-thread cycle counts at context switch time.  Enables comparing
CPU time spent in Gale-replaced code paths vs. stock Zephyr.

### Tracing

```
CONFIG_TRACING=y
```

Full tracing infrastructure (requires a backend like SEGGER SystemView or
CTF).  Heavyweight -- not recommended for size comparison, but useful for
latency profiling on real hardware.

### Stack canaries

```
CONFIG_STACK_CANARIES=y
```

Places canary values at stack boundaries.  Detects stack overflow at runtime.
Already enabled by default on most Zephyr boards.


## 6. CI Size Comparison

The `size-comparison` job in `.github/workflows/zephyr-tests.yml` automates
the measurement.  It:

1. Builds the semaphore test suite **without** Gale (stock Zephyr)
2. Records FLASH/RAM usage from `arm-zephyr-eabi-size`
3. Builds the same test **with** Gale overlay
4. Records FLASH/RAM usage
5. Computes and reports the delta

This runs on every push to `main` and on pull requests, so regressions in
binary size are caught automatically.

Additionally, the `scripts/size_compare.sh` script can be run locally to
perform the same comparison for any test suite.

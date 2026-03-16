# Research: Cross-Language LTO for Gale FFI Boundary Elimination

**Date**: 2026-03-16
**Status**: Proof of Concept
**Prerequisite**: Zephyr LLVM/Clang toolchain support

## Problem Statement

Gale's verified kernel primitives are called from C shims through an `extern "C"`
FFI boundary. Each call (e.g., `gale_sem_count_give`) passes through:

```
  Zephyr C code (gale_sem.c)
       |  bl gale_sem_count_give   <-- function call overhead
       v
  Rust FFI     (ffi/src/lib.rs)
       |
       v
  gale crate   (plain/src/sem.rs)
```

The Rust functions are pure arithmetic (no side effects, no allocations, no
syscalls). For example, `gale_sem_count_give` is just:

```rust
pub extern "C" fn gale_sem_count_give(count: u32, limit: u32) -> u32 {
    if count != limit { count + 1 } else { count }
}
```

This should be a single compare-and-increment instruction sequence, but the
FFI boundary forces a real function call: register spill, branch, return.
On Cortex-M3, each FFI call costs ~10-20 cycles of overhead for what could
be a 2-3 cycle inline operation.

## What Is Cross-Language LTO?

Both Clang and Rust use the LLVM backend. When both emit LLVM bitcode
instead of native object code, the LLVM linker can perform Link Time
Optimization (LTO) across the language boundary -- treating Rust and C
functions as if they were in the same compilation unit.

This means:
- **Function inlining**: `gale_sem_count_give` gets inlined directly into
  `z_impl_k_sem_give` in the C shim -- no function call at all.
- **Dead code elimination**: Unused Rust functions are stripped entirely.
- **Cross-language constant propagation**: If the C caller passes a compile-time
  constant, the optimizer can fold it through the Rust code.
- **Interprocedural optimization**: Register allocation and instruction
  scheduling can span both languages.

## Current Binary Size

Existing `libgale_ffi.a` built with `opt-level = "z"` and fat Rust-only LTO:

```
File:  ffi/target/thumbv7m-none-eabi/release/libgale_ffi.a
Size:  4.9 MB (5,090,802 bytes)
Type:  current ar archive (native ARM object code)
```

This archive contains native ARM object files. The linker only pulls in the
`.o` files that are actually referenced, so the final contribution to the
Zephyr binary is much smaller. However, the objects are opaque to the C-side
optimizer -- no cross-boundary optimization is possible.

## Zephyr Clang/LLVM Toolchain Support

Zephyr has first-class support for the LLVM toolchain:

**Toolchain variant: `llvm`**
- Location: `zephyr/cmake/toolchain/llvm/`
- Sets `COMPILER=clang`, `BINTOOLS=llvm`
- Configures target triple for ARM: `armv7m-none-eabi` (matches Rust's `thumbv7m-none-eabi`)
- Supports both `ld` (GNU) and `lld` (LLVM) linkers via `CONFIG_LLVM_USE_LLD`

**Toolchain variant: `clang` (host-installed clang)**
- Location: `zephyr/cmake/compiler/clang/`
- Inherits GCC compiler flags and overrides clang-specific ones
- Supports ARM, ARM64, RISC-V targets

**Zephyr LTO support:**
- `CONFIG_LTO=y` in `Kconfig.zephyr` enables link-time optimization
- GCC backend: `-flto=auto` / `-flto=1`
- LLD linker flags: currently empty (LLD handles LTO natively via bitcode)
- Dependency: requires `ISR_TABLES_LOCAL_DECLARATION` if using generated ISR tables

## How Cross-Language LTO Works

### Step 1: Rust emits LLVM bitcode

Add to the cargo build command:

```
RUSTFLAGS="-C linker-plugin-lto -C embed-bitcode=yes"
```

- `-C linker-plugin-lto`: Emits LLVM bitcode (`.bc`) in object files instead
  of native machine code. The resulting `.a` archive contains bitcode that
  the LLVM linker can optimize together with clang's bitcode.
- `-C embed-bitcode=yes`: Ensures bitcode is embedded in the object files
  (redundant when `linker-plugin-lto` is set, but explicit is good).

### Step 2: Clang emits LLVM bitcode

The C shims must also be compiled with Clang using `-flto=thin` or `-flto`:

```
ZEPHYR_TOOLCHAIN_VARIANT=llvm  (or clang)
CONFIG_LTO=y
CONFIG_LLVM_USE_LLD=y
```

This makes Clang emit LLVM bitcode for all C files. The linker (LLD) then
performs whole-program optimization across both Rust and C bitcode.

### Step 3: LLD links everything with LTO

LLD natively understands LLVM bitcode. When it encounters bitcode objects
(from both Rust and C), it runs the LLVM LTO pipeline on the merged module:

```
[Rust bitcode .o files] + [C bitcode .o files]
                    |
                    v
             LLVM LTO pipeline
                    |
                    v
            Optimized native ELF
```

### LLVM version compatibility

**Critical**: The Rust compiler and Clang must use the same LLVM version.
Mismatched LLVM versions produce incompatible bitcode.

Check versions:
```bash
rustc --version --verbose | grep LLVM
clang --version | grep version
```

As of Rust 1.85 (edition 2024, which Gale uses), rustc ships LLVM 19.
Clang 19 is required for compatibility.

## CMakeLists.txt Integration

Changes needed in `zephyr/CMakeLists.txt` to enable cross-language LTO:

```cmake
# In the cargo build command, add RUSTFLAGS for bitcode emission
set(GALE_CARGO_ENV
  GALE_MAX_SEMS=${CONFIG_GALE_MAX_SEMS}
  GALE_MAX_WAITERS=${CONFIG_GALE_MAX_WAITERS}
)

# When Zephyr is built with Clang + LTO, emit Rust bitcode too
if(CONFIG_LTO AND "${CMAKE_C_COMPILER_ID}" STREQUAL "Clang")
  set(GALE_RUSTFLAGS "-C linker-plugin-lto -C embed-bitcode=yes")
  list(APPEND GALE_CARGO_ENV RUSTFLAGS=${GALE_RUSTFLAGS})
endif()

add_custom_command(
  OUTPUT ${GALE_FFI_LIB}
  COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_ENV}
    cargo build --release ${CARGO_TARGET_ARGS}
    --manifest-path ${GALE_FFI_DIR}/Cargo.toml
  ...
)
```

The C shim compilation automatically picks up Clang + LTO flags from Zephyr's
build system when `ZEPHYR_TOOLCHAIN_VARIANT=llvm` and `CONFIG_LTO=y`.

## ffi/Cargo.toml Changes

The current Cargo.toml already has optimal settings for Rust-internal LTO:

```toml
[profile.release]
opt-level = "z"
lto = true         # fat LTO within Rust
codegen-units = 1
panic = "abort"
```

For cross-language LTO, `lto = true` (fat Rust LTO) is redundant because
the LLVM linker will perform LTO across everything. Change to:

```toml
[profile.release]
opt-level = "z"
lto = "thin"       # or false -- LLVM linker does the real LTO
codegen-units = 1
panic = "abort"
```

Using `lto = "thin"` is recommended: it allows the Rust compiler to do a
fast pre-link optimization pass, and the final cross-language LTO happens
at link time.

## Expected Benefits

### 1. FFI call elimination (primary)

Every `gale_*` function call from C into Rust becomes an inline expansion.
For `gale_sem_count_give`:

**Before** (native object linking):
```arm
  bl    gale_sem_count_give    ; function call
  ; ... use return value
```

**After** (cross-language LTO):
```arm
  cmp   r0, r1                ; count != limit?
  addne r0, r0, #1            ; count + 1
  ; ... use result directly, no call
```

Estimated savings: ~10-20 cycles per FFI call on Cortex-M3.

### 2. Binary size reduction

- Dead Rust functions (e.g., unused primitives) are eliminated entirely
- Inlined functions don't need stack frame setup / teardown code
- String tables from Rust panic infrastructure (if any leaks through) are stripped

### 3. Reduced register pressure

The compiler can allocate registers across the C/Rust boundary. No need
to save/restore caller-saved registers for the FFI call.

## Build Command (Full Example)

```bash
# Activate Zephyr environment
source /Volumes/Home/git/zephyr/.venv/bin/activate
export ZEPHYR_BASE=/Volumes/Home/git/zephyr/zephyr

# Build with Clang + LTO + cross-language LTO
west build -b qemu_cortex_m3 \
  -s zephyr/tests/kernel/semaphore/semaphore \
  -- \
  -DZEPHYR_TOOLCHAIN_VARIANT=llvm \
  -DCONFIG_LTO=y \
  -DCONFIG_LLVM_USE_LLD=y \
  -DZEPHYR_EXTRA_MODULES=/Volumes/Home/git/zephyr/gale \
  -DOVERLAY_CONFIG=/Volumes/Home/git/zephyr/gale/zephyr/gale_overlay.conf
```

The Gale CMakeLists.txt detects `CONFIG_LTO` + Clang and passes the
appropriate `RUSTFLAGS` to cargo automatically.

## Risks and Mitigations

### LLVM version mismatch
- **Risk**: Rust's bundled LLVM and the system Clang may differ.
- **Mitigation**: Pin Clang version to match `rustc --version --verbose | grep LLVM`.
  Nix can provide both from the same LLVM release.

### Thin LTO vs Fat LTO
- **Risk**: Thin LTO may miss some optimization opportunities vs fat LTO.
- **Mitigation**: Fat LTO (`-flto` without `=thin`) gives maximum optimization
  but slower builds. Use thin for development, fat for release.

### Debug info
- **Risk**: Cross-language LTO may complicate debugging (inlined Rust in C frames).
- **Mitigation**: Use `-C debuginfo=2` in RUSTFLAGS. LLVM preserves debug info
  through LTO. GDB/LLDB can still show Rust source for inlined code.

### ISR table generation
- **Risk**: `CONFIG_LTO` requires `ISR_TABLES_LOCAL_DECLARATION` or
  disabling `GEN_ISR_TABLES` / `GEN_IRQ_VECTOR_TABLE`.
- **Mitigation**: Cortex-M3 (qemu) uses `GEN_ISR_TABLES`; need to verify
  compatibility or enable `ISR_TABLES_LOCAL_DECLARATION`.

### Verification impact
- **Risk**: None. LTO is a linker optimization -- it does not change
  the source-level semantics. Verus proofs verify the Rust source,
  not the generated machine code.
- **Note**: If code-level certification (e.g., object code verification for
  DO-178C / ISO 26262 ASIL-D) is required, the LTO output would need
  separate analysis. But Gale's current ASIL-D strategy is source-level
  verification, so LTO is safe to use.

## Concrete Next Steps

1. **Verify LLVM version alignment**: Check that the installed Clang and
   `rustc`'s LLVM are the same major version.

2. **Build with bitcode**: Run cargo with `RUSTFLAGS="-C linker-plugin-lto"`
   and verify that `libgale_ffi.a` contains LLVM bitcode:
   ```bash
   RUSTFLAGS="-C linker-plugin-lto -C embed-bitcode=yes" \
     cargo build --manifest-path ffi/Cargo.toml \
     --release --target thumbv7m-none-eabi
   llvm-ar t ffi/target/thumbv7m-none-eabi/release/libgale_ffi.a
   llvm-objdump -d ffi/target/thumbv7m-none-eabi/release/libgale_ffi.a
   # Should show bitcode sections, not native ARM instructions
   ```

3. **West build with Clang + LTO**: Attempt a full Zephyr build using the
   LLVM toolchain variant with `CONFIG_LTO=y`.

4. **Compare binary sizes**: Build the semaphore test suite with and without
   cross-language LTO, compare `zephyr.elf` sizes.

5. **Inspect disassembly**: Verify that `gale_sem_count_give` is inlined
   into `z_impl_k_sem_give`:
   ```bash
   arm-none-eabi-objdump -d build/zephyr/zephyr.elf | grep -A 20 z_impl_k_sem_give
   ```

6. **Run tests**: Confirm all semaphore/mutex/msgq/stack/pipe tests still pass
   with the LTO build (functional correctness must be preserved).

7. **Automate**: Add a CMake option `CONFIG_GALE_CROSS_LANG_LTO=y` that
   automatically enables the RUSTFLAGS when building with Clang.

## References

- [Rust linker-plugin-lto documentation](https://doc.rust-lang.org/rustc/codegen-options/index.html#linker-plugin-lto)
- [Clang ThinLTO](https://clang.llvm.org/docs/ThinLTO.html)
- [Zephyr LLVM toolchain](https://docs.zephyrproject.org/latest/develop/toolchains/llvm.html)
- [Cross-language LTO in Firefox](https://bugzilla.mozilla.org/show_bug.cgi?id=1486025) (production precedent)

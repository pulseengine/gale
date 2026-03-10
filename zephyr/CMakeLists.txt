# Gale Zephyr Module — formally verified kernel semaphore
#
# When CONFIG_GALE_KERNEL_SEM is enabled:
#   1. Builds the gale-ffi Rust crate as a static library (libgale_ffi.a)
#   2. Compiles the C shim (gale_sem.c) that bridges Zephyr's k_sem API
#      to the Rust FFI
#   3. Links both into the kernel

if(CONFIG_GALE_KERNEL_SEM)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple from Zephyr's ARCH and CPU
# --------------------------------------------------------------------------
if(CONFIG_CPU_CORTEX_M0 OR CONFIG_CPU_CORTEX_M0PLUS)
  set(RUST_TARGET "thumbv6m-none-eabi")
elseif(CONFIG_CPU_CORTEX_M3)
  set(RUST_TARGET "thumbv7m-none-eabi")
elseif(CONFIG_CPU_CORTEX_M4 OR CONFIG_CPU_CORTEX_M7)
  if(CONFIG_FPU)
    set(RUST_TARGET "thumbv7em-none-eabihf")
  else()
    set(RUST_TARGET "thumbv7em-none-eabi")
  endif()
elseif(CONFIG_CPU_CORTEX_M33 OR CONFIG_CPU_CORTEX_M55 OR CONFIG_CPU_CORTEX_M85)
  if(CONFIG_FPU)
    set(RUST_TARGET "thumbv8m.main-none-eabihf")
  else()
    set(RUST_TARGET "thumbv8m.main-none-eabi")
  endif()
elseif(CONFIG_CPU_CORTEX_R5)
  set(RUST_TARGET "armv7r-none-eabihf")
elseif(CONFIG_RISCV)
  if(CONFIG_64BIT)
    set(RUST_TARGET "riscv64imac-unknown-none-elf")
  else()
    set(RUST_TARGET "riscv32imac-unknown-none-elf")
  endif()
elseif(CONFIG_X86)
  if(CONFIG_64BIT)
    set(RUST_TARGET "x86_64-unknown-none")
  else()
    set(RUST_TARGET "i686-unknown-none")
  endif()
else()
  # Fallback: native host (for qemu_native / native_sim testing)
  set(RUST_TARGET "")
endif()

# --------------------------------------------------------------------------
# Build Rust static library via cargo
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)
set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/release/libgale_ffi.a)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/release/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/release/libgale_ffi.a)
endif()

# Pass config to Rust via environment
set(GALE_CARGO_ENV
  GALE_MAX_SEMS=${CONFIG_GALE_MAX_SEMS}
  GALE_MAX_WAITERS=${CONFIG_GALE_MAX_WAITERS}
)

add_custom_command(
  OUTPUT ${GALE_FFI_LIB}
  COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_ENV}
    cargo build --release ${CARGO_TARGET_ARGS}
    --manifest-path ${GALE_FFI_DIR}/Cargo.toml
  DEPENDS
    ${GALE_FFI_DIR}/Cargo.toml
    ${GALE_FFI_DIR}/src/lib.rs
    ${GALE_DIR}/plain/src/error.rs
    ${GALE_DIR}/plain/src/lib.rs
  COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
  VERBATIM
)

add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})

# --------------------------------------------------------------------------
# C shim — bridges Zephyr k_sem API to Gale Rust FFI
# --------------------------------------------------------------------------
zephyr_library_named(gale_sem)
zephyr_library_sources(gale_sem.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
# Kernel-internal headers needed by the C shim (wait_q.h, ksched.h)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

# Link the Rust static library
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_sem gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_SEM

# ==========================================================================
# Gale mutex — formally verified mutex state machine
# ==========================================================================

if(CONFIG_GALE_KERNEL_MUTEX)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple (shared with sem, idempotent)
# --------------------------------------------------------------------------
if(NOT DEFINED RUST_TARGET)
  if(CONFIG_CPU_CORTEX_M0 OR CONFIG_CPU_CORTEX_M0PLUS)
    set(RUST_TARGET "thumbv6m-none-eabi")
  elseif(CONFIG_CPU_CORTEX_M3)
    set(RUST_TARGET "thumbv7m-none-eabi")
  elseif(CONFIG_CPU_CORTEX_M4 OR CONFIG_CPU_CORTEX_M7)
    if(CONFIG_FPU)
      set(RUST_TARGET "thumbv7em-none-eabihf")
    else()
      set(RUST_TARGET "thumbv7em-none-eabi")
    endif()
  elseif(CONFIG_CPU_CORTEX_M33 OR CONFIG_CPU_CORTEX_M55 OR CONFIG_CPU_CORTEX_M85)
    if(CONFIG_FPU)
      set(RUST_TARGET "thumbv8m.main-none-eabihf")
    else()
      set(RUST_TARGET "thumbv8m.main-none-eabi")
    endif()
  elseif(CONFIG_CPU_CORTEX_R5)
    set(RUST_TARGET "armv7r-none-eabihf")
  elseif(CONFIG_RISCV)
    if(CONFIG_64BIT)
      set(RUST_TARGET "riscv64imac-unknown-none-elf")
    else()
      set(RUST_TARGET "riscv32imac-unknown-none-elf")
    endif()
  elseif(CONFIG_X86)
    if(CONFIG_64BIT)
      set(RUST_TARGET "x86_64-unknown-none")
    else()
      set(RUST_TARGET "i686-unknown-none")
    endif()
  else()
    set(RUST_TARGET "")
  endif()
endif()

# --------------------------------------------------------------------------
# Build Rust static library via cargo (shared target with sem)
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/release/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/release/libgale_ffi.a)
endif()

# Ensure the FFI build target exists (may have been created by sem)
if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env
      cargo build --release ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS
      ${GALE_FFI_DIR}/Cargo.toml
      ${GALE_FFI_DIR}/src/lib.rs
      ${GALE_DIR}/plain/src/error.rs
      ${GALE_DIR}/plain/src/lib.rs
      ${GALE_DIR}/plain/src/mutex.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

# --------------------------------------------------------------------------
# C shim — bridges Zephyr k_mutex API to Gale Rust FFI
# --------------------------------------------------------------------------
zephyr_library_named(gale_mutex)
zephyr_library_sources(gale_mutex.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
# Kernel-internal headers needed by the C shim
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

# Link the Rust static library
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_mutex gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_MUTEX

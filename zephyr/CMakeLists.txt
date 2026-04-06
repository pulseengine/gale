# Gale Zephyr Module — formally verified kernel primitives
#
# When CONFIG_GALE_KERNEL_SEM is enabled:
#   1. Builds the gale-ffi Rust crate as a static library (libgale_ffi.a)
#   2. Compiles the C shim (gale_sem.c) that bridges Zephyr's k_sem API
#      to the Rust FFI
#   3. Links both into the kernel

# ==========================================================================
# Cross-language LTO support (LLVM toolchain only)
#
# When building with ZEPHYR_TOOLCHAIN_VARIANT=llvm and CONFIG_LTO=y,
# instruct Rust to emit LLVM bitcode via -C linker-plugin-lto.
# This allows the LLVM linker (lld) to perform cross-language link-time
# optimization, inlining Rust FFI functions directly into C call sites.
# ==========================================================================
set(GALE_CARGO_RUSTFLAGS_ENV "")
set(GALE_CARGO_PROFILE_ARGS "--release")
set(GALE_CARGO_PROFILE_DIR "release")
if("${ZEPHYR_TOOLCHAIN_VARIANT}" STREQUAL "llvm" AND CONFIG_LTO)
  # Tell rustc to emit LLVM bitcode for linker-plugin-based LTO.
  # The linker (lld) will merge C and Rust bitcode and optimize together.
  #
  # -Clinker-plugin-lto: emit LLVM bitcode so lld can do cross-language LTO
  # -Cembed-bitcode=yes: ensure bitcode is embedded (explicit for clarity)
  #
  # These are combined into a single RUSTFLAGS value. CMake's VERBATIM
  # mode ensures the space-separated flags are passed correctly via the
  # shell's quoting to cmake -E env.
  set(GALE_CARGO_RUSTFLAGS_ENV
    "RUSTFLAGS=-Clinker-plugin-lto -Cembed-bitcode=yes"
  )
  # Use the release-lto profile which sets lto=false so rustc does NOT
  # perform fat LTO internally. With lto=true (the default release profile),
  # rustc consumes all LLVM bitcode and emits native machine code — the .a
  # then contains no bitcode and lld cannot do cross-language optimization.
  # The release-lto profile inherits everything from release but leaves LTO
  # to the external linker.
  set(GALE_CARGO_PROFILE_ARGS "--profile" "release-lto")
  set(GALE_CARGO_PROFILE_DIR "release-lto")
  message(STATUS "Gale: cross-language LTO enabled (LLVM linker-plugin-lto)")
endif()

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

# x86 targets need static relocation model — the default PIC mode
# generates .got.plt sections that the Zephyr linker script discards,
# causing link failures.
if(CONFIG_X86)
  if(GALE_CARGO_RUSTFLAGS_ENV STREQUAL "")
    set(GALE_CARGO_RUSTFLAGS_ENV "RUSTFLAGS=-Crelocation-model=static")
  else()
    string(REPLACE "RUSTFLAGS=" "RUSTFLAGS=-Crelocation-model=static "
           GALE_CARGO_RUSTFLAGS_ENV "${GALE_CARGO_RUSTFLAGS_ENV}")
  endif()
endif()

# --------------------------------------------------------------------------
# Build Rust static library via cargo
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)
set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

# Pass config to Rust via environment
set(GALE_CARGO_ENV
  GALE_MAX_SEMS=${CONFIG_GALE_MAX_SEMS}
  GALE_MAX_WAITERS=${CONFIG_GALE_MAX_WAITERS}
)

# Collect Cargo features from Kconfig — only compile FFI code for enabled modules
set(GALE_CARGO_FEATURES "")
set(GALE_FEATURE_MAP
  CONFIG_GALE_KERNEL_SEM sem
  CONFIG_GALE_KERNEL_MUTEX mutex
  CONFIG_GALE_KERNEL_CONDVAR condvar
  CONFIG_GALE_KERNEL_MSGQ msgq
  CONFIG_GALE_KERNEL_PIPE pipe
  CONFIG_GALE_KERNEL_STACK stack
  CONFIG_GALE_KERNEL_TIMER timer
  CONFIG_GALE_KERNEL_MEM_SLAB mem_slab
  CONFIG_GALE_KERNEL_EVENT event
  CONFIG_GALE_KERNEL_FIFO fifo
  CONFIG_GALE_KERNEL_LIFO lifo
  CONFIG_GALE_KERNEL_QUEUE queue
  CONFIG_GALE_KERNEL_MBOX mbox
  CONFIG_GALE_KERNEL_TIMEOUT timeout
  CONFIG_GALE_KERNEL_POLL poll
  CONFIG_GALE_KERNEL_FUTEX futex
  CONFIG_GALE_KERNEL_TIMESLICE timeslice
  CONFIG_GALE_KERNEL_KHEAP kheap
  CONFIG_GALE_KERNEL_THREAD_LIFECYCLE thread_lifecycle
  CONFIG_GALE_KERNEL_WORK work
  CONFIG_GALE_KERNEL_FATAL fatal
  CONFIG_GALE_KERNEL_MEMPOOL mempool
  CONFIG_GALE_KERNEL_DYNAMIC dynamic
  CONFIG_GALE_KERNEL_SMP_STATE smp_state
  CONFIG_GALE_KERNEL_IPI ipi
  CONFIG_GALE_KERNEL_SCHED sched
  CONFIG_GALE_KERNEL_MEM_DOMAIN mem_domain
  CONFIG_GALE_KERNEL_USERSPACE userspace
  CONFIG_GALE_KERNEL_HEAP heap
  CONFIG_GALE_KERNEL_SPINLOCK_VALIDATE spinlock_validate
  CONFIG_GALE_KERNEL_SPINLOCK spinlock
  CONFIG_GALE_KERNEL_ATOMIC atomic
  CONFIG_GALE_KERNEL_CPU_MASK cpu_mask
)
list(LENGTH GALE_FEATURE_MAP _map_len)
math(EXPR _map_end "${_map_len} - 1")
foreach(_i RANGE 0 ${_map_end} 2)
  list(GET GALE_FEATURE_MAP ${_i} _config)
  math(EXPR _j "${_i} + 1")
  list(GET GALE_FEATURE_MAP ${_j} _feature)
  if(${_config})
    list(APPEND GALE_CARGO_FEATURES ${_feature})
  else()
    message(STATUS "Gale: feature '${_feature}' disabled (${_config} not set)")
  endif()
endforeach()

# ring_buf, bitarray, rbtree are utility modules — always include when any module is active
if(GALE_CARGO_FEATURES)
  list(APPEND GALE_CARGO_FEATURES ring_buf bitarray rbtree)
  list(JOIN GALE_CARGO_FEATURES "," GALE_CARGO_FEATURES_STR)
  set(GALE_CARGO_FEATURE_ARGS --no-default-features --features ${GALE_CARGO_FEATURES_STR})
else()
  set(GALE_CARGO_FEATURE_ARGS --no-default-features)
endif()

add_custom_command(
  OUTPUT ${GALE_FFI_LIB}
  COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_ENV} ${GALE_CARGO_RUSTFLAGS_ENV}
    cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
    ${GALE_CARGO_FEATURE_ARGS}
    --manifest-path ${GALE_FFI_DIR}/Cargo.toml
  DEPENDS
    ${GALE_FFI_DIR}/Cargo.toml
    ${GALE_FFI_DIR}/src/lib.rs
    ${GALE_DIR}/plain/src/error.rs
    ${GALE_DIR}/plain/src/lib.rs
  COMMENT "Building Gale FFI static library for ${RUST_TARGET} [features: ${GALE_CARGO_FEATURES_STR}]"
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
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

# Ensure the FFI build target exists (may have been created by sem)
if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
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

# ==========================================================================
# Gale message queue — formally verified ring buffer index arithmetic
# ==========================================================================

if(CONFIG_GALE_KERNEL_MSGQ)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple (shared, idempotent)
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
# Build Rust static library via cargo (shared with sem/mutex)
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS
      ${GALE_FFI_DIR}/Cargo.toml
      ${GALE_FFI_DIR}/src/lib.rs
      ${GALE_DIR}/plain/src/error.rs
      ${GALE_DIR}/plain/src/lib.rs
      ${GALE_DIR}/plain/src/msgq.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

# --------------------------------------------------------------------------
# C shim — bridges Zephyr k_msgq API to Gale Rust FFI
# --------------------------------------------------------------------------
zephyr_library_named(gale_msgq)
zephyr_library_sources(gale_msgq.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_msgq gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_MSGQ

# ==========================================================================
# Gale stack — formally verified LIFO count/capacity arithmetic
# ==========================================================================

if(CONFIG_GALE_KERNEL_STACK)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple (shared, idempotent)
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
# Build Rust static library via cargo (shared with sem/mutex/msgq)
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS
      ${GALE_FFI_DIR}/Cargo.toml
      ${GALE_FFI_DIR}/src/lib.rs
      ${GALE_DIR}/plain/src/error.rs
      ${GALE_DIR}/plain/src/lib.rs
      ${GALE_DIR}/plain/src/stack.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

# --------------------------------------------------------------------------
# C shim — bridges Zephyr k_stack API to Gale Rust FFI
# --------------------------------------------------------------------------
zephyr_library_named(gale_stack)
zephyr_library_sources(gale_stack.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_stack gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_STACK

# ==========================================================================
# Gale pipe — formally verified state machine + byte count validation
# ==========================================================================

if(CONFIG_GALE_KERNEL_PIPE)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple (shared, idempotent)
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
# Build Rust static library via cargo (shared with sem/mutex/msgq/stack)
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS
      ${GALE_FFI_DIR}/Cargo.toml
      ${GALE_FFI_DIR}/src/lib.rs
      ${GALE_DIR}/plain/src/error.rs
      ${GALE_DIR}/plain/src/lib.rs
      ${GALE_DIR}/plain/src/pipe.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

# --------------------------------------------------------------------------
# C shim — bridges Zephyr k_pipe API to Gale Rust FFI
# --------------------------------------------------------------------------
zephyr_library_named(gale_pipe)
zephyr_library_sources(gale_pipe.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_pipe gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_PIPE

# ==========================================================================
# Gale event — formally verified bitmask operations
# ==========================================================================

if(CONFIG_GALE_KERNEL_EVENT)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple (shared, idempotent)
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
# Build Rust static library via cargo (shared with sem/mutex/msgq/stack/pipe)
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS
      ${GALE_FFI_DIR}/Cargo.toml
      ${GALE_FFI_DIR}/src/lib.rs
      ${GALE_DIR}/plain/src/error.rs
      ${GALE_DIR}/plain/src/lib.rs
      ${GALE_DIR}/plain/src/event.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

# --------------------------------------------------------------------------
# C shim — bridges Zephyr k_event API to Gale Rust FFI
# --------------------------------------------------------------------------
zephyr_library_named(gale_event)
zephyr_library_sources(gale_event.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_event gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_EVENT

# ==========================================================================
# Gale timer — formally verified status counter arithmetic
# ==========================================================================

if(CONFIG_GALE_KERNEL_TIMER)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple (shared, idempotent)
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
# Build Rust static library via cargo (shared with sem/mutex/msgq/stack/pipe)
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS
      ${GALE_FFI_DIR}/Cargo.toml
      ${GALE_FFI_DIR}/src/lib.rs
      ${GALE_DIR}/plain/src/error.rs
      ${GALE_DIR}/plain/src/lib.rs
      ${GALE_DIR}/plain/src/timer.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

# --------------------------------------------------------------------------
# C shim — provides Gale-validated timer helpers for kernel/timer.c
# --------------------------------------------------------------------------
zephyr_library_named(gale_timer)
zephyr_library_sources(gale_timer.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_timer gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_TIMER

# ==========================================================================
# Gale memory slab — formally verified block count arithmetic
# ==========================================================================

if(CONFIG_GALE_KERNEL_MEM_SLAB)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple (shared, idempotent)
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
# Build Rust static library via cargo (shared with sem/mutex/msgq/stack/pipe/timer)
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS
      ${GALE_FFI_DIR}/Cargo.toml
      ${GALE_FFI_DIR}/src/lib.rs
      ${GALE_DIR}/plain/src/error.rs
      ${GALE_DIR}/plain/src/lib.rs
      ${GALE_DIR}/plain/src/mem_slab.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

# --------------------------------------------------------------------------
# C shim — bridges Zephyr k_mem_slab API to Gale Rust FFI
# --------------------------------------------------------------------------
zephyr_library_named(gale_mem_slab)
zephyr_library_sources(gale_mem_slab.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_mem_slab gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_MEM_SLAB

# ==========================================================================
# Gale fifo — formally verified unbounded queue count arithmetic
# ==========================================================================

if(CONFIG_GALE_KERNEL_FIFO)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple (shared, idempotent)
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
# Build Rust static library via cargo (shared with other primitives)
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
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
endif()

# --------------------------------------------------------------------------
# C shim — bridges Zephyr k_fifo API to Gale Rust FFI
# --------------------------------------------------------------------------
zephyr_library_named(gale_fifo)
zephyr_library_sources(gale_fifo.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_fifo gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_FIFO

# ==========================================================================
# Gale lifo — formally verified unbounded queue count arithmetic
# ==========================================================================

if(CONFIG_GALE_KERNEL_LIFO)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple (shared, idempotent)
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
# Build Rust static library via cargo (shared with other primitives)
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
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
endif()

# --------------------------------------------------------------------------
# C shim — bridges Zephyr k_lifo API to Gale Rust FFI
# --------------------------------------------------------------------------
zephyr_library_named(gale_lifo)
zephyr_library_sources(gale_lifo.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_lifo gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_LIFO

# ==========================================================================
# Gale queue — formally verified unbounded queue count arithmetic
# ==========================================================================

if(CONFIG_GALE_KERNEL_QUEUE)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple (shared, idempotent)
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
# Build Rust static library via cargo (shared with other primitives)
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
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
endif()

# --------------------------------------------------------------------------
# C shim — bridges Zephyr k_queue API to Gale Rust FFI
# --------------------------------------------------------------------------
zephyr_library_named(gale_queue)
zephyr_library_sources(gale_queue.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_queue gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_QUEUE

# ==========================================================================
# Gale mbox — formally verified stateless mailbox validation
# ==========================================================================

if(CONFIG_GALE_KERNEL_MBOX)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)

# --------------------------------------------------------------------------
# Determine Rust target triple (shared, idempotent)
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
# Build Rust static library via cargo (shared with other primitives)
# --------------------------------------------------------------------------
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(CARGO_TARGET_ARGS --target ${RUST_TARGET})
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(CARGO_TARGET_ARGS "")
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
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
endif()

# --------------------------------------------------------------------------
# C shim — bridges Zephyr k_mbox API to Gale Rust FFI
# --------------------------------------------------------------------------
zephyr_library_named(gale_mbox)
zephyr_library_sources(gale_mbox.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)

zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_mbox gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_MBOX

# ==========================================================================
# Gale timeout — formally verified tick arithmetic
# ==========================================================================

if(CONFIG_GALE_KERNEL_TIMEOUT)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(NOT DEFINED RUST_TARGET)
  set(RUST_TARGET "")
endif()

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_timeout)
zephyr_library_sources(gale_timeout.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_timeout gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_TIMEOUT

# ==========================================================================
# Gale poll — formally verified poll event state machine
# ==========================================================================

if(CONFIG_GALE_KERNEL_POLL)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_poll)
zephyr_library_sources(gale_poll.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_poll gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_POLL

# ==========================================================================
# Gale futex — formally verified futex value comparison
# ==========================================================================

if(CONFIG_GALE_KERNEL_FUTEX)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_futex)
zephyr_library_sources(gale_futex.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_futex gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_FUTEX

# ==========================================================================
# Gale timeslice — formally verified tick accounting
# ==========================================================================

if(CONFIG_GALE_KERNEL_TIMESLICE)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_timeslice)
zephyr_library_sources(gale_timeslice.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_timeslice gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_TIMESLICE

# ==========================================================================
# Gale kheap — formally verified byte-level allocation tracking
# ==========================================================================

if(CONFIG_GALE_KERNEL_KHEAP)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_kheap)
zephyr_library_sources(gale_kheap.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_kheap gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_KHEAP

# ==========================================================================
# Gale thread lifecycle — formally verified create/exit counting
# ==========================================================================

if(CONFIG_GALE_KERNEL_THREAD_LIFECYCLE)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_thread_lifecycle)
zephyr_library_sources(gale_thread_lifecycle.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_thread_lifecycle gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_THREAD_LIFECYCLE

# ==========================================================================
# Gale work — formally verified work item state machine
# ==========================================================================

if(CONFIG_GALE_KERNEL_WORK)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_work)
zephyr_library_sources(gale_work.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_work gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_WORK

# ==========================================================================
# Gale fatal — formally verified fatal error classification
# ==========================================================================

if(CONFIG_GALE_KERNEL_FATAL)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_fatal)
zephyr_library_sources(gale_fatal.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_fatal gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_FATAL

# ==========================================================================
# Gale mempool — formally verified fixed-block pool tracking
# ==========================================================================

if(CONFIG_GALE_KERNEL_MEMPOOL)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_mempool)
zephyr_library_sources(gale_mempool.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_mempool gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_MEMPOOL

# ==========================================================================
# Gale dynamic — formally verified dynamic thread pool tracking
# ==========================================================================

if(CONFIG_GALE_KERNEL_DYNAMIC)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_dynamic)
zephyr_library_sources(gale_dynamic.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_dynamic gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_DYNAMIC

# ==========================================================================
# Gale SMP state — formally verified SMP CPU state tracking
# ==========================================================================

if(CONFIG_GALE_KERNEL_SMP_STATE)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_smp_state)
zephyr_library_sources(gale_smp_state.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_smp_state gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_SMP_STATE

# ==========================================================================
# Gale IPI — formally verified IPI mask creation
# ==========================================================================

if(CONFIG_GALE_KERNEL_IPI)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_ipi)
zephyr_library_sources(gale_ipi.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_ipi gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_IPI

# ==========================================================================
# Gale sched — formally verified scheduler primitives
# ==========================================================================

if(CONFIG_GALE_KERNEL_SCHED)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_sched)
zephyr_library_sources(gale_sched.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_sched gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_SCHED

# ==========================================================================
# Gale mem_domain — formally verified partition management
# ==========================================================================
#
# Only compiled with CONFIG_USERSPACE (mem_domain.c is userspace-only).

if(CONFIG_GALE_KERNEL_MEM_DOMAIN)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_mem_domain)
zephyr_library_sources(gale_mem_domain.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_mem_domain gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_MEM_DOMAIN

# ==========================================================================
# Gale userspace — formally verified permission/type/init validation
# ==========================================================================

if(CONFIG_GALE_KERNEL_USERSPACE)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_userspace)
zephyr_library_sources(gale_userspace.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_userspace gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_USERSPACE

# ==========================================================================
# Gale sys_heap — formally verified chunk-level allocation invariants
# ==========================================================================
#
# NOTE: heap.c lives in lib/heap/, not kernel/.
# ZEPHYR_EXTRA_MODULES cannot selectively exclude lib/ source files.
#
# Integration options:
#   1. Patch lib/heap/CMakeLists.txt:
#        zephyr_sources_ifndef(CONFIG_GALE_KERNEL_HEAP heap.c)
#      Then this module's gale_heap.c replaces it entirely.
#   2. Link alongside upstream heap.c — decision functions act as
#      runtime guards (lower coverage but zero fork needed).
#
# For option 1, add to zephyr/lib/heap/CMakeLists.txt:
#   if(NOT CONFIG_GALE_KERNEL_HEAP)
#     zephyr_sources(heap.c)
#   endif()

if(CONFIG_GALE_KERNEL_HEAP)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_heap)
zephyr_library_sources(gale_heap.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/lib/heap
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_heap gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_HEAP

# ==========================================================================
# Gale bitarray — formally verified bounds validation
# ==========================================================================
#
# NOTE: This does NOT replace lib/utils/bitarray.c. It provides validated
# wrapper functions with Rust-verified bounds / overflow checks that
# delegate to the upstream implementation after validation passes.

if(CONFIG_GALE_KERNEL_BITARRAY)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_bitarray)
zephyr_library_sources(gale_bitarray.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_bitarray gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_BITARRAY

# ==========================================================================
# Gale red-black tree — formally verified color invariant validation
# ==========================================================================
#
# NOTE: This does NOT replace lib/utils/rb.c. It provides assertion
# helpers that verify RB-tree invariants (no red-red violation, correct
# color swap after rotation) via Rust FFI. The actual tree structure
# and all mutations remain in Zephyr's rb.c.

if(CONFIG_GALE_KERNEL_RB)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_rb)
zephyr_library_sources(gale_rb.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_rb gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_RB

# ==========================================================================
# Gale spinlock validate — formally verified spinlock ownership validation
# ==========================================================================
#
# Replaces spinlock_validate.c: lock/unlock validity checks and owner
# encoding are handled by verified Rust code.  The actual spinlock
# acquire/release mechanism and IRQ handling remain in Zephyr.

if(CONFIG_GALE_KERNEL_SPINLOCK_VALIDATE)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_spinlock_validate)
zephyr_library_sources(gale_spinlock_validate.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_spinlock_validate gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_SPINLOCK_VALIDATE

# ==========================================================================
# Gale CPU mask — formally verified CPU affinity mask arithmetic
# ==========================================================================

if(CONFIG_GALE_KERNEL_CPU_MASK)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_cpu_mask)
zephyr_library_sources(gale_cpu_mask.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_cpu_mask gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_CPU_MASK

# ==========================================================================
# Gale condvar — formally verified wait-queue decision functions
# ==========================================================================
#
# Replaces kernel/condvar.c: signal, broadcast, and wait action decisions
# are handled by verified Rust code via the Extract→Decide→Apply pattern.

if(CONFIG_GALE_KERNEL_CONDVAR)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_condvar)
zephyr_library_sources(gale_condvar.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_condvar gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_CONDVAR

# ==========================================================================
# Gale atomic — formally verified software atomic value arithmetic
# ==========================================================================
#
# Replaces kernel/atomic_c.c: the arithmetic transformations (add, sub, or,
# and, xor, nand, set, cas) are handled by verified Rust code.  Only
# applies when CONFIG_ATOMIC_OPERATIONS_C is selected.

if(CONFIG_GALE_KERNEL_ATOMIC)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_atomic)
zephyr_library_sources(gale_atomic.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_atomic gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_ATOMIC

# ==========================================================================
# Gale ring_buf — formally verified index arithmetic for ring_buffer.c
# ==========================================================================
#
# Does NOT replace lib/utils/ring_buffer.c.  Provides validated wrapper
# helpers for claim, finish, space_get, and size_get operations.

if(CONFIG_GALE_KERNEL_RING_BUF)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_ring_buf)
zephyr_library_sources(gale_ring_buf.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_ring_buf gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_RING_BUF

# ==========================================================================
# Gale spinlock — formally verified nesting-discipline state machine
# ==========================================================================
#
# Complements gale_spinlock_validate (low-level encoding) with the
# high-level ownership/nesting depth invariants from src/spinlock.rs.

if(CONFIG_GALE_KERNEL_SPINLOCK)

set(GALE_DIR ${CMAKE_CURRENT_LIST_DIR}/..)
set(GALE_FFI_DIR ${GALE_DIR}/ffi)

if(RUST_TARGET)
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${RUST_TARGET}/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
else()
  set(GALE_FFI_LIB ${GALE_FFI_DIR}/target/${GALE_CARGO_PROFILE_DIR}/libgale_ffi.a)
endif()

if(NOT TARGET gale_ffi_build)
  add_custom_command(
    OUTPUT ${GALE_FFI_LIB}
    COMMAND ${CMAKE_COMMAND} -E env ${GALE_CARGO_RUSTFLAGS_ENV}
      cargo build ${GALE_CARGO_PROFILE_ARGS} ${CARGO_TARGET_ARGS}
      --manifest-path ${GALE_FFI_DIR}/Cargo.toml
    DEPENDS ${GALE_FFI_DIR}/Cargo.toml ${GALE_FFI_DIR}/src/lib.rs
    COMMENT "Building Gale FFI static library for ${RUST_TARGET}"
    VERBATIM
  )
  add_custom_target(gale_ffi_build DEPENDS ${GALE_FFI_LIB})
endif()

zephyr_library_named(gale_spinlock)
zephyr_library_sources(gale_spinlock.c)
zephyr_library_include_directories(${GALE_DIR}/ffi/include)
zephyr_library_include_directories(
  ${ZEPHYR_BASE}/kernel/include
  ${ARCH_DIR}/${ARCH}/include
)
zephyr_library_link_libraries(${GALE_FFI_LIB})
add_dependencies(gale_spinlock gale_ffi_build)

endif() # CONFIG_GALE_KERNEL_SPINLOCK

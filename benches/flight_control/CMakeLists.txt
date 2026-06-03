# SPDX-License-Identifier: Apache-2.0
#
# Flight-control macro benchmark.
#
# Composes ring_buf + sem + mutex + msgq + condvar on a 100 Hz fixed-rate
# control loop. Sibling of benches/engine_control/ which exercises only
# ring_buf + sem. See docs/research/macro-bench-design.md for the design
# contract.
#
# Build baseline:
#   west build -b qemu_cortex_m3 -s benches/flight_control \
#     -d /tmp/flight-baseline
#
# Build with Gale primitives:
#   west build -b qemu_cortex_m3 -s benches/flight_control \
#     -d /tmp/flight-gale \
#     -- -DZEPHYR_EXTRA_MODULES=<gale-root> \
#        -DOVERLAY_CONFIG=<gale>/benches/flight_control/prj-gale.conf

cmake_minimum_required(VERSION 3.20)

find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(gale_flight_control)

target_sources(app PRIVATE
  src/main.c
)
target_include_directories(app PRIVATE src)

# --------------------------------------------------------------------------
# GALE_FC_WASM_LTO (opt-in, default OFF): build the flight algorithm
# (filter_step + controller_step) via the wasm-cross-LTO pipeline
# (clang -> wasm-ld -> loom dissolve -> synth -> Cortex-M object) instead of
# the native src/control.c, so the bench measures the dissolved algorithm
# head-to-head with the native build. Cortex-M4F target (nucleo_g474re);
# needs clang(wasm32), wasm-ld, loom, synth on PATH. A thin r11=0 trampoline
# (wasm/control_wasm_tramp.S) bridges Zephyr's calling convention to synth's
# linear-memory-base addressing. Functionally verified identical to native
# (cmd=0x07FDF307 on the reference vector). See ../engine_control/silicon/.
# --------------------------------------------------------------------------
option(GALE_FC_WASM_LTO
  "Link filter_step/controller_step via wasm-cross-LTO (synth Cortex-M) instead of native C" OFF)

if(GALE_FC_WASM_LTO)
  find_program(GALE_WLTO_CLANG  clang   REQUIRED)
  find_program(GALE_WLTO_WASMLD wasm-ld REQUIRED)
  find_program(GALE_WLTO_LOOM   loom    REQUIRED)
  find_program(GALE_WLTO_SYNTH  synth   REQUIRED)
  set(_fc_src ${CMAKE_CURRENT_SOURCE_DIR}/src/control.c)
  set(_fc_o   ${CMAKE_CURRENT_BINARY_DIR}/control_wasm.o)
  add_custom_command(
    OUTPUT ${_fc_o}
    COMMENT "flight_control: dissolving control.c via wasm-cross-LTO (clang->wasm-ld->loom->synth)"
    COMMAND ${GALE_WLTO_CLANG} --target=wasm32-unknown-unknown -O2 -nostdlib
            -I${CMAKE_CURRENT_SOURCE_DIR}/src -c ${_fc_src} -o control.wasm.o
    COMMAND ${GALE_WLTO_WASMLD} --no-entry --export=filter_step --export=controller_step
            --allow-undefined --gc-sections control.wasm.o -o control.wasm
    COMMAND ${GALE_WLTO_LOOM} optimize control.wasm --passes inline --attestation false
            -o control.loom.wasm
    COMMAND ${GALE_WLTO_SYNTH} compile control.loom.wasm --target cortex-m4f
            --all-exports --relocatable -o control.synth.o
    COMMAND ${CMAKE_OBJCOPY} --redefine-sym filter_step=synth_filter_step
            --redefine-sym controller_step=synth_controller_step control.synth.o ${_fc_o}
    DEPENDS ${_fc_src}
    WORKING_DIRECTORY ${CMAKE_CURRENT_BINARY_DIR}
    VERBATIM)
  add_custom_target(gale_fc_control_wasm DEPENDS ${_fc_o})
  set_source_files_properties(${_fc_o} PROPERTIES EXTERNAL_OBJECT TRUE GENERATED TRUE)
  target_sources(app PRIVATE wasm/control_wasm_tramp.S ${_fc_o})
  add_dependencies(app gale_fc_control_wasm)
  message(STATUS "flight_control: GALE_FC_WASM_LTO ON — control.c via wasm-cross-LTO (cortex-m4f)")
else()
  target_sources(app PRIVATE src/control.c)
endif()

# -DENGINE_BENCH_TOTAL_SAMPLES / -DENGINE_BENCH_SWEEP override the
# defaults in main.c. Reuses the engine_control naming so existing
# CI plumbing (run_qemu_bench.sh, env conventions) translates 1:1.
if(DEFINED ENGINE_BENCH_TOTAL_SAMPLES)
  target_compile_definitions(app PRIVATE
    TOTAL_SAMPLES=${ENGINE_BENCH_TOTAL_SAMPLES}u)
endif()

if(DEFINED ENGINE_BENCH_SWEEP)
  target_compile_definitions(app PRIVATE
    ENGINE_BENCH_SWEEP_${ENGINE_BENCH_SWEEP}=1)
endif()

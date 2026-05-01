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
  src/control.c
)
target_include_directories(app PRIVATE src)

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

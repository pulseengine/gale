# SPDX-License-Identifier: Apache-2.0
#
# Engine-control interrupt benchmark.
#
# Simulates a crank-position sensor firing at the rate of a 4-stroke ICE
# sweeping 500 → 10,000 RPM. Each simulated interrupt runs the Zephyr
# primitive chain (spinlock + ring_buf + k_sem give-from-isr + scheduler
# wake) that Gale replaces, allowing a back-to-back baseline-vs-Gale
# latency/jitter comparison.
#
# Build baseline:
#   west build -b qemu_cortex_m3 -s benches/engine_control \
#     -d /tmp/engine-baseline
#
# Build with Gale primitives:
#   west build -b qemu_cortex_m3 -s benches/engine_control \
#     -d /tmp/engine-gale \
#     -- -DZEPHYR_EXTRA_MODULES=<gale-root> \
#        -DOVERLAY_CONFIG=<gale>/zephyr/gale_overlay.conf

cmake_minimum_required(VERSION 3.20)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(gale_engine_control)

target_sources(app PRIVATE
  src/main.c
  src/control.c
  src/tables.c
)
target_include_directories(app PRIVATE src)

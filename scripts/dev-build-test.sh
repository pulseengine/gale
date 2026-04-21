#!/usr/bin/env bash
# Local Zephyr + Gale build / run loop.
#
# Mirrors what the Zephyr Kernel Tests CI workflow does, but against the
# workspace checkout at /Users/<you>/git/.../z/. Use this for fast
# iteration on a C shim or Rust FFI change without waiting on GitHub CI.
#
# Prerequisites (one-time):
#   1. pipx install --python python3.12 west
#   2. /Users/<you>/.local/pipx/venvs/west/bin/python -m pip install \
#        -r $ZEPHYR/scripts/requirements-base.txt
#   3. cd <workspace-root> && west init -l <zephyr-fork>
#   4. west update --narrow -o=--depth=1
#   5. west sdk install -t arm-zephyr-eabi
#   6. tar -xf <sdk>/gnu/toolchain_gnu_*-arm-zephyr-eabi.tar.xz -C <sdk>/gnu/
#   7. rustup target add thumbv7m-none-eabi
#
# Usage:
#   scripts/dev-build-test.sh [test-path] [overlay-conf]
# Defaults:
#   test-path   = tests/kernel/mem_heap/k_heap_api
#   overlay-conf = $GALE_ROOT/zephyr/gale_overlay.conf

set -euo pipefail

: "${ZEPHYR_WORKSPACE:=/Users/r/git/pulseengine/z}"
: "${ZEPHYR_BASE:=${ZEPHYR_WORKSPACE}/zephyr}"
: "${ZEPHYR_SDK_INSTALL_DIR:=/Users/r/zephyr-sdk-1.0.1}"
: "${GALE_ROOT:=${ZEPHYR_WORKSPACE}/gale}"
: "${BOARD:=qemu_cortex_m3}"
: "${BUILD_DIR:=/tmp/gale-build}"

export ZEPHYR_BASE ZEPHYR_SDK_INSTALL_DIR

TEST_PATH="${1:-tests/kernel/mem_heap/k_heap_api}"
OVERLAY="${2:-${GALE_ROOT}/zephyr/gale_overlay.conf}"

echo "==> Cleaning build dir: ${BUILD_DIR}"
rm -rf "${BUILD_DIR}"

echo "==> Building ${TEST_PATH} on ${BOARD} with overlay ${OVERLAY}"
cd "${ZEPHYR_WORKSPACE}"
west build -b "${BOARD}" \
  -d "${BUILD_DIR}" \
  -s "zephyr/${TEST_PATH}" \
  -- \
  -DZEPHYR_EXTRA_MODULES="${GALE_ROOT}" \
  -DOVERLAY_CONFIG="${OVERLAY}"

echo "==> Running ${BOARD} in QEMU (Ctrl-A X to exit early)"
timeout 90 west build -d "${BUILD_DIR}" -t run 2>&1 | tee "${BUILD_DIR}/test.log"

if grep -q "PROJECT EXECUTION SUCCESSFUL" "${BUILD_DIR}/test.log"; then
  echo "PASS"
  exit 0
else
  echo "FAIL"
  exit 1
fi

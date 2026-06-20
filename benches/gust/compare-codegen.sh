#!/usr/bin/env bash
# Codegen-size comparison: same gust_poll/gust_mix, native rustc->thumbv7m vs
# wasm->loom->synth->cortex-m3. Requires: rustc thumbv7m target, clang(wasm32),
# wasm-ld, loom, synth, arm-zephyr-eabi-nm on PATH (or via $TC).
set -euo pipefail
SCOUT="${SCOUT:-/tmp/gust-wasm-scout}"   # the pointer-passing kernel crate (gust_poll/gust_mix)
TC="${TC:-/Volumes/Home/zephyr-sdk/zephyr-sdk-1.0.1/gnu/arm-zephyr-eabi/bin/arm-zephyr-eabi}"
sz() { "$TC-nm" --print-size --size-sort "$1" 2>/dev/null | grep -iE 'gust_poll|gust_mix' \
  | python3 -c 'import sys
for l in sys.stdin:
    p=l.split()
    if len(p)>=4: print(f"  {p[-1]}: {int(p[1],16)} B")'; }
echo "NATIVE (rustc -> thumbv7m):"
( cd "$SCOUT" && cargo rustc --release --target thumbv7m-none-eabi --crate-type staticlib >/dev/null 2>&1 )
sz "$SCOUT"/target/thumbv7m-none-eabi/release/*.a
echo "DISSOLVED (wasm -> loom -> synth -> cortex-m3):"
sz "$(dirname "$0")"/wasm-kernel/gust_kernel-cortex-m3.o

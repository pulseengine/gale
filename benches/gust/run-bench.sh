#!/usr/bin/env bash
# Build + run the gust scheduler benchmark on qemu Cortex-M3 (deterministic, -icount).
set -euo pipefail; cd "$(dirname "$0")"
cargo build --release --bin bench
qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic -icount shift=1 \
  -semihosting-config enable=on,target=native -kernel target/thumbv7m-none-eabi/release/bench

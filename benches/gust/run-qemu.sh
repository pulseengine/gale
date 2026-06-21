#!/usr/bin/env bash
# Build + boot gust on a Cortex-M3 in qemu (lm3s6965evb). Zero-install local path.
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release
ELF=target/thumbv7m-none-eabi/release/gust
qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic -icount shift=1 \
  -semihosting-config enable=on,target=native -kernel "$ELF"

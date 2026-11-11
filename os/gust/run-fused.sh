#!/usr/bin/env bash
# Oracle for the full-pipeline demonstrator: boot the meld-fused, synth-dissolved
# Component-Model composition on a bare-metal Cortex-M3 (qemu lm3s6965evb) and
# assert run-demo() == 53 — the SAME result the composition produces on wasmtime
# (crates/gale-app-demo/run.sh). No wasm runtime is present on the metal.
#
# Kill-criterion: a value != 53, or a boot that prints nothing, fails the gate.
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --bin gust_fused
ELF=target/thumbv7m-none-eabi/release/gust_fused
out="$(qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic \
        -semihosting-config enable=on,target=native -kernel "$ELF" 2>&1 || true)"
echo "$out"
if echo "$out" | grep -q "run-demo() = 53"; then
  echo "[OK ] demonstrator: meld-fused CM composition dissolves to native and boots; run-demo() = 53 (== wasmtime)"
else
  echo "[BAD] demonstrator: expected 'run-demo() = 53'"; exit 1
fi

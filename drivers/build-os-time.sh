#!/usr/bin/env bash
# Build the gust:os v0.4.0 STEP-1 node: an app importing only gust:os/time, wac-plugged
# with a time provider (backed by gust:hal/mmio), dissolved to ONE relocatable object.
# component new ×2 -> wac plug -> meld fuse --memory shared -> loom inline -> synth
# --target cortex-m3 --all-exports --relocatable -> os-node/os-time-cm3.o. All-scalar
# time interface => 0 SRAM; the only import (TCB atom) is read32.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
WT="${WASM_TOOLS:-wasm-tools}"; MELD="${MELD:-meld}"; LOOM="${LOOM:-loom}"; SYNTH="${SYNTH:-synth}"
T="$(mktemp -d)"
( cd "$HERE/app-time" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
( cd "$HERE/time-provider" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
"$WT" component new "$(find "$HERE/app-time/target" -name gust_app_time.wasm|head -1)" -o "$T/app.comp.wasm"
"$WT" component new "$(find "$HERE/time-provider/target" -name gust_time_provider.wasm|head -1)" -o "$T/prov.comp.wasm"
wac plug "$T/app.comp.wasm" --plug "$T/prov.comp.wasm" -o "$T/os.composite.wasm"
"$MELD" fuse "$T/os.composite.wasm" --memory shared -o "$T/os.fused.wasm" >/dev/null 2>&1
"$LOOM" optimize "$T/os.fused.wasm" --passes inline --attestation false -o "$T/os.loom.wasm" >/dev/null 2>&1
"$SYNTH" compile "$T/os.loom.wasm" --target cortex-m3 --all-exports --relocatable -o "$HERE/os-node/os-time-cm3.o" >/dev/null 2>&1
echo "os-time-cm3.o: $(arm-none-eabi-size "$HERE/os-node/os-time-cm3.o" | awk 'NR==2{print "text="$1" data="$2" bss="$3}')"

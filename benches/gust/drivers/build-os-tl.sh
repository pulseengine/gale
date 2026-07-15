#!/usr/bin/env bash
# Build the gust:os v0.4.0 STEP-2 node: an app importing gust:os {time, log} —
# LOG IS THE FIRST BUFFER-CARRYING CAPABILITY (log.line takes list<u8>) — wac-plugged
# with a time provider + a log provider, dissolved to ONE relocatable object that fits
# the STM32F100 8 KiB SRAM budget (bounded-SRAM, NOT 0 — the canonical ABI needs real
# linmem for the buffer). component new x3 -> wac plug -> meld fuse --memory shared ->
# loom inline -> synth 0.43.0 --native-pointer-abi --shadow-stack-size 2048 ->
# os-node/os-tl-cm3.o.
#
# REQUIRES synth >= 0.43.0 (synth#746 FIXED: the log-memcpy function — an i64.load
# from a static above sp_init, previously synth-skipped — now compiles + relocates).
#
# NO WAT SURGERIES. Earlier revisions of this script carried two documented
# workarounds for meld#334 (S1: unify the fused module's duplicate __stack_pointer
# globals; S2: drop a vestigial mis-typed call_indirect shim export). Both are
# OBSOLETE at this toolchain pin: synth >= 0.39.1 absorbs the multi-SP rebase
# itself (synth#707 — "re-basing N aliased __stack_pointer globals ... shares one
# reservation"), and meld 0.41.0 fixes meld#334 at the source (SP coalescing +
# dead-shim DCE). The loom output now dissolves DIRECTLY, no WAT patching.
#
# RESULT (synth 0.43.0): text=1818 data=52 bss=3096 = 4966 B, fits 8192; 13
# functions, 2 external symbols (read32/write32 only — the mmio seam), NO skipped
# functions; exports gust:os/log@0.1.0#line + gust:os/time@0.1.0#{now,elapsed,
# deadline} + cabi_realloc.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
WT="${WASM_TOOLS:-wasm-tools}"; WAC="${WAC:-wac}"
MELD="${MELD:-$HOME/pe-toolchain/meld-0.41.0/meld-v0.41.0-aarch64-apple-darwin/meld}"
LOOM="${LOOM:-$HOME/pe-toolchain/loom-1.2.0/loom}"
SYNTH="${SYNTH:-$HOME/pe-toolchain/synth-0.43.0/synth}"
T="$(mktemp -d)"

# 1. build the three guest wasm modules
for c in app-tl time-provider log-provider; do
  ( cd "$HERE/$c" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
done
"$WT" component new "$(find "$HERE/app-tl/target"        -name gust_app_tl.wasm        | grep -v deps | head -1)" -o "$T/app.comp.wasm"
"$WT" component new "$(find "$HERE/time-provider/target" -name gust_time_provider.wasm | grep -v deps | head -1)" -o "$T/time.comp.wasm"
"$WT" component new "$(find "$HERE/log-provider/target"  -name gust_log_provider.wasm  | grep -v deps | head -1)" -o "$T/log.comp.wasm"

# 2. compose (app imports gust:os {time, log}; the two providers export them) + fuse
#    + inline
"$WAC" plug "$T/app.comp.wasm" --plug "$T/time.comp.wasm" --plug "$T/log.comp.wasm" -o "$T/composite.wasm"
"$MELD" fuse "$T/composite.wasm" --memory shared -o "$T/fused.wasm" >/dev/null 2>&1
"$LOOM" optimize "$T/fused.wasm" --passes inline --attestation false -o "$T/loom.wasm" >/dev/null 2>&1

# 3. dissolve to the bounded-SRAM relocatable object -- directly, no WAT surgery
mkdir -p "$HERE/os-node"
"$SYNTH" compile "$T/loom.wasm" --target cortex-m3 --all-exports --relocatable \
  --native-pointer-abi --shadow-stack-size 2048 -o "$HERE/os-node/os-tl-cm3.o"

arm-none-eabi-size "$HERE/os-node/os-tl-cm3.o" 2>/dev/null | awk 'NR==2{print "os-tl-cm3.o: text="$1" data="$2" bss="$3" ("$1+$2+$3" B / 8192 budget)"}' \
  || echo "os-tl-cm3.o: $(wc -c < "$HERE/os-node/os-tl-cm3.o") B"

#!/usr/bin/env bash
# Build the gust:os v0.4.0 STEP-3 node: an app importing gust:os {time, spawn} —
# SPAWN IS THE FIRST EXECUTOR-BACKED CAPABILITY (start/poll marshal onto the
# Verus+Kani-proven executor, plain/src/executor.rs, included verbatim by
# spawn-provider) — wac-plugged with a time provider + a spawn provider, dissolved
# to ONE relocatable object within the STM32F100 8 KiB SRAM budget.
# component new x3 -> wac plug -> meld fuse --memory shared -> loom inline ->
# synth --native-pointer-abi --shadow-stack-size 2048 -> os-node/os-ts-cm3.o.
#
# The executor's trusted `poll_task` dispatch crosses the WIT-typed
# gust:os/taskdisp seam (spawn-provider forwards its extern "C" poll_task to the
# taskdisp import — the design spawn-provider/RESULTS.md deferred; without it,
# `wasm-tools component new` rejects the raw env::poll_task core import). So the
# dissolved node's external symbols are read32/write32 (the mmio seam, from
# time-provider) + poll_task (the trusted dispatch seam), each resolved by the
# node's bridge/probe at final native link (gust_os_ts_probe locally).
#
# Same toolchain pins as build-os-tl.sh; NO WAT SURGERIES (see that script's
# header for why the meld#334-era workarounds are obsolete at this pin).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
WT="${WASM_TOOLS:-wasm-tools}"; WAC="${WAC:-wac}"
MELD="${MELD:-$HOME/pe-toolchain/meld-0.41.0/meld-v0.41.0-aarch64-apple-darwin/meld}"
LOOM="${LOOM:-$HOME/pe-toolchain/loom-1.2.0/loom}"
SYNTH="${SYNTH:-$HOME/pe-toolchain/synth-0.45.1/synth}"
T="$(mktemp -d)"

# 1. build the three guest wasm modules. spawn-provider's checked-in .cargo pins
#    -zstack-size=1024 for its SINGLE-component dissolve (RESULTS.md: no
#    --shadow-stack-size there, so wasm-ld must bound the stack). In THIS composed
#    node that pin puts spawn-provider's static data below the 1 MiB base, mixing
#    geometries with the default-stack app/time modules and forcing synth's
#    one-PROGBITS fallback (which rejects --shadow-stack-size, synth#383). Override
#    to the wasm-ld default 1 MiB so all three modules share the tl-node geometry —
#    the synth-side --shadow-stack-size 2048 shrink does the bounding, as in
#    build-os-tl.sh. (--allow-undefined is no longer needed: the trusted poll_task
#    is WIT-typed through gust:os/taskdisp, no raw env import remains.)
for c in app-ts time-provider; do
  ( cd "$HERE/$c" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
done
( cd "$HERE/spawn-provider" && RUSTFLAGS="-C link-arg=-zstack-size=1048576" \
    cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
"$WT" component new "$(find "$HERE/app-ts/target"         -name gust_app_ts.wasm         | grep -v deps | head -1)" -o "$T/app.comp.wasm"
"$WT" component new "$(find "$HERE/time-provider/target"  -name gust_time_provider.wasm  | grep -v deps | head -1)" -o "$T/time.comp.wasm"
"$WT" component new "$(find "$HERE/spawn-provider/target" -name gust_spawn_provider.wasm | grep -v deps | head -1)" -o "$T/spawn.comp.wasm"

# 2. compose (app imports gust:os {time, spawn}; the two providers export them) +
#    fuse + inline
"$WAC" plug "$T/app.comp.wasm" --plug "$T/time.comp.wasm" --plug "$T/spawn.comp.wasm" -o "$T/composite.wasm"
"$MELD" fuse "$T/composite.wasm" --memory shared -o "$T/fused.wasm" >/dev/null 2>&1
"$LOOM" optimize "$T/fused.wasm" --passes inline --attestation false -o "$T/loom.wasm" >/dev/null 2>&1

# 3. dissolve to the bounded-SRAM relocatable object -- directly, no WAT surgery
mkdir -p "$HERE/os-node"
"$SYNTH" compile "$T/loom.wasm" --target cortex-m3 --all-exports --relocatable \
  --native-pointer-abi --shadow-stack-size 2048 -o "$HERE/os-node/os-ts-cm3.o"

arm-none-eabi-size "$HERE/os-node/os-ts-cm3.o" 2>/dev/null | awk 'NR==2{print "os-ts-cm3.o: text="$1" data="$2" bss="$3" ("$1+$2+$3" B / 8192 budget)"}' \
  || echo "os-ts-cm3.o: $(wc -c < "$HERE/os-node/os-ts-cm3.o") B"

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
#
#    exec-provider joins the node because spawn-provider is now STATELESS: it holds
#    no executor of its own and imports gust:sched/tasks, which only the executor's
#    owner exports (one-scheduler-one-clock). Same stack-geometry override applies.
for c in app-ts time-provider; do
  ( cd "$HERE/$c" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
done
for c in spawn-provider exec-provider; do
  ( cd "$HERE/$c" && RUSTFLAGS="-C link-arg=-zstack-size=1048576" \
      cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
done
"$WT" component new "$(find "$HERE/app-ts/target"         -name gust_app_ts.wasm         | grep -v deps | head -1)" -o "$T/app.comp.wasm"
"$WT" component new "$(find "$HERE/time-provider/target"  -name gust_time_provider.wasm  | grep -v deps | head -1)" -o "$T/time.comp.wasm"
"$WT" component new "$(find "$HERE/spawn-provider/target" -name gust_spawn_provider.wasm | grep -v deps | head -1)" -o "$T/spawn.comp.wasm"
"$WT" component new "$(find "$HERE/exec-provider/target"  -name gust_exec_provider.wasm  | grep -v deps | head -1)" -o "$T/exec.comp.wasm"

# 2. compose + fuse + inline. `wac plug` no longer suffices: it wires plugs into the
#    SOCKET's imports only, never plug→plug, so spawn's gust:sched/tasks import would
#    survive as a residual (verified: the plug result still imports gust:sched/tasks
#    even with exec-provider passed as a fourth --plug). The spawn→exec edge has to be
#    named, so this is a `wac compose` script like build-fused-gustos.sh's.
cat > "$T/os-ts.wac" <<'WAC'
package gale:os-ts@0.1.0;
let x = new gust:exec-provider  { ... };
let t = new gust:time-provider  { ... };
let s = new gust:spawn-provider { "gust:sched/tasks@0.1.0": x["gust:sched/tasks@0.1.0"], ... };
let a = new gust:app-ts { "gust:os/time@0.1.0":  t["gust:os/time@0.1.0"],
                          "gust:os/spawn@0.1.0": s["gust:os/spawn@0.1.0"], ... };
export a["run"];
WAC
"$WAC" compose "$T/os-ts.wac" \
  --dep gust:app-ts="$T/app.comp.wasm" \
  --dep gust:time-provider="$T/time.comp.wasm" \
  --dep gust:spawn-provider="$T/spawn.comp.wasm" \
  --dep gust:exec-provider="$T/exec.comp.wasm" \
  -o "$T/composite.wasm"
"$MELD" fuse "$T/composite.wasm" --memory shared -o "$T/fused.wasm" >/dev/null 2>&1
"$LOOM" optimize "$T/fused.wasm" --passes inline --attestation false -o "$T/loom.wasm" >/dev/null 2>&1

# 3. dissolve to the bounded-SRAM relocatable object -- directly, no WAT surgery.
#    OSNODE is overridable so the pipeline can be exercised without overwriting the
#    checked-in object gust_os_ts_probe links against.
OSNODE="${OSNODE:-$HERE/os-node}"
mkdir -p "$OSNODE"
"$SYNTH" compile "$T/loom.wasm" --target cortex-m3 --all-exports --relocatable \
  --native-pointer-abi --shadow-stack-size 2048 -o "$OSNODE/os-ts-cm3.o"

arm-none-eabi-size "$OSNODE/os-ts-cm3.o" 2>/dev/null | awk 'NR==2{print "os-ts-cm3.o: text="$1" data="$2" bss="$3" ("$1+$2+$3" B / 8192 budget)"}' \
  || echo "os-ts-cm3.o: $(wc -c < "$OSNODE/os-ts-cm3.o") B"

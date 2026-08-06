#!/usr/bin/env bash
# EXECUTE the fused gust:os component on a host engine and observe the one-scheduler
# / one-clock properties (REQ-OS-COMPOSITE-EXEC-001 / VER-OS-COMPOSITE-EXEC-001).
#
# build-fused-gustos.sh gates the composite STRUCTURALLY (import routing across the
# unbundled core modules, executor-source ownership) and says so itself: "NOT lowered
# to native and NOT run on hardware". This script is the next rung: it RUNS the
# artifact that script produces, on wasmtime, with the host supplying the composite's
# two residual imports (gust:hal/mmio and gust:os/taskdisp). Host-engine only — it
# claims nothing about the dissolved/native path.
#
#   bash benches/gust/drivers/run-gustos-hostrun.sh          # compose + execute + controls
#   SKIP_BUILD=1 bash .../run-gustos-hostrun.sh              # reuse an existing composite
#
# Exit: 0 = the claims held; 1 = a claim was REFUTED (the valuable outcome — read the
# FAILED lines); 2 = the composite could not be instantiated; 3 = the claims held but
# a WIT return-contract deviation was observed; 4 = a NEGATIVE CONTROL did not fail,
# i.e. the harness is not actually a gate.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${OUT:-$HERE/gustos-components}"
WAC="${WAC:-wac}"
FUSED="$OUT/fused-gustos.component.wasm"

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  # `cmd | tail || exit` tests TAIL's status, not the script's — which is exactly how a
  # bash syntax error in build-fused-gustos.sh reached main unnoticed. Check the
  # producer's own status via PIPESTATUS.
  OUT="$OUT" bash "$HERE/build-fused-gustos.sh" | tail -2
  [ "${PIPESTATUS[0]}" -eq 0 ] || { echo "build-fused-gustos.sh FAILED — see above"; exit 1; }
  echo ""
fi
[ -f "$FUSED" ] || { echo "no composite at $FUSED (run build-fused-gustos.sh)"; exit 1; }

# --target is not optional: benches/gust/.cargo/config.toml pins build.target to
# thumbv7m-none-eabi for the firmware bins, and every crate below that directory
# inherits it — including this harness, which must be built FOR THE HOST.
HOST_TRIPLE="$(rustc -vV | sed -n 's/^host: //p')"
( cd "$HERE/gustos-hostrun" && cargo build --release --target "$HOST_TRIPLE" "$@" ) >/dev/null || exit 1
RUN="$HERE/gustos-hostrun/target/$HOST_TRIPLE/release/gustos-hostrun"

"$RUN" "$FUSED"
rc=$?

# ── negative controls ───────────────────────────────────────────────────────────
# A harness nobody has watched fail is not a gate. Both controls are composed from
# the SAME five unmodified provider components; only the wiring differs, which is
# exactly the class of mistake the structural gate exists to prevent.
deps=(
  --dep gust:time-provider="$OUT/time-provider.component.wasm"
  --dep gust:log-provider="$OUT/log-provider.component.wasm"
  --dep gust:spawn-provider="$OUT/spawn-provider.component.wasm"
  --dep gust:exec-provider="$OUT/exec-provider.component.wasm"
  --dep gust:timer-provider="$OUT/timer-provider.component.wasm"
)

# Control 1 — TWO scheduler instances: spawn binds to x's task table, timer to a
# second instance x2's. This is not a hypothetical shape. It is structurally
# INVISIBLE to every gate the repo already has, measured on this exact file:
#   - same world (5 exports, residual imports = gust:hal/mmio + gust:os/taskdisp),
#     so the residual-import gate passes;
#   - `wasm-tools component unbundle` yields the SAME 5 core modules with byte-for-
#     byte the same import routing, so build-fused-gustos.sh's gate 4 computes
#     n_mods=5 n_disp=1 n_spawn=1 n_timer=1 and ACCEPTS it;
#   - the second task table comes from `(instantiate 0)` appearing TWICE in the
#     component shell — one module, two `.bss` copies. A count over MODULES cannot
#     see a count over INSTANCES.
# Which is exactly why the property has to be EXECUTED to be known.
echo ""
echo "== negative control 1: two scheduler instances (the harness MUST refute) =="
cat > "$OUT/split-sched-gustos.wac" <<'WAC'
package gale:split-sched-gustos@0.1.0;

let x  = new gust:exec-provider  { ... };
let x2 = new gust:exec-provider  { ... };   // a SECOND task table
let t  = new gust:time-provider  { ... };
let l  = new gust:log-provider   { ... };
let s  = new gust:spawn-provider { "gust:sched/tasks@0.1.0": x["gust:sched/tasks@0.1.0"], ... };
let m  = new gust:timer-provider { "gust:sched/tasks@0.1.0": x2["gust:sched/tasks@0.1.0"],
                                   "gust:os/time@0.1.0":     t["gust:os/time@0.1.0"], ... };

export t["gust:os/time@0.1.0"];
export l["gust:os/log@0.1.0"];
export s["gust:os/spawn@0.1.0"];
export x["gust:os/exec@0.1.0"];
export m["gust:os/timer@0.1.0"];
WAC
"$WAC" compose "$OUT/split-sched-gustos.wac" "${deps[@]}" -o "$OUT/split-sched-gustos.component.wasm" || exit 1
"$RUN" "$OUT/split-sched-gustos.component.wasm" | grep -E '^\s+(FAILED|\[FAIL\])|^RESULT|REQ-OS-COMPOSITE'
nrc=${PIPESTATUS[0]}
if [ "$nrc" -ne 1 ]; then
  echo "  CONTROL DID NOT FAIL (exit $nrc): the harness accepts a two-scheduler composite"
  exit 4
fi
echo "  ok: the harness refuted the two-scheduler composite (exit 1) — it is a live gate"

# Control 2 — TWO time-provider instances: timer's `gust:os/time` import is bound to
# a second instance, while `gust:os/time` is exported from the first. This is the
# harness's KNOWN BLIND SPOT and is recorded, not gated: time-provider is stateless
# and both instances read the same TIM2_CNT through the same host mmio import, so the
# two are indistinguishable at the seam — one clock SOURCE reached twice. A second
# clock with a different source or offset WOULD be caught (checks 4c/4e pin the
# deadline to the value the host served to the observed read, and 4f rejects any read
# of a second register).
echo ""
echo "== negative control 2: two time-provider instances (known blind spot) =="
cat > "$OUT/split-clock-gustos.wac" <<'WAC'
package gale:split-clock-gustos@0.1.0;

let x  = new gust:exec-provider  { ... };
let t  = new gust:time-provider  { ... };
let t2 = new gust:time-provider  { ... };   // a SECOND clock instance
let l  = new gust:log-provider   { ... };
let s  = new gust:spawn-provider { "gust:sched/tasks@0.1.0": x["gust:sched/tasks@0.1.0"], ... };
let m  = new gust:timer-provider { "gust:sched/tasks@0.1.0": x["gust:sched/tasks@0.1.0"],
                                   "gust:os/time@0.1.0":     t2["gust:os/time@0.1.0"], ... };

export t["gust:os/time@0.1.0"];
export l["gust:os/log@0.1.0"];
export s["gust:os/spawn@0.1.0"];
export x["gust:os/exec@0.1.0"];
export m["gust:os/timer@0.1.0"];
WAC
"$WAC" compose "$OUT/split-clock-gustos.wac" "${deps[@]}" -o "$OUT/split-clock-gustos.component.wasm" || exit 1
"$RUN" "$OUT/split-clock-gustos.component.wasm" | grep -E 'REQ-OS-COMPOSITE|^RESULT'
echo "  (recorded, not gated — see the comment above this control in the script)"

exit "$rc"

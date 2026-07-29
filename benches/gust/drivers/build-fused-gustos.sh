#!/usr/bin/env bash
# Fuse the five gust:os provider components into ONE component that exports the
# gust:os capability set (DD-OS-DELIVERY-001, gale#224 — the v0.6.0-rc MUST item).
#
# The delivery claim this script is the oracle for: a downstream that wants gust's OS
# pulls ONE signed component, and the only things it must still resolve natively are
# the hardware seam (gust:hal/mmio) and its own task dispatch (gust:os/taskdisp). No
# scheduler, heap, `env` or WASI symbol leaks out — those are the core-module shape
# the release tarball ships, and they are exactly what this gate forbids.
#
#   bash benches/gust/drivers/build-fused-gustos.sh          # build + compose + verify
#   OUT=/some/dir bash .../build-fused-gustos.sh             # choose output dir
#
# NOT done here: lowering the composite to native (synth/meld dissolve) or running it
# on hardware. This produces and gates a wasm COMPONENT only. See FUSED-GUSTOS.md.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${OUT:-$HERE/gustos-components}"
WT="${WASM_TOOLS:-wasm-tools}"
WAC="${WAC:-wac}"
FUSED="$OUT/fused-gustos.component.wasm"

command -v "$WAC" >/dev/null || { echo "wac not found (cargo install wac-cli)"; exit 1; }
printf 'tools: %s / wac %s\n\n' "$("$WT" --version)" "$("$WAC" --version | awk '{print $2}')"

# Build + gate the five providers first; that script owns the per-provider invariant
# (exports gust:os/*, imports nothing outside gust:hal/* and gust:os/*).
OUT="$OUT" bash "$HERE/build-gustos-components.sh"
echo ""

# The composition graph. Deliberately a `wac compose` UNION, not a `wac plug` chain:
# the five providers form a flat antichain — no provider consumes another provider's
# export, so there is no import/export edge for `plug` to connect (it exits with
# "the socket component had no matching imports for the plugs that were provided").
# `{ ... }` forwards each instance's unsatisfied imports to the composite's own
# imports, which is what leaves gust:hal/mmio + gust:os/taskdisp as the residuals.
cat > "$OUT/fused-gustos.wac" <<'WAC'
package gale:fused-gustos@0.1.0;

let t = new gust:time-provider  { ... };
let l = new gust:log-provider   { ... };
let s = new gust:spawn-provider { ... };
let x = new gust:exec-provider  { ... };
let m = new gust:timer-provider { ... };

export t["gust:os/time@0.1.0"];
export l["gust:os/log@0.1.0"];
export s["gust:os/spawn@0.1.0"];
export x["gust:os/exec@0.1.0"];
export m["gust:os/timer@0.1.0"];
WAC

echo "== compose =="
"$WAC" compose "$OUT/fused-gustos.wac" \
  --dep gust:time-provider="$OUT/time-provider.component.wasm" \
  --dep gust:log-provider="$OUT/log-provider.component.wasm" \
  --dep gust:spawn-provider="$OUT/spawn-provider.component.wasm" \
  --dep gust:exec-provider="$OUT/exec-provider.component.wasm" \
  --dep gust:timer-provider="$OUT/timer-provider.component.wasm" \
  -o "$FUSED"
echo "  composed → $FUSED ($(wc -c < "$FUSED" | tr -d ' ') B)"

fail=""
note() { echo "  FAIL: $1"; fail="$fail;$1"; }

echo ""
echo "== verify =="
"$WT" validate --features=all "$FUSED" && echo "  ok: wasm-tools validate"
wit="$("$WT" component wit "$FUSED")" && echo "  ok: is a component (component wit decodes)"

# 1. Exports: every gust:os capability the five providers supply must survive.
for iface in time log spawn exec timer; do
  printf '%s' "$wit" | grep -qE "^\s*export gust:os/$iface@" \
    || note "composite does not export gust:os/$iface"
done
n_exports="$(printf '%s' "$wit" | grep -cE '^\s*export ' || true)"
[ "$n_exports" -eq 5 ] || note "expected exactly 5 exports, got $n_exports"

# 2. Residual imports: ONLY the hardware seam and the app-supplied dispatch. Unlike
# the per-provider gate this is exact, not a family match — after fusing, a stray
# gust:os/* import would mean a capability the composite failed to satisfy in-place.
imports="$(printf '%s' "$wit" | grep -E '^\s*import ' | sed 's/^ *//;s/;$//' || true)"
bad="$(printf '%s' "$imports" | grep -vE '^import (gust:hal/[a-z0-9-]+|gust:os/taskdisp)@' || true)"
[ -n "$bad" ] && note "residual imports outside gust:hal/* + gust:os/taskdisp: $(printf '%s' "$bad" | tr '\n' ' ')"

# 3. Deny-list sweep over EVERY raw core-wasm import string in the binary, not just
# the component-level world — a leak hiding in an inner core module would still be an
# undefined symbol the downstream has to resolve at final link.
raw="$("$WT" print "$FUSED" 2>/dev/null | grep -oE '\(import "[^"]*"' | sed 's/(import "//;s/"$//' | sort -u)"
leaks="$(printf '%s' "$raw" | grep -iE '^env$|^wasi|^GOT\.|malloc|calloc|sbrk|__heap|poll_task|scheduler|task_' || true)"
[ -n "$leaks" ] && note "core import leak (env/WASI/heap/scheduler): $(printf '%s' "$leaks" | tr '\n' ' ')"

if [ -z "$fail" ]; then
  echo "  ok: exports 5 gust:os interfaces; residual imports ="
  printf '%s\n' "$imports" | sed 's/^/        /'
  echo "  ok: no env / WASI / heap / scheduler symbol in any core import"
fi

echo ""
printf '%s\n' "$wit"
echo ""
if [ -n "$fail" ]; then
  echo "fused gust:os component invariant FAILED:$fail"; exit 1
fi
echo "fused gust:os component OK: $FUSED ($(wc -c < "$FUSED" | tr -d ' ') B)"
echo "NOT lowered to native and NOT run on hardware — that is a separate step."

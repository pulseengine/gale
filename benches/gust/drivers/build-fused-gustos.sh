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

# The composition graph. `wac compose` over an explicit WAC script, still — but no
# longer a bare union: the providers are no longer a flat antichain. exec-provider
# owns the one executor and exports `gust:sched/tasks`; spawn/timer are stateless and
# consume it, and timer additionally consumes time-provider's `gust:os/time` instead
# of reading a hardware register behind time's back. Those edges are wired by NAME
# below. `wac plug` does connect them (it now finds matching imports, which it could
# not before) but a plug result exports only the SOCKET's exports, so it cannot
# produce a composite exporting all five capabilities — see FUSED-GUSTOS.md.
#
# `{ ... }` still forwards each instance's REMAINING unsatisfied imports to the
# composite's own imports, which is what leaves gust:hal/mmio + gust:os/taskdisp —
# and only those — as the residuals.
cat > "$OUT/fused-gustos.wac" <<'WAC'
package gale:fused-gustos@0.1.0;

let x = new gust:exec-provider  { ... };
let t = new gust:time-provider  { ... };
let l = new gust:log-provider   { ... };
let s = new gust:spawn-provider { "gust:sched/tasks@0.1.0": x["gust:sched/tasks@0.1.0"], ... };
let m = new gust:timer-provider { "gust:sched/tasks@0.1.0": x["gust:sched/tasks@0.1.0"],
                                  "gust:os/time@0.1.0":     t["gust:os/time@0.1.0"], ... };

export t["gust:os/time@0.1.0"];
export l["gust:os/log@0.1.0"];
export s["gust:os/spawn@0.1.0"];
export x["gust:os/exec@0.1.0"];
export m["gust:os/timer@0.1.0"];
WAC

# The same five deps every compose in this script uses.
deps=(
  --dep gust:time-provider="$OUT/time-provider.component.wasm"
  --dep gust:log-provider="$OUT/log-provider.component.wasm"
  --dep gust:spawn-provider="$OUT/spawn-provider.component.wasm"
  --dep gust:exec-provider="$OUT/exec-provider.component.wasm"
  --dep gust:timer-provider="$OUT/timer-provider.component.wasm"
)

echo "== compose =="
"$WAC" compose "$OUT/fused-gustos.wac" "${deps[@]}" -o "$FUSED"
echo "  composed → $FUSED ($(wc -c < "$FUSED" | tr -d ' ') B)"

fail=""
note() { echo "  FAIL: $1"; fail="$fail;$1"; }

# THE residual-import gate, factored out so the negative self-test at the bottom can
# run the identical code on a deliberately mis-composed input. Prints the offending
# imports and returns non-zero; silent and zero when the composite is right.
#
# Exactly gust:hal/* + gust:os/taskdisp. Stricter than the per-provider gate, which
# also allows gust:sched/*: gust:sched is the node's INTERNAL scheduler seam, so a
# gust:sched residual means spawn/timer were not actually bound to exec-provider's
# executor — the composite would hand a downstream the power to mutate scheduler
# state, and the "one scheduler" property would be a claim rather than a fact.
check_residuals() {
  local w b
  w="$("$WT" component wit "$1")"
  b="$(printf '%s' "$w" | grep -E '^\s*import ' | sed 's/^ *//;s/;$//' \
       | grep -vE '^import (gust:hal/[a-z0-9-]+|gust:os/taskdisp)@' || true)"
  [ -z "$b" ] && return 0
  printf '%s\n' "$b" | sed 's/^/        unexpected residual: /'
  return 1
}

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

# 2. Residual imports (see check_residuals above).
imports="$(printf '%s' "$wit" | grep -E '^\s*import ' | sed 's/^ *//;s/;$//' || true)"
check_residuals "$FUSED" || note "residual imports outside gust:hal/* + gust:os/taskdisp"

# 3. Deny-list sweep over EVERY raw core-wasm import string in the binary, not just
# the component-level world — a leak hiding in an inner core module would still be an
# undefined symbol the downstream has to resolve at final link.
raw="$("$WT" print "$FUSED" 2>/dev/null | grep -oE '\(import "[^"]*"' | sed 's/(import "//;s/"$//' | sort -u)"
leaks="$(printf '%s' "$raw" | grep -iE '^env$|^wasi|^GOT\.|malloc|calloc|sbrk|__heap|poll_task|scheduler|task_' || true)"
[ -n "$leaks" ] && note "core import leak (env/WASI/heap/scheduler): $(printf '%s' "$leaks" | tr '\n' ' ')"

# 4. ONE scheduler. Unbundle the composite back into its core modules — one per
# component instance — and assert the ROUTING, positively, from each module's import
# list. This is deliberately not "look for the executor inside each module": a
# `Tasks` table lives in wasm .bss, which a module does not declare, so there is no
# reliable binary signal that a module holds one. (Tried and rejected: the
# executor's source path in the bounds-check panic location — the pre-change
# spawn/timer modules bound-check before indexing, so the string is absent from both
# even though both DID hold a table. A gate built on it would have passed the
# triplicated composite.) What IS decidable is where each instance's scheduler
# operations GO:
#   - exactly one module imports the trusted poll-task dispatch  → one dispatcher,
#     and it must import no gust:sched (it is the owner, not a client);
#   - exactly one module routes spawn's operations (admit/wake) over gust:sched;
#   - exactly one module routes timer's operations (set-deadline/slept-status) over
#     gust:sched.
# A provider that kept a private table would perform those operations locally and so
# would NOT import them — which is exactly how each historical duplication fails this
# gate. Ownership of the executor SOURCE is gated separately, and exactly, by
# build-gustos-components.sh (one crate may compile plain/src/executor.rs).
echo ""
echo "== one scheduler =="
ub="$OUT/unbundled"; rm -rf "$ub"; mkdir -p "$ub"
"$WT" component unbundle "$FUSED" --module-dir "$ub" --threshold 0 -o "$ub/shell.wasm" >/dev/null
n_mods=0; n_disp=0; n_spawn=0; n_timer=0; disp_has_sched=0
for mod in "$ub"/unbundled-module*.wasm; do
  n_mods=$((n_mods+1))
  imp="$("$WT" print "$mod" | grep -oE '\(import "[^"]+" "[^"]+"' | sed 's/(import //;s/"//g;s/@0.1.0//')"
  d=0; sp=0; tm=0
  printf '%s' "$imp" | grep -q '^gust:os/taskdisp poll-task$' && d=1
  printf '%s' "$imp" | grep -q '^gust:sched/tasks admit$'        && \
  printf '%s' "$imp" | grep -q '^gust:sched/tasks wake$'         && sp=1
  printf '%s' "$imp" | grep -q '^gust:sched/tasks set-deadline$' && \
  printf '%s' "$imp" | grep -q '^gust:sched/tasks slept-status$' && tm=1
  n_disp=$((n_disp+d)); n_spawn=$((n_spawn+sp)); n_timer=$((n_timer+tm))
  if [ "$d" -eq 1 ] && printf '%s' "$imp" | grep -q '^gust:sched/'; then disp_has_sched=1; fi
  printf '  %-24s %6s B  dispatch=%s spawn-route=%s timer-route=%s  imports: %s\n' \
    "$(basename "$mod")" "$(wc -c < "$mod" | tr -d ' ')" "$d" "$sp" "$tm" \
    "$(printf '%s' "$imp" | tr '\n' ' ')"
done
[ "$n_mods"  -eq 5 ] || note "expected 5 core modules (one per provider), got $n_mods"
[ "$n_disp"  -eq 1 ] || note "expected exactly 1 module importing the poll-task dispatch, got $n_disp"
[ "$n_spawn" -eq 1 ] || note "expected exactly 1 module routing spawn's admit/wake over gust:sched, got $n_spawn"
[ "$n_timer" -eq 1 ] || note "expected exactly 1 module routing timer's set-deadline/slept-status over gust:sched, got $n_timer"
[ "$disp_has_sched" -eq 0 ] || note "the dispatching module also imports gust:sched — it should OWN the table, not call one"
[ "$n_disp" -eq 1 ] && [ "$n_spawn" -eq 1 ] && [ "$n_timer" -eq 1 ] && [ "$disp_has_sched" -eq 0 ] \
  && echo "  ok: one dispatcher, and both spawn's and timer's scheduler operations route to it — one task table"

# A count over MODULES cannot see a count over INSTANCES. Composing the SAME
# exec-provider twice — once for spawn, once for timer — yields a composite with
# byte-identical WIT, the same five core modules and the same import routing, and two
# private task tables. Every check above accepts it. The instantiation count is what
# separates them: the split composite instantiates module 0 twice.
dup="$("$WT" print "$FUSED" 2>/dev/null | grep -oE '\(instantiate [0-9]+' | awk '{print $2}' \
      | sort -n | uniq -c | awk '$1 > 1 {printf "module %s instantiated %s times; ", $2, $1}')"
if [ -n "$dup" ]; then
    echo "  FAIL: a core module is instantiated more than once — $dup"
    echo "        two instances of one provider means two private task tables behind one"
    echo "        facade, which the module-count and routing checks above cannot detect."
    exit 1
fi
echo "  ok: every core module is instantiated exactly once (no duplicated provider state)"

if [ -z "$fail" ]; then
  echo ""
  echo "  ok: exports 5 gust:os interfaces; residual imports ="
  printf '%s\n' "$imports" | sed 's/^/        /'
  echo "  ok: no env / WASI / heap / scheduler symbol in any core import"
fi

# 5. NEGATIVE SELF-TEST of gate 2. Compose the SAME five components with the old
# union script (no edges at all, every import forwarded outward) — the mis-composed
# shape this whole change replaces — and require check_residuals to REJECT it. A gate
# nobody has watched fail is not a gate; this one fails in-band, every run.
echo ""
echo "== negative test: the residual gate must reject a mis-composed input =="
cat > "$OUT/unwired-gustos.wac" <<'WAC'
package gale:unwired-gustos@0.1.0;

let x = new gust:exec-provider  { ... };
let t = new gust:time-provider  { ... };
let l = new gust:log-provider   { ... };
let s = new gust:spawn-provider { ... };   // NOT bound to x's gust:sched/tasks
let m = new gust:timer-provider { ... };   // NOT bound to x or t

export t["gust:os/time@0.1.0"];
export l["gust:os/log@0.1.0"];
export s["gust:os/spawn@0.1.0"];
export x["gust:os/exec@0.1.0"];
export m["gust:os/timer@0.1.0"];
WAC
"$WAC" compose "$OUT/unwired-gustos.wac" "${deps[@]}" -o "$OUT/unwired-gustos.component.wasm"
if check_residuals "$OUT/unwired-gustos.component.wasm"; then
  note "negative test DID NOT FAIL: the residual gate accepts an unwired composite"
else
  echo "  ok: gate rejected the unwired composite (as it must) — the gate above is live"
fi

echo ""
printf '%s\n' "$wit"
echo ""
if [ -n "$fail" ]; then
  echo "fused gust:os component invariant FAILED:$fail"; exit 1
fi
echo "fused gust:os component OK: $FUSED ($(wc -c < "$FUSED" | tr -d ' ') B)"
echo "NOT lowered to native and NOT run on hardware — that is a separate step."

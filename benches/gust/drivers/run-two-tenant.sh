#!/usr/bin/env bash
# REQ-OS-MPU-001's kill-criterion, both arms, reproducibly.
#
# VARVE IS THE BASE. synth resolves through the pin unless $SYNTH overrides it — the same
# order check-cross-arch.py uses. The override exists because the region table (#1145 /
# RQ-62-MEMISOLATE) shipped in synth 0.62.0 and no varve layer carries it yet: 2026.09.1
# is the newest and holds 0.61.0.
#
# THE OVERRIDE IS ALWAYS ANNOUNCED. A result produced by a toolchain other than the pinned
# one is not committed evidence, and the difference has to be visible in the output rather
# than remembered. This script prints which synth ran, its version, and — when varve is the
# source — the layer identity, before it does anything.
#
#   ./run-two-tenant.sh                       # varve pin; will REFUSE if too old
#   SYNTH=/path/to/synth-0.62 ./run-two-tenant.sh
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
BENCH="$(dirname "$HERE")"
W="${TT_WORK:-/tmp/gale-two-tenant}"
mkdir -p "$W"

# ---- resolve synth: $SYNTH, else varve, else PATH (check-cross-arch.py's order) --------
if [ -n "${SYNTH:-}" ]; then
    S="$SYNTH"; SRC="\$SYNTH override"
elif command -v varve >/dev/null 2>&1 && varve which synth >/dev/null 2>&1; then
    S="$(varve which synth | head -1)"; SRC="varve pin"
else
    S="synth"; SRC="PATH"
fi
VER="$("$S" --version 2>/dev/null | head -1)"
echo "== synth: $VER"
echo "   from: $SRC"
echo "   path: $S"
if [ "$SRC" = "varve pin" ]; then
    varve which synth 2>/dev/null | sed -n '2p' | sed 's/^/   layer: /'
else
    echo "   NOTE: not the pinned toolchain — this run is a PROTOTYPE, not committed evidence."
fi

# ---- the module: two tenants, neither able to grow ------------------------------------
cat > "$W/two_tenant.wat" <<'WAT'
(module
  (memory $a 1 1)
  (memory $b 1 1)
  (data $da (memory $a) (i32.const 0) "tenant-A")
  (data $db (memory $b) (i32.const 0) "tenant-B")
  (func (export "a_store") (param i32 i32) (i32.store $a (local.get 0) (local.get 1)))
  (func (export "a_load")  (param i32) (result i32) (i32.load $a (local.get 0)))
  (func (export "b_store") (param i32 i32) (i32.store $b (local.get 0) (local.get 1)))
  (func (export "b_load")  (param i32) (result i32) (i32.load $b (local.get 0)))
  (func (export "a_escape") (param i32 i32) (i32.store $a (local.get 0) (local.get 1))))
WAT
wasm-tools parse "$W/two_tenant.wat" -o "$W/two_tenant.wasm" || exit 2
"$S" compile "$W/two_tenant.wasm" --target cortex-m3 --all-exports --relocatable \
     --embedder-data-init -o "$W/two_tenant.o" >"$W/compile.log" 2>&1 || {
    echo "!! synth compile failed:"; tail -3 "$W/compile.log"; exit 2; }

# ---- REFUSE rather than run a meaningless test ----------------------------------------
# Without the region table there is nothing to program regions FROM. An older synth
# compiles this module perfectly happily and emits no table, so the probe would run, take
# no fault, and report ESCAPED — a red that looks like a finding and is really a toolchain
# that predates the feature.
NM="${NM:-arm-none-eabi-nm}"
if ! "$NM" "$W/two_tenant.o" 2>/dev/null | grep -q "__synth_mem_base_1"; then
    echo "!! This synth emits no region table (__synth_mem_base_1 absent)."
    echo "   The table shipped in synth 0.62.0 (#1145 / RQ-62-MEMISOLATE)."
    echo "   Refusing: the probe would run, not fault, and report ESCAPED — a finding-shaped"
    echo "   artefact of an old toolchain rather than a real isolation failure."
    echo "   Either pin a layer carrying >= 0.62.0, or SYNTH=/path/to/synth-0.62 $0"
    exit 3
fi
echo "== region table present:"
"$NM" -S "$W/two_tenant.o" | grep synth_mem | sed 's/^/   /'
echo
echo "Next: build the probe against \$W/two_tenant.o and run both Renode arms."
echo "  GUST_TWO_TENANT_O=$W/two_tenant.o cargo build --release --bin gust_two_tenant"
echo "  (and again with --features no-regions for the negative control)"

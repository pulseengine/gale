#!/usr/bin/env bash
# T4 (REQ-OS-WCET-001) — sound static per-function WCET bounds from synth itself
# (`--emit-wcet`, schema synth-wcet-v1, synth >= 0.46.0 / synth#778).
#
# Emits + gates the WCET sidecar on gale's two reproducible dissolve inputs:
#   1. os-node/repro-757/loom.wasm — the frozen os-tl node (deterministic input,
#      byte-identical output: the .o must equal the committed os-tl-fixed.o).
#   2. drivers/i2c-thin — a thin-seam driver rebuilt from source (leaf protocol
#      fns get full bounds; fns with calls/loops are LOUDLY declined).
#
# The sidecar is additive: the .o with --emit-wcet is byte-identical to without
# (gated below). Declines are honest scope, not gaps swept under a percentage:
#   reason=call — bound is intra-procedural; composition is spar's job (T3,
#                 spar#331: WCRT recurrence consumes these as C_i).
#   reason=loop — a sound bound needs a trip count (scry loop-bound inference).
# NO partition budget may be sized from raw DWT high-water-marks (the build-gate
# this track exists to enforce); DWT only ever falsifies the model.
set -euo pipefail
SYNTH="${SYNTH:-$HOME/pe-toolchain/synth-0.46.0/synth}"
LOOM="${LOOM:-$HOME/pe-toolchain/loom-1.2.0/loom}"
HERE="$(cd "$(dirname "$0")" && pwd)"
TMP="$(mktemp -d)"
trap 'rm -rf "$TMP"' EXIT

gate_sidecar() { # file, min_bounded
  python3 - "$1" "$2" <<'PY'
import json, sys
d = json.load(open(sys.argv[1])); need = int(sys.argv[2])
assert d["schema"] == "synth-wcet-v1", f"schema {d['schema']}"
assert d["core_class"] == "cortex-m3", f"core {d['core_class']}"
b = [f for f in d["functions"] if f["status"] == "bounded"]
dec = [f for f in d["functions"] if f["status"] == "declined"]
assert len(b) + len(dec) == len(d["functions"]), "function with unknown status"
assert all(f["cycles"] > 0 for f in b), "non-positive bound"
assert all(f.get("reason") for f in dec), "silent decline"
assert len(b) >= need, f"bounded {len(b)} < required {need}"
print(f"  gate OK: {len(b)} bounded / {len(dec)} declined (all loud)")
for f in b:
    print(f"    BOUND {f['name']}: {f['cycles']} cyc / {f['instr_count']} instr")
PY
}

echo "## 1. os-tl node (frozen input: repro-757/loom.wasm)"
"$SYNTH" compile "$HERE/os-node/repro-757/loom.wasm" \
  --target cortex-m3 --all-exports --relocatable \
  --native-pointer-abi --shadow-stack-size 2048 \
  --emit-wcet -o "$TMP/os-tl.o" >/dev/null
cmp "$TMP/os-tl.o" "$HERE/os-node/repro-757/os-tl-fixed.o" \
  && echo "  .o byte-identical to committed os-tl-fixed.o (sidecar is additive)"
gate_sidecar "$TMP/os-tl.o.wcet.json" 4
cp "$TMP/os-tl.o.wcet.json" "$HERE/os-node/repro-757/os-tl.wcet.json"
echo "  sidecar refreshed at os-node/repro-757/os-tl.wcet.json"

echo "## 2. i2c-thin driver (rebuilt from source)"
( cd "$HERE/i2c-thin" && cargo build --release --target wasm32-unknown-unknown --quiet )
"$LOOM" optimize "$HERE/i2c-thin/target/wasm32-unknown-unknown/release/gust_i2c_thin.wasm" \
  --passes inline -o "$TMP/i2c.opt.wasm" >/dev/null
"$SYNTH" compile "$TMP/i2c.opt.wasm" \
  --target cortex-m3 --all-exports --relocatable \
  --emit-wcet -o "$TMP/i2c-thin.o" >/dev/null
gate_sidecar "$TMP/i2c-thin.o.wcet.json" 3

echo "emit-wcet: ALL GATES GREEN"

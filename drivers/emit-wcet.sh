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
# VARVE IS THE BASE, same resolution order as run-two-tenant.sh and
# check-cross-arch.py: $TOOL override, else the varve pin, else PATH. This used
# to hard-code $HOME/pe-toolchain/synth-0.46.0 -- a path that exists on one
# laptop and on no CI runner, so the job that ran this would have died on "no
# such file" rather than on a verdict.
resolve() { # varname, tool
  local __v="$1" tool="$2" path src
  if [ -n "${!__v:-}" ]; then path="${!__v}"; src="\$$__v override"
  elif command -v varve >/dev/null 2>&1 && varve which "$tool" >/dev/null 2>&1; then
    path="$(varve which "$tool" | head -1)"; src="varve pin"
  else path="$tool"; src="PATH"; fi
  printf '== %s: %s\n   from: %s\n   path: %s\n' \
    "$tool" "$("$path" --version 2>&1 | head -1)" "$src" "$path"
  [ "$src" = "varve pin" ] && varve which "$tool" 2>/dev/null | sed -n '2p' | sed 's/^/   layer: /'
  printf -v "$__v" '%s' "$path"
}
resolve SYNTH synth
resolve LOOM loom
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
compile_os_tl() { # out, extra-args...
  local out="$1"; shift
  "$SYNTH" compile "$HERE/os-node/repro-757/loom.wasm" \
    --target cortex-m3 --all-exports --relocatable \
    --native-pointer-abi --shadow-stack-size 2048 \
    "$@" -o "$out" >/dev/null
}

# ADDITIVITY, gated properly. Two corrections live here:
#
# 1. This used to compare against the committed os-tl-fixed.o. That object is a
#    synth#757 bug reproduction frozen at synth 0.45/0.46 -- era-pinned evidence,
#    not a reproducibility target for the current compiler. Codegen legitimately
#    moved, so on the pinned varve layer that comparison FAILS while claiming
#    nothing T4 asserts. The real claim, the one this file's header already
#    stated, is that --emit-wcet does not perturb the object: same synth, with
#    and without the flag, byte-identical .o. That is toolchain-independent.
#
# 2. It was written `cmp A B && echo ...`. In an `&&` list a failing left-hand
#    command is exempt from `set -e`, so a mismatch printed its diff to stderr
#    and the script ran on to announce "ALL GATES GREEN". Verified directly:
#    `cmp a b && echo` on differing files exits 0. A comparison that carries a
#    verdict must stand alone.
compile_os_tl "$TMP/os-tl-plain.o"
compile_os_tl "$TMP/os-tl.o" --emit-wcet
cmp "$TMP/os-tl-plain.o" "$TMP/os-tl.o"
echo "  --emit-wcet is additive: .o byte-identical with and without the flag"
gate_sidecar "$TMP/os-tl.o.wcet.json" 4
# A GATE MUST NOT MUTATE THE TREE. This unconditionally overwrote a committed
# sidecar, so merely running the check rewrote the evidence it was checking --
# and in CI left the working tree dirty. Refreshing is now opt-in.
if [ -n "${REFRESH_SIDECAR:-}" ]; then
  cp "$TMP/os-tl.o.wcet.json" "$HERE/os-node/repro-757/os-tl.wcet.json"
  echo "  sidecar REFRESHED at os-node/repro-757/os-tl.wcet.json (REFRESH_SIDECAR set)"
else
  echo "  sidecar gated read-only (set REFRESH_SIDECAR=1 to rewrite the committed copy)"
fi

echo "## 2. i2c-thin driver (rebuilt from source)"
( cd "$HERE/i2c-thin" && cargo build --release --target wasm32-unknown-unknown --quiet )
"$LOOM" optimize "$HERE/i2c-thin/target/wasm32-unknown-unknown/release/gust_i2c_thin.wasm" \
  --passes inline -o "$TMP/i2c.opt.wasm" >/dev/null
# --embedder-data-init / --embedder-global-init: from synth 0.60.0 the ARM relocatable path REFUSES a
# module with active data segments (#1041) rather than silently emitting an
# object whose loads read whatever the target memory happens to hold. The flag
# declares that the embedder populates the segments at instantiation, which is
# the arrangement #331 established (and checks: 0 r9 / 0 data refs) for the thin
# drivers. Without it this section dies on the refusal, not on a WCET verdict.
"$SYNTH" compile "$TMP/i2c.opt.wasm" \
  --target cortex-m3 --all-exports --relocatable \
  --embedder-data-init --embedder-global-init \
  --emit-wcet -o "$TMP/i2c-thin.o" >/dev/null
gate_sidecar "$TMP/i2c-thin.o.wcet.json" 3

echo "emit-wcet: ALL GATES GREEN"

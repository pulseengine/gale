#!/usr/bin/env bash
# MC/DC + object-disposition evidence for a thin-seam driver, on the WASM the
# dissolve actually compiles — the "evidence-on-wasm" leg REQ-OS-OBJVERIFY-001
# demands and that every stage-2 RESULTS.md lists under "does NOT establish".
#
# Chain:  cargo(+DWARF) -> component -> wac plug seam stub -> meld fuse
#         -> witness instrument -> witness run (designed vectors)
#         -> witness report --format mcdc      (the truth table)
#         -> synth --emit-provenance + witness object-disposition (WASM -> object)
#
# The seam stub returns success for all three native atoms — the SAME substitution
# the Kani harness makes, because run_switch's FFI calls cannot be linked. What is
# under test is the FSM policy, never the native context save/restore.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DRV="${DRV:-$HERE/../switch-thin}"
OUT="${OUT:-${TMPDIR:-/tmp}/gust-mcdc}"
WITNESS="${WITNESS:-witness}"
MELD="${MELD:-meld}"
# gale's PIN (0.52.0, #208), not whatever is on PATH
SYNTH="${SYNTH:-$HOME/pe-toolchain/synth-0.52.0/synth}"
[ -x "$SYNTH" ] || SYNTH="synth"

command -v "$WITNESS" >/dev/null 2>&1 || { echo "need witness (>=0.39.0) on PATH or \$WITNESS" >&2; exit 2; }

mkdir -p "$OUT"
echo "== 1. build with DWARF (witness needs it for decision reconstruction) =="
# debuginfo is NOT free: it changes the crate disambiguator and permutes four
# function indices vs the shipped build, so the WHOLE chain must run on this one
# artefact. Never join a manifest from here to a provenance map from elsewhere.
( cd "$DRV" && CARGO_PROFILE_RELEASE_DEBUG=2 cargo build --release --target wasm32-unknown-unknown )
WASM="$(ls "$DRV"/target/wasm32-unknown-unknown/release/*.wasm | head -1)"

echo "== 2. componentize, plug the seam stub, fuse to one core =="
wasm-tools component new "$WASM" -o "$OUT/drv.component.wasm"
( cd "$HERE/ctx-stub" && cargo build --release --target wasm32-unknown-unknown )
wasm-tools component new "$HERE/ctx-stub/target/wasm32-unknown-unknown/release/ctx_stub.wasm" \
  -o "$OUT/ctx-stub.component.wasm"
wac plug "$OUT/drv.component.wasm" --plug "$OUT/ctx-stub.component.wasm" -o "$OUT/sealed.wasm"
# --preserve-names: without it meld drops the name section and every function in
# the report reads "(anon)" — the gap rows become unattributable.
"$MELD" fuse "$OUT/sealed.wasm" --memory shared --preserve-names -o "$OUT/core.wasm" >/dev/null

echo "== 3. instrument + run the designed vector set =="
"$WITNESS" instrument "$OUT/core.wasm" -o "$OUT/core.inst.wasm"
# shellcheck source=vectors.sh
source "$HERE/vectors.sh"
"$WITNESS" run "$OUT/core.inst.wasm" "${A[@]}" -o "$OUT/run.json"

echo "== 4. the truth table (NOT the percentage) =="
# `witness report` without --format reports branches REACHED, which is not MC/DC.
# The MC/DC verdict — proved / gap / dead per condition — is --format mcdc.
"$WITNESS" report --input "$OUT/run.json" --format mcdc        > "$OUT/mcdc-truthtables.txt"
"$WITNESS" report --input "$OUT/run.json" --format mcdc-rollup | tee "$OUT/mcdc-rollup.txt"

echo "== 5. WASM -> object disposition =="
"$SYNTH" compile "$OUT/core.wasm" --target cortex-m3 --all-exports --relocatable \
  --native-pointer-abi --shadow-stack-size 2048 --emit-provenance -o "$OUT/prov-cm3.o" >/dev/null
"$WITNESS" object-disposition --manifest "$OUT/core.inst.wasm.witness.json" \
  --provenance "$OUT/prov-cm3.o.provenance.json" | tee "$OUT/object-disposition.txt"

echo
echo "evidence written under $OUT"

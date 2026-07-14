#!/usr/bin/env bash
# gale#173 pilot — re-discharge a by(bit_vector) ASIL-D leaf obligation through ordeal
# (certificate-checked QF_BV) instead of unchecked Z3. Prints the verdict + LRAT cert size.
set -euo pipefail
ORDEAL="${ORDEAL:-ordeal}"   # ordeal >= 0.9.1 (target/release/ordeal in pulseengine/ordeal)
HERE="$(cd "$(dirname "$0")" && pwd)"
echo "# cpu_mask power-of-two obligation (src/cpu_mask.rs:179) — expect: unsat + LRAT"
"$ORDEAL" check "$HERE/cpu_mask_pot.smt2"

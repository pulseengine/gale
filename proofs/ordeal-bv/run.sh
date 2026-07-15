#!/usr/bin/env bash
# gale#173 — re-discharge by(bit_vector) ASIL-D leaf obligations through ordeal
# (certificate-checked QF_BV, Lean-4-proven LRAT checker) instead of unchecked Z3.
# Prints each verdict + LRAT cert size. UNSAT (with a validated cert) = obligation holds.
set -euo pipefail
ORDEAL="${ORDEAL:-ordeal}"   # ordeal >= 0.9.1 (target/release/ordeal in pulseengine/ordeal)
HERE="$(cd "$(dirname "$0")" && pwd)"

echo "## Pilot 1 — cpu_mask.rs:179 power-of-two obligation (single implication)"
echo "# cpu_mask_pot.smt2 — expect: unsat + LRAT"
"$ORDEAL" check "$HERE/cpu_mask_pot.smt2"

echo
echo "## Pilot 2 — mpu.rs:98 is_power_of_two BICONDITIONAL (both directions = 2 obligations)"
echo "# mpu_pow2_fwd.smt2  (idiom => enumeration) — expect: unsat + LRAT"
"$ORDEAL" check "$HERE/mpu_pow2_fwd.smt2"
echo "# mpu_pow2_bwd.smt2  (enumeration => idiom) — expect: unsat + LRAT"
"$ORDEAL" check "$HERE/mpu_pow2_bwd.smt2"
echo "# mpu_pow2_fwd_mutant.smt2  (discrimination sanity, bv2 dropped) — expect: sat + model n=2"
"$ORDEAL" check "$HERE/mpu_pow2_fwd_mutant.smt2"

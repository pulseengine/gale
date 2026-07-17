#!/usr/bin/env bash
# gale#173 — re-discharge by(bit_vector) ASIL-D leaf obligations through ordeal
# (certificate-checked QF_BV, Lean-4-proven LRAT checker) instead of unchecked Z3.
# Prints each verdict + LRAT cert size. UNSAT (with a validated cert) = obligation holds.
set -euo pipefail
# Get ordeal from crates.io: `cargo install ordeal` (the published binary crate).
# These hand-transcribed pilots need >= 0.9.1. The REAL-VC obligation-proof path
# (ingest the by(bit_vector) VC Verus itself emits, closing the transcription gap
# below) lands with ordeal 0.12.0 on crates.io (FEAT-009/#65 merged on main, not
# yet published as of 0.11.0).
ORDEAL="${ORDEAL:-ordeal}"   # `cargo install ordeal` puts it on PATH
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

echo
echo "## Pilot 3 — spinlock_validate.rs SV4/SV5 owner encode/decode round-trip (2 obligations)"
echo "# sv_cpu_recover.smt2     (owner&3 == cpu)          — expect: unsat + LRAT"
"$ORDEAL" check "$HERE/sv_cpu_recover.smt2"
echo "# sv_thread_recover.smt2  (owner & ~3 == thread)    — expect: unsat + LRAT"
"$ORDEAL" check "$HERE/sv_thread_recover.smt2"
echo "# sv_cpu_recover_mutant.smt2  (discrimination, alignment premise dropped) — expect: sat + model"
"$ORDEAL" check "$HERE/sv_cpu_recover_mutant.smt2"

echo
echo "## Pilot 4 — fault_decode.rs:663-666 lemma_cfsr_masks_partition (CFSR sub-register partition)"
echo "# cfsr_masks_partition.smt2   (MMFSR/BFSR/UFSR disjoint + cover 0xFFFFFFFF) — expect: unsat + LRAT"
"$ORDEAL" check "$HERE/cfsr_masks_partition.smt2"
echo "# cfsr_partition_lossless.smt2  (parametric: any cfsr slices disjoint + reassemble) — expect: unsat + LRAT"
"$ORDEAL" check "$HERE/cfsr_partition_lossless.smt2"
echo "# cfsr_partition_mutant.smt2  (discrimination, UFSR bit 31 dropped) — expect: sat + model cfsr=0x80000000"
"$ORDEAL" check "$HERE/cfsr_partition_mutant.smt2"

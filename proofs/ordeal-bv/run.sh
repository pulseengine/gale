#!/usr/bin/env bash
# gale#173 — re-discharge by(bit_vector) ASIL-D leaf obligations through ordeal
# (certificate-checked QF_BV, Lean-4-proven LRAT checker) instead of unchecked Z3.
# Prints each verdict + LRAT cert size. UNSAT (with a validated cert) = obligation holds.
set -euo pipefail
# Get ordeal from crates.io: `cargo install ordeal` (the published binary crate).
# Two kinds of check run here:
#   * Section A — hand-TRANSCRIBED pilots (`ordeal check <file.smt2>`): prove the
#     transcription of each by(bit_vector) leaf. Need ordeal >= 0.9.1.
#   * Section B — REAL-VC OBLIGATION-proof (`ordeal verus <verus-log.smt2>`): feed
#     the exact by(bit_vector) VC Verus/AIR emits to Z3; ordeal's Verus-VC bridge
#     (FEAT-009/#65) slices the QF_BV obligation, solves it, and RE-CHECKS the LRAT
#     cert. This closes the transcription gap. Needs ordeal >= 0.12.0 (verified on
#     0.14.0). cpu_mask + mpu real VCs ship as ordeal test fixtures and are carried
#     here verbatim as verus_*_realvc.smt2. spinlock/fault_decode real VCs are NOT
#     yet available (their Verus logs are not shipped; local Verus not on PATH) so
#     they remain transcription-proof until the log slicer / a Verus run supplies them.
ORDEAL="${ORDEAL:-ordeal}"   # `cargo install ordeal` puts it on PATH
HERE="$(cd "$(dirname "$0")" && pwd)"

echo "############################################################"
echo "# Section A — TRANSCRIPTION-proof pilots (ordeal check)"
echo "############################################################"

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

echo
echo "############################################################"
echo "# Section B — REAL-VC OBLIGATION-proof (ordeal verus)"
echo "#   Ingests the exact by(bit_vector) VC Verus emits to Z3."
echo "#   'unsat <src-loc> (N bytes of checked LRAT)' = leaf discharged,"
echo "#   cert re-checked. This is obligation-proof, not transcription-proof."
echo "############################################################"
echo "# verus_cpu_mask_realvc.smt2 — gale src/cpu_mask.rs:171 1u32<<cpu_id power-of-two"
echo "#   expect: unsat  src/cpu_mask.rs:171:9: 171:15 (#0)  (28250 bytes of checked LRAT)"
"$ORDEAL" verus "$HERE/verus_cpu_mask_realvc.smt2"
echo "# verus_mpu_pow2_realvc.smt2 — gale src/mpu.rs:98 is_power_of_two biconditional"
echo "#   expect: unsat  src/mpu.rs:98:9: 98:15 (#0)  (63664 bytes of checked LRAT)"
"$ORDEAL" verus "$HERE/verus_mpu_pow2_realvc.smt2"

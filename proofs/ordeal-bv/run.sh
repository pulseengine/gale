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

# ---------------------------------------------------------------------------
# SOUNDNESS FLOOR -- ordeal >= 0.22.1 (GHSA-xfxf-qxr3-435x, ordeal#182).
#
# Every ordeal from 0.2.0 through 0.22.0 bit-blasted bvshl/bvlshr/bvashr and
# the rotations with too few barrel-shifter stages when the operand width is
# NOT a power of two. An affected query could return `Unsat` WITH A CERTIFICATE
# THAT RE-CHECKS: the LRAT checker certifies the CNF it was handed, and the CNF
# was the wrong encoding. So the failure is invisible downstream -- a wrong
# verdict that passes every check this script performs.
#
# That is why this REFUSES rather than warns, and why it also refuses a tool
# whose version cannot be read. A version we cannot establish is not a version
# we can call sound, and the alternative is a green run that means nothing.
# (Same shape as the missing-`gh` degradation caught in gale#418: a missing
# capability must refuse, never fall through to a weaker check.)
#
# The comments in this file already said "Need ordeal >= 0.9.1" and ">= 0.12.0"
# and nothing ever checked either. A tool-version requirement that lives only
# in prose is how this class of defect arrives unnoticed.
#
# Semver comparison is in python, not shell: `[ "$a" \< "$b" ]` is a STRING
# compare, under which "0.9.1" > "0.22.1". Per the repo rule, verdict-bearing
# logic does not live in shell.
# ---------------------------------------------------------------------------
ORDEAL_FLOOR="0.22.1"
_ov="$("$ORDEAL" --version 2>/dev/null | awk '{print $2}')" || true
python3 - "$ORDEAL_FLOOR" "${_ov:-}" "$ORDEAL" <<'PYCHECK'
import sys
floor, got, exe = sys.argv[1], sys.argv[2], sys.argv[3]
def parse(v):
    core = v.split('+')[0].split('-')[0]
    parts = core.split('.')
    if len(parts) < 3 or not all(p.isdigit() for p in parts[:3]):
        return None
    return tuple(int(p) for p in parts[:3])
f, g = parse(floor), (parse(got) if got else None)
adv = "GHSA-xfxf-qxr3-435x / ordeal#182"
if g is None:
    print(f"FATAL: cannot establish the version of `{exe}` (read {got!r}).", file=sys.stderr)
    print(f"       ordeal >= {floor} is required for SOUNDNESS, not for features:", file=sys.stderr)
    print(f"       {adv} -- affected versions can return a WRONG `Unsat`", file=sys.stderr)
    print( "       with a certificate that re-checks, so nothing downstream catches it.", file=sys.stderr)
    print( "       A version that cannot be read cannot be called sound. Point $ORDEAL", file=sys.stderr)
    print( "       at an ordeal that reports `ordeal <semver>` for --version.", file=sys.stderr)
    sys.exit(1)
if g < f:
    print(f"FATAL: {exe} is {got}; ordeal >= {floor} is required for SOUNDNESS.", file=sys.stderr)
    print(f"       {adv}: 0.2.0 through 0.22.0 bit-blast shifts and rotations", file=sys.stderr)
    print( "       incorrectly at non-power-of-two widths and can return a WRONG", file=sys.stderr)
    print( "       `Unsat` with a re-checkable certificate.", file=sys.stderr)
    print( "       NOTE: no varve layer ships >= 0.22.1 yet (2026.09.3 -> 0.19.0,", file=sys.stderr)
    print( "       2026.09.16 -> 0.22.0), so until one does this needs", file=sys.stderr)
    print(f"       `cargo install ordeal --version {floor}` or $ORDEAL pointed at it.", file=sys.stderr)
    sys.exit(1)
print(f"ordeal {got} >= {floor} -- soundness floor satisfied ({adv}).")
PYCHECK

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

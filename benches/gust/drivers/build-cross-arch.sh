#!/usr/bin/env bash
# Cross-architecture matrix for the thin-seam drivers: one wasm per driver, lowered to
# BOTH ARM Cortex-M and RISC-V RV32, reporting the symbol shape of each object.
#
# The point is the seam, not the byte count. A driver "crosses" only if the lowered
# object keeps `gust:hal`'s mmio calls as UNDEFINED symbols on both targets — that is
# what makes the native residual swappable per target while the wasm stays put. An
# object with 0 undefined symbols has not crossed; it has been silently truncated (the
# synth#871 shape, where imported calls were dropped rather than relocated).
#
#   bash benches/gust/drivers/build-cross-arch.sh
#   SYNTH=/path/to/synth bash .../build-cross-arch.sh    # test an unreleased build
#
# Requires synth >= 0.52.0 for the RISC-V leg (R_RISCV_CALL_PLT, synth#871).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
SYNTH="${SYNTH:-$HOME/pe-toolchain/synth-0.58.0/synth}"; [ -x "$SYNTH" ] || SYNTH="synth"

# PIN GUARD. This script used to default to bare `synth` on PATH, and that
# produced a FALSE RED: an ambient synth 0.49.0 skipped 14 of 18 functions on the
# RISC-V backend and emitted 0 undefined symbols, which this gate correctly reads
# as silent truncation. gale#307 recorded "the RISC-V half is red repo-wide" on
# that basis. It is not — on the pinned 0.58.0 (and on 0.60.0) every driver keeps
# its seam on BOTH targets and this gate exits 0. The finding was a stale binary,
# which is precisely the drift varve.toml exists to close.
#
# So: refuse to run unpinned rather than report a verdict from whatever is on
# PATH. A gate that silently changes its answer with the ambient toolchain is
# worse than no gate.
synth_ver="$("$SYNTH" --version 2>/dev/null | awk '{print $2}')"
case "$synth_ver" in
  0.5[89].*|0.[6-9][0-9].*|[1-9].*) : ;;
  *) echo "FATAL: synth $synth_ver is too old for this gate — it needs >= 0.58.0." >&2
     echo "       Older backends skip most functions on RISC-V and emit 0 undefined" >&2
     echo "       symbols, which reads here as truncation and produces a FALSE RED." >&2
     echo "       Set \$SYNTH, or use the pinned toolchain (varve.toml, layer 2026.08.4)." >&2
     exit 2 ;;
esac
echo "gate toolchain: synth $synth_ver"

NM="${NM:-nm}"
ARM_SIZE="${ARM_SIZE:-arm-none-eabi-size}"

DRIVERS=(wdg-thin gpio-thin i2c-thin timer-thin adc-thin dac-thin pwm-thin uart-thin)

printf '%-12s | %-22s | %-22s\n' "driver" "ARM cortex-m3" "RISC-V rv32imc"
printf '%-12s-+-%-22s-+-%-22s\n' "------------" "----------------------" "----------------------"

fail=""
for d in "${DRIVERS[@]}"; do
    [ -d "$HERE/$d" ] || continue
    ( cd "$HERE/$d" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
    w="$(find "$HERE/$d/target/wasm32-unknown-unknown/release" -maxdepth 1 -name '*.wasm' | head -1)"
    [ -n "$w" ] || { echo "$d: no wasm built" >&2; fail="y"; continue; }

    # same input wasm, two backends
    "$SYNTH" compile "$w" --target cortex-m3 --all-exports --relocatable \
             -o "/tmp/xa-$d.o" >/dev/null 2>&1 || true
    rv_log="$("$SYNTH" compile "$w" -b riscv --target esp32c3 --all-exports --relocatable \
             -o "/tmp/xr-$d.o" 2>&1 || true)"

    a_t=$("$NM" "/tmp/xa-$d.o" 2>/dev/null | grep -c ' T ' || true)
    a_u=$("$NM" "/tmp/xa-$d.o" 2>/dev/null | grep -c ' U ' || true)
    a_b=$("$ARM_SIZE" "/tmp/xa-$d.o" 2>/dev/null | tail -1 | awk '{print $1}')
    r_t=$("$NM" "/tmp/xr-$d.o" 2>/dev/null | grep -c ' T ' || true)
    r_u=$("$NM" "/tmp/xr-$d.o" 2>/dev/null | grep -c ' U ' || true)
    # `|| true`: no match means nothing was skipped, which is the good case, but grep
    # exits 1 on no-match and `set -e` would treat the best outcome as a failure.
    skip=$(printf '%s' "$rv_log" | grep -oE "[0-9]+ of [0-9]+ functions were skipped" | head -1 || true)

    printf '%-12s | %2s T  %s U  %4s B      | %2s T  %s U  %s\n' \
           "$d" "$a_t" "$a_u" "${a_b:-?}" "$r_t" "$r_u" "${skip:-complete}"

    # The gate: the seam must survive on BOTH targets. 0 undefined = silent truncation.
    [ "${a_u:-0}" -gt 0 ] || { echo "  FAIL $d: ARM object has no undefined seam symbols" >&2; fail="y"; }
    [ "${r_u:-0}" -gt 0 ] || { echo "  FAIL $d: RISC-V object has no undefined seam symbols" >&2; fail="y"; }
done

[ -z "$fail" ] || { echo; echo "cross-arch seam gate FAILED"; exit 1; }
echo
echo "cross-arch seam gate held: every driver keeps its gust:hal mmio imports as"
echo "undefined symbols on both ARM and RISC-V (no silent truncation)."

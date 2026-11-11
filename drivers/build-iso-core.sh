#!/usr/bin/env bash
# Build the dissolved ISOLATION CORE (hm-thin + mpu-thin + switch-thin) as ONE
# Cortex-M3 object — correctly.
#
# WHY THIS SCRIPT EXISTS: until now the fused isolation core was built by hand from
# the reproduce lines in the three RESULTS.md files, with `meld fuse --memory shared`.
# That path merges memories WITHOUT rebasing, so all three components place static
# data at the same base and the emitted object had FOUR OVERLAPPING data-segment
# pairs (gale#266). It shipped anyway because it was the only build that FIT the
# STM32F100RB's 8 KB — the correct alternatives were 16x to 272x over budget.
#
# meld 0.48.0 closes that: --pack-rebase places each component at its true static
# extent (needs --emit-relocs AND a retained __heap_base), and --share-stack
# collapses the N per-component shadow stacks into one. The correct build now fits.
#
#   --memory shared --pack-rebase --share-stack   SRAM 4236 B of 8192 (51%), DISJOINT
#   --memory shared                (the old way)  SRAM 3704 B, 4 overlapping pairs
#
# --share-stack is sound here ONLY because the three components are non-reentrant,
# single-threaded and MUTUALLY NON-CALLING. That last one is checked below, not
# assumed: all four seams must remain UNDEFINED in the output. If a seam ever
# disappears from that set, a component is calling a sibling, the one-live-at-a-time
# premise is broken, and --share-stack must come off.
#
# Exit: 0 = built and gated; 1 = a stage failed; 2 = the seam set is wrong;
#       5 = data segments overlap (the gale#266 defect, now a HARD gate).
set -euo pipefail

HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${OUT:-$HERE}"
OBJ="$OUT/iso-core-fused-cm3.o"
T="$(mktemp -d)"
trap 'rm -rf "$T"' EXIT

# Pins, not whatever is on PATH — a footprint measured with a different toolchain
# is not comparable to the numbers recorded in the RESULTS.md files.
MELD="${MELD:-$HOME/pe-toolchain/meld-0.48.0/meld}"; [ -x "$MELD" ] || MELD="meld"
SYNTH="${SYNTH:-$HOME/pe-toolchain/synth-0.57.0/synth}"; [ -x "$SYNTH" ] || SYNTH="synth"
LOOM="${LOOM:-loom}"

# The version is pinned in TWO places — this default and MELD_VERSION in
# .github/workflows/gustos-dissolve.yml — and they DID drift: the local default
# moved to 0.48.0 while CI still installed 0.41.3, so CI fell through to a meld
# with no --pack-rebase and failed deep inside the fuse with "unexpected
# argument". Check up front instead of discovering it there.
meld_ver="$("$MELD" --version 2>/dev/null | awk '{print $2}')"
case "$meld_ver" in
  0.4[89].*|0.[5-9][0-9].*|[1-9].*) : ;;
  *) echo "FATAL: meld $meld_ver is too old — this build needs >= 0.48.0 for" >&2
     echo "       --pack-rebase / --share-stack. Set \$MELD, or bump MELD_VERSION" >&2
     echo "       in .github/workflows/gustos-dissolve.yml if this is CI." >&2
     exit 1 ;;
esac

NM="${NM:-arm-none-eabi-nm}"
SIZE="${SIZE:-arm-none-eabi-size}"

printf 'tools: meld %s / loom %s / synth %s\n\n' \
  "$("$MELD" --version 2>/dev/null | awk '{print $2}')" \
  "$("$LOOM" --version 2>/dev/null | awk '{print $2}')" \
  "$("$SYNTH" --version 2>/dev/null | awk '{print $2}')"

echo "== 1. relocatable cores + components (--emit-relocs, __heap_base, 1-page arena) =="
OUT="$T" bash "$HERE/build-reloc-cores.sh" hm-thin mpu-thin switch-thin

echo ""
echo "== 2. meld fuse --memory shared --pack-rebase --share-stack =="
"$MELD" fuse "$T/hm-thin.comp.wasm" "$T/mpu-thin.comp.wasm" "$T/switch-thin.comp.wasm" \
  --memory shared --pack-rebase --share-stack -o "$T/iso.fused.wasm" 2>"$T/meld.err" \
  || { echo "  FAILED:"; sed 's/^/    /' "$T/meld.err" | head -20; exit 1; }
pages="$(wasm-tools print "$T/iso.fused.wasm" 2>/dev/null | grep -oE '\(memory \(;0;\) [0-9]+' | grep -oE '[0-9]+$' | head -1)"
printf '  fused core: %s B, declared arena %s page(s)\n' "$(wc -c <"$T/iso.fused.wasm" | tr -d ' ')" "${pages:-?}"

echo ""
echo "== 3. GATE: data segments must be DISJOINT (gale#266) =="
# This is the gate that could not exist while the only fitting build was the one
# that failed it. check-data-overlap.py exits 5 on any overlapping pair.
python3 "$HERE/check-data-overlap.py" "$T/iso.fused.wasm" || exit 5

echo ""
echo "== 4. loom optimize -> synth compile --target cortex-m3 =="
"$LOOM" optimize "$T/iso.fused.wasm" --passes inline --attestation false -o "$T/iso.loom.wasm" >/dev/null 2>&1 \
  || { echo "  loom FAILED"; exit 1; }
"$SYNTH" compile "$T/iso.loom.wasm" --target cortex-m3 --all-exports --relocatable \
  --native-pointer-abi -o "$OBJ" 2>"$T/synth.err" \
  || { echo "  FAILED:"; sed 's/^/    /' "$T/synth.err" | head -20; exit 1; }

read -r text data bss _ <<<"$("$SIZE" "$OBJ" | awk 'NR==2{print $1, $2, $3}')"
printf '  text=%s data=%s bss=%s\n' "$text" "$data" "$bss"
printf '  -> SRAM %s B of the STM32F100RB'"'"'s 8192 (%s%%)\n' "$((data+bss))" "$(( (data+bss)*100/8192 ))"
[ "$((data+bss))" -le 8192 ] || { echo "  FAIL: over the 8192 B SRAM budget"; exit 1; }

echo ""
echo "== 5. GATE: the seam set must be EXACTLY the four declared native atoms =="
# Doubles as the --share-stack soundness check: a seam that vanishes from this set
# is a component that has started calling a sibling, which breaks one-live-at-a-time.
undef="$("$NM" -u "$OBJ" 2>/dev/null | awk '{print $NF}' | sort -u | tr '\n' ' ' | sed 's/ $//')"
want="ctx-resume ctx-save mpu-write region-swap"
printf '  undefined: %s\n' "$undef"
if [ "$undef" != "$want" ]; then
  echo "  FAIL: expected exactly [$want]" >&2
  echo "        A MISSING seam means a component now calls a sibling — --share-stack" >&2
  echo "        is then UNSOUND and must be removed. An EXTRA one is an undeclared" >&2
  echo "        dependency. Either way this object must not ship." >&2
  exit 2
fi
echo "  ok: exactly the four declared atoms, mutually-non-calling premise intact"
echo ""
echo "wrote $OBJ"

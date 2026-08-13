#!/usr/bin/env bash
# E2 of the v0.7.0 ladder (REQ-OS-COMPOSITE-DISSOLVE-001): lower the FUSED gust:os
# composite to a native relocatable object, and measure it.
#
# Why this script exists: v0.6.0 reports the composed OS as 20 741 bytes. That is a
# WASM figure and implies nothing about code size, SRAM or cycles — no native object
# for the OS as a whole has ever existed. build-fused-gustos.sh says so itself
# ("NOT done here: lowering the composite to native"). Everything else left in
# v0.7.0 waits on this: object-code verification (REQ-OS-OBJVERIFY-001) has nothing
# to verify, and per-function WCET (REQ-OS-WCET-001) has nothing to bound.
#
# Pipeline (the same one every driver already goes through):
#   fused-gustos.component.wasm
#     -> meld fuse --memory shared        (component graph -> ONE core module)
#     -> loom optimize --passes inline
#     -> synth compile --target cortex-m3 --all-exports --relocatable
#     -> gustos-dissolved-cm3.o
#
#   bash benches/gust/drivers/build-dissolve-gustos.sh      # compose + dissolve + gate
#   SKIP_BUILD=1 bash .../build-dissolve-gustos.sh          # reuse an existing composite
#
# THE GATE, and the direction that matters: the dissolved object's undefined symbols
# must be EXACTLY the seam the composite declares — gust:hal/mmio (read32/write32)
# plus its own dispatch (gust:os/taskdisp -> poll-task). A LARGER set means the OS
# depends on something we never declared. An EMPTY set is the failure mode people
# mistake for the win: it means the seam was inlined away, and an OS whose hardware
# seam has been swallowed is no longer swappable — the whole thesis is that you
# change the tires, not the car.
#
# Exit: 0 = dissolved and the seam is intact; 1 = a stage failed;
#       2 = the seam is wrong (empty, or carrying something undeclared);
#       4 = the NEGATIVE CONTROL passed, i.e. the gate has stopped discriminating
#           and any green it reports means nothing.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${OUT:-$HERE/gustos-components}"
MELD="${MELD:-meld}"
LOOM="${LOOM:-loom}"
# Default to gale's PIN (0.52.0, #208), not whatever is on PATH — a dissolve measured
# with a different compiler is not comparable to the numbers already recorded.
SYNTH="${SYNTH:-$HOME/pe-toolchain/synth-0.52.0/synth}"
[ -x "$SYNTH" ] || SYNTH="synth"
NM="${NM:-arm-none-eabi-nm}"
SIZE="${SIZE:-arm-none-eabi-size}"
FUSED="$OUT/fused-gustos.component.wasm"
OBJ="$HERE/os-node/gustos-dissolved-cm3.o"
T="$(mktemp -d)"

for t in "$MELD" "$LOOM" "$SYNTH"; do
  command -v "$t" >/dev/null 2>&1 || [ -x "$t" ] || { echo "missing tool: $t"; exit 1; }
done
printf 'tools: meld %s / loom %s / synth %s\n\n' \
  "$("$MELD" --version 2>/dev/null | awk '{print $2}')" \
  "$("$LOOM" --version 2>/dev/null | awk '{print $2}')" \
  "$("$SYNTH" --version 2>/dev/null | awk '{print $2}')"

if [ "${SKIP_BUILD:-0}" != "1" ]; then
  OUT="$OUT" bash "$HERE/build-fused-gustos.sh" | tail -3
  [ "${PIPESTATUS[0]}" -eq 0 ] || { echo "build-fused-gustos.sh FAILED — see above"; exit 1; }
  echo ""
fi
[ -f "$FUSED" ] || { echo "no composite at $FUSED"; exit 1; }
printf '%-28s %8s B  (wasm component)\n' "$(basename "$FUSED")" "$(wc -c < "$FUSED" | tr -d ' ')"

# ── the dissolve ────────────────────────────────────────────────────────────────
echo ""
echo "== meld fuse --memory shared =="
if ! "$MELD" fuse "$FUSED" --memory shared -o "$T/gustos.fused.wasm" 2>"$T/meld.err"; then
  echo "  FAILED:"; sed 's/^/    /' "$T/meld.err" | head -20; exit 1
fi
printf '  %-26s %8s B  (core module)\n' "gustos.fused.wasm" "$(wc -c < "$T/gustos.fused.wasm" | tr -d ' ')"

echo "== loom optimize --passes inline =="
if ! "$LOOM" optimize "$T/gustos.fused.wasm" --passes inline --attestation false \
      -o "$T/gustos.loom.wasm" 2>"$T/loom.err"; then
  echo "  FAILED:"; sed 's/^/    /' "$T/loom.err" | head -20; exit 1
fi
printf '  %-26s %8s B\n' "gustos.loom.wasm" "$(wc -c < "$T/gustos.loom.wasm" | tr -d ' ')"

echo "== synth compile --target cortex-m3 --all-exports --relocatable =="
mkdir -p "$(dirname "$OBJ")"
# --native-pointer-abi: reserve the linear memory IN the object. Without it the
# object is not self-contained — data/bss read 0 while the module still declares an
# arena the embedder must supply, and nothing supplied it, so the composite could
# not execute at all (gale#275). With the arena capped to one page upstream, the
# reservation is 3 608 B of the F100's 8 192 and the object runs (gust_osfused_probe).
if ! "$SYNTH" compile "$T/gustos.loom.wasm" --target cortex-m3 --all-exports \
      --relocatable --native-pointer-abi -o "$OBJ" 2>"$T/synth.err"; then
  echo "  FAILED:"; sed 's/^/    /' "$T/synth.err" | head -20; exit 1
fi

# ── the measurement E2 exists to produce ────────────────────────────────────────
echo ""
echo "== the whole OS, dissolved =="
"$SIZE" "$OBJ" | sed 's/^/  /'
read -r text data bss _ <<<"$("$SIZE" "$OBJ" | awk 'NR==2{print $1, $2, $3}')"
printf '  -> text=%s data=%s bss=%s   total=%s B\n' "$text" "$data" "$bss" "$((text+data+bss))"
printf '  -> against the STM32F100RB budget: %s B of 8192 SRAM (%s%%)\n' \
  "$((data+bss))" "$(( (data+bss)*100/8192 ))"
# Cross-check the reservation against the arena the module DECLARES. Without
# --native-pointer-abi the linear memory is not reserved in the object, so
# data/bss read 0 while the module still needs an embedder-supplied arena —
# "0 B of 8192 SRAM" then reads as "this OS needs no RAM", the same 136x error
# mpu-thin/RESULTS.md caught for itself (gale#275). We now pass the flag, so the
# two should agree; shout if they ever diverge again.
pages="$(wasm-tools print "$T/gustos.loom.wasm" 2>/dev/null \
  | grep -oE '\(memory \(;0;\) [0-9]+' | grep -oE '[0-9]+$' | head -1)"
if [ -n "${pages:-}" ]; then
  kb=$((pages * 64))
  printf '  -> declared linear-memory arena: %s wasm page(s) = %s KB\n' "$pages" "$kb"
  if [ "$((data+bss))" -eq 0 ]; then
    echo "  WARNING: the object reserves NOTHING while the module declares ${kb} KB —"
    echo "           it is not self-contained and cannot execute standalone (gale#275)."
  fi
fi

# ── the gate ────────────────────────────────────────────────────────────────────
echo ""
echo "== seam gate: undefined symbols must be EXACTLY the declared seam =="
undef="$("$NM" -u "$OBJ" 2>/dev/null | awk '{print $NF}' | sort -u)"
n=$(printf '%s' "$undef" | grep -c . || true)
printf '  undefined (%s): %s\n' "$n" "$(printf '%s' "$undef" | tr '\n' ' ')"

if [ "$n" -eq 0 ]; then
  echo "  FAIL: NO undefined symbols. The gust:hal seam was inlined away — this looks"
  echo "        like a smaller object but the OS is no longer swappable. That is the"
  echo "        failure mode, not the win."
  exit 2
fi
# Everything left must be seam. read32/write32 = gust:hal/mmio; poll-task = taskdisp.
stray="$(printf '%s' "$undef" | grep -vE '(^|[^a-z])(read32|write32|poll[-_]task)$' || true)"
if [ -n "$stray" ]; then
  echo "  FAIL: undefined symbols outside the declared seam:"
  printf '%s\n' "$stray" | sed 's/^/          /'
  echo "        The composite declares gust:hal/mmio + gust:os/taskdisp and nothing else."
  exit 2
fi
echo "  ok: the seam is intact and is exactly what the composite declares"

# ── negative control ────────────────────────────────────────────────────────────
# A gate nobody has watched fail is not a gate. wasm-kernel/fused.wasm lowers with NO
# undefined symbols -- exactly the "seam swallowed" shape -- so the gate MUST reject
# it. If this control ever passes, the gate above has stopped discriminating and the
# green it reports means nothing.
echo ""
echo "== negative control: an object with no seam MUST be refused =="
CTL_SRC="$HERE/../wasm-kernel/fused.wasm"
if [ -f "$CTL_SRC" ]; then
  if "$SYNTH" compile "$CTL_SRC" --target cortex-m3 --all-exports --relocatable \
        -o "$T/control.o" >/dev/null 2>&1; then
    cu="$("$NM" -u "$T/control.o" 2>/dev/null | awk '{print $NF}' | sort -u)"
    cn=$(printf '%s' "$cu" | grep -c . || true)
    if [ "$cn" -eq 0 ]; then
      echo "  ok: control has 0 undefined symbols and the gate refuses it -- the gate bites"
    else
      echo "  CONTROL DID NOT FAIL: it has $cn undefined symbols [$(printf '%s' "$cu" | tr '\n' ' ')]"
      echo "        The control is no longer a seam-swallowed object, so it no longer"
      echo "        exercises the gate. Pick a new control."
      exit 4
    fi
  else
    echo "  SKIPPED: control object would not compile"
  fi
else
  echo "  SKIPPED: no control at $CTL_SRC"
fi

echo ""
echo "wrote $OBJ"

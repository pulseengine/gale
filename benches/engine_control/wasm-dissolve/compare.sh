#!/usr/bin/env bash
# Dissolve the engine-control algorithm (../src/control.c) to native via the
# gale wasm path and compare it against rustc/clang LLVM — plus a 3-runtime
# functional differential (host LLVM vs wasmtime; the dissolved .o runs on the
# MCU/Renode lane). This turns control_step into a synth/loom optimization
# surface (gale#74 task #26) and a measurable before/after for synth#390.
#
# Tools: clang (wasm32 + thumbv7m + host), wasm-ld, loom, synth, wasmtime,
# and LLVM binutils (llvm-size/llvm-nm). On macOS: brew llvm provides them —
#   export PATH="/opt/homebrew/opt/llvm/bin:$PATH"
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
SRC="$HERE/../src"
W="$(mktemp -d)"; trap 'rm -rf "$W"' EXIT
SYNTH_TARGET="${SYNTH_TARGET:-cortex-m4f}"

fsize() { llvm-nm --print-size "$1" 2>/dev/null | awk -v s="$2" '$0 ~ s" *$" || $4==s {print ("0x" $2)+0; exit}'; }

echo "toolchain: synth=$(synth --version) loom=$(loom --version) wasmtime=$(wasmtime --version | awk '{print $2}')"
echo

# ── 1. NATIVE (clang -O2 -> thumbv7m): the LLVM floor ──────────────────────
clang --target=thumbv7m-none-eabi -mthumb -O2 -ffreestanding -nostdlib -I"$SRC" -c "$SRC/control.c" -o "$W/control.thumb.o"
clang --target=thumbv7m-none-eabi -mthumb -O2 -ffreestanding -nostdlib -I"$SRC" -c "$SRC/tables.c"  -o "$W/tables.thumb.o"
clang --target=thumbv7m-none-eabi -mthumb -O2 -ffreestanding -nostdlib -I"$SRC" -c "$HERE/shim.c"   -o "$W/shim.thumb.o"
NAT_STEP=$(fsize "$W/control.thumb.o" control_step)
NAT_PACK=$(fsize "$W/shim.thumb.o" control_step_packed)

# ── 2. DISSOLVED (wasm -> loom inline -> synth -> $SYNTH_TARGET) ────────────
clang --target=wasm32-unknown-unknown -O2 -nostdlib -I"$SRC" -c "$SRC/control.c" -o "$W/control.wasm.o"
clang --target=wasm32-unknown-unknown -O2 -nostdlib -I"$SRC" -c "$SRC/tables.c"  -o "$W/tables.wasm.o"
clang --target=wasm32-unknown-unknown -O2 -nostdlib -I"$SRC" -c "$HERE/shim.c"   -o "$W/shim.wasm.o"
wasm-ld --no-entry --export=control_step_packed --allow-undefined --gc-sections \
	"$W/control.wasm.o" "$W/tables.wasm.o" "$W/shim.wasm.o" -o "$W/ec.merged.wasm"
loom optimize "$W/ec.merged.wasm" --passes inline --attestation false -o "$W/ec.opt.wasm" >/dev/null 2>&1
synth compile "$W/ec.opt.wasm" --target "$SYNTH_TARGET" --all-exports --relocatable -o "$W/ec.o" >"$W/synth.log" 2>&1
DIS_TEXT=$(llvm-size "$W/ec.o" | awk 'NR==2{print $1}')
# synth logs each function's machine-code size; the larger is control_step.
DIS_STEP=$(grep -oE '[0-9]+ bytes of machine code' "$W/synth.log" | grep -oE '[0-9]+' | sort -n | tail -1)

# ── 3. 3-RUNTIME FUNCTIONAL DIFFERENTIAL (host LLVM vs wasmtime) ────────────
# Build a host reference that prints control_step_packed for the vectors,
# then run the SAME vectors through wasmtime on the dissolved-input wasm.
cat > "$W/host.c" <<'EOF'
#include "control.h"
#include <stdint.h>
#include <stdio.h>
unsigned control_step_packed(unsigned,unsigned,int,unsigned);
int main(void){
  unsigned v[][4]={{3000,50,80,0},{3000,50,0,0},{8000,90,80,5},{500,10,0,3},
                   {1500,30,40,2},{9999,99,120,0},{0,0,0,15},{6000,75,25,4}};
  for(int i=0;i<8;i++) printf("%u\n", control_step_packed(v[i][0],v[i][1],(int)v[i][2],v[i][3]));
  return 0;
}
EOF
clang -O2 -I"$SRC" "$SRC/control.c" "$SRC/tables.c" "$HERE/shim.c" "$W/host.c" -o "$W/host"
"$W/host" > "$W/host.out"

: > "$W/wasm.out"
while read -r a b c d; do
	wasmtime run --invoke control_step_packed "$W/ec.opt.wasm" "$a" "$b" "$c" "$d" 2>/dev/null >> "$W/wasm.out"
done <<'EOF'
3000 50 80 0
3000 50 0 0
8000 90 80 5
500 10 0 3
1500 30 40 2
9999 99 120 0
0 0 0 15
6000 75 25 4
EOF

if diff -q "$W/host.out" "$W/wasm.out" >/dev/null; then DIFF="PASS (8/8 vectors identical)"; else DIFF="FAIL"; fi

# ── Scoreboard ─────────────────────────────────────────────────────────────
ratio() { awk -v a="$1" -v b="$2" 'BEGIN{if(b>0)printf "%.1fx", a/b; else print "?"}'; }
echo "=============================================================="
echo " engine-control algorithm — native vs dissolved (synth $SYNTH_TARGET)"
echo "=============================================================="
printf " %-26s %10s %12s %8s\n" "function" "native(LLVM)" "dissolved" "ratio"
printf " %-26s %9sB %11sB %8s\n" "control_step" "$NAT_STEP" "$DIS_STEP" "$(ratio "$DIS_STEP" "$NAT_STEP")"
printf " %-26s %9sB %11s %8s\n" "control_step_packed (wrap)" "$NAT_PACK" "—" "—"
printf " %-26s %10s %11sB %8s\n" "whole dissolved .text" "—" "$DIS_TEXT" "(no runtime)"
echo
echo " functional (3-runtime): host-LLVM vs wasmtime  ->  $DIFF"
echo " (the dissolved .o runs on the MCU/Renode lane; size compared above)"
echo "=============================================================="
[ "${DIFF#PASS}" = "$DIFF" ] && exit 1 || exit 0

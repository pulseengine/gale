#!/usr/bin/env bash
# Build the engine-control WebAssembly Component (forked pulseengine/wit-bindgen)
# and prove it: (1) it's a valid Component exporting a sync + an async-stream
# interface, (2) the sync `control.step` runs on wasmtime and is functionally
# identical to the C bench (../src/control.c). The async `crank-stream.process`
# live-run is blocked on pulseengine/witness#107 (see README).
#
# Tools: cargo + wasm32-wasip2, wasm-tools, wasmtime, clang (host), LLVM.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"; SRC="$HERE/../src"
W="$HERE/target/wasm32-wasip2/release/engine_control_component.wasm"

echo "== build (forked wit-bindgen) =="
cargo build --release --target wasm32-wasip2
echo "== it's a Component (exports) =="
wasm-tools component wit "$W" | grep -E 'export gale:' | sed 's/^/  /'
wasm-tools validate --features all "$W" && echo "  validate: OK"

echo "== sync control.step on wasmtime == C bench =="
H="$(mktemp -d)"; trap 'rm -rf "$H"' EXIT
cat > "$H/h.c" <<'C'
#include "control.h"
#include <stdint.h>
#include <stdlib.h>
#include <stdio.h>
int main(int c,char**v){struct engine_state in;in.rpm=atoi(v[1]);in.load_pct=atoi(v[2]);
 in.coolant_c=atoi(v[3]);in.knock_retard=atoi(v[4]);int16_t s;uint16_t f;
 control_step(&in,&s,&f);printf("spark=%d fuel=%u\n",s,f);return 0;}
C
clang -O2 -I"$SRC" "$SRC/control.c" "$SRC/tables.c" "$H/h.c" -o "$H/h"
fail=0
for v in "3000 50 80 0" "8000 90 80 5" "500 10 0 3" "1500 30 40 2" "9999 99 120 0"; do
  set -- $v
  host=$("$H/h" "$1" "$2" "$3" "$4")
  comp=$(wasmtime run -W component-model-async=y \
    --invoke "step({rpm: $1, load-pct: $2, coolant-c: $3, knock-retard: $4})" "$W" 2>/dev/null)
  hs=$(echo "$host" | sed -E 's/spark=([0-9-]+) fuel=([0-9]+)/\1 \2/')
  cs=$(echo "$comp" | sed -E 's/.*spark-advance-deg: ([0-9-]+).*fuel-duration-us: ([0-9]+).*/\1 \2/')
  if [ "$hs" = "$cs" ]; then echo "  [OK ] ($v) -> $cs"; else echo "  [BAD] ($v) C=$hs comp=$cs"; fail=1; fi
done
echo "== async crank-stream.process: built on the fork's amortized StreamReader::next;"
echo "   live-run pending pulseengine/witness#107 (see README) =="
exit $fail

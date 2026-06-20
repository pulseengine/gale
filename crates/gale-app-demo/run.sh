#!/usr/bin/env bash
# Slice 3: the no-C-FFI component loop. Build the app component (imports
# gale:kernel) + the gale-kiln host (provides gale:kernel over gale::*),
# COMPOSE them (wac), and run — proving the app's kernel calls resolve to the
# verified gale::* decisions with no C FFI. Asserts the packed result.
#
# Tools: cargo + wasm32-wasip2, wac, wasmtime (>=42 for -W component-model-async).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$HERE/target/wasm32-wasip2/release/gale_app_demo.wasm"
KILN="$HERE/../gale-kiln/target/wasm32-wasip2/release/gale_kiln.wasm"
OUT="$HERE/target/composed.wasm"

echo "== build app (imports gale:kernel) + gale-kiln (provides it) =="
cargo build --release --target wasm32-wasip2
( cd "$HERE/../gale-kiln" && cargo build --release --target wasm32-wasip2 )

echo "== compose: plug gale-kiln into the app's gale:kernel imports =="
wac plug "$APP" --plug "$KILN" -o "$OUT"
echo "  composed imports remaining:"
wasm-tools component wit "$OUT" | grep -E 'import' | sed 's/^/    /' || echo "    (none — fully satisfied)"

echo "== run composed component on wasmtime =="
got="$(wasmtime run -W component-model-async=y --invoke 'run-demo()' "$OUT" 2>/dev/null)"
# expected: take(0,true)=would-block(1) | give(0,3,false)=increment(1)<<2 | put(0,4,4,_,true)=full(3)<<4
exp=53
if [ "$got" = "$exp" ]; then echo "  [OK ] run-demo() = $got  (would-block|increment|full)"; else echo "  [BAD] run-demo() = $got exp $exp"; exit 1; fi
echo "== loop proven: app -> composed -> verified gale::* (no C FFI) =="

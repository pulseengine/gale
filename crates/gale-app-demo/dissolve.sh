#!/usr/bin/env bash
# The library-OS backing: dissolve the SAME wac-composed component (that run.sh
# runs on wasmtime) to native ELF. compose -> wasm-tools component unbundle ->
# per-core loom inline -> synth --relocatable -> native .o. Oracle: synth must
# exit 0 on every core (the composed component dissolves to native).
#
# Honest: the unbundled cores still carry the component-adapter/cabi_realloc
# canonical-ABI machinery (see FIND-BYOOS-006) — sizes here include that
# dev-time layer; the lean image links the bare gale-logic cores.
#
# Tools: cargo+wasm32-wasip2, wac, wasm-tools, loom, synth, LLVM (llvm-size).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$HERE/target/wasm32-wasip2/release/gale_app_demo.wasm"
KILN="$HERE/../gale-kiln/target/wasm32-wasip2/release/gale_kiln.wasm"
W="$HERE/target/dissolve"; rm -rf "$W"; mkdir -p "$W/mods"
TGT="${SYNTH_TARGET:-cortex-m4f}"

echo "== build + compose (app imports gale:kernel, gale-kiln provides it) =="
cargo build --release --target wasm32-wasip2
( cd "$HERE/../gale-kiln" && cargo build --release --target wasm32-wasip2 )
wac plug "$APP" --plug "$KILN" -o "$W/composed.wasm"

echo "== unbundle composed component into core modules =="
wasm-tools component unbundle "$W/composed.wasm" --module-dir "$W/mods" --output "$W/unbundled.wasm" >/dev/null
ls "$W/mods"/*.wasm | sed 's/^/  /'

echo "== dissolve each core: loom inline -> synth --relocatable ($TGT) =="
fail=0
for m in "$W"/mods/*.wasm; do
  b=$(basename "$m" .wasm)
  loom optimize "$m" --passes inline --attestation false -o "$W/$b.loom.wasm" >/dev/null 2>&1
  if synth compile "$W/$b.loom.wasm" --target "$TGT" --all-exports --relocatable -o "$W/$b.o" >"$W/$b.synth.log" 2>&1; then
    sz=$(llvm-size "$W/$b.o" 2>/dev/null | awk 'NR==2{print $1}')
    fns=$(grep -oE 'Found [0-9]+ exported' "$W/$b.synth.log" | grep -oE '[0-9]+')
    echo "  [OK ] $b -> native .o  (.text ${sz}B, ${fns} fns)"
  else
    echo "  [BAD] $b -> synth FAILED"; tail -3 "$W/$b.synth.log" | sed 's/^/      /'; fail=1
  fi
done
[ $fail -eq 0 ] && echo "== composed component dissolves to native ELF (library-OS backing) ==" || echo "== DISSOLVE FAILED =="
exit $fail

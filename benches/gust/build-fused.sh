#!/usr/bin/env bash
# Rebuild wasm-kernel/fused.o — the meld-FUSED Component-Model composition,
# dissolved to a native Cortex-M3 object that exports `run-demo`.
#
# This is the "components on top, fused down to one module, run on the gust
# stack" path (gale#89). Distinct from crates/gale-app-demo/dissolve.sh, which
# dissolves each component to its OWN .o linked against a TCB arena; here meld
# FUSES the two components into a single merged-memory core first, so the
# dissolved object is one self-contained image with no cross-component imports.
#
# Pipeline:
#   gale-app-demo + gale-kiln  (grow-free cores, cabi-realloc-extern)
#     -> wasm-tools component new --import-passthrough env::__cabi_arena_realloc
#     -> meld fuse --memory shared --address-rebase           (one merged memory)
#     -> loom optimize --passes inline
#     -> strip exports to {memory, run-demo}                  (DCE the realloc path)
#     -> synth compile --target cortex-m3 --all-exports --relocatable
#
# The result (fused.o, checked in) is linked into the gust_fused bin by build.rs
# and produces run-demo() = 53 on bare metal — identical to the same composition
# on wasmtime (crates/gale-app-demo/run.sh).
#
# Tool forks (unmerged at time of writing; re-pin to tags on merge):
#   WASM_TOOLS  pulseengine/wasm-tools@feat/import-passthrough  (wasm-tools#2)
#   wit-bindgen pulseengine/wit-bindgen@integration/embedded-rt-no-grow (#4/#6)
# Plus on PATH: meld, loom, synth, LLVM (llvm-nm/llvm-size). Override binaries:
#   WASM_TOOLS=… MELD=… LOOM=… SYNTH=… ./build-fused.sh
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
GALE="$HERE/../../crates"
WASM_TOOLS="${WASM_TOOLS:-wasm-tools}"
MELD="${MELD:-meld}"
LOOM="${LOOM:-loom}"
SYNTH="${SYNTH:-synth}"
TGT="${SYNTH_TARGET:-cortex-m3}"
W="$HERE/wasm-kernel/.fused-build"; rm -rf "$W"; mkdir -p "$W"
fail=0; note(){ printf '  %s\n' "$*"; }

echo "== 1. build grow-free cores (wasm32-unknown-unknown + cabi-realloc-extern) =="
( cd "$GALE/gale-kiln"     && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
( cd "$GALE/gale-app-demo" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
KILNC="$GALE/gale-kiln/target/wasm32-unknown-unknown/release/gale_kiln.wasm"
APPC="$GALE/gale-app-demo/target/wasm32-unknown-unknown/release/gale_app_demo.wasm"
for c in "$KILNC" "$APPC"; do
  g=$("$WASM_TOOLS" print "$c" 2>/dev/null | grep -c 'memory.grow')
  [ "$g" -eq 0 ] && note "[OK ] $(basename "$c"): memory.grow=0" || { note "[BAD] $(basename "$c") grows memory"; fail=1; }
done

echo "== 2. componentize with arena import passthrough =="
"$WASM_TOOLS" component new "$APPC"  --import-passthrough env::__cabi_arena_realloc -o "$W/app.comp.wasm"  || fail=1
"$WASM_TOOLS" component new "$KILNC" --import-passthrough env::__cabi_arena_realloc -o "$W/kiln.comp.wasm" || fail=1

echo "== 3. meld fuse (app plugs kiln) into one merged-memory core =="
"$MELD" fuse --memory shared --address-rebase "$W/app.comp.wasm" "$W/kiln.comp.wasm" -o "$W/fused.wasm" >/dev/null 2>&1 || fail=1
grow=$("$WASM_TOOLS" print "$W/fused.wasm" 2>/dev/null | grep -c 'memory.grow')
[ "$grow" -eq 0 ] && note "[OK ] fused: single shared memory, memory.grow=0" || { note "[BAD] fused grows"; fail=1; }

echo "== 4. loom inline =="
"$LOOM" optimize "$W/fused.wasm" --passes inline --attestation false -o "$W/fused.loom.wasm" >/dev/null 2>&1 || fail=1

echo "== 5. strip exports to {memory, run-demo} (DCE the cabi_realloc path) =="
"$WASM_TOOLS" print "$W/fused.loom.wasm" 2>/dev/null \
  | grep -vE '\(export "(gale:kernel|cabi_realloc|__data_end|__heap_base)' > "$W/fused.stripped.wat"
"$WASM_TOOLS" parse "$W/fused.stripped.wat" -o "$W/fused.stripped.wasm" || fail=1

echo "== 6. synth dissolve -> relocatable Cortex-M ($TGT) =="
if "$SYNTH" compile "$W/fused.stripped.wasm" --target "$TGT" --all-exports --relocatable -o "$HERE/wasm-kernel/fused.o" >"$W/synth.log" 2>&1; then
  und=$(llvm-nm --undefined-only "$HERE/wasm-kernel/fused.o" 2>/dev/null | wc -l | tr -d ' ')
  has=$(llvm-nm "$HERE/wasm-kernel/fused.o" 2>/dev/null | grep -c 'run-demo')
  sz=$(llvm-size "$HERE/wasm-kernel/fused.o" 2>/dev/null | awk 'NR==2{print $1}')
  if [ "$und" -eq 0 ] && [ "$has" -ge 1 ]; then
    note "[OK ] fused.o: ET_REL, .text=${sz}B, 0 undefined symbols, exports run-demo"
  else note "[BAD] fused.o: undefined=$und run-demo=$has"; fail=1; fi
else note "[BAD] synth failed"; tail -3 "$W/synth.log" | sed 's/^/      /'; fail=1; fi

rm -rf "$W"
[ $fail -eq 0 ] && echo "== FUSED dissolve OK -> wasm-kernel/fused.o (link with build.rs, boot: cargo run --release --bin gust_fused) ==" || { echo "== FAILED =="; exit 1; }

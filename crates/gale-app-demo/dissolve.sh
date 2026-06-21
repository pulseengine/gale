#!/usr/bin/env bash
# Dissolve the composed gale component toward a native library-OS image, via the
# CANONICAL pipeline: meld fuse -> loom -> synth. (meld "fuses", loom "weaves",
# synth "transpiles".) meld statically fuses the app + gale-kiln components into
# ONE core module — import resolution + index-space merge + canonical-ABI at
# build time — which is what loom/synth want. (An earlier revision wrongly used
# wac compose + wasm-tools unbundle, which PRESERVES per-component adapters; meld
# is the correct fusion stage.)
#
# HONEST STATUS: the lean single-address-space MCU image is BLOCKED — both
# components carry `memory.grow` (default Rust/wit-bindgen alloc), so
# `meld fuse --memory shared --address-rebase` (the MCU mode) fails. The fix is
# meld dropping the vestigial cabi_realloc on fusion (meld#298); no_std components are secondary — the same no_std/no_alloc discipline the
# payload already follows. This script demonstrates the pipeline + the block.
#
# Tools: cargo+wasm32-wasip2, meld, loom, synth, LLVM, wasm-tools.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
APP="$HERE/target/wasm32-wasip2/release/gale_app_demo.wasm"
KILN="$HERE/../gale-kiln/target/wasm32-wasip2/release/gale_kiln.wasm"
W="$HERE/target/dissolve"; rm -rf "$W"; mkdir -p "$W"
TGT="${SYNTH_TARGET:-cortex-m4f}"

echo "== build the two components =="
cargo build --release --target wasm32-wasip2
( cd "$HERE/../gale-kiln" && cargo build --release --target wasm32-wasip2 )

echo "== meld fuse (canonical fusion: app + gale-kiln -> single core) =="
meld fuse "$APP" "$KILN" -o "$W/fused.wasm" 2>&1 | grep -iE 'memory strategy|output|size|complete' | sed 's/^/  /'
echo "  gale:kernel imports remaining in fused core: $(wasm-tools print "$W/fused.wasm" 2>/dev/null | grep -c 'import.*gale:kernel')"

echo "== MCU mode: meld fuse --memory shared --address-rebase (the lean target) =="
if meld fuse --memory shared --address-rebase "$APP" "$KILN" -o "$W/fused-shared.wasm" 2>"$W/shared.err"; then
  echo "  shared-memory fuse OK -> loom -> synth"
  loom optimize "$W/fused-shared.wasm" --passes inline --attestation false -o "$W/fs.loom.wasm" >/dev/null 2>&1
  synth compile "$W/fs.loom.wasm" --target "$TGT" --all-exports --relocatable -o "$W/fs.o" && \
    llvm-size "$W/fs.o" | awk 'NR==2{print "  LEAN MCU .text: "$1"B"}'
else
  echo "  [BLOCKED] $(grep -iE 'memory.grow|unsupported' "$W/shared.err" | head -1 | sed 's/^ *//')"
  echo "  -> lean single-address-space MCU image gated on meld#298 (meld must drop vestigial cabi_realloc; gale#89 tracks)"
fi

echo "== diagnostic: multi-memory fused -> synth (NOT MCU-lowerable; shows the block) =="
loom optimize "$W/fused.wasm" --passes inline --attestation false -o "$W/mm.loom.wasm" >/dev/null 2>&1
synth compile "$W/mm.loom.wasm" --target "$TGT" --all-exports --relocatable -o "$W/mm.o" >"$W/mm.synth.log" 2>&1
mems=$(grep -c 'memory\[' "$W/mm.synth.log"); skips=$(grep -c 'skipping function' "$W/mm.synth.log")
echo "  fused multi-memory: $mems memories, synth loud-skipped $skips cross-memory copies (#369 = correct, not a miscompile)"
echo "== pipeline is meld->loom->synth; lean MCU image blocked on gale#89 =="

#!/usr/bin/env bash
# meld#427 fixture — two components whose interface carries a RESOURCE handle,
# fused both ways, so meld can measure per-domain handle tables against a real
# consumer instead of a synthetic one.
#
# Shaped after gust:os/spawn + timer (gale#408): gale intends to move those from
# bare u32 handles to resources precisely because a u32 handle is forgeable and a
# per-instance handle table is not.
#
# meld resolves through the varve pin; override with MELD=/path/to/meld.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"; cd "$HERE"
if [ -n "${MELD:-}" ]; then M="$MELD"
elif command -v varve >/dev/null 2>&1 && varve which meld >/dev/null 2>&1; then M="$(varve which meld | head -1)"
else M="meld"; fi
echo "== meld: $("$M" --version 2>/dev/null | head -1)"

# --emit-relocs on the FINAL link: without it meld REFUSES the shared path
# ("carries no relocation metadata … cannot be rebased safely"), which is itself
# part of what this fixture demonstrates.
for d in provider consumer; do
  ( cd "$d" && RUSTFLAGS="-C link-arg=--emit-relocs" cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 ) || { echo "build $d failed"; exit 2; }
  wasm-tools component new "$d/target/wasm32-unknown-unknown/release/capfix_${d}.wasm" -o "$d.comp.wasm" || exit 2
done

echo "== --memory multi  (boundary KEPT: copy + per-component handle tables)"
"$M" fuse consumer.comp.wasm provider.comp.wasm --memory multi --explain -o fused-multi.wasm | grep -E "lowering|boundaries:"
echo "== --memory shared (boundary ERASED: same-memory, nothing interposed)"
"$M" fuse consumer.comp.wasm provider.comp.wasm --memory shared --address-rebase --explain -o fused-shared.wasm | grep -E "lowering|boundaries:"

for f in fused-multi.wasm fused-shared.wasm; do
  printf "%-18s memories=%s size=%sB\n" "$f" "$(wasm-tools print "$f" | grep -cE '^\s*\(memory ')" "$(wc -c < "$f" | tr -d ' ')"
done

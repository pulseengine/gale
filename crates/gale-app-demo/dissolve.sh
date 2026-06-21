#!/usr/bin/env bash
# Dissolve the gale components to a LEAN native library-OS image — grow-free.
#
# Built on pulseengine/wit-bindgen@integration/embedded-rt-no-grow with the
# `cabi-realloc-extern` feature: the canonical-ABI cabi_realloc is routed to an
# embedder symbol `__cabi_arena_realloc` (the reference TCB arena in tcb/), so
# the component links NO growing allocator and emits NO `memory.grow`. Built for
# wasm32-unknown-unknown (the feature is gated `not(target_env=p2)`), then
# loom -> synth -> native .o, linked against the TCB arena.
#
# Result vs the old wasip2/default build: ~24.8 KB (adapter+dlmalloc+grow) -> <1 KB.
#
# Tools: cargo + wasm32-unknown-unknown, loom, synth, clang(thumbv7m), LLVM.
# NOTE: pins an unmerged wit-bindgen branch (#4/#5/#6); re-pin to a tag on merge.
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
W="$HERE/target/dissolve"; rm -rf "$W"; mkdir -p "$W"
TGT="${SYNTH_TARGET:-cortex-m4f}"
fail=0
note(){ printf '  %s\n' "$*"; }

echo "== build grow-free cores (wasm32-unknown-unknown + cabi-realloc-extern) =="
( cd "$HERE/../gale-kiln" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
( cd "$HERE"             && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
KILNC="$HERE/../gale-kiln/target/wasm32-unknown-unknown/release/gale_kiln.wasm"
APPC="$HERE/target/wasm32-unknown-unknown/release/gale_app_demo.wasm"

echo "== TCB arena provides __cabi_arena_realloc (no memory.grow) =="
clang --target=thumbv7m-none-eabi -mthumb -O2 -ffreestanding -nostdlib -c "$HERE/tcb/cabi_arena.c" -o "$W/cabi_arena.o"
llvm-nm --defined-only "$W/cabi_arena.o" | grep -q __cabi_arena_realloc && note "[OK ] TCB defines __cabi_arena_realloc" || { note "[BAD] arena symbol missing"; fail=1; }

echo "== dissolve each grow-free core: loom -> synth ($TGT) =="
for pair in "kiln:$KILNC" "app:$APPC"; do
  name=${pair%%:*}; core=${pair##*:}
  g=$(wasm-tools print "$core" 2>/dev/null | grep -c 'memory.grow')
  loom optimize "$core" --passes inline --attestation false -o "$W/$name.loom.wasm" >/dev/null 2>&1
  if synth compile "$W/$name.loom.wasm" --target "$TGT" --all-exports --relocatable -o "$W/$name.o" >"$W/$name.synth.log" 2>&1; then
    sz=$(llvm-size "$W/$name.o" 2>/dev/null | awk 'NR==2{print $1}')
    imp=$(llvm-nm --undefined-only "$W/$name.o" 2>/dev/null | grep -c __cabi_arena_realloc)
    if [ "$g" -eq 0 ] && [ "$sz" -lt 4000 ]; then
      note "[OK ] $name: memory.grow=0, synth .text=${sz}B (was ~24848B), imports __cabi_arena_realloc x$imp (-> TCB arena)"
    else note "[BAD] $name: grow=$g size=${sz}B"; fail=1; fi
  else note "[BAD] $name: synth failed"; tail -2 "$W/$name.synth.log" | sed 's/^/      /'; fail=1; fi
done

[ $fail -eq 0 ] && echo "== LEAN grow-free library-OS dissolve OK (gale#89 unblocked via wit-bindgen no-grow branch) ==" || echo "== FAILED =="
exit $fail

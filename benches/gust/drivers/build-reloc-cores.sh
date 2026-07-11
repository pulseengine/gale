#!/usr/bin/env bash
# gale#168 — produce RELOCATABLE gust:os cores that `meld fuse --memory shared` can
# rebase into the shared linear memory (the fix for meld#326's buffer/stateful
# corruption).
#
# ROOT CAUSE: the default `cargo build --target wasm32-unknown-unknown` FINAL-LINKS the
# core and strips the `linking` + `reloc.CODE`/`reloc.DATA` custom sections. meld then
# has no relocation metadata, so `--memory shared` cannot rebase dynamic/absolute
# addresses and corrupts them (log.line's list<u8> buffer, spawn's `static mut`).
#
# THE FIX IS ONE FLAG: `-C link-arg=--emit-relocs`. It keeps the reloc sections in the
# FINAL link — unlike `--relocatable`/`-r`, which (a) conflicts with rustc's injected
# `--gc-sections` (`-r and --gc-sections may not be used together`) and (b) leaves a
# dangling `env::__indirect_function_table` import that fails `wasm-tools component new`.
# `--emit-relocs` yields a VALID self-contained module (table + memory defined) that
# STILL carries `linking` v2 + `reloc.CODE`/`reloc.DATA`, and — verified here — those
# survive `wasm-tools component new` AND `wac plug` into the composite meld consumes.
# (No PIC/dylink: `component new` refuses a core importing `__memory_base`/`memory` as
# globals; this path imports neither.)
#
# Usage:  build-reloc-cores.sh <crate-dir> [<crate-dir> ...]
# Emits:  <crate>/target/wasm32-unknown-unknown/release/<name>.wasm  (reloc core)
#         $OUT/<crate-basename>.comp.wasm                            (gated component)
# Env:    OUT (default: mktemp dir, printed), WASM_TOOLS (default: wasm-tools)
# Gate:   each emitted component MUST carry `linking` + `reloc.CODE`; the script exits
#         non-zero if either is missing (the gale#168 acceptance oracle).
set -euo pipefail

WT="${WASM_TOOLS:-wasm-tools}"
OUT="${OUT:-$(mktemp -d)}"
mkdir -p "$OUT"
[ "$#" -ge 1 ] || { echo "usage: build-reloc-cores.sh <crate-dir> [<crate-dir> ...]" >&2; exit 2; }

echo "# reloc cores -> $OUT"
fail=0
for crate in "$@"; do
  name="$(basename "$crate")"
  # FINAL link, but keep the relocation metadata meld needs.
  ( cd "$crate" && cargo rustc --release --target wasm32-unknown-unknown --lib \
      -- -C link-arg=--emit-relocs >/dev/null 2>&1 )
  core="$(find "$crate/target/wasm32-unknown-unknown/release" -maxdepth 1 -name '*.wasm' \
      | grep -v deps | head -1)"
  [ -n "$core" ] || { echo "  $name: FAIL — no core wasm produced" >&2; fail=1; continue; }

  comp="$OUT/$name.comp.wasm"
  "$WT" component new "$core" -o "$comp"

  # GATE (gale#168): linking v2 + reloc.CODE must survive into the component.
  secs="$("$WT" objdump "$comp" 2>&1)"
  if echo "$secs" | grep -q 'reloc.CODE' && echo "$secs" | grep -q '"linking"'; then
    rc_bytes="$(echo "$secs" | grep 'reloc.CODE' | grep -oE '[0-9]+ bytes' | head -1)"
    printf "  %-16s OK  core=%sB  component carries linking+reloc.CODE (%s)\n" \
      "$name" "$(wc -c <"$core" | tr -d ' ')" "${rc_bytes:-?}"
  else
    echo "  $name: FAIL — component missing linking/reloc.CODE (meld cannot rebase it)" >&2
    fail=1
  fi
done
exit "$fail"

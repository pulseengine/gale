#!/usr/bin/env bash
# Build the gale wasm-cross-LTO release artifacts (sem, cortex-m4f lane).
#
# Pipeline per docs/wasm-module-distribution.md:
#   clang(wasm32) shim+FFI -> wasm-ld (DCE, exported give) -> loom inline
#   (seam dissolve) -> synth ET_REL -> objcopy import renames -> manifest.
#
# Usage: scripts/build-wasm-dist.sh <outdir> [version]
# Tools required on PATH: clang(wasm32), wasm-ld, loom, synth, cargo,
# and an arm-zephyr-eabi objcopy via $TC (default below).
set -euo pipefail
OUT=${1:?usage: build-wasm-dist.sh <outdir> [version]}
VER=${2:-$(git describe --tags --always)}
GALE_ROOT=$(cd "$(dirname "$0")/.." && pwd)
TC="${TC:-arm-zephyr-eabi}"   # prefix; objcopy resolved as ${TC}-objcopy on PATH or $TC_DIR
CLANG="${CLANG:-clang}"; WASMLD="${WASMLD:-wasm-ld}"
SHIM="$GALE_ROOT/zephyr/wasm/sem_give_shim.c"
mkdir -p "$OUT"; t=$(mktemp -d); trap 'rm -rf "$t"' EXIT

# 1. FFI as wasm staticlib (verified decision functions)
( cd "$GALE_ROOT/ffi" && cargo rustc --release --target wasm32-unknown-unknown --crate-type=staticlib )
LIBFFI="$GALE_ROOT/ffi/target/wasm32-unknown-unknown/release/libgale_ffi.a"

# 2. shim -> merged wasm (DCE keeps only the give path) -> loom dissolve
"$CLANG" --target=wasm32-unknown-unknown -O2 -nostdlib -c "$SHIM" -o "$t/shim.o"
"$WASMLD" --no-entry --export=z_impl_k_sem_give --export=gale_k_sem_give_decide \
  --allow-undefined --gc-sections "$LIBFFI" "$t/shim.o" -o "$t/merged.wasm"
loom optimize "$t/merged.wasm" --passes inline --attestation false -o "$OUT/gale-wasm-sem-$VER.wasm"
wasm-tools print "$OUT/gale-wasm-sem-$VER.wasm" > "$OUT/gale-wasm-sem-$VER.wat" 2>/dev/null || true

# 3. synth -> ET_REL (cortex-m4f) + import renames to the gale_w_* wrappers
synth compile "$OUT/gale-wasm-sem-$VER.wasm" --target cortex-m4f --all-exports --relocatable -o "$t/sem.o"
${TC}-objcopy \
  --redefine-sym k_spin_lock=gale_w_spin_lock \
  --redefine-sym z_unpend_first_thread=gale_w_unpend_first_thread \
  --redefine-sym z_ready_thread=gale_w_ready_thread \
  --redefine-sym arch_thread_return_value_set=gale_w_arch_thread_return_value_set \
  --redefine-sym z_reschedule=gale_w_reschedule \
  --redefine-sym z_impl_k_sem_give=synth_k_sem_give_body \
  "$t/sem.o" "$t/sem_renamed.o"
${TC}-objcopy --localize-symbol=gale_k_sem_give_decide "$t/sem_renamed.o"
cp "$t/sem_renamed.o" "$OUT/gale-wasm-sem-$VER-cortex-m4f.o"

# 4. manifest (the trust anchor; sigil signs this)
sha() { python3 -c "import hashlib,sys;print(hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest())" "$1"; }
cat > "$OUT/gale-wasm-manifest-$VER.json" <<JSON
{
  "version": "$VER",
  "primitive": "sem",
  "surface": "z_impl_k_sem_give (give hot path; take/init native)",
  "pipeline": "clang -> wasm-ld -> loom optimize --passes inline -> synth --target cortex-m4f --all-exports --relocatable -> objcopy gale_w_* renames",
  "tools": {
    "clang": "$($CLANG --version | head -1)",
    "wasm-ld": "$($WASMLD --version | head -1)",
    "loom": "$(loom --version)",
    "synth": "$(synth --version)"
  },
  "artifacts": {
    "gale-wasm-sem-$VER.wasm": "$(sha "$OUT/gale-wasm-sem-$VER.wasm")",
    "gale-wasm-sem-$VER-cortex-m4f.o": "$(sha "$OUT/gale-wasm-sem-$VER-cortex-m4f.o")"
  },
  "consume": "CONFIG_GALE_KERNEL_SEM=y CONFIG_GALE_WASM_LTO_SEM=y + -DGALE_WASM_LTO_OBJ_DIR=<this dir>; verify manifest signature first (sigil)"
}
JSON
echo "wasm dist -> $OUT"; ls -la "$OUT"

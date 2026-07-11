#!/usr/bin/env bash
# Build the gale wasm-cross-LTO release artifacts (sem + mutex + msgq, cortex-m4f lane).
#
# Pipeline per docs/wasm-module-distribution.md:
#   clang(wasm32) shim+FFI -> wasm-ld (DCE, exported entry) -> loom inline
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
mkdir -p "$OUT"; t=$(mktemp -d); trap 'rm -rf "$t"' EXIT

# 1. FFI as wasm staticlib (verified decision functions) — shared by all modules.
( cd "$GALE_ROOT/ffi" && cargo rustc --release --target wasm32-unknown-unknown --crate-type=staticlib )
LIBFFI="$GALE_ROOT/ffi/target/wasm32-unknown-unknown/release/libgale_ffi.a"

sha() { python3 -c "import hashlib,sys;print(hashlib.sha256(open(sys.argv[1],'rb').read()).hexdigest())" "$1"; }

# build_module <name> <shim.c> <entry-export> <decide-export> <body-sym> <synth-extra-flags> <rename-pairs...>
# Emits: gale-wasm-<name>-<VER>.wasm/.wat + gale-wasm-<name>-<VER>-cortex-m4f.o
build_module() {
  local name="$1" shim="$2" entry="$3" decide="$4" bodysym="$5" extra="$6"; shift 6
  local renames=("$@")
  "$CLANG" --target=wasm32-unknown-unknown -O2 -nostdlib -c "$shim" -o "$t/$name.shim.o"
  "$WASMLD" --no-entry --export="$entry" --export="$decide" \
    --allow-undefined --gc-sections "$LIBFFI" "$t/$name.shim.o" -o "$t/$name.merged.wasm"
  loom optimize "$t/$name.merged.wasm" --passes inline --attestation false -o "$OUT/gale-wasm-$name-$VER.wasm"
  wasm-tools print "$OUT/gale-wasm-$name-$VER.wasm" > "$OUT/gale-wasm-$name-$VER.wat" 2>/dev/null || true
  # The .wat is human-readable evidence, not a hard artifact. If wasm-tools is
  # missing/errors it leaves a 0-byte file, which GitHub's asset upload rejects
  # (HTTP 400 Bad Content-Length) and blocks the whole release. Drop it if empty so
  # a missing evidence file degrades gracefully instead of failing the release.
  [ -s "$OUT/gale-wasm-$name-$VER.wat" ] || rm -f "$OUT/gale-wasm-$name-$VER.wat"
  # shellcheck disable=SC2086
  synth compile "$OUT/gale-wasm-$name-$VER.wasm" --target cortex-m4f $extra --all-exports --relocatable -o "$t/$name.o"
  local rargs=(); local p; for p in "${renames[@]}"; do rargs+=(--redefine-sym "$p"); done
  rargs+=(--redefine-sym "$entry=$bodysym")
  "${TC}-objcopy" "${rargs[@]}" "$t/$name.o" "$t/$name.renamed.o"
  # Export ONLY the body entry; localize the decide AND synth's internal helpers
  # (func_N), which otherwise stay global with generic names and collide across
  # modules at final link (sem.o and mutex.o both carry func_7/func_8). The
  # gale_w_* imports are undefined references and unaffected by localization.
  "${TC}-objcopy" --keep-global-symbol="$bodysym" "$t/$name.renamed.o"
  cp "$t/$name.renamed.o" "$OUT/gale-wasm-$name-$VER-cortex-m4f.o"
}

# Common kernel-import renames -> the gale_w_* wrappers.
SEM_RENAMES=(
  k_spin_lock=gale_w_spin_lock
  z_unpend_first_thread=gale_w_unpend_first_thread
  z_ready_thread=gale_w_ready_thread
  arch_thread_return_value_set=gale_w_arch_thread_return_value_set
  z_reschedule=gale_w_reschedule
)
# Mutex additionally imports k_spin_unlock and gale_w_current (the latter is
# already a gale_w_* symbol so it needs no rename).
MTX_RENAMES=(
  k_spin_lock=gale_w_spin_lock
  k_spin_unlock=gale_w_spin_unlock
  z_unpend_first_thread=gale_w_unpend_first_thread
  z_ready_thread=gale_w_ready_thread
  arch_thread_return_value_set=gale_w_arch_thread_return_value_set
  z_reschedule=gale_w_reschedule
)

# 2/3. sem (value-path; no native-pointer-abi needed)
build_module sem "$GALE_ROOT/zephyr/wasm/sem_give_shim.c" \
  z_impl_k_sem_give gale_k_sem_give_decide synth_k_sem_give_body "" "${SEM_RENAMES[@]}"

# 2/3. mutex (pointer-arg path -> --native-pointer-abi + r11=0 trampoline at consume time)
build_module mutex "$GALE_ROOT/zephyr/wasm/mutex_unlock_shim.c" \
  z_impl_k_mutex_unlock gale_k_mutex_unlock_decide synth_k_mutex_unlock_body \
  "--native-pointer-abi" "${MTX_RENAMES[@]}"

# msgq imports the same spinlock/wake set as mutex (k_spin_lock + k_spin_unlock,
# unpend, ready, return_value_set, reschedule). gale_w_thread_swap_data /
# gale_w_memcpy / gale_w_msgq_pend are already gale_w_* symbols (undefined
# imports resolved at consume link) so they need no rename.
MSGQ_RENAMES=(
  k_spin_lock=gale_w_spin_lock
  k_spin_unlock=gale_w_spin_unlock
  z_unpend_first_thread=gale_w_unpend_first_thread
  z_ready_thread=gale_w_ready_thread
  arch_thread_return_value_set=gale_w_arch_thread_return_value_set
  z_reschedule=gale_w_reschedule
)

# 2/3. msgq (pointer-arg path -> --native-pointer-abi + r11=0 trampoline at consume time)
build_module msgq "$GALE_ROOT/zephyr/wasm/msgq_put_shim.c" \
  z_impl_k_msgq_put gale_k_msgq_put_decide synth_k_msgq_put_body \
  "--native-pointer-abi" "${MSGQ_RENAMES[@]}"

# 4. manifest (the trust anchor; sigil signs this)
cat > "$OUT/gale-wasm-manifest-$VER.json" <<JSON
{
  "version": "$VER",
  "primitives": ["sem", "mutex", "msgq"],
  "surfaces": {
    "sem": "z_impl_k_sem_give (give hot path; take/init native)",
    "mutex": "z_impl_k_mutex_unlock (unlock hot path; lock/init native; needs r11=0 trampoline)",
    "msgq": "z_impl_k_msgq_put (put hot path: wake-reader / put-ok / return-full dissolved; pend delegates to native gale_w_msgq_pend; get/init native; needs r11=0 trampoline)"
  },
  "pipeline": "clang -> wasm-ld -> loom optimize --passes inline -> synth --target cortex-m4f [--native-pointer-abi for mutex+msgq] --all-exports --relocatable -> objcopy gale_w_* renames",
  "tools": {
    "clang": "$($CLANG --version | head -1)",
    "wasm-ld": "$($WASMLD --version | head -1)",
    "loom": "$(loom --version)",
    "synth": "$(synth --version)"
  },
  "artifacts": {
    "gale-wasm-sem-$VER.wasm": "$(sha "$OUT/gale-wasm-sem-$VER.wasm")",
    "gale-wasm-sem-$VER-cortex-m4f.o": "$(sha "$OUT/gale-wasm-sem-$VER-cortex-m4f.o")",
    "gale-wasm-mutex-$VER.wasm": "$(sha "$OUT/gale-wasm-mutex-$VER.wasm")",
    "gale-wasm-mutex-$VER-cortex-m4f.o": "$(sha "$OUT/gale-wasm-mutex-$VER-cortex-m4f.o")",
    "gale-wasm-msgq-$VER.wasm": "$(sha "$OUT/gale-wasm-msgq-$VER.wasm")",
    "gale-wasm-msgq-$VER-cortex-m4f.o": "$(sha "$OUT/gale-wasm-msgq-$VER-cortex-m4f.o")"
  },
  "consume": "CONFIG_GALE_KERNEL_{SEM,MUTEX,MSGQ}=y CONFIG_GALE_WASM_LTO_{SEM,MUTEX,MSGQ}=y + -DGALE_WASM_LTO_OBJ_DIR=<this dir>; the mutex+msgq objects link with gale_wasm_{mutex,msgq}_tramp.S (r11=0); verify manifest signature first (sigil)"
}
JSON
echo "wasm dist -> $OUT"; ls -la "$OUT"

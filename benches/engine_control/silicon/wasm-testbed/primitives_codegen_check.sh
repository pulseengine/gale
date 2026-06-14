#!/usr/bin/env bash
# Kernel-primitives codegen-regression lane for the wasm-cross-LTO testbed.
#
# run_testbed.sh covers the value-in/value-out algo functions functionally (wasmtime).
# The kernel primitives (k_sem_give, k_mutex_unlock) can't run in wasmtime — they call
# Zephyr kernel imports and use --native-pointer-abi — so this lane is a STATIC codegen
# check on the dissolved bodies, run on every new synth release alongside run_testbed.sh:
#   1. dist-recipe build (clang shim + FFI wasm staticlib -> wasm-ld -> loom inline -> synth)
#   2. compiles without register-exhaustion skip
#   3. seam folded (no `bl gale_k_*_decide` left)
#   4. mutex ONLY: the synth#331 spill-slot-collision signature is ABSENT — the mutex-ptr
#      arg0 home spill slot must be WRITE-ONCE (the bug re-used it for z_unpend_first_thread's
#      result, so the no-waiter lock_count store hit a clobbered base -> silicon deadlock).
#      This catches a #331 regression in OUR consumption before we ship, complementing the
#      frozen synth-side fixture (jess repro/synth-331/).
# Exit 0 = all green.
set -u
CLANG="${CLANG:-/opt/homebrew/opt/llvm/bin/clang}"; WASMLD="${WASMLD:-/opt/homebrew/bin/wasm-ld}"
SYNTH="${SYNTH:-synth}"
OBJDUMP="${OBJDUMP:-/opt/homebrew/opt/llvm/bin/llvm-objdump}"
SIZE="${SIZE:-/opt/homebrew/opt/llvm/bin/llvm-size}"
GALE=/Volumes/Home/git/pulseengine/gale
GR=/Volumes/Home/git/pulseengine/gale-smart-data
SEM_SHIM="$GALE/zephyr/wasm/sem_give_shim.c"
MTX_SHIM="$GR/benches/engine_control/silicon/boards/nucleo_g474re/wasm_mutex_shim_poc.c"
PIPE_SHIM="$GR/benches/engine_control/silicon/boards/nucleo_g474re/wasm_pipe_write_shim_poc.c"
PIPE_RD_SHIM="$GR/benches/engine_control/silicon/boards/nucleo_g474re/wasm_pipe_read_shim_poc.c"
SEM_TAKE_SHIM="$GR/benches/engine_control/silicon/boards/nucleo_g474re/wasm_sem_take_shim_poc.c"
FATAL_SHIM="$GR/benches/engine_control/silicon/boards/nucleo_g474re/wasm_fatal_shim_poc.c"
LIBFFI="$GALE/ffi/target/wasm32-unknown-unknown/release/libgale_ffi.a"
fail=0; t=$(mktemp -d); trap 'rm -rf "$t"' EXIT

echo "== kernel-primitives codegen lane (synth $($SYNTH --version|head -1|awk '{print $2}'), loom $(loom --version|head -1|awk '{print $2}')) =="

# libffi must carry the `pipe` feature (gale_k_pipe_write_decide is #[cfg(feature="pipe")]);
# rebuild if absent or stale (additive feature — sem/mutex decode unchanged).
if [ ! -f "$LIBFFI" ] || ! "$OBJDUMP" -t "$LIBFFI" 2>/dev/null | grep -q gale_k_pipe_write_decide; then
  ( cd "$GALE/ffi" && cargo rustc --release --target wasm32-unknown-unknown --features pipe --crate-type=staticlib >/dev/null 2>&1 )
fi

build_primitive() { # name shim export extra-synth-flags
  local name="$1" shim="$2" exp="$3" flags="$4"
  $CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -c "$shim" -o "$t/$name.shim.o" 2>/dev/null || return 1
  $WASMLD --no-entry --export="$exp" --export="gale_k_${name}_decide" --allow-undefined --gc-sections \
    "$LIBFFI" "$t/$name.shim.o" -o "$t/$name.wasm" 2>/dev/null || return 1
  loom optimize "$t/$name.wasm" --passes inline --attestation false -o "$t/$name.loom.wasm" >/dev/null 2>&1 || return 1
  local out; out=$($SYNTH compile "$t/$name.loom.wasm" --target cortex-m4f $flags --all-exports --relocatable -o "$t/$name.o" 2>&1)
  if echo "$out" | grep -qiE 'register exhaustion|no functions compiled|skipping function'; then
    echo "    codegen: $(echo "$out"|grep -iE 'exhaustion|skipping'|head -1|sed 's/^ *//')"; return 2; fi
  # seam-fold is checked on the OBJECT's relocations by the caller (synth stdout lists
  # imports by name and would false-positive here).
  return 0
}

# --- k_sem_give ---
echo "k_sem_give (dissolved give path):"
build_primitive sem_give "$SEM_SHIM" z_impl_k_sem_give ""
rc=$?
if [ $rc -eq 0 ]; then
  # seam-fold check: no relocation to gale_k_sem_give_decide in the object
  if $OBJDUMP -r "$t/sem_give.o" 2>/dev/null | grep -q gale_k_sem_give_decide; then
    echo "  [BAD] seam not folded (decide reloc present)"; fail=1
  else echo "  [OK ] compiles + seam folded"; fi
else echo "  [BAD] build/codegen rc=$rc"; fail=1; fi

# --- k_sem_take (completes the sem primitive: give + take, v0.1.0 release target) ---
echo "k_sem_take (dissolved blocking-acquire path; completes sem primitive — gate-2 sem-shape):"
build_primitive sem_take "$SEM_TAKE_SHIM" z_impl_k_sem_take ""
rc=$?
if [ $rc -eq 0 ]; then
  seam=$($OBJDUMP -r "$t/sem_take.o" 2>/dev/null | grep -c gale_k_sem_take_decide)
  absr=$($OBJDUMP -r "$t/sem_take.o" 2>/dev/null | grep -cE "R_ARM_MOVW_ABS|R_ARM_MOVT_ABS")
  data=$($SIZE -A "$t/sem_take.o" 2>/dev/null | awk '$1==".data"{print $2}'); data=${data:-0}
  if [ "$seam" -ne 0 ]; then echo "  [BAD] seam not folded (decide reloc present)"; fail=1
  elif [ "$absr" -ne 0 ]; then echo "  [BAD] $absr abs MOVW/MOVT relocs — NOT sem-shaped"; fail=1
  elif [ "$data" -gt 16 ]; then echo "  [BAD] .data=$data B — linmem blob leaked"; fail=1
  else echo "  [OK ] compiles + seam folded + .data=${data}B + 0 abs-relocs (SEM-SHAPED — sem primitive complete: give+take)"; fi
else echo "  [BAD] build/codegen rc=$rc"; fail=1; fi

# --- k_pipe_write (2nd u64-shaped drop-in; NOT --native-pointer-abi; NOT gated on #345) ---
# Gate-2 shape check: pipe's decide returns a packed u64 (gate-1, like sem), so the
# question is whether the more complex 5-action write BODY stays in registers or spills
# to a wasm linmem frame (the synth#345 sret→linmem→64KB .data + MOVW_ABS shape). The
# drop-in criterion is sem-shaped: .data==0, abs-relocs==0, seam folded.
echo "k_pipe_write (dissolved write path; 2nd u64 drop-in — gate-2 sem-shape check):"
build_primitive pipe_write "$PIPE_SHIM" z_impl_k_pipe_write ""
rc=$?
if [ $rc -eq 0 ]; then
  seam=$($OBJDUMP -r "$t/pipe_write.o" 2>/dev/null | grep -c gale_k_pipe_write_decide)
  absr=$($OBJDUMP -r "$t/pipe_write.o" 2>/dev/null | grep -cE "R_ARM_MOVW_ABS|R_ARM_MOVT_ABS")
  data=$($SIZE -A "$t/pipe_write.o" 2>/dev/null | awk '$1==".data"{print $2}'); data=${data:-0}
  if [ "$seam" -ne 0 ]; then echo "  [BAD] seam not folded (decide reloc present)"; fail=1
  elif [ "$absr" -ne 0 ]; then echo "  [BAD] $absr abs MOVW/MOVT relocs — NOT sem-shaped (linmem leak like #345)"; fail=1
  elif [ "$data" -gt 16 ]; then echo "  [BAD] .data=$data B — linmem blob leaked (not a clean drop-in)"; fail=1
  else echo "  [OK ] compiles + seam folded + .data=${data}B + 0 abs-relocs (SEM-SHAPED — clean drop-in, #345-independent)"; fi
else echo "  [BAD] build/codegen rc=$rc"; fail=1; fi

# --- k_pipe_read (read half — pipe ships only if BOTH directions are clean) ---
echo "k_pipe_read (dissolved read path; pipe's 2nd direction — gate-2 sem-shape check):"
build_primitive pipe_read "$PIPE_RD_SHIM" z_impl_k_pipe_read ""
rc=$?
if [ $rc -eq 0 ]; then
  seam=$($OBJDUMP -r "$t/pipe_read.o" 2>/dev/null | grep -c gale_k_pipe_read_decide)
  absr=$($OBJDUMP -r "$t/pipe_read.o" 2>/dev/null | grep -cE "R_ARM_MOVW_ABS|R_ARM_MOVT_ABS")
  data=$($SIZE -A "$t/pipe_read.o" 2>/dev/null | awk '$1==".data"{print $2}'); data=${data:-0}
  if [ "$seam" -ne 0 ]; then echo "  [BAD] seam not folded (decide reloc present)"; fail=1
  elif [ "$absr" -ne 0 ]; then echo "  [BAD] $absr abs MOVW/MOVT relocs — NOT sem-shaped (linmem leak like #345)"; fail=1
  elif [ "$data" -gt 16 ]; then echo "  [BAD] .data=$data B — linmem blob leaked (not a clean drop-in)"; fail=1
  else echo "  [OK ] compiles + seam folded + .data=${data}B + 0 abs-relocs (SEM-SHAPED — pipe clean both directions)"; fi
else echo "  [BAD] build/codegen rc=$rc"; fail=1; fi

# --- k_fatal (5th/last u64-shaped clean decide — closes the u64-clean partition) ---
echo "k_fatal (dissolved k_sys_fatal_error_handler policy — gate-2 sem-shape check):"
build_primitive fatal "$FATAL_SHIM" k_sys_fatal_error_handler ""
rc=$?
if [ $rc -eq 0 ]; then
  seam=$($OBJDUMP -r "$t/fatal.o" 2>/dev/null | grep -c gale_k_fatal_decide)
  absr=$($OBJDUMP -r "$t/fatal.o" 2>/dev/null | grep -cE "R_ARM_MOVW_ABS|R_ARM_MOVT_ABS")
  data=$($SIZE -A "$t/fatal.o" 2>/dev/null | awk '$1==".data"{print $2}'); data=${data:-0}
  if [ "$seam" -ne 0 ]; then echo "  [BAD] seam not folded (decide reloc present)"; fail=1
  elif [ "$absr" -ne 0 ]; then echo "  [BAD] $absr abs MOVW/MOVT relocs — NOT sem-shaped"; fail=1
  elif [ "$data" -gt 16 ]; then echo "  [BAD] .data=$data B — linmem blob leaked"; fail=1
  else echo "  [OK ] compiles + seam folded + .data=${data}B + 0 abs-relocs (SEM-SHAPED — u64-clean partition 5/5 complete)"; fi
else echo "  [BAD] build/codegen rc=$rc"; fail=1; fi

# --- k_mutex_unlock (+ synth#331 signature) ---
echo "k_mutex_unlock (dissolved unlock path; synth#331 spill-slot-collision guard):"
build_primitive mutex_unlock "$MTX_SHIM" z_impl_k_mutex_unlock "--native-pointer-abi"
rc=$?
if [ $rc -eq 0 ]; then
  $OBJDUMP -d --triple=thumbv7em-unknown-none-eabi "$t/mutex_unlock.o" 2>/dev/null > "$t/mtx.dis"
  # isolate the z_impl_k_mutex_unlock body
  awk '/<z_impl_k_mutex_unlock>:/{f=1;next} /^[0-9a-f]+ </{if(f)exit} f' "$t/mtx.dis" > "$t/mtx.body"
  # arg0 home slot = the slot stored from r0 in the FIRST few insns (entry param spill)
  home=$(grep -oE "str\.w[[:space:]]+r0, \[sp, #0x[0-9a-f]+\]" "$t/mtx.body" | head -1 | grep -oE "#0x[0-9a-f]+")
  if [ -z "$home" ]; then echo "  [WARN] could not locate arg0 home slot; skipping #331 signature check"; else
    # WRITE-ONCE invariant: exactly one str to the home slot in the whole body
    nwrite=$(grep -cE "str(\.w)?[[:space:]]+r[0-9]+, \[sp, ${home}\]" "$t/mtx.body")
    if [ "$nwrite" -eq 1 ]; then
      echo "  [OK ] compiles + seam folded + arg0 home ($home) WRITE-ONCE (#331 signature absent)"
    else
      echo "  [BAD] #331 REGRESSION: arg0 home ($home) written $nwrite times — call result aliases it (silent miscompile -> silicon deadlock)"; fail=1
    fi
  fi
else echo "  [BAD] build/codegen rc=$rc"; fail=1; fi

echo "== primitives lane: $( [ $fail -eq 0 ] && echo GREEN || echo 'RED — see [BAD]' ) =="
exit $fail

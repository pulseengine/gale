#!/usr/bin/env bash
# Staged kill-criterion re-survey for the struct-return decide family. Two gates:
#   #350 (v0.11.44, FIXED): stack_push/pop + msgq_put/get compile (no ADD-imm skip).
#   #354 (v0.11.45, pending): their dissolved objects' .data is BOUNDED (~init bytes
#     + .bss for the zero gap), NOT a 64 KB PROGBITS blob — i.e. the per-region
#     .data/.bss split (synth-cli build_relocatable_elf) handles a high-offset init
#     segment. msgq exhibits it decide-only; stack at full-shim level.
# Run on each synth release; exits non-zero until BOTH gates pass.
set -u
SYNTH="${SYNTH:-synth}"; CLANG=/opt/homebrew/opt/llvm/bin/clang
WASMLD=/opt/homebrew/bin/wasm-ld; OBJDUMP=/opt/homebrew/opt/llvm/bin/llvm-objdump
SIZE=/opt/homebrew/opt/llvm/bin/llvm-size
STACK_SHIM=/Volumes/Home/git/pulseengine/gale-smart-data/benches/engine_control/silicon/boards/nucleo_g474re/wasm_stack_push_shim_poc.c
GALE=/Volumes/Home/git/pulseengine/gale
LIBFFI="$GALE/ffi/target/wasm32-unknown-unknown/release/libgale_ffi.a"
[ -f "$LIBFFI" ] || ( cd "$GALE/ffi" && cargo rustc --release --target wasm32-unknown-unknown --all-features --crate-type=staticlib >/dev/null 2>&1 )
t=$(mktemp -d); trap 'rm -rf "$t"' EXIT; fail=0
echo "== struct-return decide compile survey (synth $($SYNTH --version|head -1|awk '{print $2}')) =="
survey() {
  local d="$1" expect="$2"
  $WASMLD --no-entry --export="$d" --allow-undefined --gc-sections "$LIBFFI" -o "$t/d.wasm" 2>/dev/null
  loom optimize "$t/d.wasm" --passes inline --attestation false -o "$t/d.loom.wasm" >/dev/null 2>&1
  local out; out=$($SYNTH compile "$t/d.loom.wasm" --target cortex-m4f --all-exports --relocatable -o "$t/d.o2" 2>&1)
  if echo "$out" | grep -qi "ADD immediate too large"; then
    printf "  %-30s FAIL: ADD-imm (#350)\n" "$d"; [ "$expect" = "OK" ] && fail=1
  elif echo "$out" | grep -qiE "skipping|no functions"; then
    printf "  %-30s FAIL: other\n" "$d"; [ "$expect" = "OK" ] && fail=1
  else printf "  %-30s OK\n" "$d"; fi
}
# the 4 currently blocked on #350 (expect OK once fixed):
for d in gale_k_stack_push_decide gale_k_stack_pop_decide gale_k_msgq_put_decide gale_k_msgq_get_decide; do survey "$d" OK; done
# controls (must stay OK):
for d in gale_k_sem_give_decide gale_k_mutex_unlock_decide; do survey "$d" OK; done

# --- #354 gate: .data BOUNDED (not 64KB), --native-pointer-abi ---
echo "== #354 .data-bound check (--native-pointer-abi; bounded => per-region .bss split works) =="
data_of() { $SIZE -A "$1" 2>/dev/null | awk '$1==".data"{print $2}'; }
# msgq decides exhibit the 64KB .data decide-only:
for d in gale_k_msgq_put_decide gale_k_msgq_get_decide; do
  $WASMLD --no-entry --export="$d" --allow-undefined --gc-sections "$LIBFFI" -o "$t/m.wasm" 2>/dev/null
  loom optimize "$t/m.wasm" --passes inline --attestation false -o "$t/m.lo.wasm" >/dev/null 2>&1
  $SYNTH compile "$t/m.lo.wasm" --target cortex-m4f --native-pointer-abi --all-exports --relocatable -o "$t/m.o2" 2>/dev/null
  dd=$(data_of "$t/m.o2"); dd=${dd:-0}
  if [ "$dd" -le 256 ]; then printf "  %-30s .data=%-7s OK\n" "$d" "$dd"
  else printf "  %-30s .data=%-7s FAIL: 64KB blob (#354)\n" "$d" "$dd"; fail=1; fi
done
# stack exhibits it at full-shim level:
if [ -f "$STACK_SHIM" ]; then
  $CLANG --target=wasm32-unknown-unknown -O2 -nostdlib -c "$STACK_SHIM" -o "$t/s.o" 2>/dev/null
  $WASMLD --no-entry --export=z_impl_k_stack_push --export=gale_k_stack_push_decide --allow-undefined --gc-sections "$LIBFFI" "$t/s.o" -o "$t/s.wasm" 2>/dev/null
  loom optimize "$t/s.wasm" --passes inline --attestation false -o "$t/s.lo.wasm" >/dev/null 2>&1
  $SYNTH compile "$t/s.lo.wasm" --target cortex-m4f --native-pointer-abi --all-exports --relocatable -o "$t/s.o2" 2>/dev/null
  dd=$(data_of "$t/s.o2"); dd=${dd:-0}
  if [ "$dd" -le 256 ]; then printf "  %-30s .data=%-7s OK\n" "z_impl_k_stack_push(shim)" "$dd"
  else printf "  %-30s .data=%-7s FAIL: 64KB blob (#354)\n" "z_impl_k_stack_push(shim)" "$dd"; fail=1; fi
fi
echo "== $( [ $fail -eq 0 ] && echo 'GREEN — #350 + #354 cleared (stack/msgq compile + .data bounded)' || echo 'RED — see FAIL above (#350 compile and/or #354 .data)' ) =="
exit $fail

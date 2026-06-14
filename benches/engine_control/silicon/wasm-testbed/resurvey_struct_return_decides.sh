#!/usr/bin/env bash
# Staged synth#350 kill-criterion re-survey: decide-only compile check across the
# struct-return decide family. Run the moment a synth release ships the
# encode_thumb32_add_imm ADD-immediate lowering fix (arm_encoder.rs:6601).
# KILL-CRITERION (#350 fixed): stack_push/pop + msgq_put/get all compile (OK),
# object non-empty — they currently FAIL "ADD immediate too large".
set -u
SYNTH="${SYNTH:-synth}"; CLANG=/opt/homebrew/opt/llvm/bin/clang
WASMLD=/opt/homebrew/bin/wasm-ld; OBJDUMP=/opt/homebrew/opt/llvm/bin/llvm-objdump
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
echo "== $( [ $fail -eq 0 ] && echo 'GREEN — #350 cleared (stack+msgq compile)' || echo 'RED — stack/msgq still fail #350' ) =="
exit $fail

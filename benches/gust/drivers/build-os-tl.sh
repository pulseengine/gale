#!/usr/bin/env bash
# Build the gust:os v0.4.0 STEP-2 node: an app importing gust:os {time, log} —
# LOG IS THE FIRST BUFFER-CARRYING CAPABILITY (log.line takes list<u8>) — wac-plugged
# with a time provider + a log provider, dissolved to ONE relocatable object that fits
# the STM32F100 8 KiB SRAM budget (bounded-SRAM, NOT 0 — the canonical ABI needs real
# linmem for the buffer). component new ×3 -> wac plug -> meld fuse --memory shared ->
# loom inline -> [2 documented meld#334 workarounds] -> synth 0.39.0 --native-pointer-abi
# --shadow-stack-size 2048 -> os-node/os-tl-cm3.o.
#
# REQUIRES synth >= 0.39.0 (#678 statics down-shift, "unblocks wit-bindgen buffer nodes").
#
# TWO WASM SURGERIES, each a documented workaround for a meld fusion artifact (meld#334):
#   (S1) UNIFY STACK POINTERS: meld fuse --memory shared leaves one mutable __stack_pointer
#        global per fused component (here 3, all init==sp_init 0x100000). A shared memory
#        must have ONE shadow stack; separate SPs both starting at the shared top would
#        clobber each other's frames. We redirect the extra SP globals to global 0 and
#        zero their init so synth uniquely identifies the stack to re-base (synth#707,
#        fixed post-0.39 by #710 — drop S1 once that ships). The specific extra live SP is
#        the log-provider's (global 6); global 3 (time-provider) is already dead.
#   (S2) DROP DEAD SHIM TRAMPOLINE: meld flattens wac's lowered-import shim to direct calls
#        but leaves a vestigial, mis-typed call_indirect trampoline exported as "0"
#        (func: (a,b)=>table2[0](a,b), where table2[0] is the read32 import — a type
#        mismatch synth correctly declines). It is unreachable except via that export;
#        dropping the export lets synth DCE it -> clean dissolve. Drop S2 once meld#334
#        DCEs the dead trampoline.
#
# RESULT (synth 0.39.0): text=2272 data=84 bss=3104 = 5460 B, fits 8192; exports
# gust:os/log@0.1.0#line + gust:os/time@0.1.0#{now,elapsed,deadline} + cabi_realloc.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
WT="${WASM_TOOLS:-wasm-tools}"; MELD="${MELD:-meld}"; LOOM="${LOOM:-loom}"
SYNTH="${SYNTH:-$HOME/pe-toolchain/synth-0.39.0/synth}"
T="$(mktemp -d)"

# 1. build the three guest wasm modules
for c in app-tl time-provider log-provider; do
  ( cd "$HERE/$c" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
done
"$WT" component new "$(find "$HERE/app-tl/target"        -name gust_app_tl.wasm        | grep -v deps | head -1)" -o "$T/app.comp.wasm"
"$WT" component new "$(find "$HERE/time-provider/target" -name gust_time_provider.wasm | grep -v deps | head -1)" -o "$T/time.comp.wasm"
"$WT" component new "$(find "$HERE/log-provider/target"  -name gust_log_provider.wasm  | grep -v deps | head -1)" -o "$T/log.comp.wasm"

# 2. compose (app imports gust:os {time, log}; the two providers export them) + fuse + inline
wac plug "$T/app.comp.wasm" --plug "$T/time.comp.wasm" --plug "$T/log.comp.wasm" -o "$T/composite.wasm"
"$MELD" fuse "$T/composite.wasm" --memory shared -o "$T/fused.wasm" >/dev/null 2>&1
"$LOOM" optimize "$T/fused.wasm" --passes inline --attestation false -o "$T/loom.wasm" >/dev/null 2>&1

# 3. the two meld#334 surgeries (on the WAT). Indices are NOT hardcoded — meld's global
#    ordering isn't stable across fuses, so we discover the stack-pointer set dynamically.
"$WT" print "$T/loom.wasm" > "$T/node.wat"
SP_INIT=1048576   # 0x100000 = the shared-linmem top the per-component SP globals init to
#   S1: find every mutable global whose init == SP_INIT (the __stack_pointer set), keep the
#       first as the one shared descending stack, redirect the others' get/set to it and
#       zero their init so synth uniquely identifies the stack to re-base (synth#707/#710).
SPS=$(grep -oE "\(global \(;[0-9]+;\) \(mut i32\) i32.const ${SP_INIT}\)" "$T/node.wat" \
      | sed -E 's/.*\(;([0-9]+);\).*/\1/')
CANON=$(echo "$SPS" | head -1)
for i in $(echo "$SPS" | tail -n +2); do
  # redirect uses (wasm-tools print = one instr per line → anchor to end-of-line)
  sed -i.bak -E "s/^([[:space:]]*global\.(get|set)) ${i}\$/\1 ${CANON}/" "$T/node.wat"
  # zero this extra SP global's init so only CANON == SP_INIT
  sed -i.bak -E "s/\(global \(;${i};\) \(mut i32\) i32.const ${SP_INIT}\)/(global (;${i};) (mut i32) i32.const 0)/" "$T/node.wat"
done
echo "S1: unified $(echo "$SPS" | wc -l | tr -d ' ') __stack_pointer globals (kept ;${CANON};) -> $(grep -cE "\(global \(;[0-9]+;\) \(mut i32\) i32.const ${SP_INIT}\)" "$T/node.wat") remaining at SP_INIT"
#   S2: drop the vestigial wac-shim trampoline export so synth DCEs it. meld exports it
#       under a numeric name ("0"); it's the flattened lowered-import shim, unreachable.
SHIM_EXPORT=$(grep -oE '\(export "[0-9]+" \(func [0-9]+\)\)' "$T/node.wat" | head -1)
if [ -n "$SHIM_EXPORT" ]; then grep -vF "$SHIM_EXPORT" "$T/node.wat" > "$T/node.clean.wat"
  echo "S2: dropped vestigial shim export $SHIM_EXPORT"
else cp "$T/node.wat" "$T/node.clean.wat"; fi
"$WT" parse "$T/node.clean.wat" -o "$T/node.clean.wasm"

# 4. dissolve to the bounded-SRAM relocatable object
mkdir -p "$HERE/os-node"
"$SYNTH" compile "$T/node.clean.wasm" --target cortex-m3 --all-exports --relocatable \
  --native-pointer-abi --shadow-stack-size 2048 -o "$HERE/os-node/os-tl-cm3.o" >/dev/null 2>&1

arm-none-eabi-size "$HERE/os-node/os-tl-cm3.o" 2>/dev/null | awk 'NR==2{print "os-tl-cm3.o: text="$1" data="$2" bss="$3" ("$1+$2+$3" B / 8192 budget)"}' \
  || echo "os-tl-cm3.o: $(wc -c < "$HERE/os-node/os-tl-cm3.o") B"

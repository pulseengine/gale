#!/usr/bin/env bash
# Build gale's gust:os PROVIDER components — the consumable, publishable unit of
# gust's OS-as-wasm-component-model (DD-OS-DELIVERY-001, gale#223/#224).
#
# Each provider is a wasm COMPONENT that EXPORTS a gust:os capability interface and
# imports ONLY seams the node itself resolves — the exact delivery invariant: a
# downstream that imports gust:os pulls the signed component and fuses it for its
# target; the only native residuals are gust:hal (mmio) and the app-supplied
# gust:os/taskdisp dispatch. Consumed by build-fused-gustos.sh, which composes the
# whole set into the single fused gust:os component (gale#224).
#
#   bash benches/gust/drivers/build-gustos-components.sh          # build + verify
#   OUT=/some/dir bash .../build-gustos-components.sh             # choose output dir
#
# Verifies (the oracle for the DD invariant): every built component EXPORTS at least
# one gust:os/* interface and imports NOTHING outside gust:hal/*, gust:os/* and the
# node-internal gust:sched/* seam. A leaking scheduler / env / heap / WASI import
# (the core-module shape the release tarball ships) fails here.
#
# Also verifies OWNERSHIP: exactly ONE provider crate compiles the verified executor,
# i.e. exactly one provider can hold scheduler state. spawn/timer reach it over
# gust:sched/tasks instead of each carrying a private copy (which is what made a
# spawn handle meaningless to timer/exec before).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${OUT:-$HERE/gustos-components}"
WT="${WASM_TOOLS:-wasm-tools}"
mkdir -p "$OUT"

# gust:os provider crates → the wasm cdylib each emits. This is the complete v0.4.0
# published set; build-fused-gustos.sh composes exactly these five.
PROVIDERS=(
  "time-provider:gust_time_provider"
  "log-provider:gust_log_provider"
  "spawn-provider:gust_spawn_provider"
  "exec-provider:gust_exec_provider"
  "timer-provider:gust_timer_provider"
)

# Checked at SOURCE, not in the binary. A `Tasks` table lives in wasm .bss, which a
# module never declares, so no reliable binary signal says "this module holds one" —
# see build-fused-gustos.sh's gate 4 for the signal that was tried and rejected.
# Compiling plain/src/executor.rs is how a provider comes to hold scheduler state,
# and that IS exactly decidable here.
# Matches the `#[path = ".../executor.rs"] mod executor;` directive itself, not any
# mention of the file — spawn/timer both DISCUSS it in comments now.
EXEC_INCLUDE='#\[path *= *"[^"]*executor\.rs"'
owners=""

fail=""
for entry in "${PROVIDERS[@]}"; do
  crate="${entry%%:*}"; wasm_name="${entry##*:}"
  printf '== %s ==\n' "$crate"
  # Cap the declared linear memory. wasm-ld defaults to a 1 MB shadow stack under
  # --stack-first plus a 1 MB --global-base, so each provider declares 17 pages
  # (1088 KB). Fused, that is what synth --native-pointer-abi must reserve: the
  # composite came out at data 1 049 632 B — 128x the STM32F100RB's whole 8 KB of
  # SRAM, so the object could not be self-contained and could not run (gale#275).
  # At one page (the wasm minimum) the same composite dissolves to
  # data 512 / bss 3096 = 3 608 B of 8 192 (44%) and executes correctly.
  ( cd "$HERE/$crate" && RUSTFLAGS="${RUSTFLAGS:-} -C link-arg=-zstack-size=2048 -C link-arg=--initial-memory=65536 -C link-arg=--max-memory=65536" \
      cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
  core="$(find "$HERE/$crate/target/wasm32-unknown-unknown/release" -maxdepth 1 -name "$wasm_name.wasm" | head -1)"
  comp="$OUT/$crate.component.wasm"
  "$WT" component new "$core" -o "$comp"

  wit="$("$WT" component wit "$comp")"
  exports_gustos="$(printf '%s' "$wit" | grep -cE '^\s*export gust:os/' || true)"
  # The leak check: any `import X` whose target is neither the hardware seam
  # (gust:hal/*) nor a gust:os/* capability nor the node-internal scheduler seam
  # (gust:sched/*). gust:os/taskdisp is the one import that legitimately survives
  # composition — it is the trusted dispatch the APPLICATION supplies. gust:sched/*
  # must NOT survive it; build-fused-gustos.sh gates that on the composite.
  imports="$(printf '%s' "$wit" | grep -E '^\s*import ' | sed 's/^ *//;s/;$//' || true)"
  bad_imports="$(printf '%s' "$imports" | grep -vE '^import (gust:hal|gust:os|gust:sched)/' || true)"

  # `|| true` inside the group: grep exits 1 on no-match, which under `pipefail`
  # would abort the whole script at the first provider that (correctly) has none.
  n_inc="$({ grep -rlE "$EXEC_INCLUDE" "$HERE/$crate/src" 2>/dev/null || true; } | wc -l | tr -d ' ')"
  if [ "$n_inc" -gt 0 ]; then owners="$owners $crate"; fi

  if [ "$exports_gustos" -lt 1 ]; then
    echo "  FAIL: exports no gust:os/* interface"; fail="$fail $crate"
  elif [ -n "$bad_imports" ]; then
    echo "  FAIL: imports outside gust:hal/gust:os:"; printf '%s\n' "$bad_imports" | sed 's/^/    /'; fail="$fail $crate"
  else
    echo "  ok: exports gust:os ($exports_gustos iface) → $comp"
    printf '%s\n' "$imports" | sed 's/^/    residual /'
  fi
done

echo ""
echo "== executor ownership =="
echo "  provider crates with a #[path] include of the executor:$owners"
if [ "$owners" != " exec-provider" ]; then
  echo "  FAIL: exactly one provider (exec-provider) may compile the executor"
  fail="$fail executor-ownership"
else
  echo "  ok: exec-provider owns the scheduler; the others import gust:sched/tasks"
fi

if [ -n "$fail" ]; then
  echo ""; echo "gust:os component invariant FAILED:$fail"; exit 1
fi
echo ""
echo "gust:os provider components built + invariant held (export gust:os, import only gust:hal/gust:os/gust:sched, one executor owner). Output: $OUT"

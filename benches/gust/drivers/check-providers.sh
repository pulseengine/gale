#!/usr/bin/env bash
# gust:os syscall-seam drift gate (gale#214).
#
# The gust:os providers implement WIT Guest traits generated from
# drivers/wit-os/gust-os.wit via `wit_bindgen::generate!`. If the WIT gains a
# method but a Guest impl is not updated, the provider crate fails to compile
# (E0046 "not all trait items implemented") — but NOTHING in CI built these
# crates, so the break shipped GREEN and only surfaced when the dissolve path
# was exercised by hand (#202 added time.resolution() to the WIT without the
# time-provider impl → silently broke gust_os_tl/ts; #213 fixed it).
#
# This gate builds every WIT-implementing provider + app crate for wasm32 (the
# same target build-os-tl.sh / build-os-ts.sh dissolve from), so a WIT/impl
# drift fails CI RED. Cargo-only — no meld/loom/synth/qemu needed; the compile
# IS the oracle (wit-bindgen turns the WIT into the Guest trait).
#
#   bash benches/gust/drivers/check-providers.sh
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE"

# App worlds (import gust:os capabilities) + the providers that export them.
# Every crate here calls wit_bindgen::generate! against ../wit-os, so a WIT
# change that outruns an impl breaks the crate that must implement it.
CRATES=(
  app-time app-tl app-ts
  time-provider log-provider spawn-provider timer-provider exec-provider
)

fail=""
for c in "${CRATES[@]}"; do
  if [ ! -f "$c/Cargo.toml" ]; then
    echo "SKIP  $c (no Cargo.toml)"
    continue
  fi
  printf '%-16s ' "$c"
  if ( cd "$c" && cargo build --release --target wasm32-unknown-unknown ) >/tmp/gust-prov-$c.log 2>&1; then
    echo "ok"
  else
    echo "FAILED"
    echo "----- $c build output (tail) -----"
    tail -20 /tmp/gust-prov-$c.log
    fail="$fail $c"
  fi
done

if [ -n "$fail" ]; then
  echo ""
  echo "gust:os provider drift gate FAILED:$fail"
  echo "A WIT change in drivers/wit-os/gust-os.wit likely outran a Guest impl"
  echo "(E0046). Update the provider's impl to match the WIT interface."
  exit 1
fi
echo ""
echo "gust:os provider drift gate: all ${#CRATES[@]} provider/app crates build for wasm32 — WIT ↔ impl in sync."

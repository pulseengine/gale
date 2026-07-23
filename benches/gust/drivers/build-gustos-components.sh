#!/usr/bin/env bash
# Build gale's gust:os PROVIDER components — the consumable, publishable unit of
# gust's OS-as-wasm-component-model (DD-OS-DELIVERY-001, gale#223/#224).
#
# Each provider is a wasm COMPONENT that EXPORTS a gust:os capability interface and
# imports ONLY the gust:hal seam — the exact delivery invariant: a downstream that
# imports gust:os pulls the signed component and fuses it for its target; the only
# native residual is gust:hal. This is the first, minimal demonstrator (the time
# provider) ahead of the full fused gale-nano runtime component (gale#224).
#
#   bash benches/gust/drivers/build-gustos-components.sh          # build + verify
#   OUT=/some/dir bash .../build-gustos-components.sh             # choose output dir
#
# Verifies (the oracle for the DD invariant): every built component EXPORTS at least
# one gust:os/* interface and imports NOTHING outside gust:hal/*. A leaking scheduler
# / env / heap import (the core-module shape the release tarball ships) fails here.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
OUT="${OUT:-$HERE/gustos-components}"
WT="${WASM_TOOLS:-wasm-tools}"
mkdir -p "$OUT"

# gust:os provider crates → the wasm cdylib each emits. Start with the time provider
# (0-SRAM, exports gust:os/time); extend to log/spawn/timer as they harden into the
# published set (the full fused runtime is gale#224).
PROVIDERS=(
  "time-provider:gust_time_provider"
)

fail=""
for entry in "${PROVIDERS[@]}"; do
  crate="${entry%%:*}"; wasm_name="${entry##*:}"
  printf '== %s ==\n' "$crate"
  ( cd "$HERE/$crate" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
  core="$(find "$HERE/$crate/target/wasm32-unknown-unknown/release" -maxdepth 1 -name "$wasm_name.wasm" | head -1)"
  comp="$OUT/$crate.component.wasm"
  "$WT" component new "$core" -o "$comp"

  wit="$("$WT" component wit "$comp")"
  exports_gustos="$(printf '%s' "$wit" | grep -cE '^\s*export gust:os/' || true)"
  # imports outside gust:hal/* (the leak check): any `import X` whose target isn't gust:hal/
  bad_imports="$(printf '%s' "$wit" | grep -E '^\s*import ' | grep -vE 'import gust:hal/' || true)"

  if [ "$exports_gustos" -lt 1 ]; then
    echo "  FAIL: exports no gust:os/* interface"; fail="$fail $crate"
  elif [ -n "$bad_imports" ]; then
    echo "  FAIL: imports outside gust:hal:"; printf '%s\n' "$bad_imports" | sed 's/^/    /'; fail="$fail $crate"
  else
    echo "  ok: exports gust:os ($exports_gustos iface), residual import = gust:hal only → $comp"
  fi
done

if [ -n "$fail" ]; then
  echo ""; echo "gust:os component invariant FAILED:$fail"; exit 1
fi
echo ""
echo "gust:os provider components built + invariant held (export gust:os, import only gust:hal). Output: $OUT"

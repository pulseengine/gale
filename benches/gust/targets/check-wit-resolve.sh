#!/usr/bin/env bash
# Assert every generated per-target WIT world RESOLVES against the real gust:hal
# seam — i.e. its imports (e.g. gust:hal/mmio) name interfaces that actually exist
# in benches/gust/drivers/wit/gust-hal.wit. Catches a generated world that drifts
# from the seam. Needs wasm-tools on PATH.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"           # benches/gust/targets
HAL="$HERE/../drivers/wit/gust-hal.wit"
GEN="$HERE/generated"

shopt -s nullglob
worlds=("$GEN"/world-*.wit)
if [ ${#worlds[@]} -eq 0 ]; then
  echo "check-wit-resolve: no generated worlds found in $GEN" >&2
  exit 1
fi

for w in "${worlds[@]}"; do
  d="$(mktemp -d)"
  mkdir -p "$d/deps/hal"
  cp "$w" "$d/world.wit"
  cp "$HAL" "$d/deps/hal/gust-hal.wit"
  if wasm-tools component wit "$d" >/dev/null; then
    echo "check-wit-resolve: $(basename "$w") resolves against gust:hal — OK"
  else
    echo "::error::$(basename "$w") does NOT resolve against gust:hal" >&2
    rm -rf "$d"
    exit 1
  fi
  rm -rf "$d"
done

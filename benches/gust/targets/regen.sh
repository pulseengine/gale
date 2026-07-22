#!/usr/bin/env bash
# Regenerate benches/gust/targets/generated/ from the AADL target models.
#
# The committed spar-items JSON (tools/gust-target-gen/tests/golden/f100.items.json)
# is the canonical spar output. When `spar` is on PATH (local dev) this script
# refreshes that fixture from the .aadl models first; in CI (no spar) it uses the
# committed fixture as-is. Either way the generated/ tree is produced from the
# fixture, so the CI drift gate (git diff --exit-code) needs only cargo — no spar.
#
# The generated files are COMMITTED and DO NOT EDIT them by hand.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"            # benches/gust/targets
REPO="$(cd "$HERE/../../.." && pwd)"             # repo root
GEN="$HERE/generated"
FIXTURE="$REPO/tools/gust-target-gen/tests/golden/f100.items.json"
mkdir -p "$GEN"

if command -v spar >/dev/null 2>&1; then
  echo "regen: spar found — refreshing $FIXTURE from the .aadl models"
  spar items --format json "$HERE"/*.aadl > "$FIXTURE"
else
  echo "regen: spar not found — using the committed spar-items fixture"
fi

for BOARD in \
  "STM32F100::Board.vldiscovery" \
  "STM32G474::Board.nucleo"; do
  cargo run --quiet --manifest-path "$REPO/tools/gust-target-gen/Cargo.toml" -- \
    --items "$FIXTURE" --board "$BOARD" --out "$GEN"
done

echo "regen: generated/ is up to date."

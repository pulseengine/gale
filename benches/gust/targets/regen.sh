#!/usr/bin/env bash
# Regenerate benches/gust/targets/generated/ from the AADL target models.
# The generated files are COMMITTED (DO NOT EDIT them by hand); a CI drift gate
# re-runs this and asserts `git diff --exit-code` over generated/.
#
# Needs: spar (~/.cargo/bin/spar) + a Rust toolchain for gust-target-gen.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"          # benches/gust/targets
REPO="$(cd "$HERE/../../.." && pwd)"           # repo root
GEN="$HERE/generated"
mkdir -p "$GEN"

J="$(mktemp)"
trap 'rm -f "$J"' EXIT
spar items --format json "$HERE"/*.aadl > "$J"

for BOARD in \
  "STM32F100::Board.vldiscovery" \
  "STM32G474::Board.nucleo"; do
  cargo run --quiet --manifest-path "$REPO/tools/gust-target-gen/Cargo.toml" -- \
    --items "$J" --board "$BOARD" --out "$GEN"
done

echo "regen: generated/ is up to date."

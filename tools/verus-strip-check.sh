#!/usr/bin/env bash
# Verus-strip convergence check.
#
# Builds verus-strip and runs --check to compare stripped src/ against plain/.
# Tagged "manual" in Bazel — run explicitly via: bazel test //:verus_strip_check
#
# Exit codes:
#   0 — all files converge (stripped src/ matches plain/)
#   1 — divergences found (expected while plain/ is hand-maintained)

set -euo pipefail

GALE_ROOT="$(cd "$(dirname "$0")/.." && pwd)"

# Build verus-strip if needed
cargo build --release --manifest-path "$GALE_ROOT/tools/verus-strip/Cargo.toml" --quiet 2>&1

# Run convergence check
"$GALE_ROOT/tools/verus-strip/target/release/verus-strip" \
    --check "$GALE_ROOT/src/" "$GALE_ROOT/plain/src/" 2>&1

exit $?

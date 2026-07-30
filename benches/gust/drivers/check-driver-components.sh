#!/usr/bin/env bash
# Oracle for REQ-DRV-COMPONENT-001: every thin-seam driver is a wasm COMPONENT whose
# imports are gust:hal interfaces, and no raw `env` import survives anywhere.
#
# This gate is written BEFORE the drivers are componentized and is EXPECTED TO FAIL until
# they are. That is the point: a gate whose red state nobody has seen is not evidence that
# the green state means anything. Run it now and it should report every driver as a core
# module importing env.
#
#   bash benches/gust/drivers/check-driver-components.sh
#
# Fails non-zero if any driver is not a component, or imports anything outside gust:hal.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
WT="${WASM_TOOLS:-wasm-tools}"

# DISCOVERED, not enumerated. A hardcoded list is how spi-thin, can-thin and dma-own
# were silently skipped by the first version of this gate — a driver nobody checks is
# indistinguishable from a driver that passes.
DRIVERS=()
for _d in "$HERE"/*-thin "$HERE"/dma-own; do
    [ -f "$_d/Cargo.toml" ] || continue
    DRIVERS+=("$(basename "$_d")")
done
[ "${#DRIVERS[@]}" -gt 0 ] || { echo "no drivers discovered under $HERE — the gate is not looking where it thinks"; exit 1; }

fail=""
printf '%-12s | %-11s | %s\n' "driver" "shape" "imports"
printf '%-12s-+-%-11s-+-%s\n' "------------" "-----------" "----------------------------------"

for d in "${DRIVERS[@]}"; do
    [ -d "$HERE/$d" ] || continue
    ( cd "$HERE/$d" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 ) || true
    core="$(find "$HERE/$d/target/wasm32-unknown-unknown/release" -maxdepth 1 -name '*.wasm' 2>/dev/null | head -1)"
    [ -n "$core" ] || { printf '%-12s | %-11s | %s\n' "$d" "NO WASM" "-"; fail="y"; continue; }

    # A component decodes as a component; a core module does not.
    if "$WT" component new "$core" -o "/tmp/dc-$d.wasm" >/dev/null 2>&1; then
        wit="$("$WT" component wit "/tmp/dc-$d.wasm" 2>/dev/null || true)"
        imports="$(printf '%s' "$wit" | sed -n 's/^  import \(.*\);$/\1/p' | tr '\n' ' ')"
        # Every import must be a gust:hal interface. gust:os is NOT allowed here: a driver
        # sits BELOW the OS and must not depend upward on it.
        bad="$(printf '%s' "$wit" | sed -n 's/^  import \(.*\);$/\1/p' | grep -v '^gust:hal/' || true)"
        if [ -n "$bad" ]; then
            printf '%-12s | %-11s | %s\n' "$d" "component" "$imports"
            echo "    FAIL: import outside gust:hal: $(printf '%s' "$bad" | tr '\n' ' ')"
            fail="y"
        else
            printf '%-12s | %-11s | %s\n' "$d" "COMPONENT" "$imports"
        fi
    else
        envs="$("$WT" print "$core" 2>/dev/null | grep -oE '\(import "env" "[a-z0-9_]+"' | sed 's/.*"\([a-z0-9_]*\)"$/\1/' | tr '\n' ' ' || true)"
        printf '%-12s | %-11s | %s\n' "$d" "core module" "env: ${envs:-?}"
        echo "    FAIL: not a component — raw env imports are untyped (REQ-DRV-COMPONENT-001)"
        fail="y"
    fi
done

echo
if [ -n "$fail" ]; then
    cat <<'MSG'
REQ-DRV-COMPONENT-001 NOT satisfied.

Expected while axis A is in progress. Each driver must move onto the wit_bindgen pattern
(see drivers/time-provider/src/lib.rs) so its mmio calls are gust:hal interface calls
rather than raw env externs. Note that WIT-typing a driver changes its dissolved object's
undefined-symbol shape, as exec-provider did in v0.6.0 — re-pin the per-driver symbol
contract and record it rather than absorbing it silently.
MSG
    exit 1
fi
echo "REQ-DRV-COMPONENT-001: all drivers are components importing only gust:hal."

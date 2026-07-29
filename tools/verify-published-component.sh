#!/usr/bin/env bash
# Verify a PUBLISHED gale component the way a stranger would: pull it from the registry,
# check its signature, and read its world back — using nothing from this working tree.
#
# This is the step that turns DD-OS-DELIVERY-001 from "implemented" into evidenced. A
# signature nobody outside has verified is not evidence, and a delivery invariant that
# only holds in our own build directory has not been delivered.
#
#   tools/verify-published-component.sh 0.6.0
#   tools/verify-published-component.sh 0.6.0 gale-nano-time
#
# Requires: wkg, cosign, wasm-tools. Run it somewhere other than a gale checkout if you
# want the "clean machine" property to mean anything.
set -euo pipefail

VERSION="${1:?usage: verify-published-component.sh <version> [package]}"
PKG="${2:-gale-nano}"
REF="ghcr.io/pulseengine/${PKG}:${VERSION}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

echo "==> pulling $REF"
wkg oci pull "$REF" -o "$WORK/c.wasm"
printf '    %s bytes\n' "$(wc -c <"$WORK/c.wasm" | tr -d ' ')"

echo "==> verifying the signature (keyless OIDC, must chain to this repo's workflow)"
cosign verify "$REF" \
    --certificate-identity-regexp 'https://github.com/pulseengine/gale/.*' \
    --certificate-oidc-issuer https://token.actions.githubusercontent.com \
    >/dev/null
echo "    signature OK"

echo "==> it is a component, and it validates"
wasm-tools validate "$WORK/c.wasm"
wit="$(wasm-tools component wit "$WORK/c.wasm")"
echo "    valid component"

# The delivery invariant, checked against the PUBLISHED bytes rather than a local build.
if [ "$PKG" = "gale-nano" ]; then
    echo "==> delivery invariant (DD-OS-DELIVERY-001 / REQ-OS-DELIVERY-001)"
    residual="$(printf '%s' "$wit" | sed -n 's/^  import \(.*\);$/\1/p' | sort)"
    expected="$(printf 'gust:hal/mmio@0.1.0\ngust:os/taskdisp@0.1.0\n')"
    if [ "$residual" != "$(printf '%s' "$expected" | sort)" ]; then
        echo "FAIL: residual imports are not exactly the hardware seam + app task poll:"
        printf '%s\n' "$residual" | sed 's/^/      /'
        exit 1
    fi
    echo "    residual imports are exactly gust:hal/mmio + gust:os/taskdisp"

    # gust:sched is the internal scheduler seam. If it is reachable from the published
    # world, a tenant importing gust:os could mutate scheduling state — the exact reason
    # it was put in its own package instead of being folded into gust:os.
    if printf '%s' "$wit" | grep -q "gust:sched"; then
        echo "FAIL: gust:sched is visible in the published component — scheduler seam leaked"
        exit 1
    fi
    echo "    gust:sched sealed (not reachable from the published world)"

    n="$(printf '%s' "$wit" | grep -c '^  export gust:os/')"
    [ "$n" -eq 5 ] || { echo "FAIL: expected 5 gust:os exports, got $n"; exit 1; }
    echo "    exports 5 gust:os interfaces"
fi

echo
echo "$REF verifies: pulled, signed, and the world is what we said it is."

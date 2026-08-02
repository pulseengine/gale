#!/usr/bin/env bash
# Pull the PUBLISHED, signed gale-nano component and transpile it for the browser.
#
# The point of this demo is that it consumes the SAME ARTIFACT the STM32 runs a dissolved
# version of — not a browser-shaped rebuild of the same source. So it starts from
# `wkg oci pull`, not from `cargo build`. If the bytes here differ from the bytes on the
# registry, the demo has lost its meaning.
#
#   ./build.sh                    # pull 0.6.0 + transpile
#   VERSION=0.6.0 ./build.sh
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
VERSION="${VERSION:-0.6.0}"
REF="ghcr.io/pulseengine/gale-nano:${VERSION}"

command -v wkg >/dev/null || { echo "wkg not found (cargo install wkg)"; exit 1; }
command -v npx >/dev/null || { echo "npx not found (needed for @bytecodealliance/jco)"; exit 1; }

echo "==> pulling $REF"
wkg oci pull "$REF" -o "$HERE/web/gale-nano.wasm"
printf '    %s bytes\n' "$(wc -c <"$HERE/web/gale-nano.wasm" | tr -d ' ')"

# Verifiable on stage: the same signature check the release notes publish.
if command -v cosign >/dev/null; then
    echo "==> cosign verify (the artifact is signed; this is checkable by anyone)"
    cosign verify "$REF" \
        --certificate-identity-regexp 'https://github.com/pulseengine/gale/.*' \
        --certificate-oidc-issuer https://token.actions.githubusercontent.com >/dev/null \
        && echo "    signature OK"
else
    echo "    (cosign not installed — skipping signature check)"
fi

echo "==> jco transpile"
npx --yes @bytecodealliance/jco transpile "$HERE/web/gale-nano.wasm" \
    -o "$HERE/web/gen" --no-typescript >/dev/null
echo "    -> web/gen/"

echo
echo "serve:  cd web && python3 -m http.server 8000   →  http://localhost:8000/"

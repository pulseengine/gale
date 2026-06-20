#!/usr/bin/env bash
# Build the browser gust wasm (the same kiln-async kernel as benches/gust, as wasm).
set -euo pipefail
cd "$(dirname "$0")"
cargo build --release --target wasm32-unknown-unknown
mkdir -p web
cp target/wasm32-unknown-unknown/release/gust_browser.wasm web/gust.wasm
echo "built web/gust.wasm ($(wc -c < web/gust.wasm) bytes)"
echo "serve:  (cd web && python3 -m http.server) then open http://localhost:8000/"

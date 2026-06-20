#!/usr/bin/env bash
# Build the gale-kiln host component and prove the verified gale::* decisions
# run behind the gale:kernel Component Model interface (no C FFI). Asserts the
# decision for representative inputs; exits non-zero on any mismatch.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
W="$HERE/target/wasm32-wasip2/release/gale_kiln.wasm"
cargo build --release --target wasm32-wasip2
wasm-tools component wit "$W" | grep -E 'export gale:' | sed 's/^/  /'
wasm-tools validate --features all "$W" >/dev/null && echo "  validate: OK"
inv() { wasmtime run -W component-model-async=y --invoke "$1" "$W" 2>/dev/null; }
chk() { local got; got="$(inv "$1")"; if [ "$got" = "$2" ]; then echo "  [OK ] $1 -> $got"; else echo "  [BAD] $1 -> got '$got' exp '$2'"; FAIL=1; fi; }
FAIL=0
echo "== gale::sem::give_decide =="
chk "give(0, 3, false)" increment
chk "give(3, 3, false)" saturated
chk "give(0, 3, true)"  wake
echo "== gale::sem::take_decide =="
chk "take(0, true)"  would-block
chk "take(2, false)" acquired
echo "== gale::mutex::lock_decide =="
chk "lock(0, true, false, false)"  acquire
chk "lock(1, false, true, false)"  reentrant
echo "== gale::msgq::put_decide / event::post_decide =="
chk "put(0, 4, 4, false, true)"  full     # full + no-wait -> fail
chk "put(0, 4, 4, false, false)" pend     # full + blocking -> pend
chk "put(0, 0, 4, false, false)" store    # space -> store
chk "post(0, 5, 15)" 5
exit $FAIL

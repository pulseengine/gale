#!/usr/bin/env bash
# wsc.facts phase-1 ingestion oracle — the gale-side tripwire for the 0.7x lever
# (VCR-PERF-002 / synth#494 / synth#242).
#
# synth 0.31.0 shipped `wsc.facts` custom-section ingestion (schema-v1, #624):
# loom (eventually) forwards a proof-carrying facts section, synth parses it and
# — in a later phase, behind SYNTH_FACT_SPEC — elides provably-dead code (the
# gust_mix clamp under `ch in [524,1524]` collapses to `add #476`, the measured
# 0.45x-native floor in gust_floor_bench).
#
# Phase 1 has NO consumer: a facts-carrying module compiles .text-byte-IDENTICAL
# to the stripped module. This script asserts exactly that on gale's OWN gust_mix,
# and doubles as a regression tripwire:
#
#   * PASS (byte-identical) => phase-1 (ingestion-only) is still what ships.
#   * FAIL (bytes differ)   => a synth build now CONSUMES the facts. That is the
#                              signal to flip to gust_floor_bench and measure the
#                              specialized gust_mix against the 0.45x floor.
#
# It also exercises the normative fail-safe skew rule (docs/design/
# wsc-facts-encoding.md): malformed / unknown-version / unknown-kind sections are
# ignored with a stderr warning, never an error, always byte-identical.
#
# Requires: synth (>=0.31.0) + wasm-tools + python3 on PATH, or SYNTH=/path/to/synth.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
KERNEL="$HERE/../wasm-kernel/gust_kernel.wasm"
SYNTH="${SYNTH:-synth}"
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT

command -v "$SYNTH" >/dev/null || { echo "FAIL: synth not found (set SYNTH=)"; exit 2; }
command -v wasm-tools >/dev/null || { echo "FAIL: wasm-tools not found"; exit 2; }

compile() { "$SYNTH" compile "$1" --target cortex-m3 --all-exports --relocatable -o "$2" 2>"$3"; }

# gust_mix is export func index 2; its clamp premise is value-range [524,1524] on
# value_id 0 (`local.get 0`, the `ch` parameter). Emit the schema-v1 section and
# the three skew fixtures.
python3 - "$KERNEL" "$WORK" <<'PY'
import sys
src = open(sys.argv[1], 'rb').read(); WORK = sys.argv[2]
def uleb(n):
    o = bytearray()
    while True:
        b = n & 0x7f; n >>= 7
        o.append(b | 0x80 if n else b)
        if not n: break
    return bytes(o)
def sect(payload):
    name = b'wsc.facts'; content = bytes([len(name)]) + name + payload
    return bytes([0x00]) + uleb(len(content)) + content
def write(tag, payload): open(f"{WORK}/{tag}.wasm", 'wb').write(src + sect(payload))
# valid v1: value-range [524,1524] on func 2 (gust_mix), value 0 (ch); 524=8c 04, 1524=f4 0b (sLEB128)
write('facts',   bytes([0x01, 0x01, 0x01, 0x02, 0x00, 0x04, 0x8c, 0x04, 0xf4, 0x0b]))
write('badver',  bytes([0x99, 0x01, 0x01, 0x02, 0x00, 0x04, 0x8c, 0x04, 0xf4, 0x0b]))  # unknown version
write('trunc',   bytes([0x01, 0x01, 0x01, 0x02, 0x00, 0x04, 0x8c, 0x04]))              # body_len overruns
write('unkkind', bytes([0x01, 0x01, 0x40, 0x02, 0x00, 0x02, 0xaa, 0xbb]))              # unknown kind 0x40
PY

wasm-tools validate "$WORK/facts.wasm"

compile "$KERNEL"          "$WORK/base.o"  "$WORK/base.err"
compile "$WORK/facts.wasm" "$WORK/facts.o" "$WORK/facts.err"

fail=0

# 1) Phase-1 gate: valid facts => byte-identical, and NO skip/skew warning (fact kept).
if cmp -s "$WORK/base.o" "$WORK/facts.o"; then
  echo "PASS  phase-1 gate: valid wsc.facts compiles byte-identical ($(wc -c <"$WORK/facts.o" | tr -d ' ') B ELF)"
else
  echo "SIGNAL phase-1 gate BROKEN: facts-carrying gust_mix now compiles DIFFERENTLY"
  echo "       => a synth build CONSUMES the facts. Go measure the specialized"
  echo "          gust_mix against the 0.45x floor (cargo run --bin gust_floor_bench)."
  fail=1
fi
if grep -qiE "wsc\.facts|skew|skipped|ignoring" "$WORK/facts.err"; then
  echo "FAIL  valid v1 fact triggered a skew/skip warning (should be kept silently):"
  grep -iE "wsc\.facts|skew|skipped|ignoring" "$WORK/facts.err" | sed 's/^/      /'
  fail=1
else
  echo "PASS  valid v1 fact kept (no skew/skip warning)"
fi

# 2) Fail-safe skew rule: each malformed section warns + stays byte-identical.
for t in badver trunc unkkind; do
  compile "$WORK/$t.wasm" "$WORK/$t.o" "$WORK/$t.err"
  warned=0; identical=0
  grep -qiE "wsc\.facts|skew|skipped|ignoring|unparseable" "$WORK/$t.err" && warned=1
  cmp -s "$WORK/base.o" "$WORK/$t.o" && identical=1
  if [ $warned -eq 1 ] && [ $identical -eq 1 ]; then
    echo "PASS  skew/$t: warned + byte-identical (fail-safe held)"
  else
    echo "FAIL  skew/$t: warned=$warned byte-identical=$identical (expected 1/1)"
    fail=1
  fi
done

[ $fail -eq 0 ] && echo "ALL PASS — wsc.facts phase-1 ingestion verified on gale's gust_mix" || echo "see above"
exit $fail

#!/usr/bin/env bash
# gust-adc-silicon — flash + capture the REAL-hardware ADC anchor on an
# STM32VLDISCOVERY (Cortex-M3 / STM32F100, thumbv7m). The dissolved adc-thin
# driver reads the on-chip Vrefint (channel 17, 1.20 V internal reference) —
# a self-contained silicon check needing no external wiring.
#
#   ./run-adc.sh            # build for f100 + flash via the Pi + capture semihosting
#
# The VLDISCOVERY's onboard ST-LINK/V1 has old firmware that openocd can only
# drive via the HLA interface (interface/stlink-hla.cfg), and on macOS the V1
# needs openocd-under-sudo — so the board hangs off a Raspberry Pi (PI_HOST)
# which runs openocd and streams semihosting back over ssh.
#
# EXPECTED (self-checking):
#   "gust-adc-silicon OK: Vrefint = <~1646> raw on real STM32F100 silicon ...
#    implies VDDA ≈ <~2985> mV ..."   -> exit SUCCESS.
# Vrefint is factory-nominal ≈ 1.20 V; the raw code depends on VDDA (≈3.0 V on
# VLDISCOVERY → ~1638). A raw near 0 or full-scale = the internal channel was
# not converted (TSVREFE / clock / calibration problem). See RESULTS-f100.md.
set -euo pipefail
# The bench host moved: it is wohl.local now, and the default ssh user resolves
# (verified: `ssh wohl.local` lands as r@wohl), so no user prefix is needed. The
# variable keeps its name so existing PI_HOST=... invocations still work.
PI_HOST="${PI_HOST:-wohl.local}"
HERE="$(cd "$(dirname "$0")" && pwd)"
BENCH="$(dirname "$HERE")"   # benches/gust
cd "$BENCH"

cp memory.x /tmp/gust-memory.x.bak
trap 'cp /tmp/gust-memory.x.bak "$BENCH/memory.x"' EXIT
cp targets/generated/memory-stm32f100.x memory.x

echo "== build gust_adc_silicon for f100 (thumbv7m, target-f100) =="
cargo build --release --bin gust_adc_silicon \
  --no-default-features --features target-f100 --target thumbv7m-none-eabi
ELF="target/thumbv7m-none-eabi/release/gust_adc_silicon"

echo "== copy ELF to $PI_HOST and flash via openocd (ST-LINK/V1 HLA), under a claim =="
scp "$ELF" "$PI_HOST:/tmp/gust_adc_silicon.elf"
# shellcheck source=./bench-claim.sh
. "$HERE/bench-claim.sh"
# CLAIMED. This used to `ssh $PI_HOST openocd` bare: an unclaimed attach to a probe on a
# host shared with wohl, which is exactly what the claim convention exists to prevent.
#
# Pinned to the V1 by VID:PID: the host has four ST-LINKs, and stlink-hla.cfg matches all.
# Benign 'SRST error' is expected — the V1 has no hardware reset line; openocd
# falls back to sysresetreq, which works.
LOG="$(mktemp)"
rc=0
claim_remote "$PI_HOST" "${BENCH_DEV:-stlink-v1}" "gale: adc-f100" -- \
  "timeout 45 openocd -f interface/stlink-hla.cfg -c \"hla_vid_pid 0x0483 0x3744\" -f target/stm32f1x.cfg \
     -c init -c \"reset halt\" -c \"arm semihosting enable\" \
     -c \"program /tmp/gust_adc_silicon.elf verify\" -c \"reset run\" \
     -c \"sleep 3000\" -c shutdown" >"$LOG" 2>&1 || rc=$?
grep -iE "gust-adc|verified|error" "$LOG" || true

# THE VERDICT IS THE FIRMWARE'S OWN LINE. This used to be the exit status of
# `grep -iE "gust-adc|verified"` — and openocd prints "** Verified OK **" after any good
# flash, so a board that flashed and then printed nothing still exited 0.
if grep -q "^gust-adc-silicon OK:" "$LOG"; then
  echo "== PASS (openocd exit $rc)"; exit 0
fi
echo "== FAIL: no 'gust-adc-silicon OK:' line (openocd/claim exit $rc). Full log: $LOG" >&2
exit 1

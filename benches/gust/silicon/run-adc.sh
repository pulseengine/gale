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
PI_HOST="${PI_HOST:-pi@192.168.178.88}"
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

echo "== copy ELF to $PI_HOST and flash via openocd (ST-LINK/V1 HLA) =="
scp "$ELF" "$PI_HOST:/tmp/gust_adc_silicon.elf"
# Benign 'SRST error' is expected — the V1 has no hardware reset line; openocd
# falls back to sysresetreq, which works.
ssh "$PI_HOST" 'timeout 45 openocd -f interface/stlink-hla.cfg -f target/stm32f1x.cfg \
  -c "init" -c "reset halt" -c "arm semihosting enable" \
  -c "program /tmp/gust_adc_silicon.elf verify" -c "reset run" \
  -c "sleep 3000" -c "shutdown" 2>&1 | grep -iE "gust-adc|verified"'

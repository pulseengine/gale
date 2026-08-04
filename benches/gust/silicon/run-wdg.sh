#!/usr/bin/env bash
# gust-wdg-silicon — flash + capture the REAL-hardware IWDG watchdog anchor on a
# NUCLEO-G474RE (onboard ST-LINK/V3, sudo-free via probe-rs). The wdg-thin IWDG
# driver is register-identical F1==G4, so the dissolved cortex-m3 .o programs the
# G4 watchdog verbatim (thumbv7m ⊂ thumbv7em).
#
#   ./run-wdg.sh            # build for g474re + flash + stream semihosting
#
# Needs: probe-rs (brew install probe-rs-tools) + the board connected.
#
# EXPECTED (two-boot, self-checking):
#   boot 1: "gust-wdg-silicon: boot 1 ... Arming the REAL IWDG ..."
#           "gust-wdg-silicon: armed (... is_running=1). NOT refreshing ..."
#   ~1.2 s later the hardware IWDG resets the chip; probe-rs re-runs from flash:
#   boot 2: "gust-wdg-silicon OK: IWDG watchdog reset CONFIRMED on real G474RE
#            silicon (RCC_CSR=0x..., IWDGRSTF=1) ..."  -> exit SUCCESS.
#
# If probe-rs drops the session on the watchdog reset, just re-run this script:
# IWDGRSTF persists across the reset (only RMVF/power-off clears it), so the next
# boot lands on the CONFIRMED path.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
BENCH="$(dirname "$HERE")"   # benches/gust
CHIP=STM32G474RETx
cd "$BENCH"

cp memory.x /tmp/gust-memory.x.bak
trap 'cp /tmp/gust-memory.x.bak "$BENCH/memory.x"' EXIT
cp targets/generated/memory-stm32g474.x memory.x

echo "== build gust_wdg_silicon for g474re (thumbv7em) =="
cargo build --release --bin gust_wdg_silicon --target thumbv7em-none-eabi
ELF="target/thumbv7em-none-eabi/release/gust_wdg_silicon"

# Pick the ST-LINK explicitly when more than one probe is attached. With the
# ESP32-C3 also plugged in (an EspJtag probe), a bare `probe-rs run` goes
# interactive and dies with "Failed to parse probe index" under any non-tty
# harness — so the three-dies-at-once session is exactly when this breaks.
# Auto-select the first ST-LINK; override with PROBE=VID:PID:SERIAL.
PROBE="${PROBE:-$(probe-rs list 2>/dev/null \
    | sed -n 's/.*-- \([0-9a-fA-F]*:[0-9a-fA-F]*:[^ ]*\) (ST-LINK).*/\1/p' | head -1)}"
PROBE_ARG=()
[ -n "$PROBE" ] && PROBE_ARG=(--probe "$PROBE")

echo "== flash + capture on $CHIP via probe-rs${PROBE:+ (probe $PROBE)} (watch for 'CONFIRMED'; Ctrl-C after) =="
probe-rs run --chip "$CHIP" "${PROBE_ARG[@]}" "$ELF"

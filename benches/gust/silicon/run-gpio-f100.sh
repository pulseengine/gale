#!/usr/bin/env bash
# gust-gpio-silicon — flash + capture the REAL-hardware GPIO anchor on an
# STM32VLDISCOVERY (Cortex-M3 / STM32F100, thumbv7m). The dissolved gpio-thin
# driver configures PC8 (LD4, blue) and PC9 (LD3, green) as push-pull outputs and
# drives them via BSRR; every level is read back from the PIN through IDR, so the
# verdict is self-checking over semihosting — nobody has to watch an LED.
#
#   ./run-gpio-f100.sh                       # openocd on this host (needs sudo)
#   OCD_HOST=pi@192.168.178.88 ./run-gpio-f100.sh   # openocd over ssh on a Pi
#
# The VLDISCOVERY's onboard ST-LINK/V1 has old firmware that openocd can only drive
# via the HLA interface (interface/stlink-hla.cfg), and on macOS the V1 needs
# openocd-under-sudo — hence OCD_HOST, which hangs the board off a Raspberry Pi that
# runs openocd and streams semihosting back over ssh. A benign "SRST error" is
# expected: the V1 has no hardware reset line and openocd falls back to sysresetreq.
#
# EXPECTED (self-checking, single boot):
#   gust-gpio-silicon: driving REAL STM32F100 GPIOC @0x40011000 ...
#   gust-gpio-silicon: CRH=0x...33... (PC8 nibble=0x3, PC9 nibble=0x3, want 0x3)
#     | PC8 set->IDR=1 clear->IDR=0 | PC9 toggle->IDR=1 toggle->IDR=0 | PC8 after=0
#   gust-gpio-silicon OK: ... every BSRR set/clear/toggle moved the PHYSICAL pin ...
#   -> exit SUCCESS.
# Anything that reads the SAME level in both directions is reported as FAIL (stuck
# pin / unclocked port), so the run cannot false-pass. NOTE the firmware also prints
# a MODEL GAP line: the GPIOC port-clock bit (RCC_APB2ENR bit 4) is not in the AADL
# model and is declared in the firmware — keep that line in any capture.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
BENCH="$(dirname "$HERE")"   # benches/gust
cd "$BENCH"

ELF=target/thumbv7m-none-eabi/release/gust_gpio_silicon

echo "== build gust_gpio_silicon for f100 (thumbv7m, target-f100) =="
# memory.x is the qemu map in-tree; swap in the GENERATED F100 map (flash @0x08000000)
# for the duration of the build and restore it on exit.
cp memory.x /tmp/gust-memory.x.bak
trap 'cp /tmp/gust-memory.x.bak "$BENCH/memory.x"' EXIT
cp targets/generated/memory-stm32f100.x memory.x
rm -f "$ELF"                              # force a relink at 0x08000000
touch src/bin/gust_gpio_silicon.rs
cargo build --release --bin gust_gpio_silicon \
  --no-default-features --features target-f100 --target thumbv7m-none-eabi
arm-none-eabi-readelf -l "$ELF" | grep LOAD   # must show 0x08000000, not 0x0

OCD=(openocd -f interface/stlink-hla.cfg -c "transport select swd"
     -f target/stm32f1x.cfg -c "reset_config none separate")

if [ -n "${OCD_HOST:-}" ]; then
    scp "$ELF" "$OCD_HOST:/tmp/gust_gpio_silicon.elf"
    FW=/tmp/gust_gpio_silicon.elf
else
    FW="$ELF"
fi

run_ocd() {  # run_ocd <extra -c args...>
    if [ -n "${OCD_HOST:-}" ]; then
        # shellcheck disable=SC2029
        ssh "$OCD_HOST" "sudo $(printf '%q ' "${OCD[@]}" "$@")"
    else
        sudo "${OCD[@]}" "$@"
    fi
}

echo "== flash + run + capture semihosting (openocd self-terminates after ~3 s) =="
run_ocd -c init -c "reset halt" -c "arm semihosting enable" \
        -c "program $FW verify" -c "reset run" \
        -c "sleep 3000" -c shutdown 2>&1 | grep -iE "gust-gpio|verified" || true

echo "== expected: 'gust-gpio-silicon OK' with set->IDR=1 and clear->IDR=0 =="

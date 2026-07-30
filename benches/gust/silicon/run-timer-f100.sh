#!/usr/bin/env bash
# gust-timer-silicon — flash + capture the REAL-hardware TIM2 anchor on an
# STM32VLDISCOVERY (Cortex-M3 / STM32F100, thumbv7m). The dissolved timer-thin
# driver starts TIM2 and reads its counter; the run passes only if the counter
# ADVANCES BY ITSELF — the one thing the RAM-window Renode gate cannot show, since
# a plain memory window cannot distinguish a live counter from a frozen one.
#
#   ./run-timer-f100.sh                       # openocd on this host (needs sudo)
#   OCD_HOST=pi@192.168.178.88 ./run-timer-f100.sh   # openocd over ssh on a Pi
#
# Same ST-LINK/V1 constraints as run-gpio-f100.sh / run-adc.sh: HLA interface, and
# openocd-under-sudo on macOS (hence OCD_HOST to push it onto a Pi). A benign
# "SRST error" is expected — the V1 has no reset line; openocd uses sysresetreq.
#
# DO NOT set DBGMCU_CR's DBG_TIM2_STOP: openocd must leave TIM2 free-running while
# the core runs, or the counter would legitimately freeze and the run would (rightly)
# report FAIL. The default DBGMCU_CR = 0 does the right thing; this script never
# touches it.
#
# EXPECTED (self-checking, single boot):
#   gust-timer-silicon: reading REAL STM32F100 TIM2 @0x40000000 (CNT @0x40000024) ...
#   gust-timer-silicon: samples <4 strictly increasing values> (all_same=false,
#     monotonic=true) | ... | deadline: ... early=0 fired=true ... |
#     SR.UIF before ack=1 after ack=0 (wrapped=true)
#   gust-timer-silicon OK: ... read a counter that advances by itself ...
#   -> exit SUCCESS.
# If all four samples are IDENTICAL the firmware reports FAIL ("the counter is NOT
# running"), never a pass. The most likely cause is TIM2 unclocked, which is also the
# MODEL GAP the firmware prints on its first line: the AADL has no
# Rcc::Apb1enr_Offset / Tim2en_Bit, so RCC_APB1ENR is derived in the firmware rather
# than generated. Keep that line in any capture.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
BENCH="$(dirname "$HERE")"   # benches/gust
cd "$BENCH"

ELF=target/thumbv7m-none-eabi/release/gust_timer_silicon

echo "== build gust_timer_silicon for f100 (thumbv7m, target-f100) =="
cp memory.x /tmp/gust-memory.x.bak
trap 'cp /tmp/gust-memory.x.bak "$BENCH/memory.x"' EXIT
cp targets/generated/memory-stm32f100.x memory.x
rm -f "$ELF"                              # force a relink at 0x08000000
touch src/bin/gust_timer_silicon.rs
cargo build --release --bin gust_timer_silicon \
  --no-default-features --features target-f100 --target thumbv7m-none-eabi
arm-none-eabi-readelf -l "$ELF" | grep LOAD   # must show 0x08000000, not 0x0

OCD=(openocd -f interface/stlink-hla.cfg -c "transport select swd"
     -f target/stm32f1x.cfg -c "reset_config none separate")

if [ -n "${OCD_HOST:-}" ]; then
    scp "$ELF" "$OCD_HOST:/tmp/gust_timer_silicon.elf"
    FW=/tmp/gust_timer_silicon.elf
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
        -c "sleep 3000" -c shutdown 2>&1 | grep -iE "gust-timer|verified" || true

echo "== expected: 'gust-timer-silicon OK' with all_same=false and monotonic=true =="

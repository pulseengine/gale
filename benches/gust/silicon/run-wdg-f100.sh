#!/usr/bin/env bash
# wdg-thin on real STM32F100 silicon (VLDISCOVERY, ST-LINK/V1) — see RESULTS-wdg-f100.md
#
# Boot 1 arms the real IWDG through the dissolved wdg-thin object and stops refreshing
# it; the hardware resets the chip; boot 2 reads IWDGRSTF=1. openocd holds the SWD
# session across the reset, so the whole thing is one capture.
#
# The firmware prints via semihosting, so openocd must stay attached — this is not a
# flash-and-walk-away run.
#
# Env:
#   OCD_HOST   run openocd over ssh on this host (e.g. pi@192.168.178.88); local if unset
set -euo pipefail
cd "$(dirname "$0")/.."

ELF=target/thumbv7m-none-eabi/release/gust_wdg_silicon
RCC_CSR=0x40021024        # STM32F1 reset/clock control — status
RMVF=0x01000000           # clear the reset flags
AIRCR=0xE000ED0C
SYSRESETREQ=0x05FA0004

echo "==> building for F100 (only memory.x and the target feature differ; the driver .o does not)"
cp memory.x /tmp/memory.x.bak
cp targets/generated/memory-stm32f100.x memory.x
trap 'cp /tmp/memory.x.bak memory.x' EXIT
rm -f "$ELF"                              # force a relink at 0x08000000
touch src/bin/gust_wdg_silicon.rs
cargo build --release --bin gust_wdg_silicon \
      --no-default-features --features target-f100 --target thumbv7m-none-eabi

OCD=(openocd -f interface/stlink-hla.cfg -c "transport select swd"
     -f target/stm32f1x.cfg -c "reset_config none separate")

# shellcheck source=./bench-claim.sh
. "$(dirname "$0")/bench-claim.sh"
# MUST match fourpi's registry exactly (`stlink-v1`). A name gale invents locks
# nothing that jess is also holding. with-device refuses unknown names (exit 2).
BENCH_DEV="${BENCH_DEV:-stlink-v1}"

run_ocd() {  # run_ocd <extra -c args...>
    if [ -n "${OCD_HOST:-}" ]; then
        # The claim must be taken ON THE HOST THAT OWNS THE PROBE, not here --
        #   WRONG: with-device ... -- ssh $OCD_HOST openocd    (locks this laptop)
        #   RIGHT: ssh $OCD_HOST with-device ... -- openocd    (locks the Pi)
        # Two agents on two different laptops can both ssh in and collide on the Pi's
        # probe, so a local lock would protect nothing. See gale#356.
        # shellcheck disable=SC2029
        claim_remote "$OCD_HOST" "$BENCH_DEV" "gale: wdg-f100" -- \
            "sudo timeout 25 $(printf '%q ' "${OCD[@]}" "$@")"
    else
        claim "$BENCH_DEV" "gale: wdg-f100" -- sudo timeout 25 "${OCD[@]}" "$@"
    fi
}

if [ -n "${OCD_HOST:-}" ]; then scp "$ELF" "$OCD_HOST:/tmp/wdg_f100.elf"; FW=/tmp/wdg_f100.elf
else FW="$ELF"; fi

echo "==> flashing"
run_ocd -c init -c "program $FW verify" -c exit

echo "==> arming (flags cleared first, so IWDGRSTF at boot 2 cannot be stale)"
run_ocd -c init -c "arm semihosting enable" -c halt \
        -c "mww $RCC_CSR $RMVF" -c "mww $AIRCR $SYSRESETREQ" -c halt -c resume \
    2>&1 | grep --line-buffered "gust-wdg-silicon" || true

echo "==> expected: RCC_CSR 0x14000000 -> 0x34000000, IWDGRSTF=1"

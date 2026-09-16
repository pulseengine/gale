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
#   OCD_HOST   run openocd over ssh on this host (e.g. wohl.local); local if unset
set -euo pipefail
# Resolve BEFORE cd. This was `cd "$(dirname "$0")/.."` followed by
# `. "$(dirname "$0")/bench-claim.sh"`: with a relative $0 the second path no longer
# exists. Worse, on macOS bash 3.2 a failed `.` under set -e with an EXIT trap exits 0,
# so the script stopped before flashing and REPORTED SUCCESS. (bash 5 exits 1.)
HERE="$(cd "$(dirname "$0")" && pwd)"
cd "$HERE/.."

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

# PIN THE PROBE TO THE V1 (0483:3744). stlink-hla.cfg lists every ST-LINK VID:PID, and
# wohl.local carries four: unpinned, this took the `stlink-v1` claim and then attached to
# the NUCLEO-G031K8's V2-1 — stopped only by the target config's IDCODE check.
OCD=(openocd -f interface/stlink-hla.cfg -c "hla_vid_pid 0x0483 0x3744" -c "transport select swd"
     -f target/stm32f1x.cfg -c "reset_config none separate")

[ -f "$HERE/bench-claim.sh" ] || { echo "run-wdg-f100: $HERE/bench-claim.sh missing" >&2; exit 4; }
# shellcheck source=./bench-claim.sh
. "$HERE/bench-claim.sh"
# MUST match the bench host's registry exactly (`stlink-v1`). A name gale invents locks
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
LOG="$(mktemp)"
rc=0
run_ocd -c init -c "arm semihosting enable" -c halt \
        -c "mww $RCC_CSR $RMVF" -c "mww $AIRCR $SYSRESETREQ" -c halt -c resume \
    >"$LOG" 2>&1 || rc=$?
grep "gust-wdg-silicon" "$LOG" || true

echo "==> expected: RCC_CSR 0x14000000 -> 0x34000000, IWDGRSTF=1"

# THE VERDICT IS THE FIRMWARE'S OWN LINE; this script used to print the expectation and
# exit 0 whatever the board said. openocd's own exit is not it either: it reports that the
# SESSION ended, not what the firmware printed.
if grep -q "^gust-wdg-silicon OK:" "$LOG"; then
    echo "==> PASS (openocd rc=$rc)"; exit 0
fi
echo "==> FAIL: no 'gust-wdg-silicon OK:' line (openocd/claim rc=$rc). Full log: $LOG" >&2
exit 1

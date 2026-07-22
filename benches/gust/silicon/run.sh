#!/usr/bin/env bash
# Build + flash + capture silicon_bench on a real board, measuring true DWT
# cycles for native (LLVM) vs dissolved (synth) gust_mix. One TCB, two MCUs.
#
#   ./run.sh g474re   # NUCLEO-G474RE   (Cortex-M4, thumbv7em, cortex-m4 .o)
#   ./run.sh f100     # STM32VLDISCOVERY (Cortex-M3, thumbv7m,  cortex-m3 .o)
#
# Needs probe-rs (brew install probe-rs-tools) and the board's onboard ST-LINK
# connected. probe-rs run flashes, resets, and streams semihosting to the console.
#
# Fusion comparison: set GUST_MIX_O=/path/to/flag-on.o to link a SYNTH_CMP_SELECT_FUSE=1
# variant instead of the committed flag-off object (see synth#428).
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
BENCH="$(dirname "$HERE")"   # benches/gust
BOARD="${1:?usage: run.sh g474re or f100}"

case "$BOARD" in
  g474re) TARGET=thumbv7em-none-eabi; CHIP=STM32G474RETx; MEM=targets/generated/memory-stm32g474.x; DEFAULT_O=wasm-kernel/gust_mix-cm4.o ;;
  f100)   TARGET=thumbv7m-none-eabi;  CHIP=STM32F100RBTx; MEM=targets/generated/memory-stm32f100.x;   DEFAULT_O=wasm-kernel/gust_mix-cm3.o ;;
  *) echo "unknown board '$BOARD' (want g474re|f100)"; exit 1 ;;
esac

cd "$BENCH"
# swap in the board memory map for the build, restore on exit
cp memory.x /tmp/gust-memory.x.bak
trap 'cp /tmp/gust-memory.x.bak "$BENCH/memory.x"' EXIT
cp "$MEM" memory.x

echo "== build silicon_bench for $BOARD ($TARGET, $CHIP) =="
echo "   dissolved object: ${GUST_MIX_O:-$DEFAULT_O}"
# build.rs picks gust_mix-cm4.o for thumbv7em / -cm3.o for thumbv7m automatically;
# GUST_MIX_O can override (e.g. a flag-on fusion variant) via an extra link-arg.
EXTRA=""
[ -n "${GUST_MIX_O:-}" ] && EXTRA="--config build.rustflags=[\"-Clink-arg=${GUST_MIX_O}\"]"
cargo build --release --bin silicon_bench --target "$TARGET" $EXTRA
ELF="target/$TARGET/release/silicon_bench"

echo "== flash + capture on $BOARD via probe-rs (Ctrl-C after 'silicon_bench: done') =="
probe-rs run --chip "$CHIP" --catch-hardfault "$ELF"

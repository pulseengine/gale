#!/usr/bin/env bash
# Build the 4-driver breadth node (REQ-DRV-BREADTH-001): GPIO+timer+SPI+UART, each
# a verified-wasm gust:hal component, wac/meld-fused into ONE dissolved relocatable
# object exporting all four protocols — 0 SRAM, no func_N collision (the CM fuse
# gives a single function-index space, so independently-built drivers co-link).
#
# Pipeline: cargo(wasm32) ×4 -> wasm-tools component new ×4 -> meld fuse
#   --memory shared -> loom optimize --passes inline -> synth --target cortex-m3
#   --all-exports --relocatable -> objcopy --redefine-sym (CM-namespaced exports
#   `gust:hal/<iface>@0.1.0#<fn>` -> C names) -> breadth/breadth-cm3.o.
# Tools on PATH (or via env): wasm-tools, meld, loom, synth, arm-none-eabi-objcopy.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
MELD="${MELD:-meld}"; LOOM="${LOOM:-loom}"; SYNTH="${SYNTH:-synth}"
OBJCOPY="${OBJCOPY:-arm-none-eabi-objcopy}"; WT="${WASM_TOOLS:-wasm-tools}"
T="$(mktemp -d)"
comps=()
for d in gpio timer spi uart; do
  ( cd "$HERE/$d-comp" && cargo build --release --target wasm32-unknown-unknown >/dev/null 2>&1 )
  core="$(find "$HERE/$d-comp/target/wasm32-unknown-unknown/release" -name "gust_${d}_comp.wasm" | head -1)"
  "$WT" component new "$core" -o "$T/$d.comp.wasm"
  comps+=("$T/$d.comp.wasm")
done
"$MELD" fuse "${comps[@]}" --memory shared -o "$T/breadth.fused.wasm" >/dev/null 2>&1
"$LOOM" optimize "$T/breadth.fused.wasm" --passes inline --attestation false -o "$T/breadth.loom.wasm" >/dev/null 2>&1
"$SYNTH" compile "$T/breadth.loom.wasm" --target cortex-m3 --all-exports --relocatable -o "$T/breadth.raw.o" >/dev/null 2>&1
# CM-namespaced export -> C symbol (so the gust_breadth bin can call extern "C").
R=()
for p in \
  "gpio@0.1.0#configure=gpio_configure" "gpio@0.1.0#set=gpio_set" "gpio@0.1.0#clear=gpio_clear" "gpio@0.1.0#read=gpio_read" "gpio@0.1.0#toggle=gpio_toggle" \
  "timer@0.1.0#init=timer_init" "timer@0.1.0#now=timer_now" "timer@0.1.0#deadline=timer_deadline" "timer@0.1.0#elapsed=timer_elapsed" "timer@0.1.0#ack=timer_ack" \
  "spi@0.1.0#configure=spi_configure" "spi@0.1.0#xfer-byte=spi_xfer_byte" "spi@0.1.0#begin=spi_begin" "spi@0.1.0#step=spi_step" "spi@0.1.0#is-complete=spi_is_complete" "spi@0.1.0#abort=spi_abort" \
  "uart@0.1.0#init=uart_init" "uart@0.1.0#tx-byte=uart_tx_byte" "uart@0.1.0#rx=uart_rx" "uart@0.1.0#rx-fired=uart_rx_fired" ; do
  R+=(--redefine-sym "gust:hal/$p")
done
"$OBJCOPY" "${R[@]}" "$T/breadth.raw.o" "$HERE/breadth/breadth-cm3.o"
echo "breadth-cm3.o: $(arm-none-eabi-size "$HERE/breadth/breadth-cm3.o" | awk 'NR==2{print "text="$1" data="$2" bss="$3}')"

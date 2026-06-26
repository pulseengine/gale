# gust_uart on real STM32F100 silicon (STM32VLDISCOVERY)

The dissolved, Kani-verified thin-seam UART driver driven on the real F100. The
demonstrator (`src/bin/gust_uart.rs`) does the F1 board bring-up (RCC clocks +
GPIO PA9 alternate-function — SoC config, the TCB side) then calls the wasm
driver primitives; the driver TXes over USART1.

## Status (honest)
- **Renode (f100_silicon.repl: flash@0x08000000, RCC/GPIO/USART1):** emits
  `gust-uart-thin` over the modelled STM32 USART — verified.
- **Real STM32F100 (STM32VLDISCOVERY):** the F100-linked ELF **flashed + verified
  + executed to completion** (`Programming Finished` / `Verified OK`, ran to the
  post-TX `debug::exit` at pc 0x08000474) — the dissolved driver runs on the real
  chip. Byte-level on-wire capture pending an **external USB-serial on PA9** (the
  VLDISCOVERY ST-LINK/V1 has no VCP, and its hla adapter can't do live peripheral
  reads on macOS — see reference_vldiscovery_stlink_v1_flashing).

## Flash (ST-LINK/V1 → openocd; sudo, or a sudo-started openocd server then drive
## rootlessly via telnet :4444 / gdb :3333)
```sh
sudo openocd -c "adapter serial <V1-serial>" -f board/stm32vldiscovery.cfg \
  -c "init" -c "halt" -c "program <gust_uart-f100.elf> verify" -c "reset run"
```
Build the ELF with the F100 8 KB map: `cp silicon/memory-f100.x memory.x;
cargo build --release --bin gust_uart --target thumbv7m-none-eabi`.

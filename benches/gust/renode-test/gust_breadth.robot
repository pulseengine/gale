*** Settings ***
Documentation     gust:hal 4-driver BREADTH node — GPIO+timer+SPI+UART, each a
...               verified-wasm gust:hal component, wac/meld-fused into ONE dissolved
...               relocatable object (0 SRAM, no func_N collision, TCB = read32/
...               write32/poll). gust_breadth drives all four from the single fused
...               .o on a real STM32 model and emits breadth-*-ok on USART1 — the
...               uart line is TX'd by the dissolved uart driver itself. ELF +
...               platform injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Four dissolved drivers compose in one fused object and run on a real STM32 model
    Execute Command           mach create "gust-breadth"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     breadth-gate begin    timeout=30
    Wait For Line On Uart     breadth-gpio-ok       timeout=30
    Wait For Line On Uart     breadth-timer-ok      timeout=30
    Wait For Line On Uart     breadth-spi-ok        timeout=30
    Wait For Line On Uart     breadth-uart-ok       timeout=30
    Wait For Line On Uart     breadth-gate done     timeout=30

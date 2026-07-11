*** Settings ***
Documentation     gust:hal thin-seam GPIO driver — the dissolved driver (the whole
...               STM32F1 GPIO protocol in verified wasm, Kani 4/4, 0 new TCB atoms)
...               exercised on a REAL STM32 GPIO model. gust_gpio configures PC8 as
...               output push-pull via gpio_configure, drives it with gpio_set /
...               gpio_clear, and reads back the resulting CRH/ODR register bits —
...               emitting gpio-*-ok on USART1 iff the driver's register effects are
...               correct. This asserts the driver's behaviour on real peripheral
...               registers (Renode faithfully stores them), not just no-fault. ELF +
...               platform injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved thin-seam GPIO driver drives real STM32 GPIO registers
    Execute Command           mach create "gust-gpio"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     gpio-gate begin    timeout=30
    Wait For Line On Uart     gpio-cfg-ok        timeout=30
    Wait For Line On Uart     gpio-set-ok        timeout=30
    Wait For Line On Uart     gpio-clear-ok      timeout=30
    Wait For Line On Uart     gpio-gate done     timeout=30

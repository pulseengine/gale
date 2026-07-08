*** Settings ***
Documentation     gust:hal thin-seam hardware-timer driver — the dissolved driver
...               (STM32 timer config + Kani-proven wrap-safe deadline math, table-free,
...               0 new TCB atoms) exercised on a real STM32 model. gust_timer configures
...               TIM2 (PSC/ARR/CR1) and checks the register writes + the wrap-safe
...               deadline predicate, emitting timer-*-ok on USART1 iff correct. ELF +
...               platform injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved thin-seam timer driver configures TIM2 and computes wrap-safe deadlines
    Execute Command           mach create "gust-timer"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     timer-gate begin      timeout=30
    Wait For Line On Uart     timer-init-ok         timeout=30
    Wait For Line On Uart     timer-deadline-ok     timeout=30
    Wait For Line On Uart     timer-wrap-ok         timeout=30
    Wait For Line On Uart     timer-gate done       timeout=30

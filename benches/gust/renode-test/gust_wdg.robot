*** Settings ***
Documentation     gust:hal thin-seam IWDG driver — the dissolved driver (STM32F1
...               independent watchdog key-sequence lifecycle: 0x5555 unlock, PR/RLR
...               config, 0xCCCC start, 0xAAAA refresh; Kani-proven cannot-un-start —
...               no software disable transition, table-free, 0 new TCB atoms)
...               exercised on a real STM32 model. gust_wdg drives the full lifecycle
...               on a RAM-mapped IWDG window, demonstrates that every attempted
...               unlock/reconfigure/re-lock/restart is rejected once Running, and
...               that a refresh keeps it Running — emitting wdg-*-ok on USART1 iff
...               correct. ELF + platform injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved thin-seam IWDG driver runs the key-sequence lifecycle and cannot be un-started
    Execute Command           mach create "gust-wdg"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     wdg-gate begin              timeout=30
    Wait For Line On Uart     wdg-protect-ok               timeout=30
    Wait For Line On Uart     wdg-unlock-ok                timeout=30
    Wait For Line On Uart     wdg-config-ok                timeout=30
    Wait For Line On Uart     wdg-start-ok                 timeout=30
    Wait For Line On Uart     wdg-cannot-un-start-ok       timeout=30
    Wait For Line On Uart     wdg-refresh-ok               timeout=30
    Wait For Line On Uart     wdg-gate done                timeout=30

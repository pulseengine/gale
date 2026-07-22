*** Settings ***
Documentation     gust:hal thin-seam ADC driver — the dissolved driver (STM32F1
...               single-conversion path: SMPR sample-time + SQR regular-sequence
...               config and the enable→start→EOC→read cycle; Kani-proven
...               read-after-EOC exactly-once / single-shot — the data register is
...               read only from Complete and a completed read lands Ready, never
...               Converting, so the ADC never free-runs; table-free, 0 new TCB atoms)
...               exercised on a real STM32 model. gust_adc drives the full cycle on a
...               RAM-mapped ADC window, demonstrates that reading before EOC is
...               rejected with no stale sample, that the 12-bit sample is consumed
...               exactly once, and that re-converting demands an explicit start —
...               emitting adc-*-ok on USART1 iff correct. ELF + platform injected by
...               renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved thin-seam ADC driver runs the single-conversion cycle and reads after EOC exactly once
    Execute Command           mach create "gust-adc"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     adc-gate begin              timeout=30
    Wait For Line On Uart     adc-chanbound-ok            timeout=30
    Wait For Line On Uart     adc-enable-ok               timeout=30
    Wait For Line On Uart     adc-config-ok               timeout=30
    Wait For Line On Uart     adc-start-ok                timeout=30
    Wait For Line On Uart     adc-no-stale-ok             timeout=30
    Wait For Line On Uart     adc-poll-ok                 timeout=30
    Wait For Line On Uart     adc-read-ok                 timeout=30
    Wait For Line On Uart     adc-single-shot-ok          timeout=30
    Wait For Line On Uart     adc-disable-ok              timeout=30
    Wait For Line On Uart     adc-gate done               timeout=30

*** Settings ***
Documentation     gust:hal thin-seam software-triggered DAC driver — the dissolved
...               driver (STM32F1 DAC: CR channel/trigger config + the
...               enable->load->trigger->output cycle; Kani-proven glitch-free,
...               trigger-gated output — writing DHR does NOT move the pin until a
...               trigger fires; range-clamped to 12 bits; table-free, 0 new TCB atoms)
...               exercised on a real STM32 model. gust_dac drives the full cycle on a
...               RAM-mapped DAC window, demonstrates that a staged code sits in DHR
...               with DOR unmoved until a trigger — and that restaging while a code is
...               on the pin holds the old value un-glitched until the next trigger —
...               emitting dac-*-ok on USART1 iff correct. ELF + platform injected by
...               renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved thin-seam DAC driver runs the software-triggered cycle glitch-free
    Execute Command           mach create "gust-dac"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     dac-gate begin              timeout=30
    Wait For Line On Uart     dac-phase-gate-ok           timeout=30
    Wait For Line On Uart     dac-enable-ok               timeout=30
    Wait For Line On Uart     dac-load-ok                 timeout=30
    Wait For Line On Uart     dac-trigger-ok              timeout=30
    Wait For Line On Uart     dac-glitch-free-ok          timeout=30
    Wait For Line On Uart     dac-publish2-ok             timeout=30
    Wait For Line On Uart     dac-clamp-ok                timeout=30
    Wait For Line On Uart     dac-disable-ok              timeout=30
    Wait For Line On Uart     dac-gate done               timeout=30

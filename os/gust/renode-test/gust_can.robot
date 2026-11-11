*** Settings ***
Documentation     gust:hal thin-seam CAN (bxCAN) driver — the dissolved driver (STM32F1
...               bxCAN master path: the INRQ/INAK init handshake, the BTR bit-timing
...               config, and TX-mailbox / RX-FIFO gating; Kani-proven
...               config-only-in-init — bit-timing is writable ONLY inside Init,
...               table-free, 0 new TCB atoms) exercised on a real STM32 model.
...               gust_can drives the full lifecycle on a RAM-mapped bxCAN window,
...               demonstrates that a config before Init and a config once Normal are
...               both rejected without touching BTR, and that TX/RX are gated on the
...               live mailbox-empty / message-pending flags — emitting can-*-ok on
...               USART1 iff correct. ELF + platform injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved thin-seam CAN driver runs the bxCAN lifecycle with config-only-in-init
    Execute Command           mach create "gust-can"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     can-gate begin              timeout=30
    Wait For Line On Uart     can-protect-ok              timeout=30
    Wait For Line On Uart     can-init-ok                 timeout=30
    Wait For Line On Uart     can-config-ok               timeout=30
    Wait For Line On Uart     can-normal-ok               timeout=30
    Wait For Line On Uart     can-tx-ok                   timeout=30
    Wait For Line On Uart     can-rx-ok                   timeout=30
    Wait For Line On Uart     can-gate done               timeout=30

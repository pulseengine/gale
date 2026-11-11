*** Settings ***
Documentation     gust:hal thin-seam SPI driver — the dissolved driver (STM32 SPI
...               CR1 mode/baud config + full-duplex byte shift + a Kani-proven
...               transfer FSM (SQE→CQE, exclusive-bus + no-lost-byte), table-free,
...               0 new TCB atoms) exercised on a real STM32 model. gust_spi writes
...               CR1 on a RAM-mapped SPI1 window, shifts a byte, and runs the FSM,
...               emitting spi-*-ok on USART1 iff correct. ELF + platform injected by
...               renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved thin-seam SPI driver configures SPI1, shifts a byte, and runs the transfer FSM
    Execute Command           mach create "gust-spi"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     spi-gate begin        timeout=30
    Wait For Line On Uart     spi-config-ok         timeout=30
    Wait For Line On Uart     spi-xfer-ok           timeout=30
    Wait For Line On Uart     spi-fsm-ok            timeout=30
    Wait For Line On Uart     spi-gate done         timeout=30

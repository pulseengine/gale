*** Settings ***
Documentation     gust:hal thin-seam UART driver — the dissolved driver (the whole
...               STM32 USART protocol in verified wasm, Kani-proven RX decision)
...               driving a REAL STM32 USART model. The app TXes "gust-uart-thin" via
...               the driver's uart_tx_byte primitive over MMIO; this asserts the
...               emitted content on the wire (not just no-fault). A real USART is
...               capturable headless (unlike the SemihostingUart). ELF + platform
...               injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved thin-seam UART driver TXes over a real STM32 USART
    Execute Command           mach create "gust-uart"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     gust-uart-thin    timeout=30

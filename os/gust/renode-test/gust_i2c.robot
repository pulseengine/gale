*** Settings ***
Documentation     gust:hal thin-seam I2C driver — the dissolved driver (STM32F1 I2C
...               master path: CR2 FREQ + CCR divisor + TRISE timing config, and the
...               START→address→data→STOP transaction FSM; Kani-proven ACK-all-but-last —
...               the master ACKs bytes 1..N−1 and NACKs the last so CR1.STOP is issued
...               EXACTLY on the final byte; table-free, 0 new TCB atoms) exercised on a
...               real STM32 model. gust_i2c drives a 3-byte master read on a RAM-mapped
...               I2C1 window and asserts the CR2/CCR/TRISE/CR1 register effects, the
...               read START issuing CR1 = PE|START|ACK, that CR1.STOP is written only on
...               the last byte, and that dup-START/off-phase reject — emitting i2c-*-ok
...               on USART1 iff correct. ELF + platform injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved thin-seam I2C driver runs the master transaction and ACKs all but the last byte
    Execute Command           mach create "gust-i2c"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     i2c-gate begin              timeout=30
    Wait For Line On Uart     i2c-config-ok               timeout=30
    Wait For Line On Uart     i2c-start-ok                timeout=30
    Wait For Line On Uart     i2c-ack-rule-ok             timeout=30
    Wait For Line On Uart     i2c-complete-ok             timeout=30
    Wait For Line On Uart     i2c-fault-ok                timeout=30
    Wait For Line On Uart     i2c-stop-ok                 timeout=30
    Wait For Line On Uart     i2c-gate done               timeout=30

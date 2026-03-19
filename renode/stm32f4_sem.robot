*** Variables ***
# STM32F4 Discovery uses USART2 for Zephyr console
${UART}                       sysbus.usart2

*** Keywords ***
Create Machine
    Execute Command           mach create
    Execute Command           machine LoadPlatformDescription @platforms/boards/stm32f4_discovery.repl
    Execute Command           sysbus LoadELF @%{ELF}

*** Test Cases ***
Should Pass Gale Semaphore Tests On Cortex-M4F
    Create Machine
    Create Terminal Tester    ${UART}  defaultPauseEmulation=true
    Wait For Line On Uart     PROJECT EXECUTION SUCCESSFUL  timeout=120

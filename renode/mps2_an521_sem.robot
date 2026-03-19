*** Variables ***
# MPS2/AN521 Cortex-M33 uses UART0 for Zephyr console
${UART}                       sysbus.uart0

*** Keywords ***
Create Machine
    Execute Command           mach create
    Execute Command           machine LoadPlatformDescription @platforms/boards/mps2-an521.repl
    Execute Command           sysbus LoadELF @%{ELF}

*** Test Cases ***
Should Pass Gale Semaphore Tests On Cortex-M33
    Create Machine
    Create Terminal Tester    ${UART}  defaultPauseEmulation=true
    Wait For Line On Uart     PROJECT EXECUTION SUCCESSFUL  timeout=120

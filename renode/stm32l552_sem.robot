*** Variables ***
# Nucleo L552ZE-Q uses LPUART1 for Zephyr console
${UART}                       sysbus.lpuart1

*** Keywords ***
Create Machine
    Execute Command           mach create
    Execute Command           machine LoadPlatformDescription @platforms/cpus/stm32l552.repl
    Execute Command           sysbus LoadELF %{ELF}

*** Test Cases ***
Should Pass Gale Semaphore Tests On Cortex-M33
    Create Machine
    Create Terminal Tester    ${UART}  defaultPauseEmulation=true
    Wait For Line On Uart     PROJECT EXECUTION SUCCESSFUL  timeout=120

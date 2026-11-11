*** Settings ***
Documentation     gust on STM32F100 (stm32vldiscovery, Cortex-M3) in Renode.
...               Boots the mini-OS (kiln-async scheduler + TCB superloop + fixed-point
...               failsafe task) and reads the deterministic executed-instruction count —
...               the fuel->cycles WCET calibration the schedulability proof needs.
...               Run: renode-test gust_f100.robot  (ELF via %{ELF})
Resource          ${RENODEKEYWORDS}

*** Variables ***
${WAIT_TIMEOUT}   120

*** Keywords ***
Create Machine
    Execute Command           mach create "gust-f100"
    Execute Command           machine LoadPlatformDescription @platforms/boards/stm32vldiscovery.repl
    Execute Command           sysbus LoadELF @%{ELF}
    # semihosting output -> Renode log (the image's hprintln heartbeat)
    Execute Command           cpu EnableProfiler @/dev/null

*** Test Cases ***
gust Boots And Schedules On STM32F100
    Create Machine
    Create Terminal Tester    sysbus.cpu  defaultPauseEmulation=true
    Start Emulation
    # boot banner + run-to-completion (semihosting heartbeat)
    Wait For Line On Uart     gust boot                     timeout=30
    Wait For Line On Uart     poll rounds, scheduler stable      timeout=${WAIT_TIMEOUT}
    # fuel->cycles calibration: deterministic instruction count over the run
    ${instrs}=                Execute Command   sysbus.cpu ExecutedInstructions
    Log To Console            gust F100 executed-instructions: ${instrs}
    # (WCET calibration = ExecutedInstructions / poll-rounds; M3 has no cache -> ~cycles)

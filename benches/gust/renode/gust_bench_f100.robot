*** Settings ***
Documentation     gust scheduler benchmark on STM32F100 (stm32vldiscovery, Cortex-M3) in Renode.
...               Runs the bench ELF and reads ExecutedInstructions = TRUE cycle-class cost
...               (M3 has no cache, so instruction count ~= cycles). Run:
...               ELF=target/thumbv7m-none-eabi/release/bench renode-test gust_bench_f100.robot
Resource          ${RENODEKEYWORDS}
*** Test Cases ***
gust Scheduler Bench On STM32F100
    Execute Command           mach create "gust-bench"
    Execute Command           machine LoadPlatformDescription @platforms/boards/stm32vldiscovery.repl
    Execute Command           sysbus LoadELF @%{ELF}
    Create Terminal Tester    sysbus.cpu  defaultPauseEmulation=true
    Start Emulation
    Wait For Line On Uart     gust-bench: done             timeout=120
    ${instrs}=                Execute Command   sysbus.cpu ExecutedInstructions
    Log To Console            gust-bench F100 total ExecutedInstructions: ${instrs}

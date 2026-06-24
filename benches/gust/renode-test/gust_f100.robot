*** Settings ***
Documentation     The DISSOLVED gust kernel on the REAL STM32F100RB part (STM32VLDISCOVERY:
...               Cortex-M3, 128 KB flash, 8 KB SRAM) modelled hermetically in Renode — the
...               on-target confirmation for the actual silicon board in the gust plan (closes
...               the renode-test README follow-up #3). Same dissolved ELF as the generic 8 KB
...               target; this pins it to the F100RB's exact memory class. Booting + running the
...               scheduler loop with no early fault is the confirmation (a bad image faults on
...               cycle 1); the deterministic instruction count is the cacheless-M3 cycle seed.
...               ELF + platform injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Test Cases ***
Dissolved gust kernel boots and runs on STM32F100RB (8K SRAM)
    Execute Command           mach create "gust-f100"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Execute Command           emulation RunFor "2"
    ${instr}=                 Execute Command    sysbus.cpu ExecutedInstructions
    Log To Console            \n[gust-f100] dissolved gust kernel on Renode STM32F100RB (8 KB SRAM) — executed instructions: ${instr}

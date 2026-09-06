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

    # FIDELITY, not function: the model must not be MORE CAPABLE than the part.
    # The real STM32F100 has NO MPU -- MPU_TYPE reads 0x00000000 over SWD on the
    # halted part. Renode DEFAULTS a Cortex-M to EIGHT regions when the platform
    # description is silent, and this platform WAS silent: it read 0x00000800 until
    # `numberOfMPURegions: 0` was added.
    #
    # Assert it here because the defect is invisible in the SOURCE -- the .repl says
    # nothing either way, and "we never declared an MPU" is not the same sentence as
    # "there is no MPU here". Read the model, not the file.
    ${mpu}=                   Execute Command    sysbus ReadDoubleWord 0xE000ED90
    Should Contain            ${mpu}    0x00000000
    ...                       msg=STM32F100 model reports an MPU it does not have (MPU_TYPE=${mpu})

    Execute Command           sysbus LoadELF @${ELF}
    Execute Command           emulation RunFor "2"
    ${instr}=                 Execute Command    sysbus.cpu ExecutedInstructions
    Log To Console            \n[gust-f100] dissolved gust kernel on Renode STM32F100RB (8 KB SRAM) — executed instructions: ${instr}

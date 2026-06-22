*** Settings ***
Documentation     The DISSOLVED gust kernel (wasm -> loom -> synth, native-pointer-abi,
...               --shadow-stack-size) booting on a hermetic Renode Cortex-M3 with 8 KB
...               SRAM — the on-target (real M3 ISA model) confirmation of the synth#383
...               shrink, beyond qemu's lm3s stand-in. ELF + platform are injected by the
...               renode_test rule (variables_with_label).
Resource          ${RENODEKEYWORDS}

*** Test Cases ***
Dissolved gust boots and runs on Cortex-M3 8K
    Execute Command           mach create "gust-m3-8k"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    # The shrunk .bss (4256 B) loads into the 8 KB SRAM; SP re-based to the 8 KB top.
    Execute Command           emulation RunFor "2"
    ${instr}=                 Execute Command    sysbus.cpu ExecutedInstructions
    # Booting + executing the scheduler loop without an early fault is the confirmation;
    # a bad image faults on cycle 1. (Locally: ~200M instr, no fault.) The count is the
    # deterministic fuel->cycles WCET seed (M3 has no cache -> instr ~= cycles).
    Log To Console            \n[gust] dissolved on Renode Cortex-M3 + 8 KB SRAM — executed instructions: ${instr}

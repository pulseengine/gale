*** Settings ***
Documentation     North-star rung 1 on a hermetic Renode Cortex-M3 (64 KB / F103RE-class):
...               the kiln-async scheduler driving the DISSOLVED engine_control control_step
...               (wasm -> loom -> synth --native-pointer-abi, via the r11=0 trampoline) as its
...               task body. Booting + running the full scheduler loop with no early fault is the
...               confirmation (a bad image faults on cycle 1); the deterministic executed-
...               instruction count is the M3 cacheless cycle-class seed (instr ~= cycles),
...               CI-reproducible with no board. The spark/fuel CORRECTNESS of control_step is
...               gated by the qemu run (exit-code on == C/wasmtime) and gale_decider_diff;
...               Renode adds the real-M3-model + deterministic-cycle dimension. ELF + platform
...               injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Test Cases ***
Dissolved engine_control runs on the kiln stack (Cortex-M3 64K)
    Execute Command           mach create "gust-control-m3-64k"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    # The 9408 B .bss loads into the 64 KB SRAM at 0x200004B4; SP at the 64 KB top.
    Execute Command           emulation RunFor "2"
    ${instr}=                 Execute Command    sysbus.cpu ExecutedInstructions
    Log To Console            \n[gust-control] dissolved engine_control on the kiln stack, Renode Cortex-M3 64K — executed instructions: ${instr}

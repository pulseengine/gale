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
    # Content, not a budget. Renode 1.17 implements semihosting SYS_WRITE and
    # SYS_EXIT, so the kernel's heartbeat is capturable and its debug::exit stops
    # the machine: on 1.17 the kernel exits after 0xCAE27 (831,015) instructions.
    # Under 1.16 neither worked: Renode ignored the exit and the core kept
    # executing until the 2 s budget ran out. The 0x0BEBC200 (200,000,000) this
    # test used to log was that budget (2 s x 100 MIPS), not the kernel, and a
    # floor under it could not tell a finished scheduler from a busy one. Now the gate asserts the boot banner,
    # the 5000-round completion line, and a SUCCESS exit (EXIT_FAILURE reports a
    # different ADP reason and fails the log wait).
    Create Log Tester         10
    Create Terminal Tester    sysbus.cpu.semihosting.uartSemihosting    defaultPauseEmulation=true
    Wait For Line On Uart     gust boot: kiln-async scheduler on Cortex-M3    timeout=5
    Wait For Line On Uart     gust: 5000 poll rounds, scheduler stable    timeout=5
    Wait For Log Entry        Program exited with reason ADP_Stopped_ApplicationExit    timeout=5
    ${instr}=                 Execute Command    sysbus.cpu ExecutedInstructions
    # The count is the deterministic fuel->cycles WCET seed (M3 has no cache -> instr ~= cycles).
    Log To Console            \n[gust] dissolved on Renode Cortex-M3 + 8 KB SRAM — executed instructions: ${instr}

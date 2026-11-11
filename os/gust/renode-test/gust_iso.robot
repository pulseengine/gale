*** Settings ***
Documentation     The dissolved ISOLATION CORE — hm-thin + mpu-thin + switch-thin,
...               wac-composed, meld-fused (--memory shared), loom-optimised and
...               synth-lowered to ONE Cortex-M3 object with four native atoms —
...               EXECUTED. Every prior claim about this object was static: seam
...               sets, sizes, BIN-VERIFY rules, witness MC/DC, a WASM->object
...               disposition. None of them runs it, and two defects reached main
...               through that gap. This gate closes it: the ARINC-653 major-frame
...               validator, non-maskable boundary preemption, the seam ORDER
...               (ctx-save -> region-swap -> ctx-resume) observed rather than
...               assumed, MPU region validation, cross-component non-interference,
...               and the health-monitor predicates — emitting iso-*-ok on USART1
...               iff correct. ELF + platform injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved isolation core runs its FSM, seams and region programmer correctly
    Execute Command           mach create "gust-iso"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     iso-gate begin           timeout=30
    Wait For Line On Uart     iso-frame-ok             timeout=30
    Wait For Line On Uart     iso-frame-reject-ok      timeout=30
    Wait For Line On Uart     iso-window-ok            timeout=30
    Wait For Line On Uart     iso-preempt-ok           timeout=30
    Wait For Line On Uart     iso-seam-order-ok        timeout=30
    Wait For Line On Uart     iso-region-ok            timeout=30
    Wait For Line On Uart     iso-mpu-seam-ok          timeout=30
    Wait For Line On Uart     iso-nointerfere-ok       timeout=30
    Wait For Line On Uart     iso-hm-ok                timeout=30
    Wait For Line On Uart     iso-gate done            timeout=30

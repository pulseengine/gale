*** Settings ***
Documentation     v0.6.0 MULTI-CORE placement demo for the outer partition switch
...               (REQ-OS-SWITCH-001 / VER-OS-SWITCH-001): TWO Cortex-M3 cores, each
...               with its own NVIC + private flash/SRAM/UART (per-core bus
...               registration). Core 0 drives the VERIFIED Switcher (gale
...               src/partition_switch.rs) across a 3-partition major frame
...               (flight P0 / mission P1 / payload P2 sharing the core), the MPU
...               programmed ONLY via the VERIFIED switch_to_partition; core 1 runs
...               the estimator partition concurrently under its own
...               verified-programmed map. Content-gated per line on each core's
...               OWN UART. HONEST SCOPE (spike mpu_spike_renode.rs): Renode's M3
...               holds MPU registers as readable STATE but does NOT enforce them —
...               this gate asserts window sequence + save->swap->resume order +
...               map-live-at-resume via RBAR readback + multi-core placement;
...               map-ENFORCEMENT evidence (real MemManage denials, CFSR=0x82) is
...               the merged qemu demonstrator (gust_switch_probe.rs). ELFs +
...               platform injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Test Cases ***
Verified partition switch places 3+1 partitions across two M3 cores
    Execute Command           mach create "gust-switch-2core"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF0} cpu=cpu0
    Execute Command           sysbus LoadELF @${ELF1} cpu=cpu1
    # NOTE: no defaultPauseEmulation here — with the 2-cpu platform it
    # deadlocks the tester (reproduced on Renode 1.16.1: the wait never
    # matches and never times out); plain testers + explicit start work.
    ${t0}=                    Create Terminal Tester    sysbus.uart0
    ${t1}=                    Create Terminal Tester    sysbus.uart1
    Execute Command           start

    # ---- Core 0: the 3-partition major frame under the VERIFIED Switcher ----
    Wait For Line On Uart     gust-switch-2core core0 begin dregion 8    timeout=30    testerId=${t0}
    # Window sequence P0 -> P1 -> P2 -> P0, each window: own-scratch write lands
    # + the Verus-proven covers_addr query denies the neighbour's scratch.
    Wait For Line On Uart     core0 win0 P0 own-scratch-ok covers-denied-cross    timeout=30    testerId=${t0}
    # Every switch: seam order save->swap->resume observed AND the incoming
    # partition's scratch region ALREADY live in real MPU register state at
    # resume (RNR:=3 RBAR readback == that partition's scratch base).
    Wait For Line On Uart     core0 switch0 -> P1 seam-order-ok map-live rbar 0x20008800    timeout=30    testerId=${t0}
    Wait For Line On Uart     core0 win1 P1 own-scratch-ok covers-denied-cross    timeout=30    testerId=${t0}
    Wait For Line On Uart     core0 switch1 -> P2 seam-order-ok map-live rbar 0x20009000    timeout=30    testerId=${t0}
    Wait For Line On Uart     core0 win2 P2 own-scratch-ok covers-denied-cross    timeout=30    testerId=${t0}
    Wait For Line On Uart     core0 switch2 -> P0 seam-order-ok map-live rbar 0x20008000    timeout=30    testerId=${t0}
    Wait For Line On Uart     core0 win3 P0 own-scratch-ok covers-denied-cross    timeout=30    testerId=${t0}
    Wait For Line On Uart     core0 switch3 -> P0 seam-order-ok map-live rbar 0x20008000    timeout=30    testerId=${t0}
    Wait For Line On Uart     gust-switch-2core core0 OK: frame wrapped P0->P1->P2->P0, 4 verified switches save->swap->resume, map-live-at-resume via RBAR readback 4/4, covers-query denies cross-partition (enforcement evidence: qemu gust_switch_probe)    timeout=30    testerId=${t0}

    # ---- Core 1: the estimator partition, concurrently, own verified map ----
    Wait For Line On Uart     gust-switch-2core core1 estimator begin dregion 8    timeout=30    testerId=${t1}
    Wait For Line On Uart     core1 estimator map-live rbar 0x20008000 mpu-ctrl-enabled    timeout=30    testerId=${t1}
    # Deterministic fixed-point convergence (values computed by the image and
    # checked in-image for monotone error decrease + final bound).
    Wait For Line On Uart     core1 estimator-hb 1 est 0x0003da0d    timeout=30    testerId=${t1}
    Wait For Line On Uart     core1 estimator-hb 8 est 0x0003e7f9    timeout=30    testerId=${t1}
    Wait For Line On Uart     gust-switch-2core core1 OK: estimator partition on its own core, map programmed via verified switch_to_partition (RBAR readback), 8 heartbeats, converged    timeout=30    testerId=${t1}

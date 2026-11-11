*** Settings ***
Documentation     gust:hal DMA-as-own<buffer> ownership driver — the dissolved driver
...               (gale#124, Kani 6/6: a DMA transfer modeled as an own<buffer>
...               ownership round-trip, wasm->engine on start and back on completion,
...               with the cache/barrier op emitted BY CONSTRUCTION at every handoff;
...               importing only the gust:hal dma seam — descriptor program / coherency
...               barrier / completion-IRQ poll, the 3 irreducible trusted atoms)
...               exercised on a real STM32 model. gust_dma drives the full ownership
...               FSM on a RAM-mapped DMA descriptor + coherency window: start hands
...               Wasm->Dma (clean+DSB, descriptor programmed), a double-arm is rejected
...               (exclusive ownership), a poll with no IRQ yields, completion hands
...               Dma->Wasm (invalidate+DMB), an unpaired completion is rejected, and
...               abort returns the buffer to Wasm from any state (never ownerless) —
...               emitting dma-*-ok on USART1 iff correct. ELF + platform injected by
...               renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved dma-own driver runs the ownership round-trip with barriers by construction
    Execute Command           mach create "gust-dma"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     dma-gate begin              timeout=30
    Wait For Line On Uart     dma-start-ok                timeout=30
    Wait For Line On Uart     dma-exclusive-ok            timeout=30
    Wait For Line On Uart     dma-yield-ok                timeout=30
    Wait For Line On Uart     dma-complete-ok             timeout=30
    Wait For Line On Uart     dma-unpaired-ok             timeout=30
    Wait For Line On Uart     dma-abort-ok                timeout=30
    Wait For Line On Uart     dma-gate done               timeout=30

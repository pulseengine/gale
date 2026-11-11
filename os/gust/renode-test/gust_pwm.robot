*** Settings ***
Documentation     gust:hal thin-seam advanced-timer PWM driver — the dissolved driver
...               (STM32 PWM output path: PSC/ARR period, CCMR1 PWM-mode-1 + preload,
...               CCER/BDTR.MOE output enable, CCR1 duty; Kani-proven duty clamp
...               (CCR ≤ ARR always) + total/latching failsafe (MOE cleared from any
...               state, un-clearable by a stray start), table-free, 0 new TCB atoms)
...               exercised on a real STM32 model. gust_pwm drives the full PWM
...               lifecycle on a RAM-mapped timer window, demonstrates that a commanded
...               duty above the period is clamped so CCR1 never exceeds ARR, and that a
...               tripped failsafe (MOE off) cannot be undone by start/set_duty — only a
...               reconfigure re-arms — emitting pwm-*-ok on USART1 iff correct. ELF +
...               platform injected by renode_test.
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1

*** Test Cases ***
Dissolved thin-seam PWM driver clamps duty to period and latches its failsafe
    Execute Command           mach create "gust-pwm"
    Execute Command           machine LoadPlatformDescription @${REPL}
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     pwm-gate begin              timeout=30
    Wait For Line On Uart     pwm-protect-ok               timeout=30
    Wait For Line On Uart     pwm-config-ok                timeout=30
    Wait For Line On Uart     pwm-clamp-ok                 timeout=30
    Wait For Line On Uart     pwm-start-ok                 timeout=30
    Wait For Line On Uart     pwm-failsafe-ok              timeout=30
    Wait For Line On Uart     pwm-latch-ok                 timeout=30
    Wait For Line On Uart     pwm-gate done                timeout=30

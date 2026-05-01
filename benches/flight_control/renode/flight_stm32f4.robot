*** Settings ***
Documentation    Flight-control macro benchmark on stm32f4_disco
...              (Cortex-M4F @ 168 MHz). Mirrors engine_stm32f4.robot
...              with a longer wait window — the macro bench's
...              composite control loop (six threads + two timers
...              + five primitives) processes the same ~4500 events
...              in a similar wall-clock to engine_control's long
...              sweep but with more contention bookkeeping.
Resource         ${RENODEKEYWORDS}

*** Variables ***
${UART}          sysbus.usart2
${WAIT_TIMEOUT}  1800   # 30 min — same as engine_control's long lane

*** Keywords ***
Create Machine
    Execute Command           mach create
    Execute Command           machine LoadPlatformDescription @platforms/boards/stm32f4_discovery.repl
    Execute Command           sysbus LoadELF @%{ELF}

Capture Bench CSV
    [Documentation]    Drive UART2 into a file the analyzer parses.
    ${csv_path}=              Set Variable     %{BENCH_CSV_OUT}
    Execute Command           showAnalyzer ${UART} Antmicro.Renode.Analyzers.LoggingUartAnalyzer
    Execute Command           ${UART} CreateFileBackend @${csv_path} true

*** Test Cases ***
Flight Control Bench Completes With Zero Drops
    Create Machine
    Capture Bench CSV
    Create Terminal Tester    ${UART}  defaultPauseEmulation=true
    Start Emulation
    Wait For Line On Uart     flight_control bench starting  timeout=60
    Wait For Line On Uart     === END ===                    timeout=${WAIT_TIMEOUT}
    # Post-emulation parsing (drops==0 + zero-starvation telemetry
    # checks) lives in analyze.py; this test asserts firmware reached
    # the END marker within the budget.

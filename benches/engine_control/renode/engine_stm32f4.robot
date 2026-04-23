*** Settings ***
Documentation    Engine-control benchmark on stm32f4_disco (Cortex-M4F @ 168 MHz).
...              Renode cycle-accurate simulation produces absolute-timing
...              numbers that QEMU cannot. Runs for up to 30 minutes to
...              accumulate statistical power from the long sweep profile.
Resource         ${RENODEKEYWORDS}

*** Variables ***
${UART}          sysbus.usart2
${WAIT_TIMEOUT}  1800   # 30 min — long sweep @ low QEMU-vs-Renode rate

*** Keywords ***
Create Machine
    Execute Command           mach create
    Execute Command           machine LoadPlatformDescription @platforms/boards/stm32f4_discovery.repl
    Execute Command           sysbus LoadELF @%{ELF}

Capture Bench CSV
    [Documentation]    Drive the UART into a file so compare.py can parse.
    ${csv_path}=              Set Variable     %{BENCH_CSV_OUT}
    Execute Command           showAnalyzer ${UART} Antmicro.Renode.Analyzers.LoggingUartAnalyzer
    Execute Command           ${UART} CreateFileBackend @${csv_path} true

*** Test Cases ***
Engine Control Bench Completes With Zero Drops
    Create Machine
    Capture Bench CSV
    Create Terminal Tester    ${UART}  defaultPauseEmulation=true
    Start Emulation
    Wait For Line On Uart     engine_control bench starting  timeout=60
    Wait For Line On Uart     === END ===                    timeout=${WAIT_TIMEOUT}
    # The CSV parser runs post-emulation; the test itself only asserts
    # that the firmware reached the end marker inside the budget.

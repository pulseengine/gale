*** Variables ***
# ZynqMP RPU uses UART0 for Zephyr console
${UART}                       sysbus.uart0

*** Keywords ***
Create Machine
    # Use the upstream zynqmp_zephyr.resc script which:
    #   - Loads the ZynqMP platform (APU + RPU)
    #   - Halts all A53 cores
    #   - Enables RPU core 0
    #   - Loads the ELF onto rpu0
    Execute Command           set bin %{ELF}
    Execute Command           include @scripts/single-node/zynqmp_zephyr.resc
    Execute Command           machine SetSerialExecution True

*** Test Cases ***
Should Pass Gale Semaphore Tests On Cortex-R5
    Create Machine
    ${tester}=                Create Terminal Tester  ${UART}  defaultPauseEmulation=true
    Wait For Line On Uart     PROJECT EXECUTION SUCCESSFUL  timeout=120  testerId=${tester}

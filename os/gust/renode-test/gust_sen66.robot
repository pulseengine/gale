*** Settings ***
Documentation     The air-sensor stack (gale#511) on Renode: the dissolved SEN66 device
...               driver composed with the dissolved I2C v2 bus driver, running on
...               Renode's STM32 I2C v2 model (STM32F7_I2C) with a SEN66 slave model
...               (sen66_model.cs) at 0x6B. The robot queues the model's answers, the
...               firmware (gust_sen66) prints each raw result, and the expected packed
...               values below are computed here from the gust:sen66 encoding, not by
...               the firmware.
...
...               Emulation evidence only: the model is written from the SEN66
...               datasheet, and Renode's I2C model does not time the bus. A slave cannot
...               NACK through Renode's I2C interface, so the driver's busy-NACK retry is
...               not exercised here (Kani c8 covers it against the driver's own model).
Resource          ${RENODEKEYWORDS}

*** Variables ***
${UART}           sysbus.usart1
${SENSOR}         sysbus.i2c1.sen66

*** Test Cases ***
SEN66 samples cross the real I2C v2 register path, and bad data never becomes a value
    Execute Command           include @${MODEL}
    Execute Command           mach create "gust-sen66"
    Execute Command           machine LoadPlatformDescription @${REPL}
    # One entry per data-ready poll, in order:
    # 1 a fresh sample: CO2 612 ppm, PM2.5 12.3 (raw 123), VOC 101.0 (raw 1010)
    Execute Command           ${SENSOR} QueueSample 612 123 1010
    # 2 Get Data Ready answers false
    Execute Command           ${SENSOR} QueueNotReady
    # 3 a fresh sample whose CO2 word (word 8) carries a bad CRC
    Execute Command           ${SENSOR} QueueCorrupt 612 123 1010 8
    # 4 CO2 and VOC unknown (0xFFFF, 0x7FFF), PM2.5 20.0
    Execute Command           ${SENSOR} QueueSample 65535 200 32767
    # 5 a negative VOC raw value (-25 = 0xFFE7), CO2 400, PM2.5 0
    Execute Command           ${SENSOR} QueueSample 400 0 65511
    # 6 nothing queued: no new sample since read 5
    Execute Command           sysbus LoadELF @${ELF}
    Create Terminal Tester    ${UART}    defaultPauseEmulation=true
    Wait For Line On Uart     sen66-gate begin                          timeout=30
    Wait For Line On Uart     sen66-gate configure=0x00000000           timeout=30
    Wait For Line On Uart     sen66-gate start=0x00000000               timeout=30
    # (612) | (123 << 16) | (1010 << 32) | valid CO2, PM2.5, VOC (bits 48..50)
    Wait For Line On Uart     sen66-gate read1=0x000703f2007b0264       timeout=30
    # bit 63 | not-ready (0x10)
    Wait For Line On Uart     sen66-gate read2=0x8000000000000010       timeout=30
    # bit 63 | crc (0x11)
    Wait For Line On Uart     sen66-gate read3=0x8000000000000011       timeout=30
    # only PM2.5 valid: (200 << 16) | bit 49
    Wait For Line On Uart     sen66-gate read4=0x0002000000c80000       timeout=30
    # (400) | (0xFFE7 << 32) | all three valid
    Wait For Line On Uart     sen66-gate read5=0x0007ffe700000190       timeout=30
    # the sensor would repeat sample 5; the driver reports not-ready instead
    Wait For Line On Uart     sen66-gate read6=0x8000000000000010       timeout=30
    Wait For Line On Uart     sen66-gate stop=0x00000000                timeout=30
    Wait For Line On Uart     sen66-gate done                           timeout=30
    # The driver issued Start once, and Read Measured Values only after a true Get
    # Data Ready: reads 1, 3, 4 and 5, never 2 or 6.
    ${starts}=                Execute Command    ${SENSOR} StartCount
    Should Contain            ${starts}    0x00000001
    ${values}=                Execute Command    ${SENSOR} ValuesReads
    Should Contain            ${values}    0x00000004

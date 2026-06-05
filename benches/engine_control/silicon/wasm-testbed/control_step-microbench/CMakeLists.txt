cmake_minimum_required(VERSION 3.20.0)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(control_step_bench)
target_sources(app PRIVATE src/main.c)
# libcs.a = synth control_step body (synth_cs_body, r11-relative table reads) + cs_tramp.o
# (r11=&wasm_linmem; tables populated at offset 65536/65936) + native ref (control_wasm.c) + tables.c.
# Built by build.sh. Buffer-harness model (fp=linmem base), same as flat_flight-microbench.
target_link_libraries(app PRIVATE ${GALE_CS_LIB})

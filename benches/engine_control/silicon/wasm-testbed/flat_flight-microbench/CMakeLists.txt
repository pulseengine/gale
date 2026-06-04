cmake_minimum_required(VERSION 3.20.0)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(flat_flight_bench)
target_sources(app PRIVATE src/main.c)
# libff.a = synth flat_flight body (renamed synth_flat_flight_body) + ff_tramp.o
# (fp=&wasm_linmem base; 0 statics so this is faithful). Path via -DGALE_FF_LIB or edit here.
target_link_libraries(app PRIVATE ${GALE_FF_LIB})

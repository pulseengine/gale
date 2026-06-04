cmake_minimum_required(VERSION 3.20.0)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(mutex_microbench)
target_sources(app PRIVATE src/main.c)
# Mirror the override macro into main.c's printk label when the wasm variant is built.
if(DEFINED GALE_WASM_LTO_MUTEX_LIB)
  target_compile_definitions(app PRIVATE GALE_WASM_LTO_OVERRIDE_MUTEX_UNLOCK=1)
endif()

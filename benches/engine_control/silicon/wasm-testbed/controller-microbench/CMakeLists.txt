cmake_minimum_required(VERSION 3.20.0)
find_package(Zephyr REQUIRED HINTS $ENV{ZEPHYR_BASE})
project(controller_bench)
target_sources(app PRIVATE src/main.c)
# libctl.a = synth controller_step body (renamed synth_ctl_body) + ctl_tramp.o (AAPCS->r0-r7 shuffle).
# Built by build.sh; pass its path via -DGALE_CTL_LIB=...
target_link_libraries(app PRIVATE ${GALE_CTL_LIB})

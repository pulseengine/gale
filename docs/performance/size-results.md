# Binary Size Results — Semaphore Test, qemu_cortex_m3

**Date:** 2026-03-28
**Board:** qemu_cortex_m3 (ARM Cortex-M3, 256KB FLASH, 64KB RAM)
**Test:** tests/kernel/semaphore/semaphore

## Results

| Section | Baseline | With Gale | Delta | % Change |
|---------|----------|-----------|-------|----------|
| text | 36,716 B | 39,632 B | +2,916 B | +7.9% |
| data | 828 B | 856 B | +28 B | +3.4% |
| bss | 12,531 B | 13,371 B | +840 B | +6.7% |
| **FLASH** | **37,544 B** | **40,488 B** | **+2,944 B** | **+7.8%** |
| **RAM** | **12,784 B** | **13,648 B** | **+864 B** | **+6.8%** |

## Gale Symbol Sizes (in final binary)

| Symbol | Size (bytes) |
|--------|-------------|
| gale_k_pipe_write_decide | 112 |
| gale_k_pipe_read_decide | 98 |
| gale_k_fatal_decide | 58 |
| gale_k_sem_take_decide | 58 |
| gale_k_sem_give_decide | 44 |
| gale_k_stack_push_decide | 38 |
| gale_k_timeslice_tick_decide | 32 |
| gale_sem_count_init | 26 |
| gale_mem_slab_init_validate | 26 |
| **Total gale_ symbols** | **492 bytes** |

## Analysis

- **FLASH +7.8%** — within SYSREQ-PERF-001 threshold (10%)
- **RAM +6.8%** — within acceptable range
- **Gale code footprint: 492 bytes** — the verified Rust functions are tiny
- The 2.9KB text increase is mostly from C shim functions which are slightly
  larger than the inline C they replace (FFI call overhead + decision struct)
- Compiled with `opt-level = "z"`, fat LTO, `panic = "abort"`, `codegen-units = 1`
- Dead code elimination removes unused FFI functions from the static library

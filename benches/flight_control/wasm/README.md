# flight_control wasm-cross-LTO variant

Builds the flight algorithm (`filter_step` + `controller_step` from `../src/control.c`)
through the wasm-cross-LTO pipeline instead of native C, so the bench measures the
dissolved algorithm head-to-head with the native build (Phase 5).

```sh
west build -b nucleo_g474re -s benches/flight_control -- -DGALE_FC_WASM_LTO=ON
# needs clang(wasm32) + wasm-ld + loom + synth on PATH
```

Pipeline (in CMakeLists.txt, `add_custom_command`):
`clang --target=wasm32 -O2` → `wasm-ld --export=filter_step --export=controller_step`
→ `loom optimize --passes inline` (dissolves the seam) → `synth compile --target cortex-m4f`
→ `objcopy --redefine-sym` (rename exports to `synth_*`) → `control_wasm.o`.

`control_wasm_tramp.S` provides `filter_step`/`controller_step`: thin wrappers that set
`r11 = 0` (synth's wasm linear-memory base — `[r11+ptr] == [ptr]`, native pointer deref),
call the `synth_*` body, and restore `r11`. synth passes the ≤2 pointer args in r0/r1, so
no stack-arg handling is needed.

Functionally verified identical to native: on the reference vector
(st pitch=1000 roll=−500 yaw=200 updates=7; imu gyro=100,−50,30 accel=300,−200)
→ `cmd=0x07FDF307`, state pitch=937 roll=−396 yaw=230 — bit-identical to `control.c`,
including when called with a garbage incoming `r11` (the trampoline zeroes it).

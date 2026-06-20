# gust in the browser

The **same** verified `kiln-async` scheduler + fixed-point failsafe mixer that boots
gust on a Cortex-M3 (`../src/main.rs`, 8 KB SRAM) and dissolves to native via
`wasm → loom → synth` — here compiled to wasm (3.1 KB, **zero imports**) and run
**unmodified** in a browser wasm engine.

This is the browser leg of gale#74's *wasm-as-universal-substrate* (`FIND-BYOOS-002`):
**one verified component, three runtimes** — browser · host (wasmtime/kiln) · dissolved-native.
The component is identical; only the *runtime* changes.

## Run
```sh
./build.sh              # cargo -> wasm32, copies web/gust.wasm
cd web && python3 -m http.server
# open http://localhost:8000/  (wasm needs HTTP, not file://)
```

The page boots the scheduler (`gust_boot`), then drives one `gust_poll(rc)` per frame with a
swept RC input, showing live poll-round count, the mixed PWM output, and a sparkline — plus a
slider to call the `gust_mix` failsafe mixer directly.

## What it proves
- The verified kernel logic runs in a browser with **no porting** — same wasm that targets the MCU.
- It is the foundation for the rest of the lifecycle story: host **differential testing**
  (the wasm-testbed already does wasmtime/unicorn-arm/rv32), a browser **inspector / HIL**, and
  **record-on-HW / replay-in-wasmtime** debugging at the same import seam (`FIND-BYOOS-003`).

## Validation
`gust_mix` checked in wasmtime (same engine semantics as the browser): `1024→1500` (centre),
`0/512→1000` and `1536/2047→2000` (Q8 clamp). `gust_poll` is the same kiln-async scheduler
proven by the native gust boot (5000 stable rounds) and the synth dissolution scout.

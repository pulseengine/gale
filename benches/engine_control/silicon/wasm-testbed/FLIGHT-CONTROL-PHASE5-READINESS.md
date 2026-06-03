# flight_control (Phase 5) — wasm-cross-LTO readiness

The `flat_flight` testbed function is the dissolved value-in/value-out composition of the
**real** `benches/flight_control/src/control.c` algorithm (`filter_step` + `controller_step`).
This note records that the actual bench algorithm — not just the testbed shim — is
wasm-cross-LTO-ready on both backends, so wiring a wasm variant into the bench is de-risked.

## Verified 2026-06-03 (synth v0.11.27, loom v1.1.10)

**1. The real bench `control.c` dissolves + compiles on both synth backends:**

| function | wasm→loom→synth ARM (cortex-m4f) | synth RISC-V (rv32imac) |
|----------|----------------------------------|--------------------------|
| `filter_step` (ptr in/out) | 202 B | 336 B |
| `controller_step` (ptr in) | 420 B | 476 B |

Pipeline: `clang --target=wasm32 -O2 -Isrc -c control.c` → `wasm-ld --export=filter_step
--export=controller_step` → `loom optimize --passes inline` (594 B) → `synth compile`
(`--target cortex-m4f` and `-b riscv -t rv32imac`). Both exports present as symbols in both objects.

**2. Functionally identical to the validated `flat_flight`:**
native `control.c` with the flat_flight vector (st pitch=1000 roll=−500 yaw=200 updates=7;
imu gyro=100,−50,30 accel=300,−200) → `cmd=0x07FDF307`, state pitch=937 roll=−396 yaw=230 —
**exactly** flat_flight's ground truth. So flat_flight's prior validation transfers directly:
- ARM silicon (G474RE, loom v1.1.10 full dissolution): 315 cyc vs 99 native = 3.18×
- RV32 qemu-icount (v0.11.27): 181 vs 75 = 2.41× (0 callee-saved spills)
- Functional: correct on ARM (unicorn) + RV32 (qemu)

## Remaining to stand up the bench's wasm variant
- A `GALE_WASM_LTO` CMake path in `benches/flight_control/CMakeLists.txt` that links the dissolved
  `control.o` (ARM) in place of the native `control.c`, with the fp/s11 pointer trampoline (the
  `flat_flight` harness already proves the s11=0 / fp=0 native-deref trampoline works for these
  pointer-arg functions).
- ABI: synth passes args in registers r0..r7 (ARM) / a0..a7 (RV32), not AAPCS stack — the
  trampoline/caller must match (see `arm_funccheck.py`). `filter_step`/`controller_step` are
  ≤2 ptr args so this is moot for them, but noted for the composed entry.
- Then measure on silicon as the flight-control second data point (the algo segment must stay
  byte-identical baseline-vs-gale to pass the bench's <10% algo-delta integrity assert).

## UPDATE — bench verified functional + dissolved path validated end-to-end (2026-06-03)

**The bench itself is stood up:** `west build -b qemu_cortex_m3` builds clean and runs — it boots,
drives the 100 Hz loop, and emits the event stream `E,<seq>,<step>,<load>,<algo>,<handoff>,<t_lock>,
<t_post>,<t_round>,<t_bcast>`. Baseline `algo` (filter_step+controller_step in the ISR) ≈ **53 cyc**
on qemu_cortex_m3 — the reference the wasm-cross-LTO path will be compared against.

**The dissolved real `control.c` runs correctly end-to-end with the pointer trampoline (ARM, unicorn):**
set up `flight_state`/`imu_sample` in memory, `fp(r11)=0` for native `[fp+ptr]=[ptr]` deref, then call
the synth-compiled `filter_step(&st,&s)` (mutates st → pitch=937 roll=−396 yaw=230) and
`controller_step(&st)` (→ `cmd=0x07FDF307`). **Exact match** to flat_flight ground truth. This closes the
last functional gap: the bench's two real pointer-arg functions work through clang→wasm-ld→loom→synth→ARM
with the fp trampoline, not just the value-in/out shim.

**Phase-5 readiness checklist:**
- [x] real `control.c` dissolves (loom) + compiles both backends (ARM + RV32)
- [x] functionally identical to validated flat_flight (native ground truth)
- [x] dissolved ARM path correct end-to-end with the fp=0 pointer trampoline (unicorn)
- [x] bench builds + runs in qemu (baseline algo ≈ 53 cyc)
- [ ] `GALE_WASM_LTO` CMake path: build `control.o` (synth ARM) + fp-trampoline `.S`, link in place of `control.c`
- [ ] silicon measurement on nucleo_g474re (DWT), algo byte-identity preserved for the <10% integrity assert

# On-silicon results — STM32VLDISCOVERY (STM32F100, Cortex-M3), real hardware

The **literal-silicon anchor on the exact gale#65 px4io target**: the dissolved
gust_mix vs native LLVM, measured by the real Cortex-M3 **DWT cycle counter** on
an STM32F100RB (STM32VLDISCOVERY). Captured 2026-06-25.

| | native (LLVM thumbv7m) | dissolved (synth 0.15.0, cortex-m3) | ratio | correctness |
|---|---|---|---|---|
| gust_mix | **26.0 cyc/call** | **45.0 cyc/call** | **1.73×** | IDENTICAL over [0,2047] |

(`silicon_bench,ratio_x1000,,1730`.) Consistent with the qemu `-icount` codegen
bench (1.81×) and the G474RE (M4) DWT result — the synth 0.15.0 dissolve holds at
~1.7× native on real F100 silicon, bit-identical.

## Flashing the VLDISCOVERY — the ST-LINK/V1 reality (≠ probe-rs)

The VLDISCOVERY's onboard **ST-LINK/V1** is NOT usable by probe-rs or st-flash on
macOS without root (macOS claims the old mass-storage-class V1; libusb needs the
`com.apple.vm.device-access` entitlement or root). `silicon/run.sh` (probe-rs)
works for the G474's ST-LINK/V3 but **not** the V1. The working path is **openocd
under sudo**, selecting the V1 by serial (two probes may be attached):

```sh
# build the F100 image (8 KB memory map)
cd benches/gust && cp targets/generated/memory-stm32f100.x memory.x
cargo build --release --bin silicon_bench --target thumbv7m-none-eabi
cp memory.x.bak memory.x   # restore (run.sh does this automatically)

# flash + run + capture semihosting via openocd (ST-LINK/V1, needs sudo on macOS)
sudo openocd -c "adapter serial <V1-serial>" -f board/stm32vldiscovery.cfg \
  -c "init" -c "halt" -c "arm semihosting enable" \
  -c "program <silicon_bench.elf> verify" -c "reset run"
```

Expect benign `SRST error` lines — the V1 has no hardware-reset line; openocd
falls back to soft reset (sysresetreq), which works. Use the **ELF** (load address
baked in), not the `.bin`+offset (openocd 0.12 `program` arg quirk). For a more
ergonomic setup, drive the F100 via the G474's **ST-LINK/V3 as an external SWD
probe** (remove the VLDISCOVERY CN3 jumpers, wire SWD) — then probe-rs / run.sh
work unchanged.

---

_Toolchain note: current pins are synth 0.49.0 / loom 1.2.0 (#208), not the synth
0.15.0 dissolve measured above. `gust_mix` has not been re-measured on real F100
silicon under 0.49; the ratio above is historical until that re-run happens._

---

## ADC / Vrefint on real F100 silicon — `gust_adc_silicon` (2026-07-23)

The dissolved **adc-thin** driver (`adc-thin-cm3.o`, synth 0.49.0) reading the on-chip
**Vrefint** (channel 17, the 1.20 V internal reference) on the physical STM32VLDISCOVERY
— a self-contained silicon check, no external wiring. Firmware
`src/bin/gust_adc_silicon.rs`, flash `silicon/run-adc.sh`.

**Captured (openocd ST-LINK/V1 HLA via the Pi, semihosting):**
```
gust-adc-silicon: reading Vrefint (ch17) on real STM32F100 ADC1 @0x40012400 via the
  dissolved adc-thin driver (TSVREFE carried through cr2_extra)...
gust-adc-silicon OK: Vrefint = 1646 raw on real STM32F100 silicon — the dissolved
  adc-thin driver read the internal channel through the real ADC. From the 1.20 V
  nominal Vrefint this implies VDDA ≈ 2985 mV (VLDISCOVERY runs ~3.0 V; band
  1450..1780, EOC cleared=true).
```

**What this anchors:**
- The dissolved driver's **conversion lifecycle runs on real hardware**: `adc_enable →
  adc_configure → adc_start → adc_poll(EOC) → adc_read(DR)` drives the real ADC1 and
  returns a correct sample. `EOC cleared=true` = the driver's **read-after-EOC
  exactly-once** property (the DR read consumes the sample and clears EOC) held on
  silicon, not just in the qemu probe / Renode gate.
- **Vrefint (an F1 internal channel) requires `CR2.TSVREFE`** during the conversion.
  This is the payoff of the gale#216 fix: `TSVREFE` is threaded through the driver's
  `cr2_extra` mask on every managed CR2 write, so the internal channel actually
  connects. Before the fix the driver's absolute CR2 writes dropped it and this read
  would have been a floating/near-zero garbage code.
- **1646 raw ⇒ VDDA ≈ 2.985 V** (from Vrefint's 1.20 V factory nominal — the classic
  "Vrefint measures the rail" use). This is exactly VLDISCOVERY's ~3.0 V VDD, and is
  why a naive 3.3 V-ref expectation (~1489) undershoots — the board rail is 3.0 V, so
  Vrefint reads ~1638. A 3.3 V-ref interpretation of 1646 would put Vrefint at 1.33 V,
  above its 1.24 V spec max — impossible — which independently confirms the 3.0 V rail
  and that the code really is Vrefint.

**Silicon-vs-model note (tracked, gale#216):** the driver writes `CR2=ADON` in both
`adc_enable` and `adc_configure`; on F1 a `1`-written-while-ADON=1 can kick a
conversion, so on hardware `adc_configure` may trigger an extra ch17 conversion before
`adc_start`. Both convert ch17/Vrefint, so the DR holds a valid sample either way (the
1646 read confirms it) — but a strict single-shot on F1 would want the FSM to separate
power-on from register config. Harmless for this single-channel read; noted for a
follow-on refinement.

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

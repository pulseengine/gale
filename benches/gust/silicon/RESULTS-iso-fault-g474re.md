# MPU enforcement on silicon — NUCLEO-G474RE

`gust_iso_fault_probe`, the oracle `VER-OS-ISO-001` cites, run on the physical board
via `probe-rs run --chip STM32G474RETx`.

## Why this exists

Every MPU **enforcement** result gale held was qemu. The probe had never run on
silicon: no reference under `benches/gust/silicon/`, and no board region map.

A downstream (gale#348) then measured **stock Renode 1.16.1 on an emulated RT1176**
and found the MPU register file fully modelled — `MPU_TYPE`, `CTRL`, `RBAR`, `RASR`
all reading back byte-perfect — while an out-of-region write **landed**, `CFSR=0`, no
MemManage. Renode models the register file, not the MPU.

They asked which platform gale's contradicting result came from. The answer is qemu
(`benches/gust/.cargo/config.toml` → `qemu-system-arm -machine lm3s6965evb`), and gale
had already found the same Renode behaviour on the M3 (`mpu_spike_renode.rs`, recorded
in `gust_switch_2core.robot`'s scope note).

But their sharper point — *only the access separates "protected" from "the registers
accepted the write"* — applied to gale's own chain. Hence this.

## Result

| | qemu lm3s6965evb (M3) | **G474RE silicon (M4)** |
|---|---|---|
| enforcement | denied, `CFSR=0x82` | **denied, `CFSR=0x82`** |
| control (`grant-hole`) | falls through, exit 1 | **falls through, exit 1** |

```
OK:   inside-write ok, outside-write denied @0x20008000
      (CFSR=0x00000082 DACCVIOL+MMARVALID; MPU programmed via verified switch_to_partition)
FAIL: write to denied 0x20008000 fell through — MPU not enforcing        (grant-hole)
```

The control grants exactly the denied range through the same verified
`switch_to_partition`, so on hardware the denial is demonstrably caused by the region
table rather than by anything incidental.

## What this does not extend to

The STM32F100 has **no MPU** (`MPU_TYPE == 0`, measured over its ST-LINK) — see
`RESULTS-unpriv-g474re.md`. Enforcement cannot be demonstrated there because there is
nothing to enforce with.

And the qemu result is not evidence about Renode, nor the reverse: three platforms,
three behaviours — qemu enforces, Renode does not, silicon enforces. Only naming the
platform makes any of these results reproducible.

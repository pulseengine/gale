# REQ-OS-UNPRIV-001 on silicon — NUCLEO-G474RE

`gust_iso_unpriv_probe`, flashed and run on the physical board via
`probe-rs run --chip STM32G474RETx` (ST-LINK V3, serial `003B001A3235511337333439`).
Semihosting streamed to the console; both arms exit through `debug::exit`.

## Result

| arm | qemu (lm3s6965evb, cortex-m3) | **G474RE silicon (cortex-m4)** |
|---|---|---|
| default — *is the gap open?* | escape succeeds | **escape succeeds** |
| `drop-priv` — *does the remedy work?* | blocked, **MemManage** | **blocked, BusFault** |

Silicon, gap-open arm:

```
OK(gap-open): privileged tenant cleared MPU_CTRL (0x00000001 -> 0x00000000) and wrote
the DENIED address 0x20008000, read back 0xc0ffee00. Fault-containment holds;
SECURITY-containment does not.
```

Silicon, mechanism arm:

```
OK(mechanism-works): unprivileged tenant could NOT write MPU_CTRL -- BusFault raised.
Dropping to CONTROL.nPRIV=1 blocks the PPB escape.
```

## Two things only silicon found

**1. The region map was qemu-shaped.** The first silicon run died before reaching the
escape:

```
HardFault <Cause: Escalated MemManage Fault <Cause: Derived fault on exception entry>>
  in compiler_builtins::mem::memcpy
```

qemu's lm3s6965evb puts flash at `0x0000_0000`; the STM32G474 puts it at
`0x0800_0000`, and RAM is 96K with the stack at `0x2001_8000` rather than 64K ending
at `0x2001_0000`. So region 0 granted a range the code was not in and region 2 missed
the stack entirely. *Derived fault on exception entry* means the handler could not
even be entered. A deny-by-default MPU is unforgiving about a map that does not
describe the part — which is the point of it.

Fixed by the `silicon-g474` feature carrying the board's real map. The map is
per-board and cannot be inherited from the emulator.

**2. The fault taxonomy differs, and the earlier caveat was right.** qemu answers an
unprivileged PPB write with a **MemManage**; the G474 answers with a **BusFault** —
`Precise data access error at location 0xe000ed94` (`MPU_CTRL`). With `BUSFAULTENA`
clear it escalated straight to HardFault, which the debugger caught before any handler
of ours could report, and which reads exactly like a crash.

The probe had only enabled MemManage (`SHCSR` bit 16) — a qemu-shaped assumption that
was invisible until this board. It now enables MemManage, BusFault and UsageFault, and
names whichever exception it observed.

**The protection is the same on both; the exception is not.** Anything that hardcodes
the exception type would have been wrong on one of the two platforms while looking
green on the other.

## What this establishes, and what it does not

Establishes: on real Cortex-M4 silicon, a privileged tenant can disable its own MPU,
and `CONTROL.nPRIV=1` prevents it. `REQ-OS-UNPRIV-001`'s remedy is real, not
theoretical.

Does not establish: that gust runs tenants unprivileged. That is an architecture
change, not a probe. This says the change is worth making because the hardware
honours it.

## Not measured here

STM32F100 (VLDISCOVERY) is reached through the Raspberry Pi rather than this host, so
the F100 leg is deferred to that route. The F100 is Cortex-M3 and its `MPU_TYPE.DREGION`
must be checked before anything is concluded — the probe refuses to start if it is not
8, so a wrong part reports itself rather than producing a misleading pass.

## F100 leg: the part has no MPU, measured

Reached through the Raspberry Pi (`ssh pi@192.168.178.88`, ST-LINK/V1 `0483:3744`,
the VLDISCOVERY's onboard probe), using the same openocd route as
`run-wdg-f100.sh`. No firmware needed — two register reads answer it:

```
0xe000ed90 (MPU_TYPE): 00000000     <- DREGION = 0, i.e. NO MPU
0xe000ed00 (CPUID)   : 411fc231     <- Cortex-M3 r1p1
```

**The STM32F100 has no MPU.** It is optional on Cortex-M3 and this part does not
implement it. So on the F100 there is nothing to program, nothing to escape from, and
no unprivileged remedy to apply: `REQ-OS-MPU-001`, `REQ-OS-UNPRIV-001` and
`REQ-OS-MULTITENANT-001` are **unachievable on this target by hardware**, not by
missing work.

That is worth stating plainly because the F100 is one of gust's two silicon targets —
it has a Renode device class (`gust-f100-renode`), a generated memory map, a target
model entry and its own silicon results. The isolation milestone excludes it.

Note the qemu evidence is **not** transferable here: `VER-OS-ISO-001`'s fault-injection
oracle runs on qemu's `lm3s6965evb`, which is a Cortex-M3 *with* a v7-M PMSA MPU. Same
core family, different option — an M3 result does not imply an M3-with-MPU result.

The probe would have reported this rather than mislead: it refuses to start unless
`MPU_TYPE.DREGION == 8`. Reading the register directly just made the answer cheaper
than a flash cycle.

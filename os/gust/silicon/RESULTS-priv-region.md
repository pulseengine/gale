# The privilege axis, observed on silicon (2026-09-23)

gale#410 gave the verified region model a second permission axis so that
supervisor-private state is expressible: a region added with `UNPRIV_NONE` is
emitted with an ARMv7-M AP encoding that grants unprivileged code no access.

Verus proves the **emission** (1168 verified, 0 errors) and Kani cross-checks it
(6 harnesses, k5 decoding the AP back out of the emitted RASR, k6 its negative
control). Neither reaches the question that matters for isolation: **does the
hardware fault an unprivileged access to AP 0b001 / 0b101?** That is an ARMv7-M
architectural fact, assumed exactly as the `mpu_write` seam contract is.

This is the run that discharges it.

## Result — NUCLEO-WL55JC1 (STM32WL55, Cortex-M4, MPU 8 regions)

Same image, same three regions, the same `ldr r2, [r0]`. The ONLY difference
between the arms is the target region's `unpriv`.

| arm | target region `unpriv` | AP emitted | observed | verdict |
|---|---|---|---|---|
| `priv-shared` | `UNPRIV_SAME` | 0b011 | read returned **0x51572a10**, **0 faults** | PASS |
| `priv-private` | `UNPRIV_NONE` | 0b001 | **MemManage at 0x20008000**, `CFSR=0x00000082`, target word never read | PASS |

`CFSR = 0x82` decodes as **DACCVIOL** (bit 1 — data access violation) plus
**MMARVALID** (bit 7 — MMFAR holds the faulting address), and MMFAR is exactly
`TARGET_BASE`. The fault is a MemManage, not an escalated HardFault, which is
what distinguishes "the MPU denied this access" from "something else went wrong".

**The pair is the evidence, not the fault.** A single green on `priv-private`
would prove the platform: a read faults just as convincingly when the region was
never mapped, when the base was wrong, or when the MPU denied it for an unrelated
reason. `priv-shared` is what makes the fault attributable to the one field that
changed. Same shape as the two-tenant criterion (gale#399).

## The first run refuted the probe, not the model

The WL55's first attempt flashed, verified, and printed **nothing**. The probe
programs 32 KiB at `0x2000_0000` and enables the MPU with PRIVDEFENA clear, so
everything outside the programmed regions is denied *even to privileged code* —
and the part's reset stack top is `0x2001_0000`, 32 KiB above that region. The
first push after the MPU came on faulted, and the image never reached its own
report.

The three MPU-bearing bench boards put their stack at three different addresses
(WL55 64 KiB, G474 96 KiB, WB55 192 KiB), so the probe cannot assume any of them.
It now pins its own: `--defsym=_stack_start=0x20008000`, the top of the granted
region and the base of the target region, so the stack grows down into memory it
is granted and never touches the word under test.

Worth recording because the failure mode was silent. The runner called it
**REFUSED** — "deadline passed with no verdict" — rather than a pass, which is
the only reason it was noticed at all.

## Scope

- **One part.** WL55 only. The G474 is not attached to the runner host at the
  moment and the WB55's probe was claimed by another job; both arms are
  registered for `wl55jc`, `wb55rg` and `g474re` and re-run with
  `cargo xtask silicon matrix --bins gust_priv_region_probe`.
- **`g031k8` is excluded by hardware**, not by convenience: ARMv6-M PMSA has no
  AP encodings that distinguish privileged from unprivileged access.
- **This is not multi-tenant isolation.** It says the encoding the verified model
  now emits does what the model says. REQ-OS-UNPRIV-001 (gale#408) — gust tenant
  code actually running unprivileged — remains open, and
  `gust_iso_unpriv_probe` still records that gap.
- Toolchain: varve layer 2026.09.3 (`sha256:5fc6f43f…`), synth 0.65.0.

# Cross-architecture: one wasm, two instruction sets (current verdict: 2026-11-18, synth 0.89.0)

The thin-seam drivers are written once, built once to wasm, and lowered to **both** ARM
Cortex-M and RISC-V RV32 from that same input. This is the "change the tires, not the
car" claim reduced to something checkable: the driver does not change, the native
residual under it does.

## Current verdict (2026-11-18, synth 0.89.0, 15 drivers)

Reproduce: `python3 drivers/check-cross-arch.py`, and
`RV_CC=<a clang with the RISC-V backend> python3 drivers/check-cross-arch.py --self-test`.
`RV_CC` defaults to `clang`. CI uses ubuntu-24.04's apt `clang`. Apple's `/usr/bin/clang`
cannot assemble RV32, so on macOS point `RV_CC` at a Homebrew LLVM clang.

| driver | imports | ARM cortex-m3 | RV32 esp32c3 | A-ext in RV32 object |
|---|---|---|---|---|
| adc-thin | `read32 write32` | ok | ok | 0 |
| can-thin | `read32 write32` | ok | ok | 0 |
| dac-thin | `read32 write32` | ok | ok | 0 |
| gpio-thin | `read32 write32` | ok | ok | 0 |
| hm-thin | `(none)` | ok | ok | 0 |
| i2c-thin | `read32 write32` | ok | ok | 0 |
| i2c-v2-thin | `read32 write32` | ok | declined, ledger (synth#1724) | no object |
| mpu-thin | `mpu-write` | ok | declined, ledger (synth#1254 + untracked ops) | no object |
| pwm-thin | `write32` | ok | ok | 0 |
| sen66-thin | `begin end xfer` | ok | ok | 0 |
| spi-thin | `read32 write32` | ok | ok | 0 |
| switch-thin | `ctx-resume ctx-save region-swap` | ok | declined, ledger (synth#1254) | no object |
| timer-thin | `read32 write32` | ok | ok | 0 |
| uart-thin | `poll read32 write32` | ok | ok | 0 |
| wdg-thin | `read32 write32` | ok | ok | 0 |

"ok" means all of the following hold for that object:

- **seam:** the undefined set equals the wasm's import names;
- **exports:** every exported wasm function is defined under at least one of its export
  names;
- **code:** the object has a non-empty executable section;
- **RV32 only:** the object contains no A-extension instruction.

ARM: 15 of 15. RV32: 12 of 15, and the other 3 are on the ledger. A-extension scan: 0 hits
in the 12 RV32 objects emitted. The 15 wasm inputs contain 0 atomic instructions, so on
this corpus the scan can only fire if the backend invents an atomic. The evidence that it
can fire at all is the self-test's planted objects, not this column.

**The seam rule alone cannot fail an import-free driver.** If hm-thin's object is empty or
missing, its undefined set is empty, and so is its import set. A planted synth that
handed hm-thin an empty object scored "ok ok" before the exports and code rules existed.
It now fails with exit 4.

**Export aliases: interim contract, then a planned tightening.** wit-bindgen exports
one function body under several names when the bodies are identical, and synth 0.89.0
emits one symbol per function. So 11 of 15 drivers define only one name per aliased
function, on both legs. For example, wdg-thin defines `wdg_refresh` but not
`gust:hal/wdg@0.1.0#refresh`. synth has confirmed this is a bug and is tracking it as
synth#1746. Its fix will emit every export name as a GLOBAL symbol at the same address,
and will refuse loudly when a name cannot be emitted.

- **Interim contract (now):** every exported function must be defined under **at least
  one** of its export names. Every run prints the missing alias names, citing synth#1746.
- **Planned tightening:** once the varve pin carries the #1746 fix, the rule becomes
  **every export name defined**, so an alias with no symbol of its own fails the leg.

### What the gate checks, and how each check is shown to fail

`--self-test` runs the gate's own functions on planted inputs:
`imports`, `func_exports`, `undefined`, `defined`, `assess`, `a_ext_scan`, `build_wasm`,
`verdict` and `ledger_findings`. It does not compare set literals, which is what the
earlier self-test did (gale#389 item 3). That version still passed with the comparator
replaced by `good = True`. The self-test's date is pinned, so it does not change verdict
as the calendar moves.

- **Seam, exports and code.** The planted wasms import either `read32 write32` or
  nothing. The planted ARM and RV32 objects cover these cases:
  - the seam exact;
  - an extra leaked `synth_func_18`;
  - 0 undefined;
  - no object;
  - a corrupt object;
  - an empty object;
  - the export defined in `.data` with no code;
  - code present but the export missing.

  Only the seam-exact case passes. Missing objects and nm failures read as "no
  information" (`None`), never as the empty set.
- **Exit code.** `verdict()` is the only place `main()` gets its exit code. Planted rows
  must produce each code: 3 (build), 4 (ARM, or a native-pointer violation), 6 (A-ext),
  5 (RV32) and 0 (clean).
- **Build.** A driver whose `cargo build` fails is a gate failure (exit 3). The gate uses
  the wasm that cargo's own `compiler-artifact` message names for this invocation, not a
  glob of `target/`, and copies it into a per-run temp dir. Freshness is cargo's call.
  There is no mtime check of our own: it accepted a backdated wasm, and it stayed red
  after a `touch Cargo.toml` or a stray `src/.DS_Store`. The self-test covers three cases:
  - a fixture crate builds;
  - the same crate, with a touched `Cargo.toml` and a stray `.DS_Store`, still builds;
  - a syntax error, or a broken second target in the same crate, fails with no wasm
    returned.

  The old gate ignored cargo's exit status and re-lowered the previous wasm, so a syntax
  error in `wdg-thin` printed `ok`.
- **No fixed paths.** Objects, the wasm copy and the fixtures all live in a
  `tempfile.TemporaryDirectory` per run. `build-cross-arch.sh` uses a `mktemp -d`. Before
  this change both wrote `/tmp/xa-*.o` and `/tmp/xr-*.o`.
- **A-extension scan (RV32).** esp32c3 is RV32IMC. Every RV32 object synth emits is
  decoded from the bytes of its executable sections. The scan tests every 2-byte-aligned
  offset for an RV32A encoding: major opcode `0101111`, funct3 `010`, one of the eleven
  RV32A funct5 values, and `rs2 = 0` for LR.W.
  - It does not grep disassembly, because a disassembler prints an AMO as `.word` under a
    `$d` mapping symbol, or when the A extension is not enabled.
  - It is not a linear sweep, because a stray `.half 0x0013` reads as the low half of a
    32-bit instruction and hides the AMO after it.

  The self-test plants four objects, and each must fail the RV32 leg:
  - `amoadd.w` as a mnemonic;
  - `amoadd.w` as `.word`;
  - `lr.w` as `.word`;
  - `amoadd.w` behind `.half 0x0013`.

  Over-approximation: a 32-bit window that straddles two real instructions could match.
  There are 0 such hits on today's 12 RV32 objects.

### The RISC-V ledger rules

gale#506 asked for a rule that stops a ledger from absorbing a leg. Its suggested rule,
"fail when the ledger covers all subjects", is too weak, because a ledger covering 14 of
15 satisfies it.

**What these rules do:** they make every exception visible, attributed and dated, so a
reviewer sees it in the diff and the gate sees it expire.

**What they do not do:** prevent a bad ledger edit. Swapping one entry for another at the
same cap, with a made-up tracker such as `synth#0`, passes every rule. Review of
`RISCV_LEDGER` is the control for that, not this script.

The self-test plants a violation of every clause, plus both sides of each boundary.

| rule | fails when |
|---|---|
| R1 stale | a ledgered driver now crosses (it must leave the ledger) |
| R2 regression | a driver not on the ledger does not cross |
| R3 entry | an entry has no upstream tracker (`synth#N`), no owner issue (`gale#N`) or no ISO expiry, or it names a driver that does not exist. Each clause is checked separately |
| R4 expiry | an entry is past its expiry date (the expiry day itself passes), or its expiry is more than 120 days ahead (exactly 120 passes) |
| R5 cap | `len(RISCV_LEDGER) != RISCV_LEDGER_CAP`, so adding or removing an entry also needs a second, visible edit to the cap |
| R6 majority | the ledger covers half the subjects or more (`2 * ledgered >= subjects`, so 4 of 8 fails and 3 of 7 passes). Past half, the leg is a list of exceptions rather than a gate. The right response is to demote it to advisory in review, not to grow the list |

Within 30 days of an entry's expiry, the gate emits a `::warning::` naming the entry and
its owner issue. Past expiry, R4 fails the gate. So the gate goes red on 2027-03-01 even if
no file changes. That is intended: the ledger has to be re-measured.

Today the ledger holds 3 of 15 (cap 3). Every entry expires 2027-02-28, has owner gale#506,
and names its blocker as measured on synth 0.89.0:

- **switch-thin:** 9 functions hit synth#1254 (a constant address of 0x100000+k folded
  into the 12-bit load/store offset). One more declines on `I64Load32U`.
- **mpu-thin:** 3 functions hit synth#1254. It also declines 3 functions on `I32Popcnt`
  and 1 on `MemoryFill`, and **no upstream tracker covers those yet**. Fixing #1254 alone
  will not clear this entry.
- **i2c-v2-thin:** 1 function declines on synth#1724 (`Call(0)`).

### ARMv6-M policy: advisory

synth's `--target` list (0.89.0) has no ARMv6-M profile (Cortex-M0/M0+, `thumbv6m`). No
driver is lowered for ARMv6-M, and nothing about ARMv6-M is gated. The leg is still
reported in two places:

- a line in every gate run;
- its own CI step (`--armv6m-advisory`, `continue-on-error: true`, exit 0), which emits a
  `::warning::` annotation.

That step sits inside the required job "seam survives lowering on ARM and RISC-V (all thin
drivers)", not in a job of its own. So it adds no required context, and no verdict
depends on it. When synth lists an ARMv6-M target, the warning says so, and the leg should
then become a gated column under the same rules.

## History: measured 2026-08-28, synth 0.58.0


| driver | imports | ARM cortex-m3 | RISC-V rv32imc |
|---|---|---|---|
| adc-thin | `read32 write32` | 45 T, 2 U — complete | 26 T, 3 U — 1 of 27 skipped, **1 dangling** |
| can-thin | `read32 write32` | 34 T, 2 U — complete | 19 T, 3 U — 1 of 20 skipped, **1 dangling** |
| dac-thin | `read32 write32` | 43 T, 2 U — complete | 24 T, 3 U — 1 of 25 skipped, **1 dangling** |
| gpio-thin | `read32 write32` | 30 T, 2 U — complete | 17 T, 3 U — 1 of 18 skipped, **1 dangling** |
| hm-thin | `(none)` | 19 T, 0 U — complete | 12 T, 1 U — 1 of 13 skipped, **1 dangling** |
| i2c-thin | `read32 write32` | 38 T, 2 U — complete | 21 T, 3 U — 1 of 22 skipped, **1 dangling** |
| mpu-thin | `mpu-write` | 27 T, 1 U — complete | **no object** — synth exits non-zero (#952) |
| pwm-thin | `write32` | 36 T, 1 U — complete | 21 T, 2 U — 1 of 22 skipped, **1 dangling** |
| spi-thin | `read32 write32` | 34 T, 2 U — complete | 19 T, 3 U — 1 of 20 skipped, **1 dangling** |
| switch-thin | `ctx-resume ctx-save region-swap` | 41 T, 3 U — complete | 17 T, 10 U — 13 of 30 skipped, **10 dangling** |
| timer-thin | `read32 write32` | 30 T, 2 U — complete | 17 T, 3 U — 1 of 18 skipped, **1 dangling** |
| uart-thin | `poll read32 write32` | 27 T, 3 U — complete | 16 T, 4 U — 1 of 17 skipped, **1 dangling** |
| wdg-thin | `read32 write32` | 35 T, 2 U — complete | 20 T, 3 U — 1 of 21 skipped, **1 dangling** |

Added 2026-11-14 on synth 0.89.0, so not comparable to the rows above: **i2c-v2-thin**
(`read32 write32`): ARM complete, undefined set equals its imports. RV32 declines with no
object, only on synth#1724 (a call inside a block with an operand beneath its arguments).
It does not hit #1254. The #1254 address on every other driver here is 0x100000+k, and
that is wit-bindgen's `run_ctors_once` flag at 0x10000C. This driver disables that guard
(gale#516), so it has no linear-memory access for #1254 to fold.

Updated 2026-11-17 on synth 0.89.0 (gale#516): every wit-bindgen driver's `generate!`
now sets `disable_run_ctors_once_workaround`. The wasm of each of the 13 drivers above
then has 0 `run_ctors_once` references (a control build of uart-thin without the flag
has 5). **RISC-V: 11 of 13 now cross**, with each object's undefined set equal to its
imports: adc, can, dac, gpio, hm, i2c, pwm, spi, timer, uart and wdg -thin. The
0x100000+k address they all failed at was that guard. mpu-thin and switch-thin still
decline and stay on the ledger. The rows above are the earlier measurement and are kept
as history; `check-cross-arch.py` is the current verdict.

**ARM: 13 of 13 lower completely**, and each object's undefined set is *exactly* the
set of names its wasm imports — no leak, no truncation. That half of the claim holds.

**RISC-V: 0 of 13.** Twelve emit an object that cannot be linked, because synth's RV32
selector skips a function it cannot select and emits the call site anyway, leaving a
dangling `synth_func_N`. The thirteenth (`mpu-thin`) emits nothing at all. This is not
new and is not a regression: synth 0.52.0 — the version the previous version of this
table cited — produces byte-for-byte the same result on today's sources, as does
0.60.0, the latest release. `ld.lld` refuses the objects outright:

```
ld.lld: error: undefined symbol: synth_func_18
>>> referenced by out.o:(gust:hal/gpio@0.1.0#set)
```

Filed upstream as **synth#1102**, and **fixed on synth `main` as #1104** — not
released in **synth 0.61.0** (2026-09-02) as RQ-61-DANGLE.

Measured against 0.61.0, the fix changes the failure mode and not the outcome:

| | 0.58 / 0.60 | 0.61.0 |
|---|---|---|
| emitted an unlinkable object at exit 0 | 12 of 13 | **0** |
| refused at exit 1, no object written | 1 | **13** |

```
Error: #1102: 8 retained function(s) relocate against declined function(s)
```

The silent-truncation class is closed: nothing that cannot link is emitted. But
no driver crosses RV32, because the RV32 selector gaps are untouched (immediate
too large for memory offset; `GlobalGet` unsupported by the RV32 skeleton). The
ledger stays at 13, for a better reason.

We remain pinned to 0.58.0; the above was measured against a downloaded 0.61.0.

**The fix corrected our framing, and it is worth stating plainly.** This report
described the defect as rv32-specific, on the strength of the ARM column being
clean. It was not: synth's fix found **Thumb-2 and A32 shipping the identical
dangling symbol at exit 0**, on a module that declines on those backends. The
guard was missing everywhere; the ARM objects here are clean only because the ARM
backend declines *nothing* on this corpus, so the broken path is never entered.

That distinction matters for how the ARM row above should be read. "13 of 13,
undefined set equals imports" is a true statement about **these objects**. It is
not evidence that the ARM lowering path is sound, and it should not be cited as
such.

## What the previous table said, and why it was wrong

It reported eight drivers, six of them "complete" on both targets, gpio-thin at
"5 T, 2 U — complete". That was measured before these drivers were componentized
(`wdg-thin` had 6 functions then; it has 21 now), so it describes sources that no
longer exist. It was not re-measured when the sources changed.

Two mechanisms kept it from being caught:

1. **The gate enumerated its drivers by hand.** `build-cross-arch.sh` carried
   `DRIVERS=(wdg-thin gpio-thin i2c-thin timer-thin adc-thin dac-thin pwm-thin
   uart-thin)` — eight names, while thirteen `*-thin` directories exist. can, hm,
   mpu, spi and switch were never gated.
2. **The rule was `undefined_count > 0`.** See below.

## Why `> 0` was the wrong rule

The old gate failed an object with **zero** undefined symbols, on the reasoning that
a vanished seam means a truncated object (the synth 0.49 shape, where the RISC-V
backend dropped every function containing an imported call).

That reasoning is right about the failure it names and blind to its neighbour. `> 0`
is satisfied by *any* undefined symbol — including a leaked one. Every RISC-V object
above carries a dangling `synth_func_N`, so every one of them counted as passing. The
gate was green because the objects were broken in a way it could not distinguish from
being correct.

`hm-thin` shows the other edge: it imports nothing, so a correct object has **zero**
undefined symbols. Under `> 0` a correct driver fails.

The rule is therefore neither `> 0` nor `== 0` but **equality**:

> the lowered object's undefined set must equal the set of names the wasm imports.

That is what `check-cross-arch.py` gates, and it is the same rule
`check-driver-components.py` already applies on its object axis. It is correct for a
driver importing nothing, for a driver whose seam is not `gust:hal` (`mpu-thin`,
`switch-thin`), and it rejects a leak, which `> 0` cannot.

The RISC-V leg is carried as an explicit known-defect ledger in that script rather
than skipped: if a driver starts crossing cleanly the gate **fails** until it is
removed from the ledger, so the list shrinks to empty instead of outliving the bug.

## Honest boundary

- **Executed on ARM silicon only.** The ARM objects are validated on two dies —
  Cortex-M4 and Cortex-M3, `silicon/RESULTS-wdg-g474re.md` and
  `silicon/RESULTS-wdg-f100.md`. The RISC-V objects are **compiled and seam-correct;
  they have not been run.**
- **And they could not meaningfully be run as-is.** These drivers carry STM32 register
  maps (the IWDG at `0x4000_3000`, and so on). Lowering them for RV32 proves the
  *toolchain* crosses; pointing them at an ESP32-C3's peripherals would be nonsense
  without an ESP32 target model. A faithful RISC-V silicon result needs an RV32 target
  in `targets/`, which is not done.
- **Import-free logic already has real RISC-V silicon numbers.** `gust_mix` is measured
  on a physical ESP32-C3 at 1.839x native — see `esp32c3/RESULTS.md`. That lane is
  execution evidence; this table is lowering evidence. They are different claims.
- **History (synth 0.58.0, superseded by the current verdict above).** Two functions
  declined then, reported upstream as synth#882: `wdg_unlock` hit an
  unsupported `BrTable` in the RV32 selector (a declared gap, declining loudly), and
  `i2c_step` failed with `undefined label 'Lend0'`, which looked like a genuine emit
  defect rather than a missing feature.

## What changed to make this possible

synth#871 — filed from this repo on 2026-07-29 after the RV32 backend was found to
decline every seam-importing function — was fixed the same day in synth 0.52.0, which
emits `auipc`/`jalr` plus `R_RISCV_CALL_PLT` into a `.rela.text`. Before that release,
no thin-seam driver crossed at all.

# wave060 — gust v0.6.0 outer-partition-switch demonstrator (VER-OS-SWITCH-001 part 2)

Branch: `feat/gust-switch-demo`
Deliverable: `benches/gust/src/bin/gust_switch_probe.rs` — the end-to-end
evidence composing the two already-merged verified cores. **No verified source
was touched** (`src/partition_switch.rs`, `src/mpu_switch.rs`, and their
`plain/` mirrors are exactly as merged in #187/#193), so no Verus/Kani/strip
rerun was required.

## What was built

One qemu probe (lm3s6965evb, cortex-m3 — a real v7-M PMSA MPU) that drives the
VERIFIED `Switcher` (partition-switch FSM, Verus 1095/0 + Kani 4/4) across a
3-partition major frame with the VERIFIED `RegionTable` (I-ISO region
programmer, Verus 1159/0) wired in as the `region_swap` trusted seam, and
proves — with real, handler-flag-gated MemManage hardware exceptions — that
each partition is confined to BOTH its time window AND its memory map, with
region-swap-strictly-before-resume observed at every switch.

### Construction

**Major frame** (`MAX_WINDOWS == 4`, so window 3 revisits P0 — the wrap is
itself evidence): `partition_id [0,1,2,0]`, `offset [0,10,20,30]`,
`budget [10,10,10,10]`, `frame_len 40`. `frame.check()` (Verus-ensured
`== frame_inv`) asserted at runtime before use.

**Region table** — built EXCLUSIVELY through the verified builder
(`RegionTable::new()` + 12 × `try_add_region`, each return asserted true),
programmed EXCLUSIVELY through `switch_to_partition` (P1–P4 proven) → 
`apply_program` → the probe's faithful `mpu_write` bridge (DSB+ISB on
MPU_CTRL writes, init-time `MPU_TYPE.DREGION == 8` refusal — both contract
items). Per partition p ∈ {0,1,2}:

| slot | region | grant |
|---|---|---|
| 0 | flash `[0x0000_0000, 0x0004_0000)` 256K RO | common (code/rodata/vectors) |
| 1 | SRAM `[0x2000_0000, 0x2000_8000)` 32K RW | common (.data/.bss + probe statics) |
| 2 | SRAM `[0x2000_C000, 0x2001_0000)` 16K RW | common (stack window, MSP 0x2001_0000) |
| 3 | 2 KiB scratch: P0 `0x2000_8000`, P1 `0x2000_8800`, P2 `0x2000_9000` | per-partition (word @ base+0x10) |
| 4–7 | emitted DISABLED by the verified core (P2 deny-by-default) | — |

Deliberate deviation from the tasking's suggested scratch addresses
(0x2000_0010/0x2000_0810/0x2000_1010): low SRAM is where cortex-m-rt places
the probe's own .data/.bss, which must stay writable in EVERY window, so the
per-partition scratch regions live in the mid-SRAM hole above the common data
grant instead. `[0x2000_9800, 0x2000_C000)` stays granted to nobody
(PRIVDEFENA clear — deny-by-default even for privileged code), and the grant
shape is sanity-checked pre-enable through the verified `covers_addr` mirror
(each p covers only its own scratch word; nobody covers 0x2000_9800). No
custom linker script needed (scratch is raw never-linked SRAM), hence **no
build.rs / Cargo.toml change** — the bin is auto-discovered and links no
dissolved object; the existing build union is untouched (verified:
`cargo build --bins` exit 0).

**Seams** (`#[no_mangle] extern "C"`, resolving `partition_switch`'s trusted
externs): `region_swap(part)` → `THE_TABLE.switch_to_partition(part)`;
`ctx_save`/`ctx_resume` → recording stubs (`CURRENT_PART := part` on resume).
Each seam stamps a monotonic sequence number, and `ctx_resume` additionally
reads the hardware back (RNR := 3, RBAR & ~0x1F must equal the INCOMING
partition's scratch base, MPU_CTRL == ENABLE) — so
region-swap-before-resume is not only proven (the `swapped` ledger +
`lemma_resume_implies_region_swap`) but OBSERVED per switch:
`save_seq < swap_seq < resume_seq` + the RBAR readback match are oracle-gated.

**Drive loop** (windows 0..3, then wrap): per window w with owner p —
1. `CURRENT_PART == p` and `frame.current_window(mid) == w` (temporal cross-check);
2. own-access OK: write p's scratch word — fault delta must be 0, read-back exact;
3. cross-access DENIED: write neighbor (p+1)%3's scratch word — must raise
   exactly one MemManage with `MMFAR ==` that exact address and
   `CFSR & 0x82 == 0x82` (DACCVIOL+MMARVALID);
4. non-vacuity control (w == 1): P0's scratch word — writable one window
   earlier — must NOW fault. A static/un-swapped map would let it through and
   FAIL the probe, so confinement is attributable to the switch, not the
   layout. **This is the in-probe control; no separate control bin** (stated
   per tasking's "if in-probe is cleaner, that's sufficient").
5. boundary: `tick(mid)` must NOT preempt; `tick(win_end-1)` MUST (S1);
   `run_switch()` must show seam order + resume-map readback + outgoing
   `ctx_save(p)` + `cur` advanced by exactly one (S3).

**Fault recovery** (generalizing the v0.5.0 single-fault idiom to re-armable):
naked `MemoryManagement` shim → recorder that accepts a fault ONLY if the
one-shot `EXPECT_MMFAR` is armed and matches MMFAR exactly (anything else —
including a faulting own-write — FAILs from the handler), then advances the
stacked PC past the faulting store (Thumb 16/32-bit width decoded from the
instruction) with ICI/IT cleared + T forced. The store itself is a dedicated
`#[inline(never)]` plain-str helper so the skip is well-defined. HardFault
handler FAILs explicitly. There is no fall-through OK path: the final oracle
also requires `FAULT_COUNT == 5` exactly (4 cross + 1 control), `sw.cur == 0`
and `CURRENT_PART == 0` (full wrap).

## Gates run (verbatim, with exit codes)

**RUN IT — dev profile** (`cd benches/gust && cargo run --bin gust_switch_probe`), exit 0:

```
     Running `qemu-system-arm -cpu cortex-m3 -machine lm3s6965evb -nographic -icount shift=1 -semihosting-config enable=on,target=native -kernel target/thumbv7m-none-eabi/debug/gust_switch_probe`
gust-switch-probe OK: 3 partitions across the major frame (windows P0->P1->P2->P0, wrapped to window 0) — each confined to its time window AND its MPU map, region-swap-before-resume observed at every switch (seam order + RBAR readback), 5 expected cross-partition faults denied (CFSR DACCVIOL+MMARVALID) @ 0x20008810 0x20009010 0x20008010 0x20008010 0x20008810
```

The five denied addresses match the construction exactly: w0 P0→P1's word
(0x20008810); w1 P1→P2's word (0x20009010) + the P0 control (0x20008010);
w2 P2→P0's word (0x20008010); w3 P0→P1's word (0x20008810).

**Release profile** (`cargo run --release --bin gust_switch_probe` — opt-z +
LTO changes the faulting-store shape, so the skip-recovery is exercised under
optimization too), exit 0: identical OK line.

**Regression** (existing bins still green):
- `cargo run --bin gust_os_tl_probe` → `gust-os-tl-probe OK: log=="gust:os up\n", run()=0xA`, exit 0
- `cargo run --bin gust_iso_fault_probe` → `gust-iso-fault-probe OK: inside-write ok, outside-write denied @0x20008000 (CFSR=0x00000082 ...)`, exit 0
- `cargo build --bins` → exit 0 (only pre-existing warning in gust_breadth_probe; gust_switch_probe compiles warning-free)

**Verus/Kani/strip**: not run — no file under `src/` or `plain/` was touched
(both modules were already exported from the lib; no gap found).

## Honest gaps

- **Single-core qemu, not multi-core Renode.** This demonstrates the
  temporal + spatial + ordering core (window-boundary preemption, per-window
  MPU confinement, swap-before-resume) on one core, with the probe itself
  playing all three partitions. Multi-core placement on Renode is a separate
  follow-on.
- **ctx_save/ctx_resume are recording stubs** — no real register-file
  save/restore. The FSM ordering and the MPU programming are what is under
  test; a real context switch is future work (they are trusted seams in the
  verified model too).
- **Software-driven tick.** `tick(t)` is fed timeline values by the probe
  loop, not by a hardware timer interrupt. The tick source is a trusted seam
  in the verified model; a SysTick-driven variant is a follow-on.
- **Fault-containment scope, W+X regions** (inherited from mpu_switch's
  documented XN=0 model): privileged code could reprogram the un-checked PPB;
  this is the same scope as the v0.5.0 I-ISO evidence, not a regression.
- Evidence is local qemu (real v7-M MPU emulation), not yet CI-gated.

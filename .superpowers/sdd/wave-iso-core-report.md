# Wave report — I-ISO region-program core (v0.5.0 keystone)

Date: 2026-07-16. Branch: `feat/gust-iso-region-core` (off main `bc099e9`).
Deliverable: `src/mpu_switch.rs` — verified MPU region-programming core
discharging invariant I-ISO (plan §v0.5.0: the hardware MPU as sole
isolation root, programmed by the verified core on every partition switch).

## What was built

- `src/mpu_switch.rs` (Verus, inside `verus!{}`): static `RegionTable`
  (MAX_PARTITIONS=4 × MAX_REGIONS=8, flat scalar arrays of
  base/size/enabled/writable — no trait objects, closures, heap, or SRAM
  state beyond the table itself), ARMv7-M PMSA model.
- `region_wf(base, size)` spec: power-of-2 size >= 32 (reuses
  `crate::mpu::is_pow2_spec` / `MIN_REGION_SIZE` — the existing verified
  characterisation behind `validate_region`), base aligned to size
  (`base % size == 0`), no address-space wrap (the same U-6 bound
  `validate_region` enforces at runtime).
- `RegionTable::table_inv()`: every enabled slot `region_wf` AND enabled
  slots of the SAME partition pairwise disjoint over [base, base+size).
- `RegionTable::program_partition(&self, part) -> ProgramSeq` with
  machine-checked ensures:
  - **P1** emitted-matches-table: enabled slot r emits
    `rbar == table.base`, `rasr == rasr_enabled_spec(size, writable)`
    (ENABLE bit + SIZE field bits 5:1 + AP field bits 26:24, arithmetic
    encoding equal to the shifted bit layout).
  - **P2** deny-by-default: every slot NOT enabled emits RASR == 0 /
    RBAR == 0 (ENABLE bit clear) — unused hardware slots explicitly
    turned off, never stale from the previous partition.
  - **P3** total: all 8 slots addressed, exactly one write per slot,
    slot r at sequence position r (`out.w[r].rnr == r`).
  - **P4** ordered: the single MPU_CTRL enable write is the LAST sequence
    element (`out.w[8].rnr == MPU_CTRL_ID`, `rasr == MPU_CTRL_ENABLE`);
    `apply_program` emits in index order, so all region programming
    reaches the hardware before any enable bit.
- Trusted seam (executor `poll_task` pattern): `unsafe extern "C" fn
  mpu_write(rnr, rbar, rasr)` declared OUTSIDE `verus!{}` under a single
  narrowly-scoped `#[allow(unsafe_code)]`; `emit_write` is the
  `#[verifier::external_body]` wrapper (deliberately NO ensures — no proof
  rests on what the store did); `apply_program`'s loop over the sequence
  is verified (invariant + `decreases`). `switch_to_partition` composes
  compute + emit, fully verified down to the single store.
- Kani harnesses (4, `#[cfg(kani)] mod iso_kani`, over `kani::any` +
  assumed exec-form table_inv via `crate::mpu::validate_region` +
  pairwise-disjointness): `iso_deny_by_default` (k1),
  `iso_emitted_matches_table` (k2, SIZE field independently recomputed via
  `trailing_zeros`), `iso_emissions_disjoint` (k3, extents decoded back
  OUT of the emitted RASR), `iso_sequence_total_and_ordered` (k4).
- Wiring: `pub mod mpu_switch;` in `src/lib.rs`; `src/mpu_switch.rs` in
  `BUILD.bazel` VERUS_SRCS; plain mirror regenerated via
  `tools/verus-strip` into `plain/src/mpu_switch.rs` (plus regenerated
  `plain/src/lib.rs`); wired into BOTH convergence lists
  (`plain/BUILD.bazel` verus_srcs + plain_srcs) and the FILES list in
  `tools/verus-strip/tests/gate.rs`. plain/ never hand-edited.

## Design decisions

1. **Sequence as data, ordering as a postcondition.** `ProgramSeq` is a
   9-triple array: 8 region writes + the trailing MPU_CTRL enable
   (sentinel `rnr == MPU_CTRL_ID`). P4 is a real ensures over the data
   (enable strictly last), not prose; in-order emission by the verified
   `apply_program` loop carries it to the hardware.
2. **Arithmetic register encoding, no bit-vector obligations.**
   `rasr_enabled_spec = 1 + 2*SIZE + 0x0100_0000*AP` — equal to the
   shifted bit layout but stated multiplicatively, so every proof
   discharges in plain linear arithmetic (no `by(bit_vector)` needed in
   this module at all). The SIZE field uses the same flat-enumeration
   style as `mpu::is_pow2_spec` (27 valid sizes, 32..2^31); the exec
   encoder's trailing branch is proven unreachable (`assert(false)`
   discharged from the pow2 enumeration minus sizes < 32).
3. **Deny-by-default twice over.** Disabled slots emit RASR=0 (P2), and
   `MPU_CTRL_ENABLE = 1` deliberately does NOT set PRIVDEFENA — the
   background region stays disabled even for privileged code.
4. **Reuse, not duplication.** `is_pow2_spec` and `MIN_REGION_SIZE` come
   from `crate::mpu`; `region_wf` restates exactly the property
   `mpu::validate_region` checks at runtime (including the U-6
   no-overflow bound), and the Kani harnesses use `validate_region`
   itself as the exec-form assumption — binding the spec-level and
   runtime characterisations together across two independent engines.
   Spec-only items are referenced fully qualified (a top-level `use` of a
   spec fn would survive verus-strip while its definition does not).
5. **Kani checks the shipped path.** Harnesses run the post-strip plain
   code of `program_partition` (the exact shipped function), with
   expectations recomputed independently (`trailing_zeros` for the SIZE
   field; extents decoded back out of the emitted RASR for k3).

## Gate evidence (verbatim, with exit codes)

### Gate 1 — Verus (bazel test //:verus_test)

Baseline on main (bc099e9), before this change:

    verification results:: 1081 verified, 0 errors
    //:verus_test                                                   (cached) PASSED in 3.0s
    Executed 0 out of 1 test: 1 test passes.

After (final tree):

    verification results:: 1096 verified, 0 errors
    //:verus_test                                                            PASSED in 3.2s
    Executed 1 out of 1 test: 1 test passes.

bazel exit code 0 (`VERUS_EXIT_PIPE=0`). 1096 >= 1081: +15 newly verified
items, zero regressions, zero errors.

### Gate 2 — strip gate

Regenerated (never hand-edited):

    cargo run --manifest-path tools/verus-strip/Cargo.toml -- src/mpu_switch.rs -o plain/src/mpu_switch.rs
    Wrote plain/src/mpu_switch.rs
    cargo run --manifest-path tools/verus-strip/Cargo.toml -- src/lib.rs -o plain/src/lib.rs
    Wrote plain/src/lib.rs

    $ cargo test --manifest-path tools/verus-strip/Cargo.toml --test gate
    running 2 tests
    test plain_standalone_matches_stripped_standalone ... ok
    test plain_matches_stripped_src ... ok
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.23s
    GATE_EXIT=0

### Gate 3 — Kani (cargo kani, root crate = plain, same as executor harnesses)

    iso_deny_by_default:            VERIFICATION:- SUCCESSFUL (16.9s)  exit 0
    iso_emitted_matches_table:      VERIFICATION:- SUCCESSFUL (22.4s)  exit 0
    iso_emissions_disjoint:         VERIFICATION:- SUCCESSFUL (125.7s) exit 0
    iso_sequence_total_and_ordered: VERIFICATION:- SUCCESSFUL (16.0s)  exit 0

(each printed `Complete - 1 successfully verified harnesses, 0 failures, 1 total.`)
Note: harness unwind is 33, not 10 — `kani::any::<[T; 32]>()`'s internal
array-construction loop needs 32+1 unwindings; the first run failed the
unwinding assertion at unwind(10) and was fixed by raising the bound (a
harness-infrastructure bound, not a property weakening).

### Gate 4 — honesty

- `grep 'assume(' src/mpu_switch.rs` minus `kani::assume`: **none** — no
  Verus-level assume anywhere. The 3 `kani::assume` calls are the
  sanctioned harness-input-constraint pattern (same as executor's
  `arbitrary_tasks_bounded`).
- No ensures weakened or removed; `emit_write` (`external_body`) carries
  NO ensures at all, so nothing is taken on faith beyond "the store
  happened".
- Supporting checks on the final tree: `cargo build` exit 0;
  `cargo clippy --lib -- -D warnings` exit 0; `cargo test` all suites ok
  (34+42+16 passed, 0 failed).

## Concerns / pre-existing observations (not caused by this change)

1. **Pre-existing local clippy/fmt toolchain drift on main.** With the
   local stable toolchain (clippy/rustfmt 1.97-era), `cargo clippy
   --all-targets -- -D warnings` fails on `tests/cbprintf_integration.rs`
   (new `byte_char_slices` lint) and `cargo fmt --check` reports ~590
   diffs across pre-existing plain/ files — both reproduce on a clean
   main checkout (verified via git stash roundtrip) and are toolchain
   version noise, not regressions from this change. My file's only fmt
   note (leading blank line in the generated plain mirror) is the same
   artifact every verus-strip output has (e.g. plain/src/executor.rs:1),
   and main is CI-green with it.
2. **Scope**: this lands the verified compute+emit core (the plan's
   region-table + program-on-switch sliver). The plan's remaining v0.5.0
   oracle-gate items — Renode/qemu fault-injection demo, the synth#757
   containment demo, and rivet `REQ-OS-MPU-001`/`VER-OS-MPU-001` artifacts
   — are follow-on work, as is a verified builder API for constructing
   non-trivial static tables (deployments discharge `table_inv` on their
   constant table at build time; `RegionTable::new()` provides the proven
   all-disabled baseline).
3. The referenced plan doc (`docs/superpowers/plans/2026-07-15-gust-safety-release-line.md`)
   is not on main — it lives on branch `plan/gust-safety-release-line`
   (commit a612b01); §v0.5.0 was read from there.

## Fix wave

Applied 2026-07-16 in response to the adversarial review
(`.superpowers/sdd/wave-iso-core-review.md`, verdict FIX_REQUIRED). All 3
Important findings fixed (none scoped-away), plus the cheap minors M1/M4.

### What changed (all in `src/mpu_switch.rs` + regenerated plain mirror)

- **I1 — disable-first sequence.** `ProgramSeq` now emits
  `MPU_CTRL := MPU_CTRL_DISABLE` (0) as sequence element 0; `SEQ_LEN`
  9 → 10; region slot `r` moves to position `r + 1`; the enable stays
  last (position `MAX_REGIONS + 1`). P4 is now P4a (disable-first) +
  P4b (enable-last) in `program_partition`'s ensures — machine-checked,
  not narrated — so regions are rewritten only while the MPU is
  disabled and no transient mixed old/new map is ever enforced (ARM's
  recommended reprogramming discipline). Module header P4, `ProgramSeq`
  doc, and `apply_program` doc updated to match; the header also states
  that the switch window runs privileged under the default map (the
  review's point (d)).
- **I2 — barrier obligation in the seam contract.** `mpu_write`'s
  doc-comment now carries an explicit "Platform contract (trusted —
  load-bearing for I-ISO)" section: DSB followed by ISB after every
  `rnr == MPU_CTRL_ID` write (both disable and enable) before returning,
  with the consequence of omission spelled out. A platform implementing
  the contract to the letter now delivers the "physically denies" claim.
- **I3 — DREGION==8 platform precondition.** New documented const
  `REQUIRED_DREGION: u32 = 8`; the seam contract (item 2) and the module
  header state the init-time check obligation: read `MPU_TYPE.DREGION`,
  refuse to start unless it equals 8 — on a 16-region part (Cortex-M7 /
  i.MX RT1176) slots 8..=15 would be left stale, defeating P2.
  Parametrizing `MAX_REGIONS` over DREGION is the NAMED follow-on for
  16-region targets.
- **M1 — RASR field model documented.** Module header now documents
  XN=0 (every granted region executable ⇒ writable regions are W+X;
  an `executable` bit in the verified table-builder is the named
  follow-on before the security-containment demo) and TEX/C/B/S=0000
  (strongly-ordered).
- **M4 — `mpu_write` is now `pub(crate)`** (was `pub`), shrinking the
  raw-seam bypass surface.
- **k4 extended** (`iso_sequence_total_and_ordered`): now asserts
  disable-first (`w[0].rnr == MPU_CTRL_ID`, `w[0].rasr == 0`, ENABLE
  bit clear) in addition to totality and enable-last; k1–k3 re-indexed
  to the new positions (extents still decoded back out of the emitted
  RASR, independent of the table).

Not changed (per review, not required): M2 (ordering-at-hardware remains
code-inspection; `apply_program` doc now phrases it as in-order emission +
the barrier contract), M3 (top-32-bytes modeling restriction, deliberate
consistency with `validate_region`'s U-6 model), M5/M6 (follow-on scope,
already tracked above).

### Gate evidence (fix wave, fresh runs, with exit codes)

    $ bazel test //:verus_test --cache_test_results=no --test_output=errors
    verification results:: 1098 verified, 0 errors
    //:verus_test  PASSED in 5.6s
    exit 0            # 1098 >= 1096: +2 (P4a obligations), zero regressions

    $ cargo run --manifest-path tools/verus-strip/Cargo.toml -- src/mpu_switch.rs -o plain/src/mpu_switch.rs
    Wrote plain/src/mpu_switch.rs
    $ cargo test --manifest-path tools/verus-strip/Cargo.toml --test gate
    test result: ok. 2 passed; 0 failed   exit 0

    $ cargo kani --harness iso_deny_by_default             VERIFICATION:- SUCCESSFUL  exit 0
    $ cargo kani --harness iso_emitted_matches_table       VERIFICATION:- SUCCESSFUL  exit 0
    $ cargo kani --harness iso_emissions_disjoint          VERIFICATION:- SUCCESSFUL  exit 0
    $ cargo kani --harness iso_sequence_total_and_ordered  VERIFICATION:- SUCCESSFUL  exit 0

    $ cargo clippy --lib -- -D warnings    exit 0
    $ cargo test                           all suites ok, 0 failed

Honesty: still zero Verus-level `assume`/`admit`; the same 3
`kani::assume` harness-input constraints only; no ensures weakened —
P4 got STRONGER (P4a added); `emit_write` still carries no ensures.

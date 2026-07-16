# Wave report — verified I-ISO region-table builder (wave050)

Date: 2026-07-16. Branch: `feat/gust-iso-table-builder` (off main `6669b7d`).
Deliverable: extend the merged I-ISO core (`src/mpu_switch.rs`) with the
VERIFIED table-builder API — the "cover the partition" completeness item
deferred in #188's review. [REQ-OS-ISO-001]

## What was built

All inside `verus!{}` in `src/mpu_switch.rs`, scalar/table-free (no trait
objects, closures, heap, or async — the intersection discipline):

- `RegionTable::try_add_region(&mut self, part, base, size, writable) -> bool`
  — the constructor path. Machine-checked ensures:
  - **B1 (invariant preservation, the load-bearing one):** the resulting
    table satisfies `table_inv` — every enabled region well-formed
    (`region_wf`) AND same-partition enabled regions pairwise disjoint —
    on EVERY path, accepted or rejected. A caller building exclusively
    through `new()` + `try_add_region` cannot construct an
    isolation-violating table, and `table_inv` is exactly
    `program_partition`'s table precondition, so the merged core's
    deny-by-default (P2) + disjointness proofs compose on any
    builder-constructed table by construction (`new()` establishes the
    invariant, `try_add_region` preserves it — inductive over any build
    sequence).
  - **B2 (rejected ⇒ unchanged):** `!ok ==> *self == *old(self)`.
  - **B3 (accepted ⇒ well-formed + granted):** `ok ==> region_wf(base,
    size) && self.covers(part, base)` (and `part` in range).
  - **B4 (accepted ⇒ first-free-slot + frame):** an `exists` witness —
    the region landed in partition `part`'s FIRST free slot (every
    earlier slot was already occupied in `old(self)`), with base / size /
    writable stored exactly and every OTHER table slot untouched.
  - Reject reasons (all proven to leave the table unchanged):
    out-of-range `part` (defensive — the stripped exec builder is total),
    size not a power-of-two >= 32 (reuses the verified
    `crate::mpu::is_power_of_two`; same characterisation
    `crate::mpu::validate_region` enforces), base not size-aligned,
    `base + size` wrap (the U-6 bound, via `checked_add`), OVERLAP with
    an enabled region already granted to `part` (the isolation-bearing
    check, Gate 2), or partition full.
- `RegionTable::covers(part, addr)` (spec) + `slot_contains(i, addr)`
  (spec): the address-level grant predicate — some enabled region of
  `part` contains `addr` over [base, base+size).
- `RegionTable::covers_addr(part, addr) -> bool` (exec): verified mirror,
  `ensures b == self.covers(part, addr)` — the runtime/Kani-checkable
  form of `covers` post-strip.
- `lemma_covers_unique` (proof): on ANY `table_inv` table, at most one
  enabled region of a partition contains a given address — the grant is
  deterministic (the ARMv7-M PMSA ambiguous-overlap case can never arise
  within a builder-constructed partition).
- `lemma_same_partition_block` (proof, private): Euclidean-division
  bridge — `table_inv`'s `same_partition` guard (flat-index division) is
  exactly the builder's Gate-2 scan range `slot_of(part, 0..MAX_REGIONS)`
  (via `vstd::arithmetic::div_mod::lemma_remainder`).
- Kani harnesses (3, `#[cfg(kani)] mod builder_kani`, over `kani::any` +
  assumed exec-form `table_inv` — `validate_region` per enabled slot +
  pairwise same-partition range-disjointness over ALL 32 slots):
  - **kb1** `builder_preserves_table_inv`: any `table_inv` table, any
    request (including out-of-range `part`) → result still satisfies
    exec-form `table_inv`.
  - **kb2** `builder_reject_leaves_table_unchanged`: `ok == false` →
    all four arrays byte-identical to the snapshot (element-wise — array
    `==` lowers to a 128-byte memcmp needing a larger unwind bound).
  - **kb3** `builder_added_region_covered_exclusive`: fresh table + any
    `validate_region`-valid request MUST be accepted, and the added
    region is `covers_addr`-reachable at `base` and `base + size - 1`
    but NOT at `base + size` (exclusive upper bound; fresh table ⇒ no
    adjacent region can mask the probe).
- Mirror: `plain/src/mpu_switch.rs` regenerated via `tools/verus-strip`
  (never hand-edited). No BUILD/lib wiring changes needed — the module
  was already in `VERUS_SRCS`, both plain convergence lists, and the
  gate FILES list from #188.

## Gates (fresh, exit-checked, on the final source)

Baseline before the change:

    verification results:: 1152 verified, 0 errors

`bazel test //:verus_test --test_output=all --cache_test_results=no`:

    verification results:: 1159 verified, 0 errors
    //:verus_test                                                            PASSED in 3.4s

(count ROSE 1152 → 1159, +7.)

`cargo test --manifest-path tools/verus-strip/Cargo.toml --test gate`:

    test plain_standalone_matches_stripped_standalone ... ok
    test plain_matches_stripped_src ... ok
    test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out

`cargo kani --harness builder_preserves_table_inv` (re-run after the
final kb2 edit, so the result is against the shipped source):

    VERIFICATION:- SUCCESSFUL
    Verification Time: 281.85385s
    Complete - 1 successfully verified harnesses, 0 failures, 1 total.

`cargo kani --harness builder_reject_leaves_table_unchanged`:

    VERIFICATION:- SUCCESSFUL
    Verification Time: 44.62167s
    Complete - 1 successfully verified harnesses, 0 failures, 1 total.

`cargo kani --harness builder_added_region_covered_exclusive`:

    VERIFICATION:- SUCCESSFUL
    Verification Time: 1.7711575s
    Complete - 1 successfully verified harnesses, 0 failures, 1 total.

Also run: `cargo build` clean, `cargo clippy --lib` clean (exit 0),
`cargo test --lib` ok.

## Design decisions

1. **Requires only `table_inv`, not `part < MAX_PARTITIONS`.** The
   builder rejects an out-of-range partition id instead of requiring it
   away, so the post-strip exec function is total (no panic on any
   input) and the Kani harnesses can drive it with a fully arbitrary
   `part`.
2. **Alignment via `%`, not bit-masking.** `region_wf` states alignment
   as `base % size == 0`; the exec check uses the same form (size >= 32
   > 0 at that point), so the proof needs no `by(bit_vector)` bridge.
   Semantically identical to `validate_region`'s `base & (size-1) == 0`
   for power-of-two sizes — kb1 checks the actual exec equivalence
   mechanically against `validate_region`.
3. **Two scans, one insertion.** Gate 2 (overlap scan over `part`'s 8
   slots) carries a loop invariant accumulating disjointness against
   every enabled grant; Gate 3 (first-free scan) carries that result
   plus "all earlier slots occupied", so at the single mutation point
   the whole `table_inv` re-establishment is local: pairs not involving
   the new slot inherit from `old(self).table_inv()`, pairs involving it
   reduce to Gate 2's result via `lemma_same_partition_block`.
4. **`covers` split into spec + verified exec mirror** so the grant
   predicate is usable in ensures (spec) AND runnable/Kani-checkable
   post-strip (exec, proven `b == covers`).

## Concerns / follow-ons

- `//:fmt_test` fails LOCALLY on this machine on ~30 `plain/` files
  (cbprintf, condvar, executor, ...); the same failing FILE set reproduces
  on a pristine main checkout, so it is a local rustfmt-version style
  mismatch, not a new gate regression. CORRECTION (per wave050 review): the
  failing set DOES include `plain/src/mpu_switch.rs`, which this change
  touches — the newly generated builder/harness code adds six rustfmt
  divergence hunks (verus-strip pretty-printer spacing artifacts, e.g.
  `assert!(! t.covers_addr...)`). This is immaterial to correctness — no CI
  workflow runs `//:fmt_test`, and `plain/` is machine-generated and cannot
  be rustfmt-formatted without breaking the byte-exact verus-strip gate — but
  the earlier "none of them touched by this change" wording was wrong. The
  authoritative plain-convergence gate (verus-strip gate 2/2) passes. The
  `! t.` spacing is verus-strip pretty-printer friction (follow-on).
- kb1 is the heaviest harness (~5 min): the exec-form `table_inv`
  assume/assert spans all 32 slots (224 same-partition pairs each way).
  Acceptable for nightly `cargo kani --tests`; if CI time matters it can
  be split per-partition without weakening (the builder only writes one
  partition).
- XN/executable bit remains the named follow-on (`try_add_region` would
  grow an `executable` parameter) — unchanged from #188's scope.

# ordeal-bv — certificate-checked discharge of `by(bit_vector)` ASIL-D obligations (gale#173)

gale's Verus modules discharge **54 `by(bit_vector)` leaf obligations** (pure QF_BV) via
**unchecked Z3**, and `artifacts/safety_case.yaml` cites "Verus SMT/Z3" as ASIL-D evidence.
This directory re-discharges those obligations through **ordeal** — a certificate-checked
QF_BV solver whose LRAT checker is **machine-proven sound in Lean 4** — so the evidence
becomes an independently re-checkable certificate ("the solver is untrusted; only the
proven checker is trusted", CompCert-style).

## Pilot 1 — `cpu_mask.rs` power-of-two obligation

`src/cpu_mask.rs:179`, `by(bit_vector)`:

> given `cpu_id < 32` and `mask == 1u32 << cpu_id`, then `mask` is one of the 32 powers
> of two `{1, 2, …, 2^31}`.

`cpu_mask_pot.smt2` encodes the **negation** (premises ∧ `mask` is *not* any power of two)
in SMT-LIB2 QF_BV; ordeal returns **`unsat`** ⇒ the lemma holds.

**Result (ordeal 0.9.1, `ordeal check`):**
- `cpu_mask_pot.smt2` → **`unsat`**, **28,210-byte checker-validated LRAT certificate**.
- Discrimination sanity (encoding is not vacuously unsat): a satisfiable instance
  (`cpu_id=0 → mask=1`) returns **`sat`** with a self-checked model; the unreachable value
  `mask == 3` returns `unsat` (374-byte cert), confirming the `1<<cpu_id` reachability
  constraint bites.

The UNSAT verdict is only returned *after* ordeal's Lean-proven `ordeal-lrat` checker
validates the certificate; the certificate is portable and re-checkable with zero trust in
the solver (`cert.recheck()`).

## Pilot 2 — `mpu.rs` power-of-two obligation (this commit)

`src/mpu.rs:98`, `is_power_of_two`, `by(bit_vector)`. Unlike pilot 1 (a single
implication `premise ⇒ pow2`), this obligation is a **biconditional** — so it splits into
**two** directional QF_BV obligations, both re-discharged:

> for all `n: u32`:  `(n > 0 ∧ n & (n-1) == 0)  ⟺  n ∈ {1, 2, …, 2^31}`

- `mpu_pow2_fwd.smt2` — **forward** `idiom ⇒ enumeration`: refute `idiom ∧ ¬enumeration`.
- `mpu_pow2_bwd.smt2` — **backward** `enumeration ⇒ idiom`: refute `enumeration ∧ ¬idiom`.

Each side is expressed as implicitly-conjoined top-level `assert`s (ordeal 0.9.1's parser
takes no boolean `and`/`define-fun`; multiple asserts are the conjunction, and each
direction of the `⟺` is one refutation).

**Result (ordeal 0.9.1, `ordeal check`):**
- `mpu_pow2_fwd.smt2` → **`unsat`**, **15,193-byte checker-validated LRAT certificate**.
- `mpu_pow2_bwd.smt2` → **`unsat`**, **45,156-byte checker-validated LRAT certificate**.
- Discrimination sanity (`mpu_pow2_fwd_mutant.smt2`, the encoding is not vacuously unsat):
  the forward obligation with the `bv2` enumeration term **removed** returns **`sat`** with
  model **`n = 0x00000002`** — `2` satisfies the idiom yet is absent from the reduced
  enumeration, so ordeal correctly exhibits the counterexample. A vacuous/broken checker
  returning `unsat` here would be caught.

Both directions of the `⟺` hold over the full `u32` domain, independently re-checkable.

## Pilot 3 — `spinlock_validate.rs` owner encode/decode round-trip (this commit)

`src/spinlock_validate.rs`, the SV4/SV5 obligations the module's own proof notes flag as
needing `by(bit_vector)`: the spinlock owner word packs `(cpu, thread)` as `owner = thread |
cpu` (with `cpu < MAX_CPUS = 4` and `thread` aligned so its low 2 bits are free), and the
decode must **losslessly recover both** — the real concurrency-safety property (a corrupted
owner would mis-attribute a lock).

Two directions, both certificate-checked (implicitly-conjoined asserts; ordeal 0.9.1 takes no
boolean `and`/`define-fun`):

- `sv_cpu_recover.smt2` — **SV4**: `(thread | cpu) & 3 == cpu`. Refute the negation under the
  premises. → **`unsat`**, **1,064-byte LRAT**.
- `sv_thread_recover.smt2` — **SV5**: `(thread | cpu) & 0xFFFFFFFC == thread`. →
  **`unsat`**, **22,082-byte LRAT**.
- Discrimination sanity (`sv_cpu_recover_mutant.smt2`, non-vacuous): drop the
  **thread-alignment** premise and CPU recovery becomes falsifiable — ordeal returns
  **`sat`** with model `cpu=0, thread=2` (an unaligned thread's low bit corrupts `owner&3`),
  confirming the alignment premise is load-bearing.

Both round-trip directions hold over the full 32-bit domain, independently re-checkable.

## Pilot 4 — `fault_decode.rs` CFSR sub-register partition (this commit)

`src/fault_decode.rs:663-666`, `lemma_cfsr_masks_partition`: the three Cortex-M
fault-status sub-register masks — `MMFSR_MASK = 0x000000FF` (MemManage, bits 0-7),
`BFSR_MASK = 0x0000FF00` (BusFault, bits 8-15), `UFSR_MASK = 0xFFFF0000` (UsageFault,
bits 16-31) — are **pairwise non-overlapping** and together **cover all 32 bits**. This is
the well-formedness backbone of the fault decode: it guarantees a `CFSR` word is split into
the three fault classes with no bit lost and no bit double-counted (the same decode that
attributes the `CFSR=0x00000082` MemManage evidence in the v0.5.0 I-ISO oracle).

- `cfsr_masks_partition.smt2` — the lemma verbatim: masks pinned to their source constants,
  refute the 4-conjunct conclusion (3× disjoint + cover `0xFFFFFFFF`). → **`unsat`**,
  **1,832-byte LRAT**.
- `cfsr_partition_lossless.smt2` — the operational **strengthening** over a free `cfsr`:
  for ANY fault word the three masked slices are pairwise disjoint AND reassemble to `cfsr`
  (`(cfsr&MMFSR) | (cfsr&BFSR) | (cfsr&UFSR) == cfsr`). Implies the four constant conjuncts
  (take `cfsr = 0xFFFFFFFF`). → **`unsat`** + LRAT.
- Discrimination sanity (`cfsr_partition_mutant.smt2`, non-vacuous): drop bit 31 from UFSR
  (`0xFFFF0000 → 0x7FFF0000`) so coverage fails — ordeal returns **`sat`** with model
  `cfsr = 0x80000000` (bit 31 set but covered by no slice), the exact counterexample a
  vacuous checker would miss.

**4 of 54** obligation-sites now piloted (cpu_mask, mpu, spinlock_validate, fault_decode).

## Reproduce

    cargo install ordeal        # the published binary crate (crates.io)
    ./run.sh                     # or: ORDEAL=/path/to/ordeal ./run.sh

## Transcription gap → real-VC (obligation-proof)

These pilots hand-transcribe each `by(bit_vector)` leaf into `.smt2`, so today's
certificates prove **the transcription**, not the exact VC Verus checked. ordeal's
**Verus-VC bridge** (FEAT-009 / #65) ingests the by(bit_vector) VC Verus itself emits
to Z3 (let-bindings + Verus's bitvector idioms), closing that gap. It is **merged on
ordeal `main`** and verified locally to discharge gale's real cpu_mask VC (unsat,
28 250-byte LRAT), but is **not yet on crates.io** (0.11.0 is the latest publish, one
commit before #65). When ordeal **0.12.0** publishes, `cargo install ordeal` carries it
and each pilot upgrades from transcription-proof to obligation-proof. Still open upstream:
an automatic slicer to lift the BV sub-query out of a Verus log (widening to the ~64 leaves).

## Scope / next

- Boundary: only the **leaf** `by(bit_vector)` lemmas are QF_BV. The 805 top-level
  `forall/exists` properties need quantifiers and stay on Verus/Rocq/Lean.
- Full automated sweep of the 54 obligations is gated on the Verus-VC bridge shipping to
  crates.io (ordeal **0.12.0**) + the log slicer, so the VCs Verus emits are ingested
  automatically per leaf.
- On sweep: bind the certs as rivet `VER-BV-ORDEAL-001` + flip the BV-leaf evidence in
  `artifacts/safety_case.yaml` from "unchecked Z3" to "ordeal LRAT cert (re-checkable)".

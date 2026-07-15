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

## Reproduce

    # build ordeal >= 0.9.1 (pulseengine/ordeal): cargo build --release --bin ordeal
    ORDEAL=/path/to/ordeal ./run.sh

## Scope / next

- Boundary: only the **leaf** `by(bit_vector)` lemmas are QF_BV. The 805 top-level
  `forall/exists` properties need quantifiers and stay on Verus/Rocq/Lean.
- Full sweep of the 54 obligations is gated on ordeal's **SMT-LIB2 Verus-VC bridge**
  (ordeal#65, v0.11.0) so the VCs Verus already emits to Z3 are ingested automatically.
- On sweep: bind the certs as rivet `VER-BV-ORDEAL-001` + flip the BV-leaf evidence in
  `artifacts/safety_case.yaml` from "unchecked Z3" to "ordeal LRAT cert (re-checkable)".

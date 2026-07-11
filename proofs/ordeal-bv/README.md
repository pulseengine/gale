# ordeal-bv — certificate-checked discharge of `by(bit_vector)` ASIL-D obligations (gale#173)

gale's Verus modules discharge **54 `by(bit_vector)` leaf obligations** (pure QF_BV) via
**unchecked Z3**, and `artifacts/safety_case.yaml` cites "Verus SMT/Z3" as ASIL-D evidence.
This directory re-discharges those obligations through **ordeal** — a certificate-checked
QF_BV solver whose LRAT checker is **machine-proven sound in Lean 4** — so the evidence
becomes an independently re-checkable certificate ("the solver is untrusted; only the
proven checker is trusted", CompCert-style).

## Pilot — `cpu_mask.rs` power-of-two obligation (this commit)

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

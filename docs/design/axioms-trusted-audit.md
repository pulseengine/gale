# Axiom audit from outside the file: Rocq and Lean (design note)

Status: proposed, follow-up lane. No axioms gate exists in this repo. L11
(rounds 1-10) showed that any audit which derives the theorem list by lexing
the audited file, or which runs inside it, can be forged by that file. The
file controls its own grammar: its tactics, notations and elaboration
options. REQ-HONESTY-AXIOMS-001 stays `proposed` and this lane owns it.

## Design

1. **Rocq: `coqchk` per library.** For each compiled `.vo`, run
   `coqchk -o -Q <dir> <Lib> <Lib.Module>`. It re-checks the library in the
   kernel and prints the axioms and theory flags of the whole library
   (`-o`: "Print the axioms"). The list is taken from the compiled object,
   never from the source text. Compare it with an allowlist per library.
   `Set is impredicative` is a theory item.
2. **Lean: a trusted program.** It calls `importModules` on each module's
   `.olean` (with the same Mathlib oleans) and replays every constant through
   the kernel, as lean4checker does. For each of the module's constants it
   runs `collectAxioms`.
3. **Output.** Escaped or JSON names, one record per constant (Lean) or per
   library (Rocq), plus a count, so names like `«#x] [»` or names containing
   U+00A0 cannot break the parser.
4. **Twice-compile tolerance.** Bazel builds each Lean file twice (library
   and test). Audit the library artifact once, or accept identical duplicates.
5. **Recorded logs.** A recorded log is trusted input; say so. Prefer running
   the checker, not parsing a build log.

## Test suite: bypass classes from L11 (each must exit 1; controls exit 0)

- **Rocq.**
  - r3: a `Module` + `Import` shadow after the theorem.
  - r4: `Time`/`Timeout` prefixes; a theorem in a Module with a clean namesake.
  - r5-6: namesakes of every kind (Definition, Fixpoint `with`, `@{}`
    universes, Profile, Combined Scheme, Derive Inversion, Scheme Equality,
    constructors, fields); prelude names inside modules or functors.
  - r7: a later Stdlib `Require` shadowing (`le_lt_dec`, `plus_n_O`).
  - r10:
    - a mid-proof `pose I. Proof.` re-attributing the Qed to `I`;
    - `Ltac Function x := idtac.` then `Function zz_clean.` inside a dirty proof;
    - a U+00A0 inside a theorem name.
  - Controls: Section `Variable`/`Hypothesis`; a Prop inductive with a
    `_rec`-named lemma; a Stdlib `Require` between clean lemmas.
- **Lean.**
  - r1-3: a `macro_rules` override of `#print axioms`; printing via
    `#eval`/`IO.println`/`trace`; a raw string `r"\"`.
  - r4-8: namespace and scope tricks (LA-LD, L1-L3); namesakes; `id?` and
    `recOn!`; guillemet names (G1-G10; ML1/ML2 across a newline);
    `open … in` / `set_option … in` (DP1/DP2).
  - r9: `prefix:max` over the audit code; `#guard_msgs (drop info)`;
    `axiom «#x] [»`; `set_option debug.skipKernelTC true`; `Elab.async`
    private theorems.
- **Two Lows to cover (round 10).**
  - A `Quit`/`Drop` after a forged block is invisible when the gate only reads
    a recorded log (`--module`): the file stops before the real output, and
    the log looks complete.
  - A Prop-valued `Definition x : P := …` is skipped silently by a
    source-derived theorem list. A `coqchk` library audit covers it.

## Plant locations (scratch: copy into the lane's fixtures)

- Rocq r10: `/Volumes/Home/tmp/gale-lane-scratch/l11-r10/` (plants/, run.sh,
  README.txt: a1_pose_proof, a1b_pose_I, a1c_classic, a1d_admitted,
  a2c_ltac_function, a2d_tacnot_before, a3_nbsp_name, ctl_*).
- Rounds 4-8: `/Volumes/Home/tmp/gale-lane-scratch/l11-r{4..8}/plants/`
  (ax*.py, p1-p5.py, h.py; real coqc/lean outputs in real/, r5/, r6/).
- Lean r9: the L11 coordinator's session scratchpad `lean/` and `r9/`
  (PA-PE, KC, KN, Q1).

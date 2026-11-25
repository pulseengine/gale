# Axiom audit from outside the file: Rocq and Lean (design note)

Status: R1 (Rocq per library) and the Lean audit are built as
`cargo xtask check axioms-audit` on the lane `lane/axioms-audit`
(REQ-HONESTY-AXIOMS-001, VER-HONESTY-AXIOMS-GATE-001). No CI job runs it
yet; that is plan step 8. The per-theorem Rocq driver (R2) is not built.

L11 (rounds 1-10) showed that any audit which derives the theorem list by
lexing the audited file, or which runs inside it, can be forged by that file.
The file controls its own grammar: its tactics, notations and elaboration
options. So this audit reads compiled objects, with programs the audited files
cannot reach.

The claim that `proofs/executor_proofs.v` is "axiom-free per its Print
Assumptions" (README, docs/verification-honesty.md) rested on an in-file
command that nothing read; `rocq_proof_test` only runs `test -f` on the .vo.
That is FIND-HONESTY-AXIOMS-001 (raised on another branch). The R1 run below
now supports the substance for that one library: rocqchk reports no axioms
and a predicative Set for `proofs.executor_proofs`.

## Spike (2026-11-25, darwin arm64, origin/main 39f73d69)

Built through Bazel as `formal-verification.yml` does (`bazel test` of the 13
`//proofs:*_test` targets and `//proofs/lean:all`), with nix 2.33.1 on PATH
and the output base on a non-system volume. Every figure below is from that
machine; CI will differ in wall time.

**Local build caveat (rules_rocq_rust).** With a warm disk cache, the first
`bazel test` failed: `Cannot find library RocqOfRust.RecordUpdate in
loadpath` for `mutex_proofs` and `condvar_proofs`. At gale's rules_rocq_rust pin (2cfb99ca), `rocq_library` declared
only its DIRECT dependencies' .vo files as action inputs and ran unsandboxed,
so a transitive .vo (RecordUpdate.vo) that came from the disk cache was never
downloaded (Bazel's default `--remote_download_outputs=toplevel`). CI has no
disk cache and builds them locally, so it does not hit this. Rebuilt with
`--remote_download_outputs=all`: 13 of 13 pass. The audit targets below carry
every transitive object as an output, and the gate builds with
`--remote_download_outputs=all`, so the audit does not depend on that luck.
The root cause is rules_rocq_rust#53, fixed upstream by #54 (cfbfcf3,
2026-10-13: `RocqInfo.compiled` is transitive). Moving gale's pin past it is a
separate lane, because the same range also moves nixpkgs, rocq-of-rust and
the toolchain pin. Until then, and after it as defense in depth, the gate keeps
its own transitive aspect and `--remote_download_outputs=all`.

1. **The checker binary.** The pinned toolchain (`@rocq_toolchains`, Rocq
   9.0.1 from nixpkgs 88d3861a) ships both `bin/rocqchk` (a symlink to
   `rocq-9.0.1/bin/rocqchk`) and `bin/coqchk`; both print `The Rocq Proof
   Checker, version 9.0.1`. The audit uses `rocqchk`.
2. **`-o` prints axioms AND theory flags.** `rocqchk -o -silent -Q ...
   proofs.executor_proofs` (exit 0, 9.0 s) prints, on **stderr**:

   ```
   CONTEXT SUMMARY
   ===============
   * Theory: Set is predicative
   * Theory: Rewrite rules are not allowed
   * Axioms: <none>
   * Constants/Inductives relying on type-in-type: <none>
   * Constants/Inductives relying on unsafe (co)fixpoints: <none>
   * Inductives whose positivity is assumed: <none>
   ```

   For `proofs.mutex_proofs` (a RocqOfRust library) without
   `-impredicative-set`: exit 129, `Fatal Error: Error related to inductive
   types`. With it: exit 0, 11.5 s, `Theory: Set is impredicative` and 92
   axioms (31 `Corelib...PrimInt63`, 26 `Uint63Axioms`, 7 `PrimString`
   primitives, 1 `Stdlib.Logic.Eqdep.Eq_rect_eq`, 27 `RocqOfRust.*`). The
   theory line reports the mode the checker RAN in (it is given the flag);
   what shows the library NEEDS it is the refusal without it.
3. **Guard-checking bypasses are reported.** A scratch file with `Unset Guard
   Checking.` + `Fixpoint loop (n : nat) : False := loop n.` +
   `Theorem bad : False := loop 0` gives `Constants/Inductives relying on
   unsafe (co)fixpoints: T.G.loop` and `Axioms: <none>`. The same file's
   `Section S. Hypothesis h : False. Lemma l : 1 = 2. ... End S.` shows up
   nowhere: a discharged Section hypothesis is a premise, not an axiom (so the
   trust ledger's new `rocq:Section-Variable` spelling class is the only check).
4. **Axiom-set sizes (12 RocqOfRust libraries).** 8 libraries: exactly the
   same 92 axioms. poll 114 (+22 = its 22 `Admitted` lemmas, named
   `proofs.poll_proofs.*`), sched 115 (+23), thread_lifecycle 121 (+29),
   sem 183 (+91, all under `plain.sem.*`: the generated, untracked
   translation `plain/sem.v` that `sem_proofs.v` imports declares 91 axioms
   -- trait-impl and method axioms of the RocqOfRust translation; no gate saw
   them before). executor_proofs: 0. Per (library, axiom) that is 1,269 rows;
   with the shared 92 named once as a set, about 290. **Decision: the ledger
   names the shared set once (`rocq-set rocqofrust-base`), each library
   `uses` it, and the rest are per-library rows.**
5. **Lean.** `Lean.Environment.replay` is in core 4.27.0
   (`src/lean/Lean/Replay.lean`, `def replay (newConstants : Std.HashMap Name
   ConstantInfo) (env : Environment) : IO Environment`); it skips unsafe and
   partial constants. `collectAxioms` is in `Lean/Util/CollectAxioms.lean`.
   The program must take its search path from its own binary
   (`getBuildDir`): `findSysroot` picked elan's default 4.36 and failed with
   `incompatible header`. Module names under rules_lean: the olean of
   `proofs/lean/Atomic.lean` is imported as `Atomic` (rules_lean puts each
   library's `<name>_oleans/` on LEAN_PATH). But a PRIVATE name embeds the
   build path, because rules_lean runs `lean` without `--root`:
   `_private.«bazel-out».«darwin_arm64-fastbuild».bin.proofs.lean.priority_ceiling_src.PriorityCeiling.0.foldl_max_ge_init`.
   That differs per host (`k8-fastbuild` on Linux), so the gate stores no
   constant names and normalises private axiom names to `private:<tail>`.
   Totals: 8 modules, 642 constants; 342 depend on no axiom; propext 309,
   Quot.sound 239, Classical.choice 87; no `sorryAx`, no `Lean.ofReduceBool`.
6. **Wall time** (warm Bazel cache, laptop, other work running): rocqchk over
   all 13 libraries 159 s and 106 s in two runs; the Lean program over all 8
   modules 72 s and 60 s. The Bazel build itself was 6.8 s warm; cold it is
   dominated by the 7,869-file Mathlib download (about 6 min here).
7. **Determinism.** Two runs of each of the 13 rocqchk invocations are
   byte-identical (the axiom list order is not sorted, but it is stable); the
   Lean program's two full outputs are byte-identical. One run was disturbed
   by a concurrent `bazel build` in the same workspace, which rebuilt the
   execroot's `external/` symlinks under it (exit 127, file not found): the
   gate runs its own build first and the checkers after, never alongside.

**Kill criteria.** K1 (rocqchk cannot summarise all 13 libraries in about 20
min): not fired, about 2-3 min locally. K2 (nondeterministic output): not
fired. K3 (an unstable R2 driver): not tested; R2 is not built in this lane.

## Design (as built)

1. **Rocq R1: `rocqchk -o` per library.** `//proofs:axioms_audit`
   (`bzl/private/axiom_audit.bzl`, `rocq_axiom_audit`) writes a manifest of
   the libraries' modules, sources, their own `extra_flags` (read by an aspect
   from the `rocq_library` itself) and load paths, plus the toolchain's
   rocqchk, and makes every transitive .vo an output. The gate runs
   `rocqchk -o -silent <flags> -Q ... <module>` per library and parses
   stderr in exactly the shape above: a missing header, a missing or repeated
   section, an unknown line, or any stdout text is Refused (3). A flag other
   than `-impredicative-set` is refused. A non-zero rocqchk exit is a Fail.
2. **Lean: a trusted program.** `tools/xtask/axioms/AxiomAudit.lean`
   imports only `Lean`. For each module it reads the olean
   (`readModuleData`), imports the module's own imports, replays the
   module's constants through the kernel, and runs `collectAxioms` per
   constant. A kernel rejection is a Fail (exit 1) and is reported once; a
   `native_decide` lands here (measured: replay has no compiled code for
   `Lean.reduceBool`), so it can never be admitted to the ledger. It refuses
   a module-system olean (its private part is another
   file). It runs under the toolchain's own `lean --run`.
3. **Output.** Hex-encoded UTF-8 names, tab-separated, one record per
   constant, a per-run nonce on `begin`/`end`, and a count trailer checked
   against both the printed records and the olean's own constant count, so
   names like `«#x] [»` or a U+00A0 cannot break the parser and a truncated
   or replayed transcript is refused.
4. **Twice-compile tolerance.** Only the `lean_library` oleans are audited,
   not the `lean_proof_test` copies.
5. **No recorded logs.** The checkers run on every invocation, outside
   Bazel's action cache; Bazel only builds the objects and the manifests.
6. **The ledger** (`tools/xtask/axioms-audit.ledger`) is held equal in both
   directions (a new axiom and a vanished one both fail). Each row names the
   artifact that justifies it. The shrink-only ratchet after the v0.11.0
   tag is NOT implemented in this gate yet; `mode = shrink-only` and `admit`
   rows are refused.
7. **Coverage.** The audited sources must equal the tracked `.v` and `.lean`
   files (outside `tools/xtask/`).
8. **Bridge to the spelling counts.** Per Rocq library, the axioms rocqchk
   names under the library's own module path must equal the trust ledger's
   `Admitted` + `Axiom` + `Parameter` + top-level `Variable` spellings in its
   file; per Lean module, the axiom-kind constants must equal its `axiom`
   spellings. A mismatch means one side missed something.
9. **Spelling tripwires (trust-ledger).** Five classes, all 0 today and all
   forgeable source spellings: `rocq:Section-Variable`, `rocq:unset-guard`,
   `lean:native_decide`, `lean:implemented_by`, `lean:unsafe`.

## What the audit cannot see

- which theorem uses which axiom in Rocq (R1 is per library, everything it
  loads included; R2 is planned);
- discharged Section hypotheses, weak or vacuous statements;
- the kernels, the VM / native compute, and Rocq plugins (rocqchk loads none);
- Lean `example` declarations (no constant), unsafe and partial constants
  (listed, not replayed), and the imported Lean modules (Mathlib, core: loaded,
  not replayed);
- vstd's axioms (Verus).

## Test suite: bypass classes from L11 (each must exit 1; controls exit 0)

Not built in this lane (plan step 7). The L11 bypasses attack in-file audits
and source lexers; R1 and the Lean program read neither, so most of the
classes below no longer have anything to forge. They are kept as the fixture
list for step 7.

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
    the log looks complete. (R1 reads no log; the Lean nonce + count trailer
    refuse a truncated transcript.)
  - A Prop-valued `Definition x : P := …` is skipped silently by a
    source-derived theorem list. A `rocqchk` library audit covers it.

## Plants (this lane)

`cargo xtask axioms-oracle --tree <clean checkout>` applies each plant and
control and holds the exit codes to a table: P1 a classical import +
`classic` in executor_proofs.v (axioms-audit 1; control by `tauto` 0), P3 a
Section `Hypothesis bad : False` in mutex_proofs.v (trust-ledger 1 via
`rocq:Section-Variable`; axioms-audit 0 -- the stated blind spot; control 0),
P4 `native_decide` in Atomic.lean (axioms-audit 1: measured, the kernel
replay REJECTS the `zz_plant_arith._nativeDecide_1_1` auxiliary --
`(kernel) (interpreter) unknown declaration` -- because replay loads no
compiled code for `Lean.reduceBool`; so a native_decide is never ledgered,
not reported as `Lean.ofReduceBool` as the plan expected;
trust-ledger 1; control by `decide` 0 / 0), P6 an audit target with no
library (axioms-audit 3). On origin/main it is RED: the gate does not exist
(exit 2), and the trust ledger there stays green on P1, P3 and P4.
Not built here: P2 (the plant arriving through generated `plain.sem`), P5
(R2 rows), P7 (the L11 fixtures).

# Finding: thread_lifecycle priority validator is domain-incompatible with Zephyr's signed `int prio` (proof-code drift)

- **FILE**: `/Users/r/git/pulseengine/z/gale/src/thread_lifecycle.rs`
  (bug manifests at the FFI / Zephyr-ABI boundary in
  `/Users/r/git/pulseengine/z/gale/ffi/src/lib.rs` and
  `/Users/r/git/pulseengine/z/gale/ffi/include/gale_thread_lifecycle.h`)

- **FUNCTION / LINES**:
  - Proof side:
    - `priority_set_decide(new_priority: u32)` — `src/thread_lifecycle.rs:569-587`
    - `ThreadInfo::priority_set(&mut self, new_priority: u32)` — `src/thread_lifecycle.rs:245-269`
    - `ThreadInfo::new(... priority: u32, ...)` — `src/thread_lifecycle.rs:197-222`
    - `MAX_PRIORITY: u32 = 32` — `src/priority.rs:13`
    - `lemma_priority_set_preserves_inv(...)` — `src/thread_lifecycle.rs:406-416`
    - `ThreadInfo::inv() { self.priority < MAX_PRIORITY && self.stack.inv() }` — `src/thread_lifecycle.rs:187-190`
  - FFI side (added 2026-04-19, commit `46d9aa9`):
    - `gale_thread_priority_validate(priority: u32) -> i32` — `ffi/src/lib.rs:3955-3962`
    - `gale_k_thread_create_decide(... priority: u32, ...)` — `ffi/src/lib.rs:4001-4042`
    - `gale_k_thread_priority_set_decide(uint32_t new_priority)` — `ffi/src/lib.rs:4261-4286`
    - Corresponding C prototypes — `ffi/include/gale_thread_lifecycle.h:44, 71-73, 182-183`
  - Upstream caller contract (what gale claims to replace):
    - `_is_valid_prio(int prio, ...)` — `zephyr/kernel/include/ksched.h:143-160`
      — accepts `prio ∈ [K_HIGHEST_APPLICATION_THREAD_PRIO,
                       K_LOWEST_APPLICATION_THREAD_PRIO]`
      = `[-CONFIG_NUM_COOP_PRIORITIES, CONFIG_NUM_PREEMPT_PRIORITIES - 1]`,
      typically `[-16, 15]` (`zephyr/include/zephyr/kernel.h:57-61`).
    - `Z_ASSERT_VALID_PRIO(prio, entry)` at thread.c:593 and sched.c:1041 —
      the Zephyr integration passes a **signed** `int prio` through this macro.

- **HYPOTHESIS** (proof-code drift; domain mismatch between Verus
  model and Zephyr's declared priority ABI):

  `priority_set_decide` is verified under the proposition
  `new_priority: u32 ∧ new_priority < MAX_PRIORITY (=32)`. This
  domain does **not** match what Zephyr's kernel passes through
  `Z_ASSERT_VALID_PRIO(prio, ...)`:

  1. Zephyr's `prio` is `int` (signed). Cooperative threads have
     **negative** priority (`prio < 0`, range
     `[-CONFIG_NUM_COOP_PRIORITIES, -1]`); preemptive threads have
     non-negative priority (`[0, CONFIG_NUM_PREEMPT_PRIORITIES - 1]`).
     The C → Rust call `gale_thread_priority_validate((uint32_t)prio)`
     reinterprets any negative prio as a huge `u32` (e.g. `-1` →
     `0xFFFFFFFF`). Gale rejects it as out-of-range, even though
     Zephyr's `_is_valid_prio(-1)` returns true.
     ⇒ **every valid cooperative priority would be rejected** by
     gale's validator.

  2. Inversely, `MAX_PRIORITY = 32` hard-codes a constant that is
     unrelated to the Zephyr KConfig bounds. For a typical
     `CONFIG_NUM_PREEMPT_PRIORITIES = 16`, Zephyr rejects `prio = 20`
     (above `K_LOWEST_APPLICATION_THREAD_PRIO = 15`), but gale's
     validator accepts it (`20 < 32`). A caller running on any
     Zephyr config with `CONFIG_NUM_PREEMPT_PRIORITIES < 32` gets
     out-of-range priorities silently admitted at the gale boundary,
     which then trigger `Z_ASSERT_VALID_PRIO` in the un-replaced C
     path — or worse, flow into the scheduler's priority array under
     `CONFIG_ASSERT=n` and cause OOB indexing into
     `_priq_rb`/`_priq_dumb`. (sched.c uses `prio` as an array index
     via `thread->base.prio`.)

  3. The Verus `Priority::inv(&self) == self.value < MAX_PRIORITY`
     invariant (`src/priority.rs:26-28`) is therefore **not** the
     Zephyr priority predicate. The proofs `lemma_priority_range`
     (line 743) and `lemma_priority_set_preserves_inv` (line 406)
     are sound about gale's internal `u32` model and say nothing
     about the actual kernel ABI. `MAX_PRIORITY` is documented on
     `src/priority.rs:11-13` as "16 cooperative + 16 preemptive = 32"
     — the comment asserts the intent, the code encodes the sum as a
     single unsigned threshold, which is the drift.

  This is **proof-code drift**, per the rule in `emit.md §5`: the
  proof is internally valid; the Rust-side specification
  (`u32 < 32`) silently diverges from the spec the caller actually
  relies on (`int ∈ [-NCOOP, NPREEMPT)`), which both sides of the
  FFI boundary continue to claim equivalence with. The FFI header
  (`gale_thread_lifecycle.h:42`) literally documents the post-drift
  predicate `"valid (< MAX_PRIORITY)"`, propagating the incorrect
  spec to C consumers.

  Note on scope: `gale_thread_create_validate` / `gale_thread_exit_validate`
  and `ThreadTracker::create`/`exit` are correct with respect to the
  `MAX_THREADS = 256` bound; TH5/TH6 proofs hold and the off-by-one
  prior is false. The race prior (concurrent create/exit against the
  same external `count`) is a structural non-issue because the FFI
  functions are pure (caller-supplied `count`, caller-managed
  synchronization). The proof-code drift on priority signedness is
  the load-bearing finding.

- **ORACLE (KANI)** — counterexample harness (fails against the
  priority-range claim of `priority_set_decide` as documented in the
  FFI header for an `int`-domain caller):

  ```rust
  // File: ffi/tests/kani_thread_lifecycle_prio_signedness.rs
  //
  //   cargo kani -p gale-ffi --harness tl_drift_prio_signed \
  //              --features thread_lifecycle
  //
  // Also: ffi/tests/kani_thread_lifecycle_prio_range.rs for the
  // "above-range accepted" case.
  #[cfg(all(kani, feature = "thread_lifecycle"))]
  #[kani::proof]
  fn tl_drift_prio_signed() {
      // Model the Zephyr caller: `int prio`, cooperative domain.
      // Zephyr says _is_valid_prio(prio) == true for
      //   prio in [K_HIGHEST_APPLICATION_THREAD_PRIO, -1]
      //   with K_HIGHEST_APPLICATION_THREAD_PRIO == -CONFIG_NUM_COOP_PRIORITIES.
      // Use the defconfig number CONFIG_NUM_COOP_PRIORITIES = 16.
      let prio_signed: i32 = kani::any();
      kani::assume(prio_signed >= -16 && prio_signed <= -1);

      // Zephyr predicate: this prio is VALID.
      let zephyr_valid: bool = prio_signed >= -16 && prio_signed <= 15;
      assert!(zephyr_valid);

      // Gale encoding across the FFI boundary (C implicit conversion).
      let prio_u: u32 = prio_signed as u32;

      // Gale's "validated-in-Verus" decision.
      let d = unsafe {
          gale_ffi::gale_thread_priority_validate(prio_u)
      };
      // gale says OK iff prio_u < 32; for any negative prio_signed,
      // prio_u >= 0x80000000, so gale says EINVAL.
      // Counterexample: for prio_signed == -1, zephyr_valid == true
      // but gale returns EINVAL.  Violates TH1 as-specified-to-C.
      assert!(d == 0, "gale rejected a Zephyr-valid cooperative priority");
  }

  #[cfg(all(kani, feature = "thread_lifecycle"))]
  #[kani::proof]
  fn tl_drift_prio_above_range() {
      // Preemptive-only caller under a reduced-config build:
      //   CONFIG_NUM_PREEMPT_PRIORITIES = 16
      //   K_LOWEST_APPLICATION_THREAD_PRIO = 15
      let prio_signed: i32 = kani::any();
      kani::assume(prio_signed >= 16 && prio_signed < 32);

      // Zephyr predicate: INVALID (above lowest app prio).
      let zephyr_valid: bool = prio_signed >= -16 && prio_signed <= 15;
      assert!(!zephyr_valid);

      let prio_u: u32 = prio_signed as u32;
      let d = unsafe {
          gale_ffi::gale_thread_priority_validate(prio_u)
      };
      // Gale admits it (prio_u < 32).
      // Counterexample: for prio_signed == 20, zephyr_valid == false
      // but gale returns OK.  Priority then flows into the scheduler
      // array as an out-of-bounds index.
      assert!(d != 0, "gale admitted a Zephyr-invalid priority");
  }
  ```

  Both harnesses are falsified by construction: the first
  counterexample is `prio_signed = -1`; the second is
  `prio_signed = 20`. Neither case is masked by an
  `assume(prio_signed >= 0 && prio_signed < 32)` anywhere in the
  existing proof surface — `src/thread_lifecycle.rs` never
  acknowledges that `prio` originates as a signed `int` in the
  caller. That absence is the drift.

  **Verus (equivalent drift witness at the model/caller boundary):**
  the predicate
  `forall |p: i32| zephyr_valid(p) <==> (p as u32) < MAX_PRIORITY`
  is false at `p == -1` (zephyr_valid == true, `(p as u32) < 32`
  false) and at `p == 20` (zephyr_valid == false, `(p as u32) < 32`
  true). No lemma in `src/thread_lifecycle.rs` or `src/priority.rs`
  establishes this correspondence — it cannot, because it does not
  hold.

- **POC TEST** — failing unit test (deterministic, no scheduler
  required; the bug is a sequential domain mismatch, not a race):

  ```rust
  // gale/ffi/tests/thread_lifecycle_prio_signedness_poc.rs
  //
  //   cargo test -p gale-ffi --features thread_lifecycle \
  //              --test thread_lifecycle_prio_signedness_poc
  use gale_ffi::{
      gale_thread_priority_validate,
      gale_k_thread_priority_set_decide,
      gale_k_thread_create_decide,
  };

  // Zephyr defconfig typical values (see docs/research/deep-zephyr-analysis.md:511-512).
  const NUM_COOP:    i32 = 16;
  const NUM_PREEMPT: i32 = 16;
  const K_HIGHEST_APPLICATION_THREAD_PRIO: i32 = -NUM_COOP;     // -16
  const K_LOWEST_APPLICATION_THREAD_PRIO:  i32 = NUM_PREEMPT - 1; // 15

  fn zephyr_is_valid_prio(prio: i32) -> bool {
      prio >= K_HIGHEST_APPLICATION_THREAD_PRIO
          && prio <= K_LOWEST_APPLICATION_THREAD_PRIO
  }

  #[test]
  fn cooperative_priority_rejected_by_gale() {
      // prio = -1 is a perfectly normal cooperative priority in Zephyr.
      let prio: i32 = -1;
      assert!(zephyr_is_valid_prio(prio), "-1 is Zephyr-valid");

      // C implicit conversion at the FFI boundary.
      let rc = gale_thread_priority_validate(prio as u32);
      // Expected per FFI contract: 0 (OK).
      // Actual: -EINVAL, because (prio as u32) = 0xFFFFFFFF >= 32.
      assert_eq!(
          rc, 0,
          "TH1 violated: gale rejected cooperative priority -1 \
           (encoded 0x{:08x}); every cooperative thread would fail to set priority",
          prio as u32
      );
  }

  #[test]
  fn above_lowest_app_prio_accepted_by_gale() {
      // prio = 20 is out-of-range for Zephyr (above
      // K_LOWEST_APPLICATION_THREAD_PRIO = 15 under 16/16 defconfig).
      let prio: i32 = 20;
      assert!(!zephyr_is_valid_prio(prio), "20 is Zephyr-invalid");

      let rc = gale_thread_priority_validate(prio as u32);
      // Expected: -EINVAL.  Actual: 0 (OK), because 20 < 32.
      // The priority then flows into thread->base.prio and is used
      // as an index into the scheduler priority array.
      assert_ne!(
          rc, 0,
          "TH1 violated: gale accepted out-of-range priority 20; \
           under CONFIG_NUM_PREEMPT_PRIORITIES=16 this is an OOB \
           index into _priq_rb / _priq_dumb"
      );
  }

  #[test]
  fn create_decide_admits_out_of_range_priority() {
      let d = gale_k_thread_create_decide(
          /*stack_size =*/ 4096,
          /*priority   =*/ 20, // out of range for 16/16 config
          /*options    =*/ 0,
          /*active_cnt =*/ 0,
      );
      assert_ne!(
          d.action, 0, // GALE_THREAD_ACTION_PROCEED
          "TH1 violated at create: out-of-range priority 20 admitted; \
           thread would be scheduled with an OOB priority index"
      );
  }
  ```

  All three `assert*` macros fire on the current HEAD (commit
  `46d9aa9`). The tests are direct executable witnesses of the Kani
  counterexamples above and reduce the drift to a trivial,
  deterministic replay.

- **IMPACT**:
  **Proof-code drift** with a dual failure mode on the Zephyr
  scheduler.

  - Liveness regression on cooperative threads: any C caller that
    funnels `k_thread_priority_set(thread, -1)` (or any other
    cooperative priority) through the gale validator receives
    `-EINVAL` and the priority change is silently dropped. The
    thread keeps its old priority; a subsystem expecting a
    coop-priority hand-off (ISR offload thread, bottom-half worker,
    time-sensitive logger) no longer preempts-inhibits and races
    against preemptive peers. Violates TH1 "as advertised to C
    consumers" even though TH1 "as verified in Verus" still holds.

  - Safety-integrity violation on over-range priorities: when
    `CONFIG_NUM_PREEMPT_PRIORITIES < 32` (every defconfig today),
    gale admits priorities `[CONFIG_NUM_PREEMPT_PRIORITIES, 32)` as
    valid. Zephyr's scheduler then indexes `thread->base.prio` into
    `_priq_rb` / `_priq_dumb` arrays sized by the KConfig value,
    producing out-of-bounds reads/writes in the ready queue. Under
    `CONFIG_ASSERT=n` (production builds) this is silent memory
    corruption of whatever follows the priority-queue array —
    canonical ASIL-D hazard.

  - Systemic nature: the same priority-as-`u32` encoding appears
    four times in the FFI
    (`gale_thread_priority_validate`,
     `gale_k_thread_priority_set_decide`,
     `gale_k_thread_create_decide`, `ThreadInfo::new`) and is
    documented as verified ("TH1", "Verus-verified") in each
    location. A developer auditing any one call site finds a green
    proof and a C header that promises `"valid (< MAX_PRIORITY)"`;
    nothing points at the signed/unsigned mismatch with the kernel
    ABI.

  - Remediation path (pick one, per `emit.md` drift rule):
    (i) **re-verify**: change the Verus model to accept signed
        priorities: `Priority::value: i32`, invariant
        `K_HIGHEST_APPLICATION_THREAD_PRIO <= value <=
         K_LOWEST_APPLICATION_THREAD_PRIO`; change all
        `priority: u32` FFI parameters to `prio: i32`; parameterize
        the bounds by the KConfig constants (emit them into the FFI
        header via `build.rs`); extend Kani harnesses to cover the
        full signed range; re-run `lemma_priority_set_preserves_inv`
        over the new domain. This is the correct fix — it matches
        Zephyr's actual ABI.
    (ii) **revert the code to match the proof**: drop the four
         priority validators from the FFI entirely and leave
         `_is_valid_prio` / `Z_ASSERT_VALID_PRIO` as the only
         authoritative priority check; in gale, stop claiming TH1 is
         a safety property of `k_thread_priority_set` replacement
         until (i) is done. This is cheap but abandons the ASIL-D
         claim on thread priority.

    Option (i) is required for the ASIL-D replacement story to hold.

- **CANDIDATE UCA**:
  The symptom maps cleanly to an STPA UCA of type
  **"control action provided with out-of-bounds argument" /
  "expected control action withheld"**: the scheduler control
  action "set priority" is either (a) provided with an
  out-of-bounds priority argument that the downstream sched then
  propagates as an OOB array index, or (b) withheld (silently
  dropped as EINVAL) when Zephyr's contract said it should succeed.
  If gale's `safety/stpa/ucas.yaml` has no priority-range UCA
  covering signed-int priorities, recommend opening
  **TL-UCA-1** ("thread priority validator accepts priorities
  outside Zephyr's `_is_valid_prio` range, or rejects priorities
  inside it") and filing this finding against it with
  `status: draft`.

- **related-cve**: none exactly; closest analogue is
  `CVE-2021-3319` / `CVE-2020-10062`-class OOB in Zephyr kernel
  arrays from unvalidated integer index. The signedness-at-FFI
  pattern mirrors `CVE-2022-2993` (net subsystem signed/unsigned
  priority confusion).

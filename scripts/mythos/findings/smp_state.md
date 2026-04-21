# Finding: SMP model tracks only a count — SM3 "CPU 0 never stops" is unenforceable, and SM4 verifies a fabricated global lock counter that does not exist in Zephyr

- **FILE**: `/Users/r/git/pulseengine/z/gale/src/smp_state.rs`
  (drift manifests at the FFI boundary in
  `/Users/r/git/pulseengine/z/gale/ffi/src/lib.rs:5099-5176` and the
  C shim `/Users/r/git/pulseengine/z/gale/zephyr/gale_smp_state.c:33-78`.
  Upstream reference: `/Users/r/git/pulseengine/z/zephyr/kernel/smp.c:57-194`.)

- **FUNCTIONS / LINES**:
  - Proof side, defect D1 (SM3):
    `SmpState::stop_cpu` (`src/smp_state.rs:129-155`) and the standalone
    `stop_cpu_decide(active_cpus: u32)` (lines 360-378). Neither takes
    a cpu id. The doc comment on line 29 claims
    `SM3: stop_cpu when active > 1: active -= 1 (CPU 0 never stops)`
    but the actual `ensures` clause (lines 136-144) only proves
    `old(self).active_cpus > 1 ==> self.active_cpus == old.active_cpus - 1`.
    Nothing is said about which cpu is stopped; the abstraction has
    no identity.
  - Proof side, defect D2 (SM4):
    `global_lock` / `global_unlock` (lines 183-229) operate on a
    single `u32` field `global_lock_count` held in `SmpState`
    (line 51). Upstream `z_smp_global_lock` (`zephyr/kernel/smp.c:57-70`)
    stores the counter **per thread** (`_current->base.global_lock_count++`)
    and arbitrates shared ownership through a separate
    `atomic_t global_lock` (smp.c:38). The `SmpState` field models
    neither.
  - FFI side: `gale_smp_stop_cpu_decide` (`ffi/src/lib.rs:5160-5176`) is
    a thin delegate to the model's `stop_cpu_decide(active_cpus)`;
    the C shim `gale_smp_cpu_stop_checked(int id)` (zephyr/gale_smp_state.c:54-73)
    accepts a cpu id parameter and then **completely ignores it** —
    `id` never reaches Rust and is not even read in C. The
    corresponding `gale_smp_cpu_start_checked(int id, unsigned int max_cpus)`
    likewise ignores `id`.

- **HYPOTHESIS** (proof-code drift, twofold):

  **D1 — "CPU 0 never stops" is a documentation claim the proof does
  not establish.**
  The STPA-GAP-2 audit (`docs/safety/stpa-gap2-audit.md:290-297`) marks
  `gale_smp_stop_cpu_decide` RED. The Rust file's ASIL-D property list
  (lines 26-30) ships the property as proved. It is not. Because the
  abstract state is `active_cpus: u32` without a cpu id set, a
  caller that invokes `gale_smp_cpu_stop_checked(id=0)` while
  `gale_active_cpus == 2` is accepted: Rust returns
  `STOP_OK, new_active=1`; the shim updates `gale_active_cpus = 1`; no
  transition in the model can detect that cpu 0 — the boot cpu that
  owns the kernel timer, the dummy thread, and the
  first-to-respond-to-IPI role in `smp.c` — was the one asked to
  power down. The invariant `active_cpus >= 1` still holds, so every
  Verus ensures passes, even in the failing execution.

  The C shim's lock (`gale_smp_lock`, zephyr/gale_smp_state.c:31) serialises
  the decide/apply pair but locks nothing outside gale: Zephyr's
  real cpu-online transitions go through `cpu_start_lock`
  (smp.c:55), which gale does not observe. So the "CPU-count
  decision race" prior is present a second way — `gale_active_cpus`
  can drift from Zephyr's actual powered-up cpu set if any
  `k_smp_cpu_start` / `k_smp_cpu_resume` call is routed around
  `gale_smp_cpu_start_checked`. There is no interlock that forces
  the two to stay aligned. The differential tests
  (`tests/differential_smp_state.rs`) treat the shim as a pure
  replica of the model and therefore cannot catch the divergence.

  **D2 — SM4 verifies a counter that does not exist.**
  `z_smp_global_lock` in Zephyr increments
  `_current->base.global_lock_count`, a per-thread field, and uses
  `atomic_cas(&global_lock, 0, 1)` to arbitrate. The gale model
  attaches `global_lock_count` to `SmpState` (one counter shared
  across all cpus). `SM4`'s roundtrip lemma
  (`lemma_lock_unlock_roundtrip`, lines 312-319) is internally
  sound but operates on a fabricated object: the proof witnesses
  neither the atomic nor the thread-local counter. Additionally,
  the model's `global_unlock` returns `EINVAL` when
  `global_lock_count == 0` (lines 215-218); upstream
  `z_smp_global_unlock` unconditionally calls `arch_irq_unlock(key)`
  on the irq key the caller must pass in (smp.c:74-83) — and the
  model drops the key entirely. A "verified" unlock in the model
  therefore has no obligation to restore interrupt state, which is
  the entire reason Zephyr's unlock exists. No FFI entry point
  currently exposes `global_lock` / `global_unlock`, which is the
  only reason the drift is latent today; the model advertises these
  as ASIL-D verified for a future exporter.

- **ORACLE (VERUS)** — counterexample at the model level:

  Add the following ensures to `stop_cpu_decide` (lines 360-378).
  The STPA property "CPU 0 never stops" (source header line 29)
  translates to "the identity of the stopped cpu is not 0". The
  current signature has no parameter to express this; introducing
  it makes the abstraction gap visible:

  ```rust
  // REQUIRES an id-set representation. Minimal fix: track a bitset.
  pub open spec fn inv(&self) -> bool {
      &&& self.max_cpus > 0
      &&& self.max_cpus <= MAX_CPUS
      &&& self.active_set & 1 == 1            // NEW: cpu 0 always in set
      &&& popcount(self.active_set) as u32 == self.active_cpus
      &&& self.active_set < (1u32 << self.max_cpus)
  }

  pub fn stop_cpu(&mut self, cpu_id: u32) -> (rc: i32)
      requires old(self).inv(), cpu_id < old(self).max_cpus,
      ensures
          self.inv(),
          rc == OK ==> cpu_id != 0,            // NEW: SM3 (real)
          rc == OK ==> self.active_set == old(self).active_set & !(1u32 << cpu_id),
  ```

  Verus rejects the current code against this spec: with
  `active_cpus = 2, cpu_id = 0`, the body executes `active_cpus -= 1`
  and returns `OK`, but `cpu_id != 0` is violated. That is the
  exact SM3 counterexample. The existing `lemma_cpu0_never_stops`
  (lines 306-309) is vacuously true — it assumes
  `active == 1u32`, which is a different situation (last-cpu
  rejection), not the actual SM3 claim about cpu 0.

  **Kani equivalent** (FFI surface):

  ```rust
  // ffi/tests/kani_smp_stop_cpu0.rs
  #[cfg(all(kani, feature = "smp_state"))]
  #[kani::proof]
  fn smp_stop_cpu0_rejected() {
      let active: u32 = kani::any();
      kani::assume(active >= 2 && active <= 16);
      // The shim's id parameter is the only channel by which the
      // caller conveys "which cpu to stop".  Today it is dropped.
      let id: i32 = 0;
      let d = gale_smp_stop_cpu_decide(active);
      // Fails: d.action == STOP_OK whenever active > 1, regardless
      // of id.  SM3 requires id != 0 when action == STOP_OK.
      if d.action == 0 /* STOP_OK */ { assert!(id != 0); }
  }
  ```

- **POC TEST** — failing unit test (deterministic):

  ```rust
  // tests/smp_stop_cpu0_drift.rs
  //   cargo test -p gale --test smp_stop_cpu0_drift
  use gale::smp_state::stop_cpu_decide;

  #[test]
  fn sm3_cpu0_stop_is_not_actually_rejected() {
      // Initial reality: 2 cpus up (cpu0 + cpu1).  Caller asks to
      // stop cpu 0.  Per the header doc (line 29) and STPA SM3 this
      // MUST be rejected.  The model has no way to know which cpu
      // was requested, so it accepts.
      let asked_cpu_id: u32 = 0;   // << the missing channel
      let r = stop_cpu_decide(/* active_cpus = */ 2);
      let accepted = r.is_ok();
      assert!(
          !(accepted && asked_cpu_id == 0),
          "SM3 drift: model accepted stop(cpu0) while active_cpus=2"
      );
  }

  #[test]
  fn sm4_model_count_is_not_zephyrs_counter() {
      // Model exposes a single counter; Zephyr keeps it per-thread.
      // Two threads each locking once in Zephyr produce
      // (tA.count=1, tB.count=1) and a single contended atomic.
      // The model collapses both into global_lock_count=2, which is
      // a state Zephyr never enters: the second lock call spins on
      // atomic_cas instead of incrementing.  The roundtrip lemma
      // proves nothing about the Zephyr state.
      use gale::smp_state::SmpState;
      let mut s = SmpState { max_cpus: 2, active_cpus: 2, global_lock_count: 0 };
      let _ = s.global_lock();  // thread A
      let _ = s.global_lock();  // thread B — Zephyr would spin here
      assert_eq!(s.global_lock_count, 2,
          "model happily reaches count=2; Zephyr's atomic forbids it");
  }
  ```

  Test 1 fires on HEAD: `stop_cpu_decide(2)` returns `Ok(1)`, so
  `accepted && asked_cpu_id == 0` is true. Test 2 passes (the model
  *does* reach count=2) — its purpose is to demonstrate the model
  admits states the real system cannot, which is the SM4 drift
  witness.

- **IMPACT**:
  **Proof-code drift** with ASIL-D consequences.
  - **SM3 (D1)**: the kernel boot cpu can be asked to stop via the
    verified interface; the model does not reject it. On any Zephyr
    port where cpu 0 owns the system timer or the interrupt
    aggregator, this is an availability loss (fail-silent for a
    fail-operational system). Priors matched: "CPU-count decision
    race" — the race is not only temporal but *identity*-level: the
    model carries no identity.
  - **SM4 (D2)**: the global-lock proof witnesses a counter layout
    that is not the one Zephyr uses, and silently omits the IRQ
    key. Today this is latent because no FFI function exports the
    model's lock/unlock; the hazard is that the safety case cites
    SM4 as verified.
  - **Drift vs `gale_k_sched_next_up_decide`** (STPA-GAP-2 audit RED
    row 305): the scheduler decides preemption assuming the active
    cpu set. Because `smp_state` exports only a count, the
    scheduler cannot condition on "cpu 0 still here" when making
    metairq / preempted-thread decisions. The two RED audit rows
    compound: smp_state loses identity, sched consumes the lossy
    abstraction.
  - Remediation (drift rule per `emit.md`, item 5):
    (i) **fix the code to match the proof's stated SM3**: add
        `cpu_id` to `stop_cpu` / `stop_cpu_decide` and track
        `active_set: u32` instead of a count; derive
        `active_cpus = popcount(active_set)`; enforce
        `cpu_id != 0` on stop. Thread the id through the C shim
        (`gale_smp_cpu_stop_checked` already has it, just needs to
        pass it). Re-prove SM3 with identity.
    (ii) **retract SM4 from the safety claim** until the model
         matches Zephyr: drop `global_lock` / `global_unlock` from
         `SmpState`, or re-model them as (per-thread counter +
         `atomic<bool>` + irq-key restore) with a proof that the
         decide-apply ordering matches `smp.c:57-83`.

- **CANDIDATE UCA**:
  STPA-GAP-2 audit (`docs/safety/stpa-gap2-audit.md:290-297`) already
  flags `gale_smp_*_decide` as RED. The most natural UCA umbrella is
  "SMP lifecycle control action executes while the abstract state
  does not represent per-cpu identity" (UCA-type 2: control action
  given when conditions not met). Recommend filing this as
  **SMP-UCA-3** ("stop_cpu is accepted with cpu_id = 0 while
  active_cpus > 1") under the SMP grouping, with `status: draft`.
  D2 warrants a sibling **SMP-UCA-4** ("global_lock/unlock claim to
  verify Zephyr's z_smp_global_lock but model a different object"),
  same status.

- **RELATED CVE**: none directly. Structurally closest is the
  `CVE-2023-5564`-class "abstract model admits states the
  concurrent primitive does not" (double-free-under-concurrency).

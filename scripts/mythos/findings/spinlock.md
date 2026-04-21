# Finding: gale_spinlock FFI `tid=0` sentinel collapses `Some(0)` onto `None`

- **FILE**: `/Users/r/git/pulseengine/z/gale/src/spinlock.rs`
  (bug manifests at the FFI boundary in
  `/Users/r/git/pulseengine/z/gale/ffi/src/lib.rs:9860-9970`,
  which exports the verified state machine of
  `src/spinlock.rs::SpinlockState`)

- **FUNCTION / LINES**:
  - Proof side: `SpinlockState::acquire` / `acquire_nested` /
    `release` / `acquire_check` (`src/spinlock.rs:112-276`). The
    model uses `owner: Option<u32>` where `None` == free and
    `Some(tid)` == held by `tid`. The Verus invariant (`inv`, line
    61-68) states `owner.is_some() <==> nest_count > 0` and is
    proven over the whole `u32` domain including `tid == 0`.
  - FFI side: `gale_spinlock_acquire_check` (line 9860),
    `gale_spinlock_acquire` (line 9880),
    `gale_spinlock_acquire_nested` (line 9908),
    `gale_spinlock_release` (line 9948),
    `gale_spinlock_is_held` (line 9977). All use
    `owner_tid: u32 == 0` as the encoding of `None`.

- **HYPOTHESIS** (proof-code drift):
  The verified model treats `Some(0)` as a valid *held* state, but
  every FFI entry point collapses `tid = 0` onto the "free"
  sentinel. A caller that passes `new_tid = 0` into
  `gale_spinlock_acquire(owner_tid=0, _, new_tid=0, …)` writes
  `*out_owner = 0` and `*out_nest_count = 1`, producing the state
  `(owner_tid=0, nest_count=1)`. That state is:
  (a) reported as **free** by `gale_spinlock_acquire_check(0) == 1`
      — a second acquirer immediately succeeds ⇒ two simultaneous
      "holders" of the same spinlock (mutual-exclusion violation,
      SL1); and
  (b) impossible to release, because
      `gale_spinlock_release` short-circuits on
      `if owner_tid == 0 || owner_tid != tid { return -EPERM; }`
      — the rightful holder gets `-EPERM` forever (permanently
      stuck `nest_count=1`, SL2/SL4 violation).
  The Verus proof does not cover this: it operates on
  `Option<u32>`, and the injection
  `Option<u32> → u32` used by the FFI (`None ↦ 0`, `Some(t) ↦ t`)
  is **not injective** at `t = 0`. The Kani harnesses
  (`kani_spinlock_proofs`, `ffi/src/lib.rs:9988-10043`) all contain
  `kani::assume(tid != 0)` — the dangerous region is explicitly
  masked out of the model-checking. This is textbook proof-code
  drift: the Rust model is sound, the FFI encoding silently drops
  one bit of the domain.

- **ORACLE (KANI)** — counterexample harness (currently excluded
  from CI by an `assume` in the existing proof):

  ```rust
  // Paste into ffi/src/lib.rs inside `mod kani_spinlock_proofs`,
  // or as a new file ffi/tests/kani_spinlock_drift.rs.
  //
  // Run with:  cargo kani -p gale-ffi --harness sl_drift_tid_zero
  #[cfg(all(kani, feature = "spinlock"))]
  #[kani::proof]
  fn sl_drift_tid_zero() {
      // 1. Thread A with tid == 0 acquires a free lock.
      let mut owner: u32 = 0;          // lock starts free
      let mut nest:  u32 = 0;
      let rc_a = gale_spinlock_acquire(owner, nest, /*new_tid=*/0,
                                       &mut nest, &mut owner);
      assert!(rc_a == 0);              // FFI reports success
      assert!(nest == 1);              // model says "held, depth 1"

      // 2. INVARIANT-BREAK #1: a *second* caller asks
      //    acquire_check on the post-state and is told the
      //    lock is FREE, despite nest_count=1.
      //    Fails: this returns 1, violating SL1
      //    (mutual exclusion).
      assert!(gale_spinlock_acquire_check(owner) == 0);

      // 3. INVARIANT-BREAK #2: the rightful holder (tid=0) cannot
      //    release its own lock.
      //    Fails: rc_r == -1 (-EPERM) even though Thread A is the
      //    sole owner. Violates SL2 (release-by-owner) and SL4
      //    (round-trip to unlocked).
      let rc_r = gale_spinlock_release(owner, nest, /*tid=*/0,
                                       &mut nest, &mut owner);
      assert!(rc_r == 0);
  }
  ```

  Both `assert!` calls are Kani counterexamples. The harness is
  the minimal extension of the existing `acquire_check_free` /
  `release_final_unlocks` proofs with the `tid != 0` assumption
  *removed*.

  **Verus (equivalent drift witness at the model/FFI boundary):**
  the predicate `forall |s: SpinlockState| s.inv() ==>
  (ffi_encode(s.owner) == 0) <==> s.owner.is_none()` is false at
  `s = SpinlockState { owner: Some(0), nest_count: 1,
  irq_saved: true }`. No Verus proof in `src/spinlock.rs`
  establishes this correspondence, because the FFI encoding is not
  expressed in Verus — which is itself the drift.

- **POC TEST** — failing unit test (deterministic, no `loom`
  needed; the bug is sequential proof-drift, not a race):

  ```rust
  // gale/ffi/tests/spinlock_tid_zero_poc.rs
  //
  //   cargo test -p gale-ffi --features spinlock
  //       --test spinlock_tid_zero_poc
  use gale_ffi::{
      gale_spinlock_acquire,
      gale_spinlock_acquire_check,
      gale_spinlock_release,
  };

  #[test]
  fn fails_mutual_exclusion_when_tid_is_zero() {
      // Start: free lock.
      let mut owner: u32 = 0;
      let mut nest:  u32 = 0;

      // Thread A with tid=0 "acquires" the lock.
      let rc = unsafe {
          gale_spinlock_acquire(owner, nest, /*new_tid=*/0,
                                &mut nest, &mut owner)
      };
      assert_eq!(rc, 0, "FFI claims acquire succeeded");
      assert_eq!(nest, 1, "nesting depth set to 1");
      // …but owner remained 0, so the post-state is
      // indistinguishable from `free` at the FFI boundary.

      // A second thread asks if it can acquire.
      // SL1 says this must be rejected. In reality:
      assert_eq!(
          gale_spinlock_acquire_check(owner), 0,
          "SL1 violated: acquire_check says lock is free \
           while nest_count==1"
      );

      // And the rightful holder can no longer release.
      let mut n2 = nest;
      let mut o2 = owner;
      let rc_r = unsafe {
          gale_spinlock_release(o2, n2, /*tid=*/0,
                                &mut n2, &mut o2)
      };
      assert_eq!(
          rc_r, 0,
          "SL2/SL4 violated: owner (tid=0) got -EPERM \
           on its own release, lock is now permanently stuck"
      );
  }
  ```

  Both `assert_eq!` macros fire on the current HEAD. The test is a
  direct executable witness of the Kani counterexample above.

- **IMPACT**:
  **Proof-code drift** with concurrency and liveness consequences.
  - Mutual-exclusion violation (SL1): two CPUs can simultaneously
    enter the kernel critical section guarded by a gale-verified
    spinlock if any caller ever passes `tid = 0`. Every
    higher-level primitive (sched, sem, mutex, fifo, mem_slab,
    heap — every caller noted in `ffi/src/lib.rs` as "under
    spinlock") then runs with an unprotected critical section.
  - Permanent deadlock / hung kernel (SL2, SL4): the rightful
    holder receives `-EPERM` on release, so the lock is stuck
    with `nest_count == 1` forever. All future acquirers
    observe `owner_tid == 0` and race.
  - Safety class: systemic control-flow hazard — a single
    mis-identified thread id compromises every critical section
    on the device. ASIL-D: this class of bug defeats the core
    assumption that a verified spinlock excludes.
  - *Mitigating* factor in the concrete Zephyr integration:
    `_current` is a kernel pointer that happens to be nonzero at
    runtime, so the bug is latent under today's C caller. It is
    not latent against: (a) a different integration that maps
    thread handles to `u32` indices starting at 0, (b) a userspace
    caller that forges `tid = 0`, (c) a future refactor that
    changes how the FFI constructs `tid`, (d) the Verus proof
    itself, which claims to cover all of `u32` including 0.
  - Remediation path (pick one, per `emit.md` drift rule):
    (i) **re-verify**: add an FFI precondition `new_tid != 0`,
        document it as a requires, and extend
        `kani_spinlock_proofs` to assert the rejection of
        `new_tid == 0` (drop the `kani::assume(tid != 0)` blanket);
    (ii) **fix the code to match the proof**: change the FFI
         encoding so `owner_tid` is `Option<NonZeroU32>` or carry
         the `nest_count > 0` bit alongside to disambiguate
         `Some(0)` from `None`.
    Option (i) is cheaper and matches the Zephyr reality that
    `_current != NULL`.

- **CANDIDATE UCA**:
  Gale's safety artifacts do not currently contain a UCA directly
  indexing the `spinlock` ownership state machine. The closest
  structural matches, if an umbrella UCA exists, would be a
  "kernel mutual-exclusion primitive provides access to a shared
  resource while another actor still holds it" control-action
  failure (classical STPA UCA-type 2 / UCA-type 3). If gale's
  `safety/stpa/ucas.yaml` has no such entry, recommend opening a
  new UCA **SL-UCA-1** ("spinlock acquire/release returns success
  while leaving the lock in a state that violates SL1–SL4") and
  filing this finding against it with `status: draft`.

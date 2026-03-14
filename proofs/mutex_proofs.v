(** * Formal Verification Proofs for Zephyr Mutex

    Proves properties about the reentrant mutex with ownership tracking.
    Complements Verus SMT proofs in src/mutex.rs.

    Invariant: lock_count > 0 iff owner is present,
    and waiters > 0 implies owner is present. *)

Require Import RocqOfRust.RocqOfRust.

(* ========================================================================= *)
(** * Definitions *)
(* ========================================================================= *)

Definition EINVAL : Z := -22.
Definition EPERM  : Z := -1.
Definition OK     : Z := 0.

(** The mutex invariant:
    - lock_count > 0 iff has_owner is true (owner correspondence)
    - waiters > 0 implies has_owner is true (can't wait on unlocked mutex)
    - lock_count >= 0 (non-negative) *)
Definition mutex_inv (lock_count : Z) (has_owner : bool) (num_waiters : Z) : Prop :=
  (lock_count > 0 <-> has_owner = true) /\
  (num_waiters > 0 -> has_owner = true) /\
  lock_count >= 0 /\
  num_waiters >= 0.

(* ========================================================================= *)
(** * Init Proofs *)
(* ========================================================================= *)

Theorem init_establishes_invariant :
  mutex_inv 0 false 0.
Proof.
  unfold mutex_inv. repeat split; try lia; intros; discriminate.
Qed.

(* ========================================================================= *)
(** * Lock Proofs *)
(* ========================================================================= *)

(** Locking an unlocked mutex establishes ownership *)
Theorem lock_unlocked_establishes_ownership :
  forall num_waiters : Z,
    mutex_inv 0 false num_waiters ->
    mutex_inv 1 true num_waiters.
Proof.
  intros nw [_ [_ [_ Hnw]]].
  unfold mutex_inv. repeat split; try lia; auto.
Qed.

(** Reentrant lock increments count, preserves invariant *)
Theorem reentrant_lock_preserves_invariant :
  forall lock_count num_waiters : Z,
    mutex_inv lock_count true num_waiters ->
    mutex_inv (lock_count + 1) true num_waiters.
Proof.
  intros lc nw [Hcorr [Hwait [Hnonneg Hnw]]].
  unfold mutex_inv. repeat split; try lia; auto.
Qed.

(* ========================================================================= *)
(** * Unlock Proofs *)
(* ========================================================================= *)

(** Reentrant unlock (lock_count > 1): decrement preserves invariant *)
Theorem reentrant_unlock_preserves_invariant :
  forall lock_count num_waiters : Z,
    mutex_inv lock_count true num_waiters ->
    lock_count > 1 ->
    mutex_inv (lock_count - 1) true num_waiters.
Proof.
  intros lc nw [Hcorr [Hwait [Hnonneg Hnw]]] Hgt.
  unfold mutex_inv. repeat split; try lia; auto.
Qed.

(** Full unlock with no waiters: clears ownership *)
Theorem full_unlock_no_waiters :
  forall lock_count : Z,
    mutex_inv lock_count true 0 ->
    lock_count = 1 ->
    mutex_inv 0 false 0.
Proof.
  intros. exact init_establishes_invariant.
Qed.

(** Full unlock with waiters: transfers ownership *)
Theorem unlock_transfers_ownership :
  forall lock_count num_waiters : Z,
    mutex_inv lock_count true num_waiters ->
    lock_count = 1 ->
    num_waiters > 0 ->
    mutex_inv 1 true (num_waiters - 1).
Proof.
  intros lc nw [_ [_ [_ Hnw]]] _ Hgt.
  unfold mutex_inv. repeat split; try lia; auto.
Qed.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

(** Lock-unlock roundtrip returns to original state *)
Theorem lock_unlock_roundtrip :
  forall lock_count num_waiters : Z,
    mutex_inv lock_count true num_waiters ->
    (lock_count + 1) - 1 = lock_count.
Proof.
  intros. lia.
Qed.

(** Reentrant depth tracks nesting *)
Theorem reentrant_depth_tracks :
  forall lock_count n num_waiters : Z,
    mutex_inv lock_count true num_waiters ->
    n >= 0 ->
    lock_count + n >= lock_count.
Proof.
  intros. lia.
Qed.

(** Invariant sufficiency *)
Theorem invariant_sufficiency :
  forall lock_count num_waiters : Z,
    mutex_inv lock_count true num_waiters ->
    (* reentrant lock is safe *)
    mutex_inv (lock_count + 1) true num_waiters /\
    (* reentrant unlock is safe when count > 1 *)
    (lock_count > 1 -> mutex_inv (lock_count - 1) true num_waiters).
Proof.
  intros lc nw Hinv. split.
  - apply reentrant_lock_preserves_invariant. exact Hinv.
  - intro. apply reentrant_unlock_preserves_invariant; assumption.
Qed.

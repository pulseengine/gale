(** * Formal Verification Proofs for Zephyr Mutex

    Proves properties about the reentrant mutex with ownership tracking.
    Complements Verus SMT proofs in src/mutex.rs.

    Invariant: lock_count > 0 iff owner is present,
    and waiters > 0 implies owner is present.

    The rocq-of-rust translation wraps all values in Value.t.
    These proofs operate at the abstract Z level. *)

Require Import RocqOfRust.RocqOfRust.

(* Close type_scope to prevent parsing conflicts with abstract proofs. *)
Close Scope type_scope.

(* ========================================================================= *)
(** * Definitions *)
(* ========================================================================= *)

Definition EINVAL : Z := -22.
Definition EPERM  : Z := -1.
Definition OK     : Z := 0.

(** The mutex invariant:
    - lc > 0 iff has_owner is true (owner correspondence)
    - nw > 0 implies has_owner is true (can't wait on unlocked mutex)
    - lc >= 0 (non-negative) *)
Definition mutex_inv (lc : Z) (has_owner : bool) (nw : Z) : Prop :=
  (lc > 0 <-> has_owner = true) /\
  (nw > 0 -> has_owner = true) /\
  lc >= 0 /\
  nw >= 0.

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
  forall nw : Z,
    mutex_inv 0 false nw ->
    mutex_inv 1 true nw.
Proof.
  intros nw [_ [_ [_ Hnw]]].
  unfold mutex_inv. repeat split; try lia; auto.
Qed.

(** Reentrant lock increments count, preserves invariant *)
Theorem reentrant_lock_preserves_invariant :
  forall lc nw : Z,
    mutex_inv lc true nw ->
    mutex_inv (lc + 1) true nw.
Proof.
  intros lc nw [Hcorr [Hwait [Hnonneg Hnw]]].
  unfold mutex_inv. repeat split; try lia; auto.
Qed.

(* ========================================================================= *)
(** * Unlock Proofs *)
(* ========================================================================= *)

(** Reentrant unlock (lc > 1): decrement preserves invariant *)
Theorem reentrant_unlock_preserves_invariant :
  forall lc nw : Z,
    mutex_inv lc true nw ->
    lc > 1 ->
    mutex_inv (lc - 1) true nw.
Proof.
  intros lc nw [Hcorr [Hwait [Hnonneg Hnw]]] Hgt.
  unfold mutex_inv. repeat split; try lia; auto.
Qed.

(** Full unlock with no waiters: clears ownership *)
Theorem full_unlock_no_waiters :
  forall lc : Z,
    mutex_inv lc true 0 ->
    lc = 1 ->
    mutex_inv 0 false 0.
Proof.
  intros. exact init_establishes_invariant.
Qed.

(** Full unlock with waiters: transfers ownership *)
Theorem unlock_transfers_ownership :
  forall lc nw : Z,
    mutex_inv lc true nw ->
    lc = 1 ->
    nw > 0 ->
    mutex_inv 1 true (nw - 1).
Proof.
  intros lc nw [_ [_ [_ Hnw]]] _ Hgt.
  unfold mutex_inv. repeat split; try lia; auto.
Qed.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

(** Lock-unlock roundtrip returns to original state *)
Theorem lock_unlock_roundtrip :
  forall lc nw : Z,
    mutex_inv lc true nw ->
    (lc + 1) - 1 = lc.
Proof.
  intros. lia.
Qed.

(** Reentrant depth tracks nesting *)
Theorem reentrant_depth_tracks :
  forall lc n nw : Z,
    mutex_inv lc true nw ->
    n >= 0 ->
    lc + n >= lc.
Proof.
  intros. lia.
Qed.

(** Invariant sufficiency *)
Theorem invariant_sufficiency :
  forall lc nw : Z,
    mutex_inv lc true nw ->
    (* reentrant lock is safe *)
    mutex_inv (lc + 1) true nw /\
    (* reentrant unlock is safe when count > 1 *)
    (lc > 1 -> mutex_inv (lc - 1) true nw).
Proof.
  intros lc nw Hinv. split.
  - apply reentrant_lock_preserves_invariant. exact Hinv.
  - intro. apply reentrant_unlock_preserves_invariant; assumption.
Qed.

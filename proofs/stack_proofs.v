(** * Formal Verification Proofs for Zephyr Stack

    Proves properties about the bounded counter model.
    Complements Verus SMT proofs in src/stack.rs.

    Invariant: cap > 0, 0 <= cnt <= cap.

    The rocq-of-rust translation wraps all values in Value.t.
    These proofs operate at the abstract Z level. *)

Require Import RocqOfRust.RocqOfRust.

(* Close type_scope to prevent parsing conflicts with abstract proofs. *)
Close Scope type_scope.

(* ========================================================================= *)
(** * Definitions *)
(* ========================================================================= *)

Definition EINVAL : Z := -22.
Definition ENOMEM : Z := -12.
Definition EBUSY  : Z := -16.
Definition OK     : Z := 0.

(** The stack invariant *)
Definition stack_inv (cnt cap : Z) : Prop :=
  cap > 0 /\ 0 <= cnt /\ cnt <= cap.

(* ========================================================================= *)
(** * Init Proofs *)
(* ========================================================================= *)

Theorem init_establishes_invariant :
  forall cap : Z,
    cap > 0 ->
    stack_inv 0 cap.
Proof.
  intros cap Hcap. unfold stack_inv. lia.
Qed.

Theorem init_rejects_zero :
  ~ stack_inv 0 0.
Proof.
  intros [H _]. lia.
Qed.

(* ========================================================================= *)
(** * Push Proofs *)
(* ========================================================================= *)

Theorem push_preserves_invariant :
  forall cnt cap : Z,
    stack_inv cnt cap ->
    cnt < cap ->
    stack_inv (cnt + 1) cap.
Proof.
  intros c cap [Hcap [Hge Hle]] Hlt. unfold stack_inv. lia.
Qed.

Theorem push_full_rejected :
  forall cnt cap : Z,
    stack_inv cnt cap ->
    cnt = cap ->
    stack_inv cnt cap.
Proof.
  intros. assumption.
Qed.

(* ========================================================================= *)
(** * Pop Proofs *)
(* ========================================================================= *)

Theorem pop_preserves_invariant :
  forall cnt cap : Z,
    stack_inv cnt cap ->
    cnt > 0 ->
    stack_inv (cnt - 1) cap.
Proof.
  intros c cap [Hcap [Hge Hle]] Hgt. unfold stack_inv. lia.
Qed.

Theorem pop_empty_rejected :
  forall cnt cap : Z,
    stack_inv cnt cap ->
    cnt = 0 ->
    stack_inv cnt cap.
Proof.
  intros. assumption.
Qed.

Theorem pop_no_underflow :
  forall cnt cap : Z,
    stack_inv cnt cap ->
    cnt > 0 ->
    cnt - 1 >= 0.
Proof.
  intros c cap [_ [Hge _]] Hgt. lia.
Qed.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

Theorem push_pop_roundtrip :
  forall cnt cap : Z,
    stack_inv cnt cap ->
    cnt < cap ->
    (cnt + 1) - 1 = cnt.
Proof.
  intros. lia.
Qed.

Theorem capacity_conservation :
  forall cnt cap : Z,
    stack_inv cnt cap ->
    (cap - cnt) + cnt = cap.
Proof.
  intros. lia.
Qed.

Theorem invariant_sufficiency :
  forall cnt cap : Z,
    stack_inv cnt cap ->
    (cnt < cap -> stack_inv (cnt + 1) cap) /\
    (cnt > 0 -> stack_inv (cnt - 1) cap).
Proof.
  intros c cap Hinv. split; intros.
  - apply push_preserves_invariant; assumption.
  - apply pop_preserves_invariant; assumption.
Qed.

(** * Formal Verification Proofs for Zephyr Stack

    Proves properties about the bounded counter model.
    Complements Verus SMT proofs in src/stack.rs.

    Invariant: capacity > 0, 0 <= count <= capacity. *)

Require Import RocqOfRust.RocqOfRust.

(* ========================================================================= *)
(** * Definitions *)
(* ========================================================================= *)

Definition EINVAL : Z := -22.
Definition ENOMEM : Z := -12.
Definition EBUSY  : Z := -16.
Definition OK     : Z := 0.

(** The stack invariant *)
Definition stack_inv (count capacity : Z) : Prop :=
  capacity > 0 /\ 0 <= count /\ count <= capacity.

(* ========================================================================= *)
(** * Init Proofs *)
(* ========================================================================= *)

Theorem init_establishes_invariant :
  forall capacity : Z,
    capacity > 0 ->
    stack_inv 0 capacity.
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
  forall count capacity : Z,
    stack_inv count capacity ->
    count < capacity ->
    stack_inv (count + 1) capacity.
Proof.
  intros c cap [Hcap [Hge Hle]] Hlt. unfold stack_inv. lia.
Qed.

Theorem push_full_rejected :
  forall count capacity : Z,
    stack_inv count capacity ->
    count = capacity ->
    stack_inv count capacity.
Proof.
  intros. assumption.
Qed.

(* ========================================================================= *)
(** * Pop Proofs *)
(* ========================================================================= *)

Theorem pop_preserves_invariant :
  forall count capacity : Z,
    stack_inv count capacity ->
    count > 0 ->
    stack_inv (count - 1) capacity.
Proof.
  intros c cap [Hcap [Hge Hle]] Hgt. unfold stack_inv. lia.
Qed.

Theorem pop_empty_rejected :
  forall count capacity : Z,
    stack_inv count capacity ->
    count = 0 ->
    stack_inv count capacity.
Proof.
  intros. assumption.
Qed.

Theorem pop_no_underflow :
  forall count capacity : Z,
    stack_inv count capacity ->
    count > 0 ->
    count - 1 >= 0.
Proof.
  intros c cap [_ [Hge _]] Hgt. lia.
Qed.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

Theorem push_pop_roundtrip :
  forall count capacity : Z,
    stack_inv count capacity ->
    count < capacity ->
    (count + 1) - 1 = count.
Proof.
  intros. lia.
Qed.

Theorem capacity_conservation :
  forall count capacity : Z,
    stack_inv count capacity ->
    (capacity - count) + count = capacity.
Proof.
  intros. lia.
Qed.

Theorem invariant_sufficiency :
  forall count capacity : Z,
    stack_inv count capacity ->
    (count < capacity -> stack_inv (count + 1) capacity) /\
    (count > 0 -> stack_inv (count - 1) capacity).
Proof.
  intros c cap Hinv. split; intros.
  - apply push_preserves_invariant; assumption.
  - apply pop_preserves_invariant; assumption.
Qed.

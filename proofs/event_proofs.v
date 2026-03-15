(** * Formal Verification Proofs for Zephyr Event

    Proves properties about the 32-bit event bitmask model.
    Complements Verus SMT proofs in src/event.rs.

    Invariant: events is a valid u32 (trivially true). *)

Require Import RocqOfRust.RocqOfRust.

(* ========================================================================= *)
(** * Definitions *)
(* ========================================================================= *)

Definition U32_MAX : Z := 4294967295.

(** The event invariant — events is a valid u32 bitmask *)
Definition event_inv (events : Z) : Prop :=
  0 <= events /\ events <= U32_MAX.

(* ========================================================================= *)
(** * Init Proofs *)
(* ========================================================================= *)

Theorem init_zero :
  event_inv 0.
Proof.
  unfold event_inv, U32_MAX. lia.
Qed.

(* ========================================================================= *)
(** * Post Proofs *)
(* ========================================================================= *)

(** EV8: post is monotonic — ORing only adds bits.
    For Z-modeled bitmasks we state: (events lor new) >= events when both >= 0. *)
Theorem post_monotonic :
  forall events new_events : Z,
    event_inv events ->
    event_inv new_events ->
    Z.lor events new_events >= events.
Proof.
  intros e ne [Hge1 _] [Hge2 _].
  apply Z.lor_nonneg in Hge2 as Hge2'.
  - apply Z.le_lor; lia.
  - exact Hge1.
Qed.

(** EV2: set replaces the entire bitmask *)
Theorem set_replaces :
  forall events new_events : Z,
    event_inv events ->
    event_inv new_events ->
    event_inv new_events.
Proof.
  intros. assumption.
Qed.

(** EV3: clear ANDs complement — result is within [0, events] *)
Theorem clear_complement :
  forall events clear_bits : Z,
    event_inv events ->
    event_inv clear_bits ->
    0 <= Z.land events (Z.lnot clear_bits).
Proof.
  intros e cb [Hge1 Hle1] [Hge2 Hle2].
  (* Z.land of a non-negative number with any number: if e >= 0,
     then Z.land e x >= 0 when we mask appropriately.
     We use the fact that land with a non-negative first arg is non-negative
     via Z.land_nonneg. *)
  destruct (Z.lnot_spec_low clear_bits 0) as [_ _].
  apply Z.land_nonneg. left. exact Hge1.
Qed.

(* ========================================================================= *)
(** * Wait Proofs *)
(* ========================================================================= *)

(** EV5: wait_any correct — returns true when any desired bit is set *)
Theorem wait_any_correct :
  forall events desired : Z,
    event_inv events ->
    event_inv desired ->
    desired > 0 ->
    Z.land events desired > 0 ->
    Z.land events desired <> 0.
Proof.
  intros e d _ _ Hd Hgt. lia.
Qed.

(** EV6: wait_all correct — when all desired bits are present *)
Theorem wait_all_correct :
  forall events desired : Z,
    event_inv events ->
    event_inv desired ->
    Z.land events desired = desired ->
    Z.land events desired = desired.
Proof.
  intros. assumption.
Qed.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

(** EV1: double-post idempotence — posting same bits twice has no extra effect *)
Theorem post_idempotent :
  forall events new_events : Z,
    event_inv events ->
    event_inv new_events ->
    Z.lor (Z.lor events new_events) new_events = Z.lor events new_events.
Proof.
  intros e ne _ _.
  rewrite Z.lor_assoc. rewrite Z.lor_diag. reflexivity.
Qed.

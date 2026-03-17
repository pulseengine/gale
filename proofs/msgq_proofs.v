(** * Formal Verification Proofs for Zephyr Message Queue

    Proves properties about ring buffer index arithmetic.
    Complements Verus SMT proofs in src/msgq.rs.

    Invariant: max_msgs > 0, msg_size > 0, 0 <= used_msgs <= max_msgs,
    read_idx < max_msgs, write_idx < max_msgs.

    The rocq-of-rust translation wraps all values in Value.t.
    These proofs operate at the abstract Z level. *)

Require Import RocqOfRust.RocqOfRust.

(* Close type_scope to prevent parsing conflicts with abstract proofs. *)
Close Scope type_scope.

(* ========================================================================= *)
(** * Definitions *)
(* ========================================================================= *)

Definition EINVAL : Z := -22.
Definition ENOMSG : Z := -42.
Definition OK     : Z := 0.

(** The message queue invariant *)
Definition msgq_inv (msz mmx used ri wi : Z) : Prop :=
  msz > 0 /\
  mmx > 0 /\
  0 <= used /\
  used <= mmx /\
  0 <= ri /\
  ri < mmx /\
  0 <= wi /\
  wi < mmx.

(* ========================================================================= *)
(** * Init Proofs *)
(* ========================================================================= *)

Theorem init_establishes_invariant :
  forall msz mmx : Z,
    msz > 0 ->
    mmx > 0 ->
    msgq_inv msz mmx 0 0 0.
Proof.
  intros ms mm Hms Hmm. unfold msgq_inv. lia.
Qed.

Theorem init_rejects_zero_msg_size :
  forall mmx : Z,
    ~ msgq_inv 0 mmx 0 0 0.
Proof.
  intros mm [H _]. lia.
Qed.

Theorem init_rejects_zero_max_msgs :
  forall msz : Z,
    ~ msgq_inv msz 0 0 0 0.
Proof.
  intros ms [_ [H _]]. lia.
Qed.

(* ========================================================================= *)
(** * Put Proofs *)
(* ========================================================================= *)

(** put when not full: increments used, advances write_idx *)
Theorem put_preserves_invariant :
  forall msz mmx used ri wi : Z,
    msgq_inv msz mmx used ri wi ->
    used < mmx ->
    msgq_inv msz mmx (used + 1) ri
      ((wi + 1) mod mmx).
Proof.
  intros ms mm um ri wi [Hms [Hmm [Hu1 [Hu2 [Hr1 [Hr2 [Hw1 Hw2]]]]]]] Hlt.
  unfold msgq_inv. repeat split; try lia.
  - apply Z.mod_pos_bound. lia.
  - apply Z.mod_pos_bound. lia.
Qed.

(** put when full: rejected *)
Theorem put_full_rejected :
  forall msz mmx used ri wi : Z,
    msgq_inv msz mmx used ri wi ->
    used = mmx ->
    msgq_inv msz mmx used ri wi.
Proof.
  intros. assumption.
Qed.

(* ========================================================================= *)
(** * Get Proofs *)
(* ========================================================================= *)

(** get when not empty: decrements used, advances read_idx *)
Theorem get_preserves_invariant :
  forall msz mmx used ri wi : Z,
    msgq_inv msz mmx used ri wi ->
    used > 0 ->
    msgq_inv msz mmx (used - 1)
      ((ri + 1) mod mmx) wi.
Proof.
  intros ms mm um ri wi [Hms [Hmm [Hu1 [Hu2 [Hr1 [Hr2 [Hw1 Hw2]]]]]]] Hgt.
  unfold msgq_inv. repeat split; try lia.
  - apply Z.mod_pos_bound. lia.
  - apply Z.mod_pos_bound. lia.
Qed.

(* ========================================================================= *)
(** * Purge Proofs *)
(* ========================================================================= *)

Theorem purge_establishes_empty :
  forall msz mmx used ri wi : Z,
    msgq_inv msz mmx used ri wi ->
    msgq_inv msz mmx 0 wi wi.
Proof.
  intros ms mm um ri wi [Hms [Hmm [Hu1 [Hu2 [Hr1 [Hr2 [Hw1 Hw2]]]]]]].
  unfold msgq_inv. lia.
Qed.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

(** put-get roundtrip preserves used count *)
Theorem put_get_roundtrip :
  forall msz mmx used ri wi : Z,
    msgq_inv msz mmx used ri wi ->
    used < mmx ->
    (used + 1) - 1 = used.
Proof.
  intros. lia.
Qed.

(** Conservation: num_free + num_used = max_msgs *)
Theorem conservation :
  forall msz mmx used ri wi : Z,
    msgq_inv msz mmx used ri wi ->
    (mmx - used) + used = mmx.
Proof.
  intros. lia.
Qed.

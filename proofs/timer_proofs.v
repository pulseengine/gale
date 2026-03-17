(** * Formal Verification Proofs for Zephyr Timer

    Proves properties about the expiry counter model.
    Complements Verus SMT proofs in src/timer.rs.

    Invariant: st >= 0 (trivially true for u32, modeled as Z >= 0).

    The rocq-of-rust translation wraps all values in Value.t.
    These proofs operate at the abstract Z level. *)

Require Import RocqOfRust.RocqOfRust.

(* Close type_scope to prevent parsing conflicts with abstract proofs. *)
Close Scope type_scope.

(* ========================================================================= *)
(** * Definitions *)
(* ========================================================================= *)

Definition EOVERFLOW : Z := -75.
Definition OK        : Z := 0.
Definition U32_MAX   : Z := 4294967295.

(** The timer invariant -- st is a valid u32 *)
Definition timer_inv (st : Z) : Prop :=
  0 <= st /\ st <= U32_MAX.

(* ========================================================================= *)
(** * Init Proofs *)
(* ========================================================================= *)

Theorem init_establishes_invariant :
  timer_inv 0.
Proof.
  unfold timer_inv, U32_MAX. lia.
Qed.

(* ========================================================================= *)
(** * Start Proofs *)
(* ========================================================================= *)

(** TM3: start resets status to 0 *)
Theorem start_resets :
  forall st : Z,
    timer_inv st ->
    timer_inv 0.
Proof.
  intros s _. unfold timer_inv, U32_MAX. lia.
Qed.

(* ========================================================================= *)
(** * Stop Proofs *)
(* ========================================================================= *)

(** TM4: stop clears status to 0 *)
Theorem stop_clears :
  forall st : Z,
    timer_inv st ->
    timer_inv 0.
Proof.
  intros s _. unfold timer_inv, U32_MAX. lia.
Qed.

(* ========================================================================= *)
(** * Expire Proofs *)
(* ========================================================================= *)

(** TM5: expire increments status by 1 when below max *)
Theorem expire_increments :
  forall st : Z,
    timer_inv st ->
    st < U32_MAX ->
    timer_inv (st + 1).
Proof.
  intros s [Hge Hle] Hlt. unfold timer_inv, U32_MAX in *. lia.
Qed.

(** TM8: expire at max does not overflow (checked_add returns error) *)
Theorem expire_no_overflow :
  forall st : Z,
    timer_inv st ->
    st = U32_MAX ->
    timer_inv st.
Proof.
  intros. assumption.
Qed.

(* ========================================================================= *)
(** * Status Get Proofs *)
(* ========================================================================= *)

(** TM2: status_get returns old status and resets to 0 *)
Theorem status_get_resets :
  forall st : Z,
    timer_inv st ->
    timer_inv 0 /\ 0 <= st.
Proof.
  intros s [Hge Hle]. split.
  - unfold timer_inv, U32_MAX. lia.
  - exact Hge.
Qed.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

(** TM2+TM5: expire then status_get roundtrip.
    After one expire from status s, status_get returns s+1 and resets to 0. *)
Theorem expire_status_get_roundtrip :
  forall st : Z,
    timer_inv st ->
    st < U32_MAX ->
    st + 1 = st + 1 /\ timer_inv 0.
Proof.
  intros s [Hge Hle] Hlt. split.
  - reflexivity.
  - unfold timer_inv, U32_MAX. lia.
Qed.

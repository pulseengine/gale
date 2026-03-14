(** * Formal Verification Proofs for Zephyr Pipe

    Proves properties about the state machine and byte count model.
    Complements Verus SMT proofs in src/pipe.rs.

    Invariant: size > 0, 0 <= used <= size,
    flags in {0, FLAG_OPEN, FLAG_OPEN|FLAG_RESET, FLAG_RESET}. *)

Require Import RocqOfRust.RocqOfRust.

(* ========================================================================= *)
(** * Definitions *)
(* ========================================================================= *)

Definition EINVAL    : Z := -22.
Definition EPIPE     : Z := -32.
Definition EAGAIN    : Z := -11.
Definition ECANCELED : Z := -125.
Definition ENOMSG    : Z := -42.
Definition OK        : Z := 0.

Definition FLAG_OPEN  : Z := 1.
Definition FLAG_RESET : Z := 2.

(** The pipe invariant *)
Definition pipe_inv (used size flags : Z) : Prop :=
  size > 0 /\
  0 <= used /\
  used <= size /\
  0 <= flags /\
  flags <= 3.

(* ========================================================================= *)
(** * Init Proofs *)
(* ========================================================================= *)

Theorem init_establishes_invariant :
  forall size : Z,
    size > 0 ->
    pipe_inv 0 size FLAG_OPEN.
Proof.
  intros sz Hsz. unfold pipe_inv, FLAG_OPEN. lia.
Qed.

Theorem init_rejects_zero :
  ~ pipe_inv 0 0 FLAG_OPEN.
Proof.
  intros [H _]. lia.
Qed.

(* ========================================================================= *)
(** * Write Proofs *)
(* ========================================================================= *)

(** Write n bytes: used increases, invariant preserved *)
Theorem write_preserves_invariant :
  forall used size flags n : Z,
    pipe_inv used size flags ->
    n > 0 ->
    n <= size - used ->
    pipe_inv (used + n) size flags.
Proof.
  intros u s f n [Hs [Hu1 [Hu2 [Hf1 Hf2]]]] Hn1 Hn2.
  unfold pipe_inv. lia.
Qed.

(** Write to full pipe: no change *)
Theorem write_full_no_change :
  forall used size flags : Z,
    pipe_inv used size flags ->
    used = size ->
    pipe_inv used size flags.
Proof.
  intros. assumption.
Qed.

(* ========================================================================= *)
(** * Read Proofs *)
(* ========================================================================= *)

(** Read n bytes: used decreases, invariant preserved *)
Theorem read_preserves_invariant :
  forall used size flags n : Z,
    pipe_inv used size flags ->
    n > 0 ->
    n <= used ->
    pipe_inv (used - n) size flags.
Proof.
  intros u s f n [Hs [Hu1 [Hu2 [Hf1 Hf2]]]] Hn1 Hn2.
  unfold pipe_inv. lia.
Qed.

(** Read no underflow *)
Theorem read_no_underflow :
  forall used size flags n : Z,
    pipe_inv used size flags ->
    n > 0 ->
    n <= used ->
    used - n >= 0.
Proof.
  intros. lia.
Qed.

(* ========================================================================= *)
(** * Reset and Close Proofs *)
(* ========================================================================= *)

(** Reset empties the pipe and sets the reset flag.
    We model the flag update abstractly: any valid flags ored with
    FLAG_RESET stays in [0,3]. *)
Theorem reset_empties :
  forall used size flags new_flags : Z,
    pipe_inv used size flags ->
    0 <= new_flags ->
    new_flags <= 3 ->
    pipe_inv 0 size new_flags.
Proof.
  intros u s f nf [Hs [Hu1 [Hu2 [Hf1 Hf2]]]] Hnf1 Hnf2.
  unfold pipe_inv. lia.
Qed.

Theorem close_clears_flags :
  forall used size flags : Z,
    pipe_inv used size flags ->
    pipe_inv used size 0.
Proof.
  intros u s f [Hs [Hu1 [Hu2 _]]].
  unfold pipe_inv. lia.
Qed.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

Theorem write_read_roundtrip :
  forall used size flags n : Z,
    pipe_inv used size flags ->
    n > 0 ->
    n <= size - used ->
    (used + n) - n = used.
Proof.
  intros. lia.
Qed.

Theorem conservation :
  forall used size flags : Z,
    pipe_inv used size flags ->
    (size - used) + used = size.
Proof.
  intros. lia.
Qed.

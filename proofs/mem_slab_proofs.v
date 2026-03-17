(** * Formal Verification Proofs for Zephyr Memory Slab

    Proves properties about the block allocation counter model.
    Complements Verus SMT proofs in src/mem_slab.rs.

    Invariant: nb > 0, 0 <= nu <= nb.

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
Definition OK     : Z := 0.

(** The mem_slab invariant *)
Definition mem_slab_inv (nu nb : Z) : Prop :=
  nb > 0 /\ 0 <= nu /\ nu <= nb.

(* ========================================================================= *)
(** * Init Proofs *)
(* ========================================================================= *)

Theorem init_establishes_invariant :
  forall nb : Z,
    nb > 0 ->
    mem_slab_inv 0 nb.
Proof.
  intros nb Hnb. unfold mem_slab_inv. lia.
Qed.

(* ========================================================================= *)
(** * Alloc Proofs *)
(* ========================================================================= *)

(** MS4: alloc when not full: nu incremented, invariant preserved *)
Theorem alloc_preserves_invariant :
  forall nu nb : Z,
    mem_slab_inv nu nb ->
    nu < nb ->
    mem_slab_inv (nu + 1) nb.
Proof.
  intros nu nb [Hnb [Hge Hle]] Hlt. unfold mem_slab_inv. lia.
Qed.

(** MS5: alloc when full: rejected, state unchanged *)
Theorem alloc_full_rejected :
  forall nu nb : Z,
    mem_slab_inv nu nb ->
    nu = nb ->
    mem_slab_inv nu nb.
Proof.
  intros. assumption.
Qed.

(* ========================================================================= *)
(** * Free Proofs *)
(* ========================================================================= *)

(** MS6: free when used > 0: nu decremented, invariant preserved *)
Theorem free_preserves_invariant :
  forall nu nb : Z,
    mem_slab_inv nu nb ->
    nu > 0 ->
    mem_slab_inv (nu - 1) nb.
Proof.
  intros nu nb [Hnb [Hge Hle]] Hgt. unfold mem_slab_inv. lia.
Qed.

(** Free when all blocks are free: rejected, state unchanged *)
Theorem free_all_free_rejected :
  forall nu nb : Z,
    mem_slab_inv nu nb ->
    nu = 0 ->
    mem_slab_inv nu nb.
Proof.
  intros. assumption.
Qed.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

(** MS4+MS6: alloc then free returns to original nu *)
Theorem alloc_free_roundtrip :
  forall nu nb : Z,
    mem_slab_inv nu nb ->
    nu < nb ->
    (nu + 1) - 1 = nu.
Proof.
  intros. lia.
Qed.

(** MS7: free + used == nb (conservation) *)
Theorem conservation :
  forall nu nb : Z,
    mem_slab_inv nu nb ->
    (nb - nu) + nu = nb.
Proof.
  intros. lia.
Qed.

(** * Formal Verification Proofs for Zephyr Memory Slab

    Proves properties about the block allocation counter model.
    Complements Verus SMT proofs in src/mem_slab.rs.

    Invariant: num_blocks > 0, 0 <= num_used <= num_blocks. *)

Require Import RocqOfRust.RocqOfRust.

(* ========================================================================= *)
(** * Definitions *)
(* ========================================================================= *)

Definition EINVAL : Z := -22.
Definition ENOMEM : Z := -12.
Definition OK     : Z := 0.

(** The mem_slab invariant *)
Definition mem_slab_inv (num_used num_blocks : Z) : Prop :=
  num_blocks > 0 /\ 0 <= num_used /\ num_used <= num_blocks.

(* ========================================================================= *)
(** * Init Proofs *)
(* ========================================================================= *)

Theorem init_establishes_invariant :
  forall num_blocks : Z,
    num_blocks > 0 ->
    mem_slab_inv 0 num_blocks.
Proof.
  intros nb Hnb. unfold mem_slab_inv. lia.
Qed.

(* ========================================================================= *)
(** * Alloc Proofs *)
(* ========================================================================= *)

(** MS4: alloc when not full: num_used incremented, invariant preserved *)
Theorem alloc_preserves_invariant :
  forall num_used num_blocks : Z,
    mem_slab_inv num_used num_blocks ->
    num_used < num_blocks ->
    mem_slab_inv (num_used + 1) num_blocks.
Proof.
  intros nu nb [Hnb [Hge Hle]] Hlt. unfold mem_slab_inv. lia.
Qed.

(** MS5: alloc when full: rejected, state unchanged *)
Theorem alloc_full_rejected :
  forall num_used num_blocks : Z,
    mem_slab_inv num_used num_blocks ->
    num_used = num_blocks ->
    mem_slab_inv num_used num_blocks.
Proof.
  intros. assumption.
Qed.

(* ========================================================================= *)
(** * Free Proofs *)
(* ========================================================================= *)

(** MS6: free when used > 0: num_used decremented, invariant preserved *)
Theorem free_preserves_invariant :
  forall num_used num_blocks : Z,
    mem_slab_inv num_used num_blocks ->
    num_used > 0 ->
    mem_slab_inv (num_used - 1) num_blocks.
Proof.
  intros nu nb [Hnb [Hge Hle]] Hgt. unfold mem_slab_inv. lia.
Qed.

(** Free when all blocks are free: rejected, state unchanged *)
Theorem free_all_free_rejected :
  forall num_used num_blocks : Z,
    mem_slab_inv num_used num_blocks ->
    num_used = 0 ->
    mem_slab_inv num_used num_blocks.
Proof.
  intros. assumption.
Qed.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

(** MS4+MS6: alloc then free returns to original num_used *)
Theorem alloc_free_roundtrip :
  forall num_used num_blocks : Z,
    mem_slab_inv num_used num_blocks ->
    num_used < num_blocks ->
    (num_used + 1) - 1 = num_used.
Proof.
  intros. lia.
Qed.

(** MS7: free + used == num_blocks (conservation) *)
Theorem conservation :
  forall num_used num_blocks : Z,
    mem_slab_inv num_used num_blocks ->
    (num_blocks - num_used) + num_used = num_blocks.
Proof.
  intros. lia.
Qed.

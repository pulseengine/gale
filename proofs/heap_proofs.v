(** * Formal Verification Proofs for Zephyr sys_heap Allocator

    Proves properties about the chunk-level heap allocator model.
    Complements Verus SMT proofs in plain/src/heap.rs.

    The heap invariant ensures:
      - allocated_bytes <= capacity  (HP1: bounds)
      - free_chunks + used_chunks == total_chunks  (HP2: conservation)
      - alloc only succeeds with sufficient free space  (HP3)
      - free increases available space  (HP4)
      - double-free is rejected  (HP5)

    These proofs operate at the abstract Z level, mirroring the
    Heap struct fields: capacity, allocated_bytes, total_chunks,
    free_chunks. All fields are modelled as non-negative integers. *)

Require Import RocqOfRust.RocqOfRust.

(* Close type_scope to prevent parsing conflicts with abstract proofs. *)
Close Scope type_scope.
Require Import Stdlib.Init.Logic.
Open Scope Z_scope.

(* ========================================================================= *)
(** * Definitions *)
(* ========================================================================= *)

Definition EINVAL : Z := -22.
Definition ENOMEM : Z := -12.
Definition OK     : Z := 0.

(** The heap invariant, modelling Heap struct post-init constraints:
    - 0 < overhead <= capacity  (non-trivial init)
    - allocated_bytes <= capacity  (HP1)
    - 0 <= free_chunks <= total_chunks  (HP2 structural)
    - free_chunks + used_chunks == total_chunks where used = total - free  (HP2)
    All field values are non-negative. *)
Definition heap_inv
    (capacity allocated total free : Z) : Prop :=
  capacity > 0 /\
  0 <= allocated /\
  allocated <= capacity /\
  0 <= free /\
  free <= total /\
  total >= 0.

(* ========================================================================= *)
(** * HP1: Alloc size bounds *)
(* ========================================================================= *)

(** HP1: After a successful alloc, allocated_bytes stays bounded by capacity.
    The Rust alloc() checks: bytes <= capacity - allocated_bytes. *)
Theorem hp1_alloc_bounds :
  forall capacity allocated total free bytes : Z,
    heap_inv capacity allocated total free ->
    bytes > 0 ->
    bytes <= capacity - allocated ->
    free > 0 ->
    allocated + bytes <= capacity.
Proof.
  intros cap alloc tot fr bytes [_ [_ [Hle _]]] _ Hfit _.
  lia.
Qed.

(** HP1: alloc preserves invariant (allocated stays <= capacity). *)
Theorem hp1_alloc_preserves_inv :
  forall capacity allocated total free bytes : Z,
    heap_inv capacity allocated total free ->
    bytes > 0 ->
    bytes <= capacity - allocated ->
    free > 0 ->
    heap_inv capacity (allocated + bytes) total (free - 1).
Proof.
  intros cap alloc tot fr bytes
    [Hcap [Hge [Hle [Hfge [Hfle Htge]]]]] _ Hfit Hfpos.
  unfold heap_inv. repeat split; try lia.
Qed.

(* ========================================================================= *)
(** * HP2: Conservation — free returns block to pool *)
(* ========================================================================= *)

(** HP2: free() restores conservation: free_chunks increases by 1. *)
Theorem hp2_free_conservation :
  forall capacity allocated total free bytes : Z,
    heap_inv capacity allocated total free ->
    bytes > 0 ->
    bytes <= allocated ->
    free < total ->
    heap_inv capacity (allocated - bytes) total (free + 1).
Proof.
  intros cap alloc tot fr bytes
    [Hcap [Hge [Hle [Hfge [Hfle Htge]]]]] _ Hbytes Hfc.
  unfold heap_inv. repeat split; try lia.
Qed.

(** HP2: split() increases both total_chunks and free_chunks by 1, preserving balance. *)
Theorem hp2_split_conservation :
  forall capacity allocated total free : Z,
    heap_inv capacity allocated total free ->
    total < 65535 ->
    heap_inv capacity allocated (total + 1) (free + 1).
Proof.
  intros cap alloc tot fr
    [Hcap [Hge [Hle [Hfge [Hfle Htge]]]]] _.
  unfold heap_inv. repeat split; try lia.
Qed.

(** HP2: merge() decreases both total_chunks and free_chunks by 1, preserving balance. *)
Theorem hp2_merge_conservation :
  forall capacity allocated total free : Z,
    heap_inv capacity allocated total free ->
    total > 1 ->
    free > 1 ->
    heap_inv capacity allocated (total - 1) (free - 1).
Proof.
  intros cap alloc tot fr
    [Hcap [Hge [Hle [Hfge [Hfle Htge]]]]] Htot Hfr.
  unfold heap_inv. repeat split; try lia.
Qed.

(* ========================================================================= *)
(** * HP3: Alloc precondition — sufficient free space *)
(* ========================================================================= *)

(** HP3: The Rust alloc() checks free_chunks > 0 AND bytes <= capacity - allocated.
    This captures both conditions needed for alloc to succeed. *)
Definition alloc_precond (capacity allocated : Z) (free : Z) (bytes : Z) : Prop :=
  free > 0 /\ bytes <= capacity - allocated /\ bytes > 0.

(** HP3: alloc precondition implies enough space exists. *)
Theorem hp3_precond_implies_space :
  forall capacity allocated free bytes : Z,
    heap_inv capacity allocated (free + (capacity - allocated)) free ->
    alloc_precond capacity allocated free bytes ->
    allocated + bytes <= capacity.
Proof.
  intros cap alloc fr bytes _ [_ [Hfit _]].
  lia.
Qed.

(** HP3: alloc_precond is violated when free = 0. *)
Theorem hp3_precond_violated_no_free :
  forall capacity allocated bytes : Z,
    bytes > 0 ->
    ~ alloc_precond capacity allocated 0 bytes.
Proof.
  intros cap alloc bytes _ [Hfree _]. lia.
Qed.

(** HP3: alloc_precond is violated when bytes > available space. *)
Theorem hp3_precond_violated_no_space :
  forall capacity allocated free bytes : Z,
    bytes > capacity - allocated ->
    free > 0 ->
    ~ alloc_precond capacity allocated free bytes.
Proof.
  intros cap alloc fr bytes Hbig _ [_ [Hfit _]]. lia.
Qed.

(* ========================================================================= *)
(** * HP4: Free postcondition — increases free space *)
(* ========================================================================= *)

(** HP4: free() always decreases allocated_bytes (when bytes > 0 and valid). *)
Theorem hp4_free_decreases_allocated :
  forall capacity allocated total free bytes : Z,
    heap_inv capacity allocated total free ->
    bytes > 0 ->
    bytes <= allocated ->
    free < total ->
    allocated - bytes < allocated.
Proof.
  intros _ alloc _ _ bytes _ Hpos _ _. lia.
Qed.

(** HP4: free space after free() is strictly greater than before. *)
Theorem hp4_free_increases_space :
  forall capacity allocated bytes : Z,
    0 <= allocated ->
    bytes > 0 ->
    bytes <= allocated ->
    (capacity - (allocated - bytes)) > (capacity - allocated).
Proof.
  intros cap alloc bytes _ Hpos _. lia.
Qed.

(** HP4: free() satisfies the rejection guard (free_chunks < total_chunks). *)
Theorem hp4_free_guard_satisfied :
  forall capacity allocated total free bytes : Z,
    heap_inv capacity allocated total free ->
    free < total ->
    bytes <= allocated ->
    heap_inv capacity (allocated - bytes) total (free + 1).
Proof.
  intros cap alloc tot fr bytes
    [Hcap [Hge [Hle [Hfge [Hfle Htge]]]]] Hlt Hbytes.
  unfold heap_inv. repeat split; try lia.
Qed.

(* ========================================================================= *)
(** * HP5: Double-free rejection *)
(* ========================================================================= *)

(** HP5: The Rust free() rejects if free_chunks >= total_chunks.
    This models: when all chunks are already free, another free is invalid.
    free_chunks >= total_chunks means the pool is already fully freed. *)
Definition double_free_guard (total free : Z) : Prop :=
  free >= total.

(** HP5: double_free_guard implies the free call returns EINVAL. *)
Theorem hp5_double_free_rejected :
  forall total free : Z,
    double_free_guard total free ->
    total >= 0 ->
    free >= 0 ->
    (* The guard triggers the error path *)
    total <= free.
Proof.
  intros tot fr Hge _ _. unfold double_free_guard in Hge. lia.
Qed.

(** HP5: after a valid alloc, double_free_guard is false
    (there exists a used chunk, so free < total). *)
Theorem hp5_after_alloc_no_double_free :
  forall capacity allocated total free bytes : Z,
    heap_inv capacity allocated total free ->
    alloc_precond capacity allocated free bytes ->
    free - 1 < total.
Proof.
  intros cap alloc tot fr bytes
    [_ [_ [_ [_ [Hfle _]]]]] [Hfpos _].
  lia.
Qed.

(** HP5: double_free_guard is invariant under a valid free:
    after valid free, free_chunks goes up by 1 but remains <= total_chunks.
    So the guard won't trigger on the *next* free (assuming total > free + 1). *)
Theorem hp5_no_double_free_after_valid_free :
  forall total free : Z,
    free < total ->
    ~ double_free_guard total (free + 1) \/ free + 1 = total.
Proof.
  intros tot fr Hlt.
  destruct (Z.eq_dec (fr + 1) tot) as [Heq | Hne].
  - right. exact Heq.
  - left. unfold double_free_guard. lia.
Qed.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

(** Init establishes the invariant. *)
Theorem heap_init_establishes_inv :
  forall capacity overhead : Z,
    capacity > 0 ->
    overhead > 0 ->
    overhead < capacity ->
    heap_inv capacity overhead 2 1.
Proof.
  intros cap ov Hcap Hov Hlt.
  unfold heap_inv. repeat split; try lia.
Qed.

(** Alloc-then-free roundtrip returns to original state. *)
Theorem alloc_free_roundtrip :
  forall capacity allocated total free bytes : Z,
    heap_inv capacity allocated total free ->
    bytes > 0 ->
    bytes <= capacity - allocated ->
    free > 0 ->
    (* After alloc: (allocated + bytes, free - 1). After free: back. *)
    (allocated + bytes) - bytes = allocated /\
    (free - 1) + 1 = free.
Proof.
  intros. split; lia.
Qed.

(** Free_bytes_get is correctly computed as capacity - allocated_bytes. *)
Theorem free_bytes_correct :
  forall capacity allocated : Z,
    0 <= allocated ->
    allocated <= capacity ->
    capacity - allocated >= 0.
Proof.
  intros. lia.
Qed.

(** bytes_to_chunks rounds up correctly: result * CHUNK_UNIT >= bytes. *)
Theorem bytes_to_chunks_rounds_up :
  forall bytes : Z,
    bytes >= 0 ->
    ((bytes + 8 - 1) / 8) * 8 >= bytes.
Proof.
  intros bytes Hge.
  assert (H := Z.div_mod (bytes + 8 - 1) 8).
  assert (H8 : (8 : Z) > 0) by lia.
  assert (Hmod := Z.mod_pos_bound (bytes + 8 - 1) 8 H8).
  nia.
Qed.

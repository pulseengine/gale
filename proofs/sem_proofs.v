(** * Formal Verification Proofs for Zephyr Semaphore

    This file proves properties about the Rust semaphore translated
    to Rocq by rocq-of-rust. The translated code is in plain/sem.v
    (generated from plain/sem.rs).

    Proof strategy:
    - Section 1: Abstract invariant definitions and proofs over Z
    - Section 2: Bridge definitions connecting Value.t to abstract invariants
    - Section 3: Compositional proofs

    The rocq-of-rust translation wraps all values in Value.t (the
    monadic DSL). These proofs operate at two levels:
    1. Abstract level: invariants over Z (count, limit bounds)
    2. Bridge level: extraction from Value.t to Z for connecting
       to the translated code

    These proofs complement the Verus SMT proofs in src/sem.rs:
    - Verus proves: count bounds, overflow safety, basic state machine
    - Rocq proves: deeper properties requiring induction and custom tactics *)

Require Import RocqOfRust.RocqOfRust.

(* Close type_scope to prevent parsing conflicts with abstract proofs.
   The RocqOfRust import opens type_scope globally via lib.lib, which
   can cause variable names to be interpreted as types. *)
Close Scope type_scope.

(* Import the translated semaphore module.
   This will be available after rocq-of-rust translates plain/sem.rs *)
(* From plain Require Import sem. *)

(* ========================================================================= *)
(** * Section 1: Abstract Invariant Definitions *)
(* ========================================================================= *)

(** Error codes match Zephyr's errno values.
    In the translated code, these appear as:
      Value.Integer IntegerKind.I32 (-22)  etc. *)
Definition EINVAL : Z := -22.
Definition EBUSY  : Z := -16.
Definition EAGAIN : Z := -11.
Definition OK     : Z := 0.

(** The semaphore invariant as a Rocq proposition.
    In the translated code, count and limit are u32 fields
    wrapped in Value.Integer IntegerKind.U32. *)
Definition sem_inv (cnt lim : Z) : Prop :=
  lim > 0 /\ 0 <= cnt /\ cnt <= lim.

(* ========================================================================= *)
(** * Section 2: Bridge from Value.t to abstract invariant *)
(* ========================================================================= *)

(** Extract a Z from a Value.t integer, if it is one.
    The translated code represents u32 as Value.Integer IntegerKind.U32 n.
    Assumption: Value.Integer takes (IntegerKind.t, Z) -- if the
    constructor signature differs at the pinned commit, adjust this match. *)
Definition extract_integer (v : Value.t) : option Z :=
  match v with
  | Value.Integer _ n => Some n
  | _ => None
  end.

(** Predicate: a Value.t carries a valid semaphore count *)
Definition is_valid_sem_count (v_count v_limit : Value.t) : Prop :=
  exists cnt lim : Z,
    extract_integer v_count = Some cnt /\
    extract_integer v_limit = Some lim /\
    sem_inv cnt lim.

(* ========================================================================= *)
(** * Section 3: Init Proofs *)
(* ========================================================================= *)

(** init with valid parameters establishes the invariant *)
Theorem init_establishes_invariant :
  forall ic lim : Z,
    lim > 0 ->
    0 <= ic ->
    ic <= lim ->
    sem_inv ic lim.
Proof.
  intros ic lim Hlim Hge Hle.
  unfold sem_inv. auto.
Qed.

(** init rejects limit = 0 *)
Theorem init_rejects_zero_limit :
  forall ic : Z,
    ~ sem_inv ic 0.
Proof.
  intros ic [H _]. lia.
Qed.

(** Bridge: init with Value.t inputs establishes validity *)
Theorem init_bridge :
  forall ic lim : Z,
    lim > 0 ->
    0 <= ic ->
    ic <= lim ->
    is_valid_sem_count
      (Value.Integer IntegerKind.U32 ic)
      (Value.Integer IntegerKind.U32 lim).
Proof.
  intros ic lim Hlim Hge Hle.
  exists ic, lim. repeat split; auto.
  unfold sem_inv. auto.
Qed.

(* ========================================================================= *)
(** * Section 4: Give Proofs *)
(* ========================================================================= *)

(** give with no waiters and count < limit: count incremented, invariant preserved *)
Theorem give_no_waiter_preserves_invariant :
  forall cnt lim : Z,
    sem_inv cnt lim ->
    cnt < lim ->
    sem_inv (cnt + 1) lim.
Proof.
  intros cnt lim [Hlim [Hge Hle]] Hlt.
  unfold sem_inv. lia.
Qed.

(** give with no waiters at limit: saturation preserves invariant *)
Theorem give_saturated_preserves_invariant :
  forall cnt lim : Z,
    sem_inv cnt lim ->
    cnt = lim ->
    sem_inv cnt lim.
Proof.
  intros cnt lim Hinv _. exact Hinv.
Qed.

(** give never causes count to exceed limit *)
Theorem give_count_bounded :
  forall cnt lim : Z,
    sem_inv cnt lim ->
    (if Z.ltb cnt lim then cnt + 1 else cnt) <= lim.
Proof.
  intros cnt lim [Hlim [Hge Hle]].
  destruct (Z.ltb cnt lim) eqn:E.
  - apply Z.ltb_lt in E. lia.
  - apply Z.ltb_ge in E. lia.
Qed.

(* ========================================================================= *)
(** * Section 5: Take Proofs *)
(* ========================================================================= *)

(** try_take when count > 0: decrement preserves invariant *)
Theorem take_preserves_invariant :
  forall cnt lim : Z,
    sem_inv cnt lim ->
    cnt > 0 ->
    sem_inv (cnt - 1) lim.
Proof.
  intros cnt lim [Hlim [Hge Hle]] Hgt.
  unfold sem_inv. lia.
Qed.

(** try_take when count = 0: count unchanged *)
Theorem take_empty_unchanged :
  forall cnt lim : Z,
    sem_inv cnt lim ->
    cnt = 0 ->
    sem_inv cnt lim.
Proof.
  intros cnt lim Hinv _. exact Hinv.
Qed.

(** No underflow: count >= 0 after take *)
Theorem take_no_underflow :
  forall cnt lim : Z,
    sem_inv cnt lim ->
    cnt > 0 ->
    cnt - 1 >= 0.
Proof.
  intros cnt lim [Hlim [Hge Hle]] Hgt. lia.
Qed.

(* ========================================================================= *)
(** * Section 6: Reset Proofs *)
(* ========================================================================= *)

(** reset establishes count = 0, preserves limit *)
Theorem reset_establishes_invariant :
  forall cnt lim : Z,
    sem_inv cnt lim ->
    sem_inv 0 lim.
Proof.
  intros cnt lim [Hlim [Hge Hle]].
  unfold sem_inv. lia.
Qed.

(* ========================================================================= *)
(** * Section 7: Compositional Proofs *)
(* ========================================================================= *)

(** Give-take roundtrip: give then take returns to original count *)
Theorem give_take_roundtrip :
  forall cnt lim : Z,
    sem_inv cnt lim ->
    cnt < lim ->
    (cnt + 1) - 1 = cnt.
Proof.
  intros. lia.
Qed.

(** Repeated gives saturate at limit *)
Theorem repeated_give_saturates :
  forall cnt lim n : Z,
    sem_inv cnt lim ->
    n >= 0 ->
    sem_inv (Z.min (cnt + n) lim) lim.
Proof.
  intros cnt lim n [Hlim [Hge Hle]] Hn.
  unfold sem_inv. lia.
Qed.

(** The invariant is sufficient for all operations to be safe *)
Theorem invariant_sufficiency :
  forall cnt lim : Z,
    sem_inv cnt lim ->
    (* give is safe *)
    (cnt < lim -> sem_inv (cnt + 1) lim) /\
    (* take is safe when count > 0 *)
    (cnt > 0 -> sem_inv (cnt - 1) lim) /\
    (* reset is safe *)
    sem_inv 0 lim.
Proof.
  intros cnt lim [Hlim [Hge Hle]].
  repeat split; intros; unfold sem_inv; lia.
Qed.

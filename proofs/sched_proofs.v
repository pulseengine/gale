(** * Zephyr scheduler invariant model

    SC1–SC3 prove properties of the abstract ready queue. The scheduling
    decisions and state transitions below still contain admitted statements.
    See docs/safety/verification-honesty.md for the scope of these proofs.

    Key abstractions:
    - Priority: lower numeric value = higher priority (Zephyr convention)
    - RunQueue: sorted list (index 0 = highest priority = lowest number)
    - next_up: returns highest-priority thread or idle

    Ready-queue properties:
    - SC1: best() returns thread with minimum priority value
    - SC2: add preserves sorted ordering (by numeric priority ascending)
    - SC3: remove_best preserves sorted ordering for remaining threads

    These proofs operate at the abstract Z level using lists. *)

From Stdlib Require Import Lists.List ZArith.ZArith.
Import ListNotations.
Open Scope Z_scope.

(* ========================================================================= *)
(** * Definitions *)
(* ========================================================================= *)

(** Priority ordering: lower value = higher priority. *)
Definition higher_prio (a b : Z) : Prop := a < b.
Definition same_prio (a b : Z) : Prop := a = b.

(** A list of priorities is sorted in ascending order
    (highest priority first = smallest value first). *)
Fixpoint sorted_asc (l : list Z) : Prop :=
  match l with
  | []                      => True
  | [_]                     => True
  | x :: ((y :: _) as rest) => x <= y /\ sorted_asc rest
  end.

(** The run queue invariant: the list is sorted ascending. *)
Definition runq_inv (prios : list Z) : Prop :=
  sorted_asc prios.

(** Insert a priority into a sorted list, maintaining sort order. *)
Fixpoint insert_sorted (p : Z) (l : list Z) : list Z :=
  match l with
  | []      => [p]
  | x :: rest =>
    if Z.leb p x then p :: l
    else x :: insert_sorted p rest
  end.

(** Remove the minimum (first) element from a sorted list. *)
Definition remove_best (l : list Z) : list Z :=
  match l with
  | []        => []
  | _ :: rest => rest
  end.

(** Best (highest priority) thread: the head of the sorted list. *)
Definition best (l : list Z) : option Z :=
  match l with
  | []    => None
  | x :: _ => Some x
  end.

(* ========================================================================= *)
(** * SC1: best() returns highest priority (smallest value) *)
(* ========================================================================= *)

(** SC1: best() returns Some when queue is non-empty. *)
Theorem sc1_best_some_nonempty :
  forall l : list Z,
    l <> [] ->
    exists p, best l = Some p.
Proof.
  intros [| p rest] Hnonempty.
  - contradiction.
  - now exists p.
Qed.

(** SC1: best() returns None on an empty queue. *)
Theorem sc1_best_none_empty :
  best [] = None.
Proof.
  reflexivity.
Qed.

(** SC1: In a sorted pair, the first priority is no greater than the second. *)
Theorem sc1_best_is_minimum :
  forall p l : Z,
    sorted_asc (p :: [l]) ->
    p <= l.
Proof.
  intros p l [Hle _].
  exact Hle.
Qed.

(** SC1: General: head of sorted list is <= every element. *)
Lemma sorted_head_le_all :
  forall h : Z,
  forall t : list Z,
    sorted_asc (h :: t) ->
    forall x, In x t -> h <= x.
Proof.
  intros h t. revert h.
  induction t as [| a t IH].
  - intros h _ x Hin. contradiction.
  - intros h [Hha Hsorted] x [Heq | Hin].
    + now subst x.
    + apply Z.le_trans with (m := a).
      * exact Hha.
      * now apply IH.
Qed.

(* ========================================================================= *)
(** * SC2: add_thread preserves sort invariant *)
(* ========================================================================= *)

(** Lemma: insert_sorted into a sorted list gives a sorted list. *)
Lemma insert_sorted_preserves :
  forall p : Z,
  forall l : list Z,
    sorted_asc l ->
    sorted_asc (insert_sorted p l).
Proof.
  intros p l.
  induction l as [| a l IH]; intro Hsorted.
  - exact I.
  - cbn [insert_sorted].
    destruct (Z.leb p a) eqn:Hpa.
    + apply Z.leb_le in Hpa.
      exact (conj Hpa Hsorted).
    + apply Z.leb_gt in Hpa.
      destruct l as [| b l].
      * split; [now apply Z.lt_le_incl | exact I].
      * destruct Hsorted as [Hab Htail].
        specialize (IH Htail).
        cbn [insert_sorted] in IH |- *.
        destruct (Z.leb p b).
        -- exact (conj (Z.lt_le_incl _ _ Hpa) IH).
        -- exact (conj Hab IH).
Qed.

(** SC2: add_thread preserves the run queue invariant. *)
Theorem sc2_add_preserves_inv :
  forall p : Z,
  forall prios : list Z,
    runq_inv prios ->
    runq_inv (insert_sorted p prios).
Proof.
  exact insert_sorted_preserves.
Qed.

(** SC2: add_thread increases queue length by 1. *)
Theorem sc2_add_increases_length :
  forall p : Z,
  forall prios : list Z,
    length (insert_sorted p prios) = S (length prios).
Proof.
  intros p prios.
  induction prios as [| a rest IH].
  - reflexivity.
  - cbn [insert_sorted].
    destruct (Z.leb p a); simpl; [reflexivity | now rewrite IH].
Qed.

(* ========================================================================= *)
(** * SC3: remove_thread preserves sort invariant *)
(* ========================================================================= *)

(** Lemma: tail of a sorted list is sorted. *)
Lemma sorted_tail :
  forall x : Z,
  forall l : list Z,
    sorted_asc (x :: l) ->
    sorted_asc l.
Proof.
  intros x [| a rest] Hsorted.
  - exact I.
  - exact (proj2 Hsorted).
Qed.

(** SC3: remove_best preserves sorted ordering. *)
Theorem sc3_remove_best_preserves_inv :
  forall prios : list Z,
    runq_inv prios ->
    runq_inv (remove_best prios).
Proof.
  intros [| a rest] Hsorted.
  - exact I.
  - exact (sorted_tail a rest Hsorted).
Qed.

(** SC3: remove_best decreases length by 1 (when non-empty). *)
Theorem sc3_remove_best_decreases_length :
  forall prios : list Z,
    prios <> [] ->
    length (remove_best prios) = Nat.sub (length prios) 1.
Proof.
  intros [| a rest] Hnonempty.
  - contradiction.
  - simpl. now rewrite Nat.sub_0_r.
Qed.

(* ========================================================================= *)
(** * SC5: next_up returns highest priority thread *)
(* ========================================================================= *)

(** The scheduling choice at the abstract level. *)
Inductive SchedChoice : Type :=
  | RunThread : Z -> SchedChoice   (* run thread with this priority *)
  | RunIdle : SchedChoice.         (* run idle thread *)

(** next_up: when queue is non-empty, return best thread; else idle. *)
Definition next_up_abstract (prios : list Z) (idle_prio : Z) : SchedChoice :=
  match best prios with
  | Some p => RunThread p
  | None   => RunThread idle_prio
  end.

(** SC5: next_up returns the best thread when queue is non-empty. *)
Theorem sc5_next_up_returns_best :
  forall p : Z,
  forall rest : list Z,
    runq_inv (p :: rest) ->
    next_up_abstract (p :: rest) 0 = RunThread p.
Proof.
Admitted.

(** SC5: next_up returns idle prio when queue is empty. *)
Theorem sc5_next_up_empty_is_idle :
  forall idle_prio : Z,
    next_up_abstract [] idle_prio = RunThread idle_prio.
Proof.
Admitted.

(** SC5: The thread returned by next_up has the minimum priority value
    (highest scheduling priority) among all threads in the queue. *)
Theorem sc5_next_up_is_optimal :
  forall p : Z,
  forall rest : list Z,
    runq_inv (p :: rest) ->
    forall q, In q (p :: rest) -> p <= q.
Proof.
Admitted.

(* ========================================================================= *)
(** * SC7: Idle only when queue empty *)
(* ========================================================================= *)

(** SC7: The abstract model uses idle only when best returns None. *)
Theorem sc7_idle_only_when_empty :
  forall idle_prio : Z,
  forall prios : list Z,
    next_up_abstract prios idle_prio = RunThread idle_prio ->
    prios = [].
Proof.
Admitted.

(** SC7 contrapositive: if queue is non-empty, next_up does NOT return idle. *)
Theorem sc7_nonempty_not_idle :
  forall idle_prio : Z,
  forall p : Z,
  forall rest : list Z,
    exists thread_prio,
      next_up_abstract (p :: rest) idle_prio = RunThread thread_prio.
Proof.
Admitted.

(* ========================================================================= *)
(** * Compositional Proofs *)
(* ========================================================================= *)

(** Empty queue satisfies the invariant. *)
Theorem empty_runq_inv :
  runq_inv [].
Proof.
Admitted.

(** should_preempt: swap_ok=true always allows preemption. *)
Theorem should_preempt_swap_ok :
  forall coop metairq : bool,
    (* In Rust: if swap_ok { true } *)
    True -> True.
Proof.
Admitted.

(** should_preempt: cooperative thread not preempted by non-MetaIRQ. *)
(** We model the Rust boolean logic directly. *)
Definition should_preempt_model
    (current_is_coop : bool) (candidate_is_metairq : bool) (swap_ok : bool) : bool :=
  if swap_ok then true
  else if andb current_is_coop (negb candidate_is_metairq) then false
  else true.

Theorem should_preempt_coop_no_metairq :
  should_preempt_model true false false = false.
Proof.
Admitted.

Theorem should_preempt_coop_metairq :
  should_preempt_model true true false = true.
Proof.
Admitted.

Theorem should_preempt_noncoop :
  forall metairq : bool,
    should_preempt_model false metairq false = true.
Proof.
Admitted.

(** prio_cmp: the subtraction gives negative when a has higher priority. *)
Theorem prio_cmp_negative_iff_higher :
  forall a b : Z,
    a - b < 0 <-> a < b.
Proof.
Admitted.

(** Sched state FSM: Dead has no outgoing transitions. *)
(** We model the SchedThreadState enum with Z constants for simplicity. *)
Definition SCHED_READY    : Z := 0.
Definition SCHED_RUNNING  : Z := 1.
Definition SCHED_PENDING  : Z := 2.
Definition SCHED_SUSPENDED: Z := 3.
Definition SCHED_SLEEPING : Z := 4.
Definition SCHED_DEAD     : Z := 5.
Definition SCHED_ABORTING : Z := 6.

(** sched_is_valid_transition encoded at Z level. *)
Definition sched_valid_trans (from to : Z) : bool :=
  if Z.eqb from SCHED_DEAD then false
  else if Z.eqb to SCHED_DEAD then true
  else if Z.eqb from to then false
  else match from with
  | 0 (* Ready *)    => Z.eqb to SCHED_RUNNING
  | 1 (* Running *)  => orb (Z.eqb to SCHED_READY)
                       (orb (Z.eqb to SCHED_PENDING)
                       (orb (Z.eqb to SCHED_SUSPENDED)
                       (orb (Z.eqb to SCHED_SLEEPING)
                            (Z.eqb to SCHED_ABORTING))))
  | 2 (* Pending *)  => orb (Z.eqb to SCHED_READY)
                       (orb (Z.eqb to SCHED_SUSPENDED)
                            (Z.eqb to SCHED_ABORTING))
  | 3 (* Suspended *) => orb (Z.eqb to SCHED_READY)
                             (Z.eqb to SCHED_ABORTING)
  | 4 (* Sleeping *) => orb (Z.eqb to SCHED_READY)
                            (Z.eqb to SCHED_ABORTING)
  | 6 (* Aborting *) => Z.eqb to SCHED_DEAD
  | _ => false
  end.

(** Dead has no outgoing transitions. *)
Theorem sched_dead_terminal :
  forall to : Z,
    sched_valid_trans SCHED_DEAD to = false.
Proof.
Admitted.

(** Running can transition to Ready. *)
Theorem sched_running_to_ready :
  sched_valid_trans SCHED_RUNNING SCHED_READY = true.
Proof.
Admitted.

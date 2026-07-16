(** * gust executor — NO-LOST-WAKEUPS (Rocq track)  [REQ-OS-TRITRACK-001]

    STATUS: FULLY PROVEN — every statement below ends in [Qed]; nothing is
    left open or assumed, and the file declares no axioms (the closing
    [Print Assumptions] lines make the compiler print "Closed under the
    global context" for each theorem). This file is the Rocq-track
    discharge of the
    executor's load-bearing no-lost-wakeups property, complementing the
    Verus SMT proof of the same property in [src/executor.rs]
    ([lemma_no_lost_wakeup] plus the per-mutator [ensures] frame clauses)
    and the Kani bounded-model-check harnesses in the same file.

    HONEST SCOPE — read before citing:

    - This is a MODEL-LEVEL proof. The definitions below are a hand-written
      Rocq model of the wake/consume state machine in [src/executor.rs];
      the correspondence to the Rust source is a STRUCTURAL ARGUMENT
      (documented field-by-field and operation-by-operation in the comments
      next to each definition), NOT a mechanical extraction or a refinement
      proof against the compiled binary. No claim is made beyond: "this
      abstract state machine, which mirrors the Rust code shape-for-shape,
      has the no-lost-wakeups property, machine-checked by Rocq."

    - Diverse redundancy, not novelty: the SAME property is already proven
      by Verus (SMT/Z3) directly on the Rust source. The value of this file
      is a SECOND, INDEPENDENT formalization checked by a DIFFERENT engine
      (Rocq's kernel / type theory, no SMT solver in the trusted base), so
      a soundness bug or spec-encoding mistake in either track alone cannot
      silently invalidate the property.

    - Modeled operations: [wake], [consume], and [pick_next] (the operation
      set of the scheduler's steady-state loop, [poll_round]). The two
      remaining ready-bitmask mutators of the Rust source are OUT of the
      trace alphabet and their exclusion is benign for this theorem:
        * [Tasks::admit] clears the ready bit of a slot it just found
          [Free]; under the representation invariant ([inv] below, the
          Rocq twin of [Tasks::inv()]) that bit is already clear, and a
          slot holding a live wakeup is [Pending], never [Free], so
          [admit] cannot select it.
        * [Tasks::expire] only ORs bits into the mask (it can only CREATE
          readiness, never destroy it), so it cannot lose a wakeup.
      [Tasks::dispatch_one] (the trusted FFI seam) carries the Verus
      [ensures self.ready == old(self).ready] frame — it never touches the
      mask at all — and is likewise not a ready-bit mutator.

    Build: see [proofs/BUILD.bazel] ([executor_proofs] / [executor_proofs_test],
    same [rocq_proof_test] gate as the other 13 files, run by
    [.github/workflows/formal-verification.yml] job [rocq]). *)

Require Import Stdlib.NArith.BinNat.
Require Import Stdlib.NArith.Nnat.
Require Import Stdlib.Arith.PeanoNat.
Require Import Stdlib.micromega.Lia.
Require Import Stdlib.Lists.List.
Import ListNotations.

(* ========================================================================= *)
(** * The model — field-by-field correspondence to [src/executor.rs]        *)
(* ========================================================================= *)

(** Rust: [pub const MAX_TASKS: usize = 8;] *)
Definition MAX_TASKS : nat := 8.

(** Rust: [pub enum TaskState { Free, Pending, Done }] — same three
    constructors, same meaning. *)
Inductive task_state : Type := Free | Pending | Done.

(** Rust: [pub struct Tasks { state, prio, deadline, ready }].

    Field map:
    - [ready : N]  <->  Rust [ready: u32]. The u32 bitmask is modeled as an
      unbounded binary natural; agreement with u32 semantics is exact for
      every operation below because all handles are < MAX_TASKS = 8 < 32,
      so no shift/or/and-not ever produces or consults a bit at position
      >= 32 where u32 truncation could diverge from N. (The representation
      invariant [inv] additionally pins all set bits below MAX_TASKS,
      mirroring [Tasks::inv()]'s [ready < 256] conjunct.)
    - [tstate : nat -> task_state]  <->  Rust [state: [TaskState; 8]]. The
      fixed array is modeled as a total map; only indices < MAX_TASKS are
      ever constrained or consulted, mirroring the array bounds.
    - Rust [prio: [u32; 8]] and [deadline: [u64; 8]] are OMITTED: none of
      the modeled operations writes them, and the only modeled reader
      ([pick_next]) is non-mutating, so they cannot affect the ready
      bitmask this theorem is about. *)
Record exec_state : Type := mk_exec_state {
  ready  : N;
  tstate : nat -> task_state
}.

(** Rust: [1u32 << h] (in [wake]'s [self.ready | (1u32 << h)] and
    [consume]'s [self.ready & !(1u32 << h)]). Exact for h < 32. *)
Definition mask (h : nat) : N := N.shiftl 1 (N.of_nat h).

(** Rust: [Tasks::ready_bit(i)] — spec fn [(self.ready >> i) & 1u32 == 1u32].
    [N.testbit x i] is definitionally that "shift right by i, look at the
    low bit" read-out. *)
Definition ready_bit (s : exec_state) (i : nat) : bool :=
  N.testbit (ready s) (N.of_nat i).

(** Rust: [Tasks::wake(h)] —
    [if h < MAX_TASKS as u32 && matches!(self.state[h], Pending)
       { self.ready = self.ready | (1u32 << h); }]
    Same guard, same bit-OR, state array untouched. *)
Definition wake (s : exec_state) (h : nat) : exec_state :=
  if (h <? MAX_TASKS)%nat then
    match tstate s h with
    | Pending => mk_exec_state (N.lor (ready s) (mask h)) (tstate s)
    | _ => s
    end
  else s.

(** Rust: [Tasks::consume(h)] — [self.ready = self.ready & !(1u32 << h);]
    (with [requires h < MAX_TASKS as u32]). For u32 values,
    [x & !m = N.ldiff x m] exactly: AND-with-complement keeps precisely the
    bits of [x] not in [m], which is [ldiff]; bits at positions >= 32 are 0
    in [x] on both sides, so the 32-bit complement's truncation is
    invisible. State array untouched, same as the Rust body. *)
Definition consume (s : exec_state) (h : nat) : exec_state :=
  mk_exec_state (N.ldiff (ready s) (mask h)) (tstate s).

(* ========================================================================= *)
(** * Operations and traces                                                  *)
(* ========================================================================= *)

(** The trace alphabet: the scheduler-loop operations named by the theorem.
    [PickNext] is Rust [Tasks::pick_next(&self)] — a shared borrow, so
    Rust's type system already forbids it from mutating; the model records
    that fact as the identity transition. Its selection RESULT (which ready
    task has minimal prio) is proven on the Verus track and cross-checked
    by Kani; it is irrelevant to bit preservation and not re-modeled here. *)
Inductive op : Type :=
| Wake    (h : nat)
| Consume (h : nat)
| PickNext.

Definition step (s : exec_state) (o : op) : exec_state :=
  match o with
  | Wake h    => wake s h
  | Consume h => consume s h
  | PickNext  => s
  end.

(** An arbitrary interleaving = an arbitrary finite trace of operations. *)
Fixpoint run (s : exec_state) (t : list op) : exec_state :=
  match t with
  | []        => s
  | o :: rest => run (step s o) rest
  end.

(* ========================================================================= *)
(** * Bitmask lemmas — the Rocq twins of the Verus bit-vector lemmas         *)
(* ========================================================================= *)

(** [N.testbit (mask h) n = (n =? N.of_nat h)]: the mask has exactly bit h.
    Twin of the Verus [by (bit_vector)] facts underlying
    [lemma_set_bit_self] / [lemma_clear_bit_self] /
    [lemma_set_bit_other] / [lemma_clear_bit_other]. *)
Lemma mask_spec : forall (h : nat) (n : N),
  N.testbit (mask h) n = (n =? N.of_nat h)%N.
Proof.
  intros h n. unfold mask.
  rewrite N.shiftl_1_l, N.pow2_bits_eqb.
  apply N.eqb_sym.
Qed.

(** Twin of Verus [lemma_set_bit_self]: setting bit h makes bit h read 1. *)
Lemma lor_mask_self : forall (x : N) (h : nat),
  N.testbit (N.lor x (mask h)) (N.of_nat h) = true.
Proof.
  intros x h. rewrite N.lor_spec, mask_spec, N.eqb_refl.
  apply Bool.orb_true_r.
Qed.

(** Twin of Verus [lemma_set_bit_other]: setting bit h leaves bit j (j<>h)
    unchanged. *)
Lemma lor_mask_other : forall (x : N) (h j : nat),
  j <> h ->
  N.testbit (N.lor x (mask h)) (N.of_nat j) = N.testbit x (N.of_nat j).
Proof.
  intros x h j Hne.
  rewrite N.lor_spec, mask_spec.
  rewrite (proj2 (N.eqb_neq (N.of_nat j) (N.of_nat h)));
    [apply Bool.orb_false_r|].
  intro Heq. apply Hne. apply Nat2N.inj. exact Heq.
Qed.

(** Twin of Verus [lemma_clear_bit_self]: clearing bit h makes bit h read 0. *)
Lemma ldiff_mask_self : forall (x : N) (h : nat),
  N.testbit (N.ldiff x (mask h)) (N.of_nat h) = false.
Proof.
  intros x h. rewrite N.ldiff_spec, mask_spec, N.eqb_refl.
  apply Bool.andb_false_r.
Qed.

(** Twin of Verus [lemma_clear_bit_other]: clearing bit h leaves bit j
    (j<>h) unchanged. *)
Lemma ldiff_mask_other : forall (x : N) (h j : nat),
  j <> h ->
  N.testbit (N.ldiff x (mask h)) (N.of_nat j) = N.testbit x (N.of_nat j).
Proof.
  intros x h j Hne.
  rewrite N.ldiff_spec, mask_spec.
  rewrite (proj2 (N.eqb_neq (N.of_nat j) (N.of_nat h)));
    [apply Bool.andb_true_r|].
  intro Heq. apply Hne. apply Nat2N.inj. exact Heq.
Qed.

(* ========================================================================= *)
(** * Per-operation facts — mirroring the Verus [ensures] clauses            *)
(* ========================================================================= *)

(** Twin of Verus [wake]'s ensures
    [(h < MAX_TASKS && state[h] == Pending) ==> ready_bit(h)]:
    a wake DELIVERED to a Pending task sets its ready bit. *)
Lemma wake_delivers : forall (s : exec_state) (h : nat),
  (h < MAX_TASKS)%nat ->
  tstate s h = Pending ->
  ready_bit (wake s h) h = true.
Proof.
  intros s h Hlt Hp. unfold wake.
  rewrite (proj2 (Nat.ltb_lt h MAX_TASKS) Hlt), Hp.
  unfold ready_bit; simpl. apply lor_mask_self.
Qed.

(** [wake] never CLEARS any ready bit (not even its own argument's):
    bit-OR is monotone. Subsumes the "wake j (j<>i) doesn't clear bit i"
    obligation, and is strictly stronger. *)
Lemma wake_preserves : forall (s : exec_state) (h i : nat),
  ready_bit s i = true ->
  ready_bit (wake s h) i = true.
Proof.
  intros s h i Hset. unfold wake.
  destruct (h <? MAX_TASKS)%nat; [|exact Hset].
  destruct (tstate s h); try exact Hset.
  unfold ready_bit in *; simpl.
  rewrite N.lor_spec, Hset. reflexivity.
Qed.

(** Twin of Verus [consume]'s ensures [!self.ready_bit(h)]. *)
Lemma consume_clears : forall (s : exec_state) (h : nat),
  ready_bit (consume s h) h = false.
Proof.
  intros s h. unfold ready_bit, consume; simpl. apply ldiff_mask_self.
Qed.

(** Twin of Verus [consume]'s ensures
    [forall j != h, ready_bit(j) == old.ready_bit(j)]:
    consuming h does not disturb any other task's readiness. *)
Lemma consume_preserves_other : forall (s : exec_state) (h i : nat),
  i <> h ->
  ready_bit (consume s h) i = ready_bit s i.
Proof.
  intros s h i Hne. unfold ready_bit, consume; simpl.
  apply ldiff_mask_other. exact Hne.
Qed.

(** No modeled operation writes the task-state array — in the Rust source,
    [wake]/[consume] touch only [self.ready], and [pick_next] is [&self]. *)
Lemma step_tstate : forall (s : exec_state) (o : op),
  tstate (step s o) = tstate s.
Proof.
  intros s o. destruct o as [h|h|]; simpl.
  - unfold wake. destruct (h <? MAX_TASKS)%nat; [|reflexivity].
    destruct (tstate s h); reflexivity.
  - reflexivity.
  - reflexivity.
Qed.

(* ========================================================================= *)
(** * The single-step kernel of the theorem                                  *)
(* ========================================================================= *)

(** Any operation that is not [Consume i] leaves a set bit i set. *)
Lemma step_no_lost_wakeup : forall (s : exec_state) (o : op) (i : nat),
  ready_bit s i = true ->
  o <> Consume i ->
  ready_bit (step s o) i = true.
Proof.
  intros s o i Hset Hne. destruct o as [h|h|]; simpl.
  - apply wake_preserves. exact Hset.
  - rewrite consume_preserves_other; [exact Hset|].
    intro Heq. apply Hne. rewrite Heq. reflexivity.
  - exact Hset.
Qed.

(* ========================================================================= *)
(** * THE THEOREM — no lost wakeups                                          *)
(* ========================================================================= *)

(** For ANY finite interleaving of [wake]/[consume]/[pick_next] operations:
    once ready bit i is set, it remains set unless the trace contains a
    [consume] of i itself. No other operation — waking any task (including
    i again), consuming any OTHER task, or picking the next task — can
    clear it. *)
Theorem no_lost_wakeups : forall (t : list op) (s : exec_state) (i : nat),
  ready_bit s i = true ->
  ~ In (Consume i) t ->
  ready_bit (run s t) i = true.
Proof.
  induction t as [|o rest IH]; intros s i Hset Hnotin; simpl.
  - exact Hset.
  - apply IH.
    + apply step_no_lost_wakeup; [exact Hset|].
      intro Heq. apply Hnotin. rewrite Heq. left. reflexivity.
    + intro Hin. apply Hnotin. right. exact Hin.
Qed.

(** End-to-end corollary in the phrasing of the requirement: a wake
    DELIVERED to a Pending task i (i.e. the guarded bit-set fired) is never
    lost — the bit survives every subsequent operation until a [consume i]
    occurs. *)
Corollary delivered_wake_survives : forall (t : list op) (s : exec_state) (i : nat),
  (i < MAX_TASKS)%nat ->
  tstate s i = Pending ->
  ~ In (Consume i) t ->
  ready_bit (run (wake s i) t) i = true.
Proof.
  intros t s i Hlt Hp Hnotin.
  apply no_lost_wakeups; [|exact Hnotin].
  apply wake_delivers; assumption.
Qed.

(* ========================================================================= *)
(** * Representation invariant — the Rocq twin of [Tasks::inv()]             *)
(* ========================================================================= *)

(** Rust [Tasks::inv()], both conjuncts:
    (1) every set ready bit belongs to a Pending slot;
    (2) no bit at position >= MAX_TASKS is set (Rust states this as
        [ready < 256]; for a u32 the two are equivalent, and the testbit
        form is the natural one on N). *)
Definition inv (s : exec_state) : Prop :=
  (forall i : nat, (i < MAX_TASKS)%nat ->
     ready_bit s i = true -> tstate s i = Pending)
  /\ (forall n : N, (N.of_nat MAX_TASKS <= n)%N ->
        N.testbit (ready s) n = false).

(** Every modeled operation preserves [inv] — the Rocq twin of the
    [ensures self.inv()] clause carried by every Verus mutator. *)
Lemma step_preserves_inv : forall (s : exec_state) (o : op),
  inv s -> inv (step s o).
Proof.
  intros s o [Hpend Hhigh]. destruct o as [h|h|]; simpl.
  - (* Wake h *)
    unfold wake.
    destruct (h <? MAX_TASKS)%nat eqn:Hlt; [|split; assumption].
    destruct (tstate s h) eqn:Hst; try (split; assumption).
    apply Nat.ltb_lt in Hlt.
    split.
    + (* set bits still point at Pending slots *)
      intros i Hi Hbit. simpl in *.
      destruct (Nat.eq_dec i h) as [->|Hne]; [exact Hst|].
      apply Hpend; [exact Hi|].
      unfold ready_bit in *; simpl in Hbit.
      rewrite lor_mask_other in Hbit; [exact Hbit|exact Hne].
    + (* no high bits appear *)
      intros n Hn. simpl.
      rewrite N.lor_spec, mask_spec.
      rewrite Hhigh by exact Hn. simpl.
      apply N.eqb_neq. unfold MAX_TASKS in *. lia.
  - (* Consume h *)
    split.
    + intros i Hi Hbit.
      destruct (Nat.eq_dec i h) as [->|Hne].
      * rewrite consume_clears in Hbit. discriminate.
      * rewrite consume_preserves_other in Hbit by exact Hne.
        simpl. apply Hpend; assumption.
    + intros n Hn. simpl.
      rewrite N.ldiff_spec, Hhigh by exact Hn. reflexivity.
  - (* PickNext *)
    split; assumption.
Qed.

(** Trace closure of the invariant. *)
Theorem run_preserves_inv : forall (t : list op) (s : exec_state),
  inv s -> inv (run s t).
Proof.
  induction t as [|o rest IH]; intros s Hinv; simpl.
  - exact Hinv.
  - apply IH. apply step_preserves_inv. exact Hinv.
Qed.

(* ========================================================================= *)
(** * Closure audit                                                          *)
(* ========================================================================= *)

(** Makes the compile log show each theorem's axiom dependencies — the
    expected output is "Closed under the global context" three times,
    i.e. the proofs rest on Rocq's kernel alone: no axioms, no holes. *)
Print Assumptions no_lost_wakeups.
Print Assumptions delivered_wake_survives.
Print Assumptions run_preserves_inv.

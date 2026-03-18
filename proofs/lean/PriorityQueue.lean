/-!
# Priority Queue Invariants

Properties of the sorted priority queue used by Zephyr's scheduler.
The ready queue and wait queues maintain tasks in priority order,
enabling O(1) access to the highest-priority ready task.

These proofs establish that the fundamental operations (insert, remove-min,
merge) preserve the sorted invariant, bridging to the Verus proofs in the
Gale implementation.
-/

/-! ## Sorted List Model -/

/-- A priority queue element with a priority key and payload. -/
structure PQEntry (α : Type) where
  priority : Nat    -- lower value = higher priority (common convention)
  payload : α
  deriving Repr

/-- A priority queue is a list of entries sorted by priority (ascending). -/
def Sorted : List (PQEntry α) → Prop
  | [] => True
  | [_] => True
  | e1 :: e2 :: es => e1.priority ≤ e2.priority ∧ Sorted (e2 :: es)

/-- Insert an entry into its correct position in a sorted list. -/
def sortedInsert (e : PQEntry α) : List (PQEntry α) → List (PQEntry α)
  | [] => [e]
  | hd :: tl =>
    if e.priority ≤ hd.priority then
      e :: hd :: tl
    else
      hd :: sortedInsert e tl

/-- Remove the first (minimum-priority) element. -/
def removeMin : List (PQEntry α) → Option (PQEntry α) × List (PQEntry α)
  | [] => (none, [])
  | hd :: tl => (some hd, tl)

/-- Length of list after sorted insert increases by 1. -/
theorem sortedInsert_length (e : PQEntry α) (q : List (PQEntry α)) :
    (sortedInsert e q).length = q.length + 1 := by
  induction q with
  | nil => simp [sortedInsert]
  | cons hd tl ih =>
    simp [sortedInsert]
    split
    case isTrue _ => simp
    case isFalse _ => simp [ih]

/-! ## Core Invariant: Insert Preserves Order -/

/-- Inserting into a sorted list preserves sortedness. -/
theorem sorted_insert_preserves_order (e : PQEntry α) (q : List (PQEntry α))
    (h : Sorted q) : Sorted (sortedInsert e q) := by
  induction q with
  | nil =>
    simp [sortedInsert, Sorted]
  | cons hd tl ih =>
    simp [sortedInsert]
    split
    case isTrue hle =>
      unfold Sorted
      exact ⟨hle, h⟩
    case isFalse hnle =>
      have hgt : hd.priority < e.priority := by omega
      cases tl with
      | nil =>
        simp [sortedInsert] at *
        unfold Sorted
        exact ⟨by omega, by simp [Sorted]⟩
      | cons hd2 tl2 =>
        simp [sortedInsert]
        split
        case isTrue hle2 =>
          -- e goes before hd2: hd ≤ e and e ≤ hd2
          unfold Sorted
          constructor
          case left => omega
          case right =>
            unfold Sorted
            exact ⟨hle2, h.2⟩
        case isFalse hnle2 =>
          -- e goes after hd2: need hd ≤ hd2 and sorted rest
          unfold Sorted
          constructor
          case left => exact h.1
          case right =>
            -- ih : Sorted (hd2 :: tl2) → Sorted (sortedInsert e (hd2 :: tl2))
            -- Since hnle2, sortedInsert e (hd2 :: tl2) = hd2 :: sortedInsert e tl2
            have ih_sorted := ih h.2
            simp [sortedInsert, hnle2] at ih_sorted
            exact ih_sorted

/-- The element returned by sortedInsert is findable. -/
theorem sortedInsert_mem (e : PQEntry α) (q : List (PQEntry α)) :
    e ∈ sortedInsert e q := by
  induction q with
  | nil => simp [sortedInsert]
  | cons hd tl ih =>
    simp [sortedInsert]
    split
    case isTrue _ => simp
    case isFalse _ => simp [ih]

/-! ## Min Element Property -/

/-- The first element of a sorted list has the minimum priority. -/
theorem min_element_is_first (e : PQEntry α) (q : List (PQEntry α))
    (h : Sorted (e :: q)) :
    ∀ x, x ∈ q → e.priority ≤ x.priority := by
  intro x hx
  induction q generalizing e with
  | nil => simp at hx
  | cons hd tl ih =>
    cases hx with
    | head => exact h.1
    | tail _ htl =>
      have hhd : e.priority ≤ hd.priority := h.1
      have hx_ge_hd : hd.priority ≤ x.priority :=
        ih hd h.2 htl
      omega

/-- removeMin returns the minimum element. -/
theorem removeMin_is_min (q : List (PQEntry α)) (h : Sorted q) (hne : q ≠ []) :
    match removeMin q with
    | (some e, rest) => ∀ x, x ∈ rest → e.priority ≤ x.priority
    | (none, _) => False := by
  cases q with
  | nil => contradiction
  | cons hd tl =>
    simp [removeMin]
    exact min_element_is_first hd tl h

/-! ## Remove Preserves Order -/

/-- Removing the first element of a sorted list preserves sortedness. -/
theorem remove_min_preserves_order (e : PQEntry α) (q : List (PQEntry α))
    (h : Sorted (e :: q)) :
    Sorted q := by
  cases q with
  | nil => simp [Sorted]
  | cons hd tl => exact h.2

/-- Removing an arbitrary element preserves sortedness. -/
def removeById [BEq β] (id : β) : List (PQEntry β) → List (PQEntry β)
  | [] => []
  | hd :: tl =>
    if hd.payload == id then tl
    else hd :: removeById id tl

/-- Helper: removeById unfolds when the head doesn't match. -/
private theorem removeById_cons_false [BEq β] (id : β) (hd : PQEntry β) (tl : List (PQEntry β))
    (h_ne : (hd.payload == id) = false) :
    removeById id (hd :: tl) = hd :: removeById id tl := by
  simp [removeById, h_ne]

theorem removeById_preserves_order [BEq β] (id : β) (q : List (PQEntry β))
    (h : Sorted q) : Sorted (removeById id q) := by
  induction q with
  | nil => simp [removeById, Sorted]
  | cons hd tl ih =>
    simp [removeById]
    split
    case isTrue _ =>
      cases tl with
      | nil => simp [Sorted]
      | cons hd2 tl2 => exact h.2
    case isFalse hne =>
      have htl_sorted : Sorted tl := by
        cases tl with
        | nil => simp [Sorted]
        | cons hd2 tl2 => exact h.2
      -- Need Sorted (hd :: removeById id tl)
      cases tl with
      | nil => simp [removeById, Sorted]
      | cons hd2 tl2 =>
        simp [removeById]
        split
        case isTrue _ =>
          -- hd2 is removed, need hd ≤ head of tl2
          cases tl2 with
          | nil => simp [Sorted]
          | cons hd3 tl3 =>
            unfold Sorted
            have : hd.priority ≤ hd2.priority := h.1
            have : hd2.priority ≤ hd3.priority := h.2.1
            exact ⟨by omega, h.2.2⟩
        case isFalse hne2 =>
          unfold Sorted
          constructor
          · exact h.1
          · -- ih gives Sorted (removeById id (hd2 :: tl2))
            -- which unfolds to Sorted (hd2 :: removeById id tl2) since hne2
            have ih_result := ih htl_sorted
            simp [removeById, hne2] at ih_result
            exact ih_result

/-! ## Queue Size Invariants -/

/-- After insert, queue size increases by exactly 1. -/
theorem insert_size (e : PQEntry α) (q : List (PQEntry α)) :
    (sortedInsert e q).length = q.length + 1 :=
  sortedInsert_length e q

/-- After removeMin of nonempty queue, size decreases by exactly 1. -/
theorem removeMin_size (q : List (PQEntry α)) (hne : q ≠ []) :
    (removeMin q).2.length = q.length - 1 := by
  cases q with
  | nil => contradiction
  | cons hd tl => simp [removeMin]

/-- Empty queue has no minimum. -/
theorem empty_no_min : (removeMin (α := α) []).1 = none := by
  simp [removeMin]

/-! ## Merge Operation -/

/-- Merge two sorted lists into a sorted result. -/
def merge : List (PQEntry α) → List (PQEntry α) → List (PQEntry α)
  | [], q2 => q2
  | q1, [] => q1
  | e1 :: t1, e2 :: t2 =>
    if e1.priority ≤ e2.priority then
      e1 :: merge t1 (e2 :: t2)
    else
      e2 :: merge (e1 :: t1) t2
termination_by q1 q2 => q1.length + q2.length

/-- Helper: Sorted for cons when we know the head relationship with the tail. -/
private theorem sorted_cons_of_head_le (e : PQEntry α) (rest : List (PQEntry α))
    (h_sorted : Sorted rest)
    (h_le : ∀ x, rest.head? = some x → e.priority ≤ x.priority) :
    Sorted (e :: rest) := by
  cases rest with
  | nil => simp [Sorted]
  | cons hd tl =>
    unfold Sorted
    constructor
    · exact h_le hd (by simp [List.head?])
    · exact h_sorted

/-- The head of a merge result when both inputs are nonempty. -/
private theorem merge_head (e1 : PQEntry α) (t1 : List (PQEntry α))
    (e2 : PQEntry α) (t2 : List (PQEntry α)) :
    (merge (e1 :: t1) (e2 :: t2)).head? =
    if e1.priority ≤ e2.priority then some e1 else some e2 := by
  simp [merge]
  split <;> simp [List.head?]

/-- If p ≤ e1.priority and p ≤ e2.priority, then p ≤ head of merge. -/
private theorem le_merge_head (e1 : PQEntry α) (t1 : List (PQEntry α))
    (e2 : PQEntry α) (t2 : List (PQEntry α))
    (p : Nat) (h1 : p ≤ e1.priority) (h2 : p ≤ e2.priority) :
    ∀ x, (merge (e1 :: t1) (e2 :: t2)).head? = some x →
    p ≤ x.priority := by
  intro x hx
  rw [merge_head] at hx
  split at hx
  · simp at hx; subst hx; exact h1
  · simp at hx; subst hx; exact h2

/-- Merge preserves sortedness. -/
theorem merge_sorted (q1 q2 : List (PQEntry α))
    (h1 : Sorted q1) (h2 : Sorted q2) :
    Sorted (merge q1 q2) := by
  induction q1, q2 using merge.induct with
  | case1 q2 => simpa [merge]
  | case2 e1 t1 => simpa [merge]
  | case3 e1 t1 e2 t2 hle ih =>
    -- e1.priority <= e2.priority, result = e1 :: merge t1 (e2 :: t2)
    simp [merge, hle]
    have ht1_sorted : Sorted t1 := by
      cases t1 with
      | nil => simp [Sorted]
      | cons h t => exact h1.2
    have ih_sorted := ih ht1_sorted h2
    apply sorted_cons_of_head_le e1 (merge t1 (e2 :: t2)) ih_sorted
    intro x hx
    cases t1 with
    | nil =>
      -- merge [] (e2 :: t2) = e2 :: t2, so head? = some e2
      simp [merge, List.head?] at hx
      subst hx; exact hle
    | cons hd1 tl1 =>
      have h_e1_hd1 : e1.priority ≤ hd1.priority := h1.1
      exact le_merge_head hd1 tl1 e2 t2 e1.priority h_e1_hd1 hle x hx
  | case4 e1 t1 e2 t2 hnle ih =>
    -- e1.priority > e2.priority, result = e2 :: merge (e1 :: t1) t2
    simp [merge, hnle]
    have ht2_sorted : Sorted t2 := by
      cases t2 with
      | nil => simp [Sorted]
      | cons h t => exact h2.2
    have ih_sorted := ih h1 ht2_sorted
    apply sorted_cons_of_head_le e2 (merge (e1 :: t1) t2) ih_sorted
    intro x hx
    cases t2 with
    | nil =>
      -- merge (e1 :: t1) [] = e1 :: t1, so head? = some e1
      simp [merge, List.head?] at hx
      subst hx
      exact Nat.le_of_lt (Nat.gt_of_not_le hnle)
    | cons hd2 tl2 =>
      have h_e2_hd2 : e2.priority ≤ hd2.priority := h2.1
      have h_e2_e1 : e2.priority ≤ e1.priority :=
        Nat.le_of_lt (Nat.gt_of_not_le hnle)
      exact le_merge_head e1 t1 hd2 tl2 e2.priority h_e2_e1 h_e2_hd2 x hx

/-- Merge preserves total length. -/
theorem merge_length (q1 q2 : List (PQEntry α)) :
    (merge q1 q2).length = q1.length + q2.length := by
  induction q1, q2 using merge.induct with
  | case1 q2 => simp [merge]
  | case2 e1 t1 => simp [merge]
  | case3 e1 t1 e2 t2 hle ih =>
    simp [merge, hle, ih]
    omega
  | case4 e1 t1 e2 t2 hnle ih =>
    simp [merge, hnle, ih]
    omega

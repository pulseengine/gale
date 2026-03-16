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
def Sorted : List (PQEntry α) -> Prop
  | [] => True
  | [_] => True
  | e1 :: e2 :: es => e1.priority <= e2.priority /\ Sorted (e2 :: es)

/-- Insert an entry into its correct position in a sorted list. -/
def sortedInsert (e : PQEntry α) : List (PQEntry α) -> List (PQEntry α)
  | [] => [e]
  | hd :: tl =>
    if e.priority <= hd.priority then
      e :: hd :: tl
    else
      hd :: sortedInsert e tl

/-- Remove the first (minimum-priority) element. -/
def removeMin : List (PQEntry α) -> Option (PQEntry α) × List (PQEntry α)
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
      -- e.priority <= hd.priority, so e goes first
      unfold Sorted
      exact ⟨hle, h⟩
    case isFalse hnle =>
      -- e.priority > hd.priority, recurse into tail
      have hgt : hd.priority < e.priority := by omega
      -- Need to show Sorted (hd :: sortedInsert e tl)
      have ih_sorted : Sorted (sortedInsert e tl) := by
        cases tl with
        | nil => exact ih (by simp [Sorted])
        | cons hd2 tl2 =>
          have htl_sorted : Sorted (hd2 :: tl2) := by
            unfold Sorted at h
            exact h.2
          exact ih htl_sorted
      -- Now show hd fits before sortedInsert e tl
      cases tl with
      | nil =>
        simp [sortedInsert] at *
        unfold Sorted
        exact ⟨by omega, by simp [Sorted]⟩
      | cons hd2 tl2 =>
        simp [sortedInsert] at *
        split
        case isTrue hle2 =>
          -- e goes before hd2: hd <= e <= hd2
          unfold Sorted
          constructor
          case left => omega
          case right =>
            unfold Sorted
            exact ⟨hle2, h.2⟩
        case isFalse hnle2 =>
          -- e goes after hd2: need hd <= hd2 and sorted rest
          unfold Sorted
          constructor
          case left => exact h.1
          case right => exact ih_sorted

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
    forall x, x ∈ q -> e.priority <= x.priority := by
  intro x hx
  induction q with
  | nil => simp at hx
  | cons hd tl ih =>
    cases hx with
    | head => exact h.1
    | tail _ htl =>
      have htl_sorted : Sorted (hd :: tl) := h.2
      have hhd : e.priority <= hd.priority := h.1
      have hx_ge_hd : hd.priority <= x.priority := by
        exact ih htl_sorted htl
      omega

/-- removeMin returns the minimum element. -/
theorem removeMin_is_min (q : List (PQEntry α)) (h : Sorted q) (hne : q ≠ []) :
    match removeMin q with
    | (some e, rest) => forall x, x ∈ rest -> e.priority <= x.priority
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
def removeById [BEq β] (id : β) : List (PQEntry β) -> List (PQEntry β)
  | [] => []
  | hd :: tl =>
    if hd.payload == id then tl
    else hd :: removeById id tl

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
    case isFalse _ =>
      have htl_sorted : Sorted tl := by
        cases tl with
        | nil => simp [Sorted]
        | cons hd2 tl2 => exact h.2
      have ih_result := ih htl_sorted
      -- Need Sorted (hd :: removeById id tl)
      cases tl with
      | nil => simp [removeById, Sorted]
      | cons hd2 tl2 =>
        simp [removeById]
        split
        case isTrue _ =>
          -- hd2 is removed, need hd <= head of tl2
          cases tl2 with
          | nil => simp [Sorted]
          | cons hd3 tl3 =>
            unfold Sorted
            have : hd.priority <= hd2.priority := h.1
            have : hd2.priority <= hd3.priority := h.2.1
            exact ⟨by omega, h.2.2⟩
        case isFalse _ =>
          unfold Sorted
          exact ⟨h.1, ih_result⟩

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
def merge : List (PQEntry α) -> List (PQEntry α) -> List (PQEntry α)
  | [], q2 => q2
  | q1, [] => q1
  | e1 :: t1, e2 :: t2 =>
    if e1.priority <= e2.priority then
      e1 :: merge t1 (e2 :: t2)
    else
      e2 :: merge (e1 :: t1) t2
termination_by q1 q2 => q1.length + q2.length

/-- Helper: head priority of merge result is min of input heads. -/
private theorem merge_head_priority (e1 : PQEntry α) (t1 : List (PQEntry α))
    (e2 : PQEntry α) (t2 : List (PQEntry α)) :
    match (merge (e1 :: t1) (e2 :: t2)).head? with
    | some e => e.priority = min e1.priority e2.priority
    | none => False := by
  simp [merge]
  split
  case isTrue hle =>
    simp [List.head?]
    omega
  case isFalse hnle =>
    simp [List.head?]
    omega

/-- Helper: the head priority of a merge is <= both input heads' priorities. -/
private theorem merge_head_le_left (e1 : PQEntry α) (t1 : List (PQEntry α))
    (e2 : PQEntry α) (t2 : List (PQEntry α)) :
    ∀ e, (merge (e1 :: t1) (e2 :: t2)).head? = some e →
    e.priority <= e1.priority := by
  simp [merge]
  split <;> simp [List.head?] <;> intro e he <;> subst he <;> omega

private theorem merge_head_le_right (e1 : PQEntry α) (t1 : List (PQEntry α))
    (e2 : PQEntry α) (t2 : List (PQEntry α)) :
    ∀ e, (merge (e1 :: t1) (e2 :: t2)).head? = some e →
    e.priority <= e2.priority := by
  simp [merge]
  split <;> simp [List.head?] <;> intro e he <;> subst he <;> omega

/-- Helper: Sorted for cons when we know the head relationship with the tail. -/
private theorem sorted_cons_of_head_le (e : PQEntry α) (rest : List (PQEntry α))
    (h_sorted : Sorted rest)
    (h_le : ∀ x, rest.head? = some x → e.priority <= x.priority) :
    Sorted (e :: rest) := by
  cases rest with
  | nil => simp [Sorted]
  | cons hd tl =>
    unfold Sorted
    constructor
    · exact h_le hd (by simp [List.head?])
    · exact h_sorted

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
      simp [merge] at hx
      simp [List.head?] at hx
      subst hx; exact hle
    | cons hd1 tl1 =>
      have h_e1_hd1 : e1.priority <= hd1.priority := h1.1
      have := merge_head_le_left hd1 tl1 e2 t2 x hx
      have := merge_head_le_right hd1 tl1 e2 t2 x hx
      omega
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
      simp [merge] at hx
      simp [List.head?] at hx
      subst hx; omega
    | cons hd2 tl2 =>
      have h_e2_hd2 : e2.priority <= hd2.priority := h2.1
      have := merge_head_le_left e1 t1 hd2 tl2 x hx
      have := merge_head_le_right e1 t1 hd2 tl2 x hx
      omega

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

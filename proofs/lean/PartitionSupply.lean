/-
  gale's side of the T3 supply obligation (REQ-OS-SCHED-001).

  spar's `ArincSupply.lean` proves the inner response-time analysis SOUND
  *conditional on* `SupplyGuarantee (∀ t, lsbf Π Θ t ≤ sbf t)` — and states
  explicitly that spar does NOT prove it: it is the partition scheduler's
  obligation, i.e. gale's.

  This file discharges the ARITHMETIC core of that obligation for gust's static
  major frame. Two facts make gust's case easier than the general periodic
  resource spar models, and one makes it harder:

  EASIER: the frame is static and cyclic (`MajorFrame::check` proves the windows
  tile `[0, frame_len)` with no gap, no overlap, every budget > 0), so over any
  Π consecutive ticks a partition receives exactly its useful budget.

  HARDER: `Switcher::tick` fires the preemption at `t == offset + budget - 1`, so
  each owned window loses its FINAL tick to the ctx-save/region-swap/ctx-resume
  sequence. A partition owning `k` windows receives `Θ_eff = Θ − k`, NOT `Θ`.
  Instantiating with the raw budget is UNSOUND: `lsbf`'s leading rate is `Θ/Π`
  against a real rate of `(Θ−k)/Π`, so the linear terms diverge. Measured by
  exhaustive simulation over all worst-case start offsets: for Π=100, Θ=40, k=1
  the guarantee first fails at t = 2560; for k=3 at t = 828.
-/

import Mathlib.Tactic

namespace Gust.PartitionSupply

/-- Worst-case contiguous blackout of a periodic resource `(Π, Θ)`: `2(Π − Θ)`.
    Mirrors `blackout` in spar's `ArincSupply.lean`. -/
def blackout (Pi Theta : Nat) : Nat := 2 * (Pi - Theta)

/-- Linear supply lower bound `lsbf_Γ(t) = ⌊Θ·(t − 2(Π−Θ))/Π⌋`, zero at or below
    the blackout. Mirrors `lsbf` in spar's `ArincSupply.lean`. -/
def lsbf (Pi Theta t : Nat) : Nat :=
  if t ≤ blackout Pi Theta then 0 else Theta * (t - blackout Pi Theta) / Pi

/-- **The bridge lemma.** For a utilisation of at most one half, `lsbf` is
    dominated by the COMPLETE frames alone — the partial frame at the end of the
    interval need not be counted.

    This is what lets gust's supply obligation be discharged from the frame
    invariant without reasoning about where the interval starts: a static cyclic
    frame delivers exactly `Θ` useful ticks per Π, so `⌊t/Π⌋·Θ` is a sound floor
    on supply over any interval of length `t`, and this lemma shows that floor
    already dominates `lsbf`.

    The hypothesis `2Θ ≤ Π + 1` is doing real work: it forces the blackout term
    `2(Π−Θ)` to be at least `Π − 1`, hence at least the largest possible partial
    frame `t % Π`. Above one-half utilisation the partial frame must be counted
    and this proof does not apply. -/
theorem lsbf_le_full_frames
    (Pi Theta t : Nat) (hPi : 0 < Pi) (hU : 2 * Theta ≤ Pi + 1) :
    lsbf Pi Theta t ≤ (t / Pi) * Theta := by
  unfold lsbf
  by_cases h : t ≤ blackout Pi Theta
  · rw [if_pos h]
    exact Nat.zero_le _
  · rw [if_neg h]
    -- the blackout swallows any partial frame, because 2Θ ≤ Π + 1
    have hb : Pi - 1 ≤ blackout Pi Theta := by
      unfold blackout at *; omega
    have hmod : t % Pi < Pi := Nat.mod_lt _ hPi
    have hdm : Pi * (t / Pi) + t % Pi = t := Nat.div_add_mod t Pi
    -- hence what remains after the blackout fits inside the complete frames
    have hle : t - blackout Pi Theta ≤ (t / Pi) * Pi := by
      have : (t / Pi) * Pi = Pi * (t / Pi) := Nat.mul_comm _ _
      omega
    calc Theta * (t - blackout Pi Theta) / Pi
        ≤ Theta * ((t / Pi) * Pi) / Pi := by
          exact Nat.div_le_div_right (Nat.mul_le_mul_left _ hle)
      _ = (t / Pi) * Theta := by
          rw [← Nat.mul_assoc, Nat.mul_div_cancel _ hPi, Nat.mul_comm]

/-- **The utilisation hypothesis is load-bearing, not decorative.**

    At `Π = 5, Θ = 4` (so `2Θ = 8 > Π + 1 = 6`) the blackout is only `2`, and at
    `t = 4` the linear bound already credits one tick of supply while no complete
    frame has elapsed:

        lsbf 5 4 4 = ⌊4·(4−2)/5⌋ = 1   >   (4/5)·4 = 0

    So `lsbf_le_full_frames` is FALSE without `2Θ ≤ Π + 1`. Above one-half
    utilisation the partial frame carries supply that must be counted, and a
    discharge for that regime needs a different argument — it is not an artefact
    of how this proof was written.

    Note `Θ < Π` here: this is not the degenerate `Θ = Π` case. -/
example : ¬ (lsbf 5 4 4 ≤ (4 / 5) * 4) := by decide

/-- Non-vacuity: the hypothesis is satisfiable at a realistic operating point.
    A partition owning one 40-tick window of a 100-tick major frame, minus the
    single tick `Switcher::tick` spends on the switch, is `Θ_eff = 39`. -/
example : 2 * 39 ≤ 100 + 1 := by decide

/-- **The arithmetic step that removes the utilisation bound.** `lsbf` is
    dominated by the complete frames PLUS what the partial frame cannot withhold,
    at every utilisation `Θ ≤ Π`.

    This is what `lsbf_le_full_frames` could not give: that lemma throws the
    partial frame away, and is genuinely FALSE above one half (its counterexample
    above). Keeping the partial term makes the comparison hold everywhere. -/
theorem lsbf_le_strong_floor (Pi Th t : Nat) (hPi : 0 < Pi) (hTh : Th ≤ Pi) :
    lsbf Pi Th t ≤ (t / Pi) * Th + (t % Pi - (Pi - Th)) := by
  unfold lsbf blackout
  by_cases h : t ≤ 2 * (Pi - Th)
  · rw [if_pos h]; exact Nat.zero_le _
  · rw [if_neg h]
    have hdm : (t / Pi) * Pi + t % Pi = t := by
      rw [Nat.mul_comm]; exact Nat.div_add_mod t Pi
    have hrlt : t % Pi < Pi := Nat.mod_lt t hPi
    have key : Th * (t - 2 * (Pi - Th))
        ≤ Pi * ((t / Pi) * Th + (t % Pi - (Pi - Th))) := by
      rcases Nat.lt_or_ge (t % Pi) (2 * (Pi - Th)) with h2 | h2
      · -- the remainder is inside the blackout: all supply comes from full frames
        have ht : t - 2 * (Pi - Th) ≤ (t / Pi) * Pi := by omega
        calc Th * (t - 2 * (Pi - Th))
            ≤ Th * ((t / Pi) * Pi) := Nat.mul_le_mul_left _ ht
          _ = Pi * ((t / Pi) * Th) := by ring
          _ ≤ Pi * ((t / Pi) * Th + (t % Pi - (Pi - Th))) :=
              Nat.mul_le_mul_left _ (Nat.le_add_right _ _)
      · -- the remainder outruns the blackout: the partial frame carries the rest
        have ht : t - 2 * (Pi - Th) = (t / Pi) * Pi + (t % Pi - 2 * (Pi - Th)) := by omega
        have hsub : t % Pi - 2 * (Pi - Th) ≤ t % Pi - (Pi - Th) := by omega
        rw [ht, Nat.mul_add]
        have h1 : Th * (t % Pi - 2 * (Pi - Th)) ≤ Pi * (t % Pi - (Pi - Th)) :=
          Nat.mul_le_mul hTh hsub
        calc Th * ((t / Pi) * Pi) + Th * (t % Pi - 2 * (Pi - Th))
            ≤ Pi * ((t / Pi) * Th) + Pi * (t % Pi - (Pi - Th)) :=
              Nat.add_le_add (le_of_eq (by ring)) h1
          _ = Pi * ((t / Pi) * Th + (t % Pi - (Pi - Th))) := by ring
    calc Th * (t - 2 * (Pi - Th)) / Pi
        ≤ Pi * ((t / Pi) * Th + (t % Pi - (Pi - Th))) / Pi := Nat.div_le_div_right key
      _ = (t / Pi) * Th + (t % Pi - (Pi - Th)) := Nat.mul_div_cancel_left _ hPi

/-! ## Part 2 — the supply floor from a static cyclic frame -/

/-- Supply over `[s, s+t)`: ticks whose position in the major frame is useful to
    this partition. `u` is a predicate on the frame-relative residue, so
    periodicity is structural rather than assumed. -/
def supply (Pi : Nat) (u : Nat → Bool) (s t : Nat) : Nat :=
  ∑ i ∈ Finset.range t, (if u ((s + i) % Pi) then 1 else 0)

/-- Useful ticks per frame — `Θ_eff`: the budget a partition owns MINUS one tick
    per owned window, since `Switcher::tick` fires at `end - 1`. -/
def thetaEff (Pi : Nat) (u : Nat → Bool) : Nat :=
  ∑ i ∈ Finset.range Pi, (if u (i % Pi) then 1 else 0)

theorem supply_succ (Pi : Nat) (u : Nat → Bool) (s t : Nat) :
    supply Pi u s (t + 1) = supply Pi u s t + (if u ((s + t) % Pi) then 1 else 0) := by
  unfold supply; rw [Finset.sum_range_succ]

/-- Supply splits at any interior point. -/
theorem supply_add (Pi : Nat) (u : Nat → Bool) (s a b : Nat) :
    supply Pi u s (a + b) = supply Pi u s a + supply Pi u (s + a) b := by
  induction b with
  | zero => simp [supply]
  | succ n ih =>
      have hb : a + (n + 1) = (a + n) + 1 := by omega
      have hs : s + (a + n) = (s + a) + n := by omega
      rw [hb, supply_succ, ih, hs, supply_succ, Nat.add_assoc]

/-- Supply is monotone in the interval length. -/
theorem supply_mono (Pi : Nat) (u : Nat → Bool) (s : Nat) {a b : Nat} (h : a ≤ b) :
    supply Pi u s a ≤ supply Pi u s b := by
  unfold supply
  exact Finset.sum_le_sum_of_subset (Finset.range_mono h)

/-- One tick of supply. -/
theorem supply_one (Pi : Nat) (u : Nat → Bool) (s : Nat) :
    supply Pi u s 1 = (if u (s % Pi) then 1 else 0) := by
  unfold supply
  rw [Finset.sum_range_one, Nat.add_zero]

/-- **Sliding the window by one tick does not change a full frame's supply.**
    The tick that leaves the front and the tick that joins the back have the same
    frame-relative position, because `(s + Π) % Π = s % Π`. This is what replaces
    a complete-residue-system bijection argument. -/
theorem supply_shift (Pi : Nat) (u : Nat → Bool) (s : Nat) :
    supply Pi u (s + 1) Pi = supply Pi u s Pi := by
  have h1 := supply_add Pi u s 1 Pi
  have h2 := supply_succ Pi u s Pi
  rw [show (1 : Nat) + Pi = Pi + 1 from by omega] at h1
  rw [h2, supply_one, Nat.add_mod_right] at h1
  omega

/-- **Any Π consecutive ticks deliver exactly `Θ_eff`** — independent of where the
    interval starts. This is gust's static cyclic major frame doing the work: the
    windows tile `[0, frame_len)` with no gap and no overlap (`MajorFrame::check`,
    Verus/Kani-proven), so the supply in a full frame does not depend on phase. -/
theorem supply_period (Pi : Nat) (u : Nat → Bool) (s : Nat) :
    supply Pi u s Pi = thetaEff Pi u := by
  induction s with
  | zero => unfold supply thetaEff; simp
  | succ n ih => rw [supply_shift]; exact ih

/-- `k` complete frames deliver exactly `k · Θ_eff`, from any start. -/
theorem supply_full (Pi : Nat) (u : Nat → Bool) (k : Nat) : ∀ s : Nat,
    supply Pi u s (k * Pi) = k * thetaEff Pi u := by
  induction k with
  | zero => intro s; simp [supply]
  | succ n ih =>
      intro s
      rw [show (n + 1) * Pi = Pi + n * Pi from by ring, supply_add, supply_period, ih]
      ring

/-- **The supply floor.** Over any interval of length `t`, from any start, a
    partition receives at least `⌊t/Π⌋ · Θ_eff` — the complete frames alone,
    ignoring the partial frame at the end. -/
theorem supply_floor (Pi : Nat) (u : Nat → Bool) (s t : Nat) :
    (t / Pi) * thetaEff Pi u ≤ supply Pi u s t := by
  calc (t / Pi) * thetaEff Pi u
      = supply Pi u s ((t / Pi) * Pi) := (supply_full Pi u (t / Pi) s).symm
    _ ≤ supply Pi u s t := supply_mono Pi u s (Nat.div_mul_le_self t Pi)

/-! ## Part 3 — counting the partial frame, which removes the utilisation bound -/

/-- A tick is either useful or not: supply and its complement partition the
    interval. -/
theorem supply_add_compl (Pi : Nat) (u : Nat → Bool) (s t : Nat) :
    supply Pi u s t + supply Pi (fun i => !u i) s t = t := by
  unfold supply
  rw [← Finset.sum_add_distrib]
  have h : ∀ x : Nat,
      ((if u ((s + x) % Pi) = true then 1 else 0)
        + if (fun i => !u i) ((s + x) % Pi) = true then 1 else 0) = 1 := by
    intro x; cases hb : u ((s + x) % Pi) <;> simp [hb]
  rw [Finset.sum_congr rfl (fun x _ => h x)]
  simp

/-- Dually, the useful and non-useful ticks of one frame sum to `Π`. -/
theorem thetaEff_add_compl (Pi : Nat) (u : Nat → Bool) :
    thetaEff Pi u + thetaEff Pi (fun i => !u i) = Pi := by
  unfold thetaEff
  rw [← Finset.sum_add_distrib]
  have h : ∀ x : Nat,
      ((if u (x % Pi) = true then 1 else 0)
        + if (fun i => !u i) (x % Pi) = true then 1 else 0) = 1 := by
    intro x; cases hb : u (x % Pi) <;> simp [hb]
  rw [Finset.sum_congr rfl (fun x _ => h x)]
  simp

/-- **The partial frame carries supply too.** Over `r ≤ Π` consecutive ticks a
    partition loses at most the whole non-useful budget `Π − Θ_eff`, because a
    sub-interval of a frame cannot contain more non-useful ticks than the frame
    does. This is the piece the `≤ ½` bound was standing in for. -/
theorem supply_partial (Pi : Nat) (u : Nat → Bool) (s r : Nat) (hr : r ≤ Pi) :
    r ≤ supply Pi u s r + (Pi - thetaEff Pi u) := by
  have hsum := supply_add_compl Pi u s r
  have hmono : supply Pi (fun i => !u i) s r ≤ supply Pi (fun i => !u i) s Pi :=
    supply_mono Pi _ s hr
  have hper : supply Pi (fun i => !u i) s Pi = thetaEff Pi (fun i => !u i) :=
    supply_period Pi _ s
  have hth := thetaEff_add_compl Pi u
  omega

/-- **The strong supply floor.** Over any interval of length `t`, from any start,
    a partition receives at least the complete frames PLUS whatever the partial
    frame cannot withhold:

        ⌊t/Π⌋ · Θ_eff  +  (t mod Π  −  (Π − Θ_eff))

    The second term is what `supply_floor` throws away. It is zero at low
    utilisation — which is why the `≤ ½` route worked — and it is exactly what is
    needed above one half. -/
theorem supply_floor_strong (Pi : Nat) (u : Nat → Bool) (s t : Nat) (hPi : 0 < Pi) :
    (t / Pi) * thetaEff Pi u + (t % Pi - (Pi - thetaEff Pi u)) ≤ supply Pi u s t := by
  have hsplit : (t / Pi) * Pi + t % Pi = t := by
    rw [Nat.mul_comm]; exact Nat.div_add_mod t Pi
  have hadd := supply_add Pi u s ((t / Pi) * Pi) (t % Pi)
  rw [hsplit, supply_full] at hadd
  have hp := supply_partial Pi u (s + (t / Pi) * Pi) (t % Pi)
    (Nat.le_of_lt (Nat.mod_lt t hPi))
  omega

/-! ## The obligation spar left to gale -/

/-- **`SupplyGuarantee`, discharged for gust's static major frame at utilisation
    up to one half.**

    spar's `ArincSupply.lean` proves the inner response-time analysis sound
    *conditional on* `∀ t, lsbf Π Θ t ≤ sbf t`, and states that spar does NOT
    prove it — it is the partition scheduler's obligation. This is that
    obligation, discharged from the frame's periodic structure:

    * `supply_floor` — the cyclic frame delivers `⌊t/Π⌋ · Θ_eff` from ANY start,
      so no reasoning about phase or interval alignment is needed;
    * `lsbf_le_full_frames` — those complete frames already dominate `lsbf`.

    `Θ` here is `thetaEff`, never the raw window budget: see the header.

    **Superseded by `supplyGuarantee`, and kept deliberately.** This is the
    simpler route — complete frames only — and it is the one that fails above one
    half, because `lsbf_le_full_frames` throws the partial frame away. Keeping it
    beside the unconditional result records that the `≤ ½` restriction was a
    property of the ROUTE, not of the conclusion: `supplyGuarantee` reaches every
    utilisation by keeping the partial term (`supply_floor_strong` +
    `lsbf_le_strong_floor`). The counterexample below still bites this lemma.
-/
theorem supplyGuarantee_of_half_utilisation
    (Pi : Nat) (u : Nat → Bool) (s t : Nat)
    (hPi : 0 < Pi) (hU : 2 * thetaEff Pi u ≤ Pi + 1) :
    lsbf Pi (thetaEff Pi u) t ≤ supply Pi u s t :=
  le_trans (lsbf_le_full_frames Pi (thetaEff Pi u) t hPi hU) (supply_floor Pi u s t)

/-- A frame cannot contain more useful ticks than it has ticks. Immediate from
    `thetaEff_add_compl`. -/
theorem thetaEff_le (Pi : Nat) (u : Nat → Bool) : thetaEff Pi u ≤ Pi := by
  have h := thetaEff_add_compl Pi u
  omega

/-- **`SupplyGuarantee`, discharged UNCONDITIONALLY for gust's static major
    frame.**

    spar's `ArincSupply.lean` proves the inner response-time analysis sound
    *conditional on* `∀ t, lsbf Π Θ t ≤ sbf t`, and states that spar does NOT
    prove it — it is the partition scheduler's obligation. This is that
    obligation, discharged from the frame's periodic structure alone, at every
    utilisation:

    * `supply_floor_strong` — the static cyclic frame delivers `⌊t/Π⌋·Θ_eff` plus
      whatever the partial frame cannot withhold, from ANY start, so no reasoning
      about phase or interval alignment is needed;
    * `lsbf_le_strong_floor` — that floor dominates `lsbf` for every `Θ ≤ Π`.

    `Θ` is `thetaEff` throughout, never the raw window budget — `Switcher::tick`
    fires at `end - 1`, so a partition owning `k` windows receives `Θ − k`, and
    instantiating with the raw budget is UNSOUND (see the header). -/
theorem supplyGuarantee (Pi : Nat) (u : Nat → Bool) (s t : Nat) (hPi : 0 < Pi) :
    lsbf Pi (thetaEff Pi u) t ≤ supply Pi u s t :=
  le_trans (lsbf_le_strong_floor Pi (thetaEff Pi u) t hPi (thetaEff_le Pi u))
           (supply_floor_strong Pi u s t hPi)

/-! ## Part 4 — connecting `u` to a concrete major-frame window -/

/-- The residue predicate induced by ONE window `[a, a+w)` of the major frame.

    The final tick is NOT useful: `Switcher::tick` fires at `t == offset + budget
    - 1`, so each owned window spends its last tick on the
    `ctx-save -> region-swap -> ctx-resume` sequence. This is where `Θ_eff` comes
    from — it is a property of the switch FSM, not an accounting choice. -/
def windowUseful (a w : Nat) : Nat → Bool :=
  fun i => decide (a ≤ i ∧ i < a + w - 1)

/-- **`Θ_eff = Θ − 1` for a single window, as a theorem rather than a comment.**

    A partition owning one window of `w` ticks in a frame of period `Π` receives
    `w − 1` useful ticks per frame. Instantiating spar's analysis with the raw
    budget `w` is therefore not a conservative approximation — it is a claim this
    theorem contradicts. -/
theorem thetaEff_window (Pi a w : Nat) (hw : 0 < w) (hfit : a + w ≤ Pi) :
    thetaEff Pi (windowUseful a w) = w - 1 := by
  unfold thetaEff windowUseful
  have hcongr : ∀ i ∈ Finset.range Pi,
      (if (decide (a ≤ i % Pi ∧ i % Pi < a + w - 1) : Bool) = true then 1 else 0)
        = (if (decide (a ≤ i ∧ i < a + w - 1) : Bool) = true then 1 else 0) := by
    intro i hi
    rw [Nat.mod_eq_of_lt (Finset.mem_range.mp hi)]
  rw [Finset.sum_congr rfl hcongr]
  have hfilter : (Finset.range Pi).filter (fun i => (decide (a ≤ i ∧ i < a + w - 1) : Bool) = true)
      = Finset.Ico a (a + w - 1) := by
    ext x
    simp only [Finset.mem_filter, Finset.mem_range, Finset.mem_Ico, decide_eq_true_eq]
    omega
  rw [← Finset.card_filter, hfilter, Nat.card_Ico]
  omega

/-- **`Θ_eff` is additive over disjoint window sets.** If no tick is useful to
    both, the useful counts add. This is the tool for a partition owning several
    windows of the major frame — `MajorFrame::check` gives exactly the
    disjointness hypothesis, since the windows tile `[0, frame_len)` without
    overlap. -/
theorem thetaEff_disjoint_add (Pi : Nat) (u v : Nat → Bool)
    (hdisj : ∀ i, u i = true → v i = true → False) :
    thetaEff Pi (fun i => u i || v i) = thetaEff Pi u + thetaEff Pi v := by
  unfold thetaEff
  rw [← Finset.sum_add_distrib]
  refine Finset.sum_congr rfl (fun i _ => ?_)
  cases hu : u (i % Pi) <;> cases hv : v (i % Pi) <;>
    simp_all

/-- **`SupplyGuarantee` for a concrete major-frame window.** A partition owning
    the window `[a, a+w)` of a `Π`-tick frame is guaranteed `lsbf Π (w−1)` — the
    raw budget MINUS the tick the switch consumes. -/
theorem supplyGuarantee_window (Pi a w s t : Nat)
    (hPi : 0 < Pi) (hw : 0 < w) (hfit : a + w ≤ Pi) :
    lsbf Pi (w - 1) t ≤ supply Pi (windowUseful a w) s t := by
  have h := supplyGuarantee Pi (windowUseful a w) s t hPi
  rwa [thetaEff_window Pi a w hw hfit] at h

/-- **`Θ_eff = Θ − 2` for a partition owning two windows.** Each owned window
    loses its own final tick to the switch, so the losses accumulate: `k` windows
    cost `k` ticks, not one. This is why splitting a partition's budget across
    more windows — the natural move to reduce its blackout — makes the raw-budget
    instantiation fail SOONER, not later. -/
theorem thetaEff_two_windows (Pi a1 w1 a2 w2 : Nat)
    (hw1 : 0 < w1) (hw2 : 0 < w2)
    (hfit1 : a1 + w1 ≤ Pi) (hfit2 : a2 + w2 ≤ Pi)
    (hsep : a1 + w1 ≤ a2) :
    thetaEff Pi (fun i => windowUseful a1 w1 i || windowUseful a2 w2 i)
      = (w1 + w2) - 2 := by
  have hdisj : ∀ i, windowUseful a1 w1 i = true → windowUseful a2 w2 i = true → False := by
    intro i h1 h2
    unfold windowUseful at h1 h2
    simp only [decide_eq_true_eq] at h1 h2
    omega
  rw [thetaEff_disjoint_add Pi _ _ hdisj,
      thetaEff_window Pi a1 w1 hw1 hfit1,
      thetaEff_window Pi a2 w2 hw2 hfit2]
  omega

/-- The guarantee for a two-window partition, with the two switch ticks paid. -/
theorem supplyGuarantee_two_windows (Pi a1 w1 a2 w2 s t : Nat)
    (hPi : 0 < Pi) (hw1 : 0 < w1) (hw2 : 0 < w2)
    (hfit1 : a1 + w1 ≤ Pi) (hfit2 : a2 + w2 ≤ Pi) (hsep : a1 + w1 ≤ a2) :
    lsbf Pi ((w1 + w2) - 2) t
      ≤ supply Pi (fun i => windowUseful a1 w1 i || windowUseful a2 w2 i) s t := by
  have h := supplyGuarantee Pi (fun i => windowUseful a1 w1 i || windowUseful a2 w2 i) s t hPi
  rwa [thetaEff_two_windows Pi a1 w1 a2 w2 hw1 hw2 hfit1 hfit2 hsep] at h

/-- **Instantiating with the RAW window budget is unsound — machine-checked.**

    Frame `Π = 4`, one window `[0, 3)` (a proper part of the frame, `w < Π`),
    interval starting at `s = 2`, length `t = 6`:

        lsbf 4 3 6 = 3   but   supply = 2

    So the guarantee FAILS with the raw budget `w = 3`. This was previously
    recorded here as a simulation result; it is now a theorem. `Switcher::tick`
    firing at `end - 1` is not an accounting detail that can be rounded away. -/
example : ¬ (lsbf 4 3 6 ≤ supply 4 (windowUseful 0 3) 2 6) := by decide

/-- The same instance with `Θ_eff = w − 1 = 2` holds, as `supplyGuarantee_window`
    requires — so the failure above is specifically the raw budget, not the
    window or the interval. -/
example : lsbf 4 2 6 ≤ supply 4 (windowUseful 0 3) 2 6 := by decide

/-- And `Θ_eff` really is `2` here, so the two examples are about the same frame. -/
example : thetaEff 4 (windowUseful 0 3) = 2 := by decide

/-- Definitional sanity, so the theorem above cannot hold because `supply` or
    `thetaEff` mean something other than intended. A partition owning ticks
    `[0, 39)` of a 100-tick major frame — one 40-tick window minus the tick
    `Switcher::tick` spends on the switch — has `Θ_eff = 39`, and satisfies the
    utilisation hypothesis with room to spare. -/
example : thetaEff 100 (fun i => decide (i < 39)) = 39 := by decide

end Gust.PartitionSupply

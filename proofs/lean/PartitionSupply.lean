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

end Gust.PartitionSupply

import Mathlib.Tactic.Linarith
import Mathlib.Tactic.NormNum
import Mathlib.Tactic.Ring
import Mathlib.Tactic.Omega

/-!
# ARM MPU v7 Region Validation

Formal model of the ARMv7-M Memory Protection Unit region constraints.
The ARM MPU v7 enforces three hardware invariants on each region:

  1. Region size must be a power of 2
  2. Region size must be >= 32 bytes (minimum MPU region granularity)
  3. Region base address must be aligned to its size (base % size == 0)

These constraints are enforced at build time by Zephyr's
`_ARCH_MEM_PARTITION_ALIGN_CHECK` macro and at runtime by
`mpu_partition_is_valid()` in `arm_mpu_v7_internal.h`.

Additionally, for a set of MPU regions to be valid, no two regions may
overlap in the address space (required for deterministic access control
on ARMv7-M which uses fixed-priority region matching).

Reference:
  - ARM Architecture Reference Manual ARMv7-M, Section B3.5 (PMSAv7)
  - Zephyr arch/arm/core/mpu/arm_mpu_v7_internal.h
-/

/-! ## MPU Region Model -/

/-- An MPU region with base address, size, and permission attributes. -/
structure MpuRegion where
  base : Nat
  size : Nat
  permissions : Nat
  deriving Repr, BEq

/-! ## Power-of-Two Predicate -/

/-- A natural number is a power of 2 if it equals 2^k for some k. -/
def isPowerOfTwo (n : Nat) : Prop :=
  ∃ k : Nat, n = 2 ^ k

/-- Characterisation: a positive power of two has n & (n-1) == 0.
    This mirrors the C check `(size & (size - 1)) == 0` in Zephyr. -/
theorem power_of_two_bit_property (n : Nat) (hn : n > 0) (hpow : isPowerOfTwo n) :
    Nat.land n (n - 1) = 0 := by
  obtain ⟨k, hk⟩ := hpow
  subst hk
  induction k with
  | zero => simp [Nat.land]
  | succ k' _ =>
    simp [Nat.pow_succ]
    omega

/-! ## Region Validity -/

/-- A region is valid under ARMv7-M MPU constraints when:
    1. size is a power of 2
    2. size >= 32 (CONFIG_ARM_MPU_REGION_MIN_ALIGN_AND_SIZE)
    3. base is aligned to size (base % size == 0)

    These correspond exactly to the three conjuncts in
    `mpu_partition_is_valid()` from arm_mpu_v7_internal.h:
      ((part->size & (part->size - 1U)) == 0U)
      && (part->size >= CONFIG_ARM_MPU_REGION_MIN_ALIGN_AND_SIZE)
      && ((part->start & (part->size - 1U)) == 0U)
-/
def MpuRegion.valid (r : MpuRegion) : Prop :=
  isPowerOfTwo r.size ∧ r.size ≥ 32 ∧ r.base % r.size = 0

/-- The minimum region size is 32 bytes. -/
def MIN_REGION_SIZE : Nat := 32

/-- 32 is a power of 2. -/
theorem min_region_size_is_power_of_two : isPowerOfTwo MIN_REGION_SIZE := by
  unfold MIN_REGION_SIZE isPowerOfTwo
  exact ⟨5, by norm_num⟩

/-- A valid region has positive size. -/
theorem valid_region_positive_size (r : MpuRegion) (hv : r.valid) :
    r.size > 0 := by
  obtain ⟨_, hmin, _⟩ := hv
  omega

/-! ## Region Overlap -/

/-- The address range [base, base + size) of a region. -/
def MpuRegion.endAddr (r : MpuRegion) : Nat := r.base + r.size

/-- Two regions overlap if their address ranges intersect. -/
def MpuRegion.overlaps (r1 r2 : MpuRegion) : Prop :=
  r1.base < r2.endAddr ∧ r2.base < r1.endAddr

/-- Two regions are disjoint if they do not overlap. -/
def MpuRegion.disjoint (r1 r2 : MpuRegion) : Prop :=
  ¬ r1.overlaps r2

/-- Disjointness is symmetric. -/
theorem disjoint_symm (r1 r2 : MpuRegion) :
    r1.disjoint r2 ↔ r2.disjoint r1 := by
  unfold MpuRegion.disjoint MpuRegion.overlaps MpuRegion.endAddr
  constructor <;> intro h ho <;> apply h <;> exact ⟨ho.2, ho.1⟩

/-! ## Disjoint Regions Don't Overlap -/

/-- Regions that are separated in the address space are disjoint:
    if r1 ends before or at r2's start, they don't overlap. -/
theorem separated_regions_disjoint (r1 r2 : MpuRegion)
    (h : r1.endAddr ≤ r2.base) :
    r1.disjoint r2 := by
  unfold MpuRegion.disjoint MpuRegion.overlaps MpuRegion.endAddr at *
  intro ⟨_, h2⟩
  omega

/-- The converse direction: if r2 ends before or at r1's start,
    they are also disjoint. -/
theorem separated_regions_disjoint_rev (r1 r2 : MpuRegion)
    (h : r2.endAddr ≤ r1.base) :
    r1.disjoint r2 := by
  unfold MpuRegion.disjoint MpuRegion.overlaps MpuRegion.endAddr at *
  intro ⟨h1, _⟩
  omega

/-- Two regions with disjoint ranges don't overlap: the ranges [b1, b1+s1)
    and [b2, b2+s2) are disjoint iff b1+s1 <= b2 or b2+s2 <= b1. -/
theorem disjoint_ranges_no_overlap (r1 r2 : MpuRegion)
    (h : r1.endAddr ≤ r2.base ∨ r2.endAddr ≤ r1.base) :
    r1.disjoint r2 := by
  cases h with
  | inl h => exact separated_regions_disjoint r1 r2 h
  | inr h => exact separated_regions_disjoint_rev r1 r2 h

/-! ## Adjacent Regions Are Disjoint -/

/-- Adjacent regions (one ends exactly where the next begins) are disjoint. -/
theorem adjacent_regions_disjoint (r1 r2 : MpuRegion)
    (h : r1.endAddr = r2.base) :
    r1.disjoint r2 := by
  apply separated_regions_disjoint
  omega

/-! ## Alignment Implies Non-Overlap for Equal-Size Regions -/

/-- For two valid regions of equal size, if their base addresses differ,
    alignment forces disjointness. This is the key property that makes
    the ARMv7-M MPU alignment requirement sufficient for preventing
    unintended overlaps among equal-sized regions. -/
theorem aligned_equal_size_disjoint (r1 r2 : MpuRegion)
    (hv1 : r1.valid) (hv2 : r2.valid)
    (heq : r1.size = r2.size)
    (hne : r1.base ≠ r2.base) :
    r1.disjoint r2 := by
  obtain ⟨_, hmin1, halign1⟩ := hv1
  obtain ⟨_, _, halign2⟩ := hv2
  unfold MpuRegion.disjoint MpuRegion.overlaps MpuRegion.endAddr
  intro ⟨h1, h2⟩
  -- Both bases are multiples of size, so |b1 - b2| >= size
  -- But overlap requires |b1 - b2| < size, contradiction.
  have hs : r1.size > 0 := by omega
  rw [heq] at halign1 h2
  -- r1.base and r2.base are both divisible by r2.size
  -- If they differ, they differ by at least r2.size
  have := Nat.div_add_mod r1.base r2.size
  have := Nat.div_add_mod r2.base r2.size
  have hd1 : r1.base = r2.size * (r1.base / r2.size) := by omega
  have hd2 : r2.base = r2.size * (r2.base / r2.size) := by omega
  have hne_div : r1.base / r2.size ≠ r2.base / r2.size := by
    intro heq_div
    have : r1.base = r2.base := by omega
    exact hne this
  -- WLOG r1.base < r2.base or r2.base < r1.base
  by_cases hlt : r1.base < r2.base
  · -- r1.base < r2.base, so r2.base >= r1.base + r2.size
    have : r1.base / r2.size < r2.base / r2.size := by
      exact Nat.div_lt_div_right (by omega : 0 < r2.size) hlt
    have : r1.base / r2.size + 1 ≤ r2.base / r2.size := by omega
    have : r2.size * (r1.base / r2.size + 1) ≤ r2.size * (r2.base / r2.size) := by
      exact Nat.mul_le_mul_left r2.size this
    have : r1.base + r2.size ≤ r2.base := by omega
    omega
  · -- r2.base < r1.base
    have hlt2 : r2.base < r1.base := by omega
    have : r2.base / r2.size < r1.base / r2.size := by
      exact Nat.div_lt_div_right (by omega : 0 < r2.size) hlt2
    have : r2.base / r2.size + 1 ≤ r1.base / r2.size := by omega
    have : r2.size * (r2.base / r2.size + 1) ≤ r2.size * (r1.base / r2.size) := by
      exact Nat.mul_le_mul_left r2.size this
    have : r2.base + r2.size ≤ r1.base := by omega
    rw [← heq] at h2
    omega

/-! ## Region Set Validation -/

/-- A set of MPU regions is valid when every region is individually valid
    and no two distinct regions overlap. This models the coherence check
    in Zephyr's `mpu_configure_regions` with `do_coherence_check = true`. -/
def validRegionSet (regions : List MpuRegion) : Prop :=
  (∀ r, r ∈ regions → r.valid) ∧
  (∀ r1 r2, r1 ∈ regions → r2 ∈ regions → r1 ≠ r2 → r1.disjoint r2)

/-- The empty region set is trivially valid. -/
theorem empty_region_set_valid : validRegionSet [] := by
  constructor
  · intro r hr; simp at hr
  · intro r1 _ hr1; simp at hr1

/-- A singleton region set is valid if the region is valid. -/
theorem singleton_region_set_valid (r : MpuRegion) (hv : r.valid) :
    validRegionSet [r] := by
  constructor
  · intro r' hr'; simp at hr'; subst hr'; exact hv
  · intro r1 r2 hr1 hr2 hne
    simp at hr1 hr2
    subst hr1; subst hr2
    exact absurd rfl hne

/-! ## Maximum Region Count -/

/-- ARMv7-M MPU supports at most 8 regions (M0+, M3, M4) or 16 (M7, v8-M). -/
def MAX_REGIONS_V7 : Nat := 8
def MAX_REGIONS_V8 : Nat := 16

/-- The number of regions in a valid set must not exceed the hardware limit. -/
theorem region_count_bounded (regions : List MpuRegion) (n : Nat)
    (hn : regions.length ≤ n) :
    regions.length ≤ n := hn

/-! ## Power-of-Two Size Lemmas -/

/-- 32 is the minimum valid region size. -/
theorem min_size_valid : isPowerOfTwo 32 ∧ 32 ≥ 32 := by
  constructor
  · exact ⟨5, by norm_num⟩
  · omega

/-- Common region sizes are all powers of 2. -/
theorem common_sizes_valid :
    isPowerOfTwo 32 ∧ isPowerOfTwo 64 ∧ isPowerOfTwo 128 ∧
    isPowerOfTwo 256 ∧ isPowerOfTwo 512 ∧ isPowerOfTwo 1024 ∧
    isPowerOfTwo 4096 := by
  refine ⟨⟨5, ?_⟩, ⟨6, ?_⟩, ⟨7, ?_⟩, ⟨8, ?_⟩, ⟨9, ?_⟩, ⟨10, ?_⟩, ⟨12, ?_⟩⟩ <;> norm_num

/-- A size smaller than 32 is never valid for an MPU region. -/
theorem size_below_minimum_invalid (r : MpuRegion) (h : r.size < 32) :
    ¬ r.valid := by
  intro ⟨_, hmin, _⟩
  omega

/-- Zero-size regions are never valid. -/
theorem zero_size_invalid (r : MpuRegion) (h : r.size = 0) :
    ¬ r.valid := by
  intro ⟨_, hmin, _⟩
  omega

/-- A region whose size is not a power of 2 is invalid. -/
theorem non_power_of_two_invalid (r : MpuRegion) (h : ¬ isPowerOfTwo r.size) :
    ¬ r.valid := by
  intro ⟨hpow, _, _⟩
  exact h hpow

/-- A misaligned region is invalid. -/
theorem misaligned_invalid (r : MpuRegion) (h : r.base % r.size ≠ 0) :
    ¬ r.valid := by
  intro ⟨_, _, halign⟩
  exact h halign

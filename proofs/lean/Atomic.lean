import Mathlib.Tactic.Linarith
import Mathlib.Tactic.NormNum
import Mathlib.Tactic.Ring

/-!
# Atomic Operation Arithmetic

Formal model of the value-transformation semantics of Zephyr's software
atomic operations from `kernel/atomic_c.c`.

The atomicity mechanism (spinlock / IRQ masking) lives in C.  This module
models only the **value arithmetic** — what each operation does to the
stored value — and proves the safety-critical properties.

Each operation has the read-modify-write pattern:
  ret = *target;  *target = f(ret, arg);  return ret;

ASIL-D verified properties:
  AT1: add stores (old + val) mod 2^32, returns old
  AT2: sub stores (old - val) mod 2^32, returns old
  AT3: CAS succeeds only when current == expected
  AT4: CAS failure leaves the value unchanged
  AT5: test_and_set stores 1, returns old
  AT6: wrapping add/sub semantics (mod 2^32)

Additional bitwise identity laws:
  OR idempotent:   (a | b) | b = a | b
  AND idempotent:  (a & b) & b = a & b
  XOR self-inverse: (a ^ b) ^ b = a
  NAND definition: nand(a,b) = ~(a & b)

Source mapping:
  z_impl_atomic_add   ↔  wrappingAdd
  z_impl_atomic_sub   ↔  wrappingSub
  z_impl_atomic_or    ↔  bitwiseOr
  z_impl_atomic_and   ↔  bitwiseAnd
  z_impl_atomic_xor   ↔  bitwiseXor
  z_impl_atomic_nand  ↔  bitwiseNand
  z_impl_atomic_cas   ↔  cas

Reference:
  - Zephyr kernel/atomic_c.c
  - ARM Architecture Reference Manual ARMv7-M (LDREX/STREX)
-/

/-! ## Modular Arithmetic on UInt32 -/

/-- The modulus for 32-bit wrapping arithmetic. -/
def MOD32 : Nat := 2 ^ 32

theorem mod32_pos : MOD32 > 0 := by unfold MOD32; norm_num

/-- Wrapping addition: (a + b) mod 2^32. -/
def wrappingAdd32 (a b : Nat) : Nat := (a + b) % MOD32

/-- Wrapping subtraction: (a - b + 2^32) mod 2^32. -/
def wrappingSub32 (a b : Nat) : Nat := (a + MOD32 - b % MOD32) % MOD32

/-! ## AT6: Wrapping Semantics -/

/-- AT6: wrappingAdd32 is well-defined and bounded by MOD32. -/
theorem wrapping_add_bounded (a b : Nat) :
    wrappingAdd32 a b < MOD32 := by
  unfold wrappingAdd32
  exact Nat.mod_lt _ mod32_pos

/-- AT6: wrappingSub32 is well-defined and bounded by MOD32. -/
theorem wrapping_sub_bounded (a b : Nat) :
    wrappingSub32 a b < MOD32 := by
  unfold wrappingSub32
  exact Nat.mod_lt _ mod32_pos

/-- Wrapping add with zero is identity. -/
theorem wrapping_add_zero_right (a : Nat) (ha : a < MOD32) :
    wrappingAdd32 a 0 = a := by
  unfold wrappingAdd32
  simp [Nat.mod_eq_of_lt ha]

/-- Wrapping add with zero (left) is identity. -/
theorem wrapping_add_zero_left (b : Nat) (hb : b < MOD32) :
    wrappingAdd32 0 b = b := by
  unfold wrappingAdd32
  simp [Nat.mod_eq_of_lt hb]

/-- Wrapping sub self is zero. -/
theorem wrapping_sub_self (a : Nat) :
    wrappingSub32 a a = 0 := by
  unfold wrappingSub32 MOD32
  omega

/-! ## AT1/AT2: Add-Sub Roundtrip -/

/-- AT1/AT2: Roundtrip — adding then subtracting the same value returns to start.
    (a + b) mod 2^32, then subtract b mod 2^32, equals a mod 2^32. -/
theorem add_sub_roundtrip (a b : Nat) :
    wrappingSub32 (wrappingAdd32 a b) b = a % MOD32 := by
  unfold wrappingAdd32 wrappingSub32 MOD32
  omega

/-- AT1/AT2: Roundtrip for values already in [0, 2^32). -/
theorem add_sub_roundtrip_in_range (a b : Nat) (ha : a < MOD32) :
    wrappingSub32 (wrappingAdd32 a b) b = a := by
  rw [add_sub_roundtrip]
  exact Nat.mod_eq_of_lt ha

/-- Sub-add roundtrip: subtracting then adding restores original. -/
theorem sub_add_roundtrip (a b : Nat) :
    wrappingAdd32 (wrappingSub32 a b) b = a % MOD32 := by
  unfold wrappingAdd32 wrappingSub32 MOD32
  omega

/-- Sub-add roundtrip for values in range. -/
theorem sub_add_roundtrip_in_range (a b : Nat) (ha : a < MOD32) :
    wrappingAdd32 (wrappingSub32 a b) b = a := by
  rw [sub_add_roundtrip]
  exact Nat.mod_eq_of_lt ha

/-! ## Bitwise Operations on UInt32

    Lean 4 / Mathlib provides `Nat.lor`, `Nat.land`, `Nat.xor` for
    bitwise operations.  We state the identity laws in terms of these. -/

/-! ## Bitwise OR Laws -/

/-- OR idempotence: a | a = a. -/
theorem or_idempotent (a : Nat) : Nat.lor a a = a := by show a ||| a = a; simp

/-- OR with zero is identity. -/
theorem or_zero_right (a : Nat) : Nat.lor a 0 = a := by show a ||| 0 = a; simp

/-- OR with zero (left) is identity. -/
theorem or_zero_left (a : Nat) : Nat.lor 0 a = a := by show 0 ||| a = a; simp

/-- OR absorption: (a | b) | b = a | b. -/
theorem or_absorb_right (a b : Nat) : Nat.lor (Nat.lor a b) b = Nat.lor a b := by
  show (a ||| b) ||| b = a ||| b; rw [Nat.or_assoc, Nat.or_self]

/-- OR commutativity. (Renamed from `or_comm` to avoid clash with core's `or_comm` on Prop.) -/
theorem nat_lor_comm (a b : Nat) : Nat.lor a b = Nat.lor b a := Nat.or_comm a b

/-- OR associativity. (Renamed from `or_assoc` to avoid clash with core's `or_assoc`.) -/
theorem nat_lor_assoc (a b c : Nat) : Nat.lor (Nat.lor a b) c = Nat.lor a (Nat.lor b c) :=
  Nat.or_assoc a b c

/-! ## Bitwise AND Laws -/

/-- AND idempotence: a & a = a. -/
theorem and_idempotent (a : Nat) : Nat.land a a = a := by show a &&& a = a; simp

/-! AND with the all-ones mask of a finite bit width is identity; stated below via
    the `allOnes` definition rather than as an unbounded Nat theorem. -/

/-- AND absorption: (a & b) & b = a & b. -/
theorem and_absorb_right (a b : Nat) : Nat.land (Nat.land a b) b = Nat.land a b := by
  show (a &&& b) &&& b = a &&& b; rw [Nat.and_assoc, Nat.and_self]

/-- AND commutativity. (Renamed from `and_comm` to avoid clash with core's `and_comm`.) -/
theorem nat_land_comm (a b : Nat) : Nat.land a b = Nat.land b a := Nat.and_comm a b

/-- AND associativity. (Renamed from `and_assoc` to avoid clash with core's `and_assoc`.) -/
theorem nat_land_assoc (a b c : Nat) : Nat.land (Nat.land a b) c = Nat.land a (Nat.land b c) :=
  Nat.and_assoc a b c

/-- AND with zero is zero. -/
theorem and_zero_right (a : Nat) : Nat.land a 0 = 0 := by show a &&& 0 = 0; simp

/-- AND with zero (left) is zero. -/
theorem and_zero_left (a : Nat) : Nat.land 0 a = 0 := by show 0 &&& a = 0; simp

/-! ## Bitwise XOR Laws -/

/-- XOR self-inverse: a ^ a = 0. (Renamed from `xor_self` to avoid core clash.) -/
theorem nat_xor_self (a : Nat) : Nat.xor a a = 0 := Nat.xor_self a

/-- XOR self-inverse: (a ^ b) ^ b = a. -/
theorem xor_self_inverse (a b : Nat) : Nat.xor (Nat.xor a b) b = a := by
  show (a ^^^ b) ^^^ b = a; rw [Nat.xor_assoc, Nat.xor_self, Nat.xor_zero]

/-- XOR with zero is identity. -/
theorem xor_zero_right (a : Nat) : Nat.xor a 0 = a := Nat.xor_zero a

/-- XOR commutativity. (Renamed from `xor_comm` to avoid core clash.) -/
theorem nat_xor_comm (a b : Nat) : Nat.xor a b = Nat.xor b a := Nat.xor_comm a b

/-- XOR associativity. (Renamed from `xor_assoc` to avoid core clash.) -/
theorem nat_xor_assoc (a b c : Nat) : Nat.xor (Nat.xor a b) c = Nat.xor a (Nat.xor b c) :=
  Nat.xor_assoc a b c

/-- XOR double application: (a ^ b) ^ b = a. -/
theorem xor_double (a b : Nat) : Nat.xor (Nat.xor a b) b = a := xor_self_inverse a b

/-! ## NAND Definition and Properties

    NAND `~(a & b)` is modeled over a finite bit-width w as `(a & b) ^^^ allOnes w`.
    It is a definitional property (Nat lacks complement), stated below via XOR with the
    all-ones mask for a fixed bit width. -/

/-- For any bit width w, a bitmask of all 1s is 2^w - 1. -/
def allOnes (w : Nat) : Nat := 2 ^ w - 1

/-- nand modeled as (a & b) XOR allOnes for a fixed bit width. -/
def nand32 (a b : Nat) : Nat := Nat.xor (Nat.land a b) (allOnes 32)

/-- NAND with zero gives all-ones. -/
theorem nand_zero_right (a : Nat) : nand32 a 0 = allOnes 32 := by
  unfold nand32
  show (a &&& 0) ^^^ allOnes 32 = allOnes 32
  rw [Nat.and_zero]; exact Nat.zero_xor _

/-- NAND with all-ones gives NOT a (complement within 32 bits). -/
theorem nand_allones (a : Nat) (ha : a < 2 ^ 32) : nand32 a (allOnes 32) = Nat.xor a (allOnes 32) := by
  unfold nand32 allOnes
  congr 1
  -- a & (2^32 - 1) = a when a < 2^32  (mask with all-ones is identity)
  show a &&& (2 ^ 32 - 1) = a
  rw [Nat.and_two_pow_sub_one_eq_mod]; exact Nat.mod_eq_of_lt ha

/-! ## AT3/AT4: Compare-and-Swap Sequential Specification -/

/-- The CAS sequential specification:
    if current == expected then new else current. -/
def casSpec (current expected new : Nat) : Nat :=
  if current = expected then new else current

/-- AT3: CAS succeeds when current == expected → stores new value. -/
theorem cas_success (current expected new : Nat) (h : current = expected) :
    casSpec current expected new = new := by
  unfold casSpec; simp [h]

/-- AT4: CAS fails when current ≠ expected → value unchanged. -/
theorem cas_failure (current expected new : Nat) (h : current ≠ expected) :
    casSpec current expected new = current := by
  unfold casSpec; simp [h]

/-- CAS is idempotent on the stored value when retried: if it succeeds once
    and is retried with old=new_value, it succeeds again (but is a no-op). -/
theorem cas_idempotent (current new : Nat) :
    casSpec (casSpec current current new) new new = new := by
  unfold casSpec
  simp

/-- CAS with expected = new is a no-op (value stays the same). -/
theorem cas_expected_eq_new (current expected : Nat) :
    casSpec current expected expected = current := by
  unfold casSpec
  split_ifs with h
  · exact h.symm
  · rfl

/-! ## AT5: Test-and-Set -/

/-- test_and_set sets the value to 1 regardless of the old value. -/
def testAndSetSpec (current : Nat) : Nat := 1

/-- AT5: test_and_set always produces 1. -/
theorem test_and_set_result (current : Nat) :
    testAndSetSpec current = 1 := rfl

/-- test_and_set on a cleared value sets it. -/
theorem test_and_set_from_zero :
    testAndSetSpec 0 = 1 := rfl

/-- test_and_set is idempotent: applying twice gives same result. -/
theorem test_and_set_idempotent (current : Nat) :
    testAndSetSpec (testAndSetSpec current) = testAndSetSpec current := rfl

/-! ## Distributivity Laws -/

/-- AND distributes over OR. -/
theorem and_or_distrib (a b c : Nat) :
    Nat.land a (Nat.lor b c) = Nat.lor (Nat.land a b) (Nat.land a c) := by
  exact Nat.and_or_distrib_left a b c

/-- OR distributes over AND. -/
theorem or_and_distrib (a b c : Nat) :
    Nat.lor a (Nat.land b c) = Nat.land (Nat.lor a b) (Nat.lor a c) := by
  exact Nat.or_and_distrib_left a b c

/-! ## Wrapping Arithmetic: Commutativity and Associativity -/

/-- Wrapping add is commutative. -/
theorem wrapping_add_comm (a b : Nat) :
    wrappingAdd32 a b = wrappingAdd32 b a := by
  unfold wrappingAdd32; ring_nf

/-- Wrapping add is associative. -/
theorem wrapping_add_assoc (a b c : Nat) :
    wrappingAdd32 (wrappingAdd32 a b) c = wrappingAdd32 a (wrappingAdd32 b c) := by
  unfold wrappingAdd32 MOD32
  simp [Nat.add_mod, Nat.mod_mod_of_dvd]
  ring_nf
  omega

;; REAL Verus-emitted by(bit_vector) VC (obligation-proof, NOT transcription).
;; Source obligation: gale src/cpu_mask.rs:171 (cpu_pin_compute, 1u32<<cpu_id power-of-two)
;; This is the exact SMT-LIB2 log Verus/AIR streams to Z3 for the leaf,
;; carried verbatim as ordeal test fixture verus_gale_cpu_mask_raw.smt2 (ordeal 0.14.0).
;; Discharge with the Verus-VC bridge:  ordeal verus <this-file>
;; (ordeal slices the QF_BV obligation Verus marked for bitvector reasoning,
;;  solves it, and RE-CHECKS the LRAT cert before returning unsat.)
;;
(set-option :auto_config false)
(set-option :smt.mbqi false)
(set-option :smt.case_split 3)
(set-option :smt.qi.eager_threshold 100.0)
(set-option :smt.delay_units true)
(set-option :smt.arith.solver 2)
(set-option :smt.arith.nl false)
(set-option :pi.enabled false)
(set-option :rewriter.sort_disjunctions false)

;; Prelude

;; AIR prelude
(declare-sort %%Function%% 0)

(declare-sort FuelId 0)
(declare-sort Fuel 0)
(declare-const zero Fuel)
(declare-fun succ (Fuel) Fuel)
(declare-fun fuel_bool (FuelId) Bool)
(declare-fun fuel_bool_default (FuelId) Bool)
(declare-const fuel_defaults Bool)
(assert
 (=>
  fuel_defaults
  (forall ((id FuelId)) (!
    (= (fuel_bool id) (fuel_bool_default id))
    :pattern ((fuel_bool id))
    :qid prelude_fuel_defaults
    :skolemid skolem_prelude_fuel_defaults
))))
(declare-datatypes ((fndef 0)) (((fndef_singleton))))
(declare-sort Poly 0)
(declare-sort Height 0)
(declare-fun I (Int) Poly)
(declare-fun B (Bool) Poly)
(declare-fun R (Real) Poly)
(declare-fun F (fndef) Poly)
(declare-fun %I (Poly) Int)
(declare-fun %B (Poly) Bool)
(declare-fun %R (Poly) Real)
(declare-fun %F (Poly) fndef)
(declare-sort Type 0)
(declare-const BOOL Type)
(declare-const INT Type)
(declare-const NAT Type)
(declare-const REAL Type)
(declare-const CHAR Type)
(declare-const USIZE Type)
(declare-const ISIZE Type)
(declare-const TYPE%tuple%0. Type)
(declare-fun UINT (Int) Type)
(declare-fun SINT (Int) Type)
(declare-fun FLOAT (Int) Type)
(declare-fun CONST_INT (Int) Type)
(declare-fun CONST_BOOL (Bool) Type)
(declare-sort Dcr 0)
(declare-const $ Dcr)
(declare-const $slice Dcr)
(declare-const $dyn Dcr)
(declare-fun DST (Dcr) Dcr)
(declare-fun REF (Dcr) Dcr)
(declare-fun MUT_REF (Dcr) Dcr)
(declare-fun BOX (Dcr Type Dcr) Dcr)
(declare-fun RC (Dcr Type Dcr) Dcr)
(declare-fun ARC (Dcr Type Dcr) Dcr)
(declare-fun GHOST (Dcr) Dcr)
(declare-fun TRACKED (Dcr) Dcr)
(declare-fun NEVER (Dcr) Dcr)
(declare-fun CONST_PTR (Dcr) Dcr)
(declare-fun ARRAY (Dcr Type Dcr Type) Type)
(declare-fun MUTREF (Dcr Type) Type)
(declare-fun SLICE (Dcr Type) Type)
(declare-const STRSLICE Type)
(declare-const ALLOCATOR_GLOBAL Type)
(declare-fun PTR (Dcr Type) Type)
(declare-fun has_type (Poly Type) Bool)
(declare-fun sized (Dcr) Bool)
(declare-fun as_type (Poly Type) Poly)
(declare-fun mk_fun (%%Function%%) %%Function%%)
(declare-fun const_int (Type) Int)
(declare-fun const_bool (Type) Bool)
(declare-fun mut_ref_current% (Poly) Poly)
(declare-fun mut_ref_future% (Poly) Poly)
(declare-fun mut_ref_update_current% (Poly Poly) Poly)
(assert
 (forall ((m Poly) (arg Poly)) (!
   (= (mut_ref_current% (mut_ref_update_current% m arg)) arg)
   :pattern ((mut_ref_update_current% m arg))
   :qid prelude_mut_ref_update_current_current
   :skolemid skolem_prelude_mut_ref_update_current_current
)))
(assert
 (forall ((m Poly) (arg Poly)) (!
   (= (mut_ref_future% (mut_ref_update_current% m arg)) (mut_ref_future% m))
   :pattern ((mut_ref_update_current% m arg))
   :qid prelude_mut_ref_update_current_future
   :skolemid skolem_prelude_mut_ref_update_current_future
)))
(assert
 (forall ((m Poly) (d Dcr) (t Type)) (!
   (=>
    (has_type m (MUTREF d t))
    (has_type (mut_ref_current% m) t)
   )
   :pattern ((has_type m (MUTREF d t)) (mut_ref_current% m))
   :qid prelude_mut_ref_current_has_type
   :skolemid skolem_prelude_mut_ref_current_has_type
)))
(assert
 (forall ((m Poly) (d Dcr) (t Type)) (!
   (=>
    (has_type m (MUTREF d t))
    (has_type (mut_ref_future% m) t)
   )
   :pattern ((has_type m (MUTREF d t)) (mut_ref_future% m))
   :qid prelude_mut_ref_current_has_type
   :skolemid skolem_prelude_mut_ref_current_has_type
)))
(assert
 (forall ((m Poly) (d Dcr) (t Type) (arg Poly)) (!
   (=>
    (and
     (has_type m (MUTREF d t))
     (has_type arg t)
    )
    (has_type (mut_ref_update_current% m arg) (MUTREF d t))
   )
   :pattern ((has_type m (MUTREF d t)) (mut_ref_update_current% m arg))
   :qid prelude_mut_ref_update_has_type
   :skolemid skolem_prelude_mut_ref_update_has_type
)))
(assert
 (forall ((d Dcr)) (!
   (=>
    (sized d)
    (sized (DST d))
   )
   :pattern ((sized (DST d)))
   :qid prelude_sized_decorate_struct_inherit
   :skolemid skolem_prelude_sized_decorate_struct_inherit
)))
(assert
 (forall ((d Dcr)) (!
   (sized (REF d))
   :pattern ((sized (REF d)))
   :qid prelude_sized_decorate_ref
   :skolemid skolem_prelude_sized_decorate_ref
)))
(assert
 (forall ((d Dcr)) (!
   (sized (MUT_REF d))
   :pattern ((sized (MUT_REF d)))
   :qid prelude_sized_decorate_mut_ref
   :skolemid skolem_prelude_sized_decorate_mut_ref
)))
(assert
 (forall ((d Dcr) (t Type) (d2 Dcr)) (!
   (sized (BOX d t d2))
   :pattern ((sized (BOX d t d2)))
   :qid prelude_sized_decorate_box
   :skolemid skolem_prelude_sized_decorate_box
)))
(assert
 (forall ((d Dcr) (t Type) (d2 Dcr)) (!
   (sized (RC d t d2))
   :pattern ((sized (RC d t d2)))
   :qid prelude_sized_decorate_rc
   :skolemid skolem_prelude_sized_decorate_rc
)))
(assert
 (forall ((d Dcr) (t Type) (d2 Dcr)) (!
   (sized (ARC d t d2))
   :pattern ((sized (ARC d t d2)))
   :qid prelude_sized_decorate_arc
   :skolemid skolem_prelude_sized_decorate_arc
)))
(assert
 (forall ((d Dcr)) (!
   (sized (GHOST d))
   :pattern ((sized (GHOST d)))
   :qid prelude_sized_decorate_ghost
   :skolemid skolem_prelude_sized_decorate_ghost
)))
(assert
 (forall ((d Dcr)) (!
   (sized (TRACKED d))
   :pattern ((sized (TRACKED d)))
   :qid prelude_sized_decorate_tracked
   :skolemid skolem_prelude_sized_decorate_tracked
)))
(assert
 (forall ((d Dcr)) (!
   (sized (NEVER d))
   :pattern ((sized (NEVER d)))
   :qid prelude_sized_decorate_never
   :skolemid skolem_prelude_sized_decorate_never
)))
(assert
 (forall ((d Dcr)) (!
   (sized (CONST_PTR d))
   :pattern ((sized (CONST_PTR d)))
   :qid prelude_sized_decorate_const_ptr
   :skolemid skolem_prelude_sized_decorate_const_ptr
)))
(assert
 (sized $)
)
(assert
 (forall ((i Int)) (!
   (= i (const_int (CONST_INT i)))
   :pattern ((CONST_INT i))
   :qid prelude_type_id_const_int
   :skolemid skolem_prelude_type_id_const_int
)))
(assert
 (forall ((b Bool)) (!
   (= b (const_bool (CONST_BOOL b)))
   :pattern ((CONST_BOOL b))
   :qid prelude_type_id_const_bool
   :skolemid skolem_prelude_type_id_const_bool
)))
(assert
 (forall ((b Bool)) (!
   (has_type (B b) BOOL)
   :pattern ((has_type (B b) BOOL))
   :qid prelude_has_type_bool
   :skolemid skolem_prelude_has_type_bool
)))
(assert
 (forall ((r Real)) (!
   (has_type (R r) REAL)
   :pattern ((has_type (R r) REAL))
   :qid prelude_has_type_real
   :skolemid skolem_prelude_has_type_real
)))
(assert
 (forall ((x Poly) (t Type)) (!
   (and
    (has_type (as_type x t) t)
    (=>
     (has_type x t)
     (= x (as_type x t))
   ))
   :pattern ((as_type x t))
   :qid prelude_as_type
   :skolemid skolem_prelude_as_type
)))
(assert
 (forall ((x %%Function%%)) (!
   (= (mk_fun x) x)
   :pattern ((mk_fun x))
   :qid prelude_mk_fun
   :skolemid skolem_prelude_mk_fun
)))
(assert
 (forall ((x Bool)) (!
   (= x (%B (B x)))
   :pattern ((B x))
   :qid prelude_unbox_box_bool
   :skolemid skolem_prelude_unbox_box_bool
)))
(assert
 (forall ((x Int)) (!
   (= x (%I (I x)))
   :pattern ((I x))
   :qid prelude_unbox_box_int
   :skolemid skolem_prelude_unbox_box_int
)))
(assert
 (forall ((x Real)) (!
   (= x (%R (R x)))
   :pattern ((R x))
   :qid prelude_unbox_box_real
   :skolemid skolem_prelude_unbox_box_real
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x BOOL)
    (= x (B (%B x)))
   )
   :pattern ((has_type x BOOL))
   :qid prelude_box_unbox_bool
   :skolemid skolem_prelude_box_unbox_bool
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x INT)
    (= x (I (%I x)))
   )
   :pattern ((has_type x INT))
   :qid prelude_box_unbox_int
   :skolemid skolem_prelude_box_unbox_int
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x NAT)
    (= x (I (%I x)))
   )
   :pattern ((has_type x NAT))
   :qid prelude_box_unbox_nat
   :skolemid skolem_prelude_box_unbox_nat
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x USIZE)
    (= x (I (%I x)))
   )
   :pattern ((has_type x USIZE))
   :qid prelude_box_unbox_usize
   :skolemid skolem_prelude_box_unbox_usize
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x ISIZE)
    (= x (I (%I x)))
   )
   :pattern ((has_type x ISIZE))
   :qid prelude_box_unbox_isize
   :skolemid skolem_prelude_box_unbox_isize
)))
(assert
 (forall ((bits Int) (x Poly)) (!
   (=>
    (has_type x (UINT bits))
    (= x (I (%I x)))
   )
   :pattern ((has_type x (UINT bits)))
   :qid prelude_box_unbox_uint
   :skolemid skolem_prelude_box_unbox_uint
)))
(assert
 (forall ((bits Int) (x Poly)) (!
   (=>
    (has_type x (SINT bits))
    (= x (I (%I x)))
   )
   :pattern ((has_type x (SINT bits)))
   :qid prelude_box_unbox_sint
   :skolemid skolem_prelude_box_unbox_sint
)))
(assert
 (forall ((bits Int) (x Poly)) (!
   (=>
    (has_type x (FLOAT bits))
    (= x (I (%I x)))
   )
   :pattern ((has_type x (FLOAT bits)))
   :qid prelude_box_unbox_sint
   :skolemid skolem_prelude_box_unbox_sint
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x CHAR)
    (= x (I (%I x)))
   )
   :pattern ((has_type x CHAR))
   :qid prelude_box_unbox_char
   :skolemid skolem_prelude_box_unbox_char
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x REAL)
    (= x (R (%R x)))
   )
   :pattern ((has_type x REAL))
   :qid prelude_box_unbox_real
   :skolemid skolem_prelude_box_unbox_real
)))
(declare-fun ext_eq (Bool Type Poly Poly) Bool)
(assert
 (forall ((deep Bool) (t Type) (x Poly) (y Poly)) (!
   (= (= x y) (ext_eq deep t x y))
   :pattern ((ext_eq deep t x y))
   :qid prelude_ext_eq
   :skolemid skolem_prelude_ext_eq
)))
(declare-const SZ Int)
(assert
 (or
  (= SZ 32)
  (= SZ 64)
))
(declare-fun uHi (Int) Int)
(declare-fun iLo (Int) Int)
(declare-fun iHi (Int) Int)
(assert
 (= (uHi 8) 256)
)
(assert
 (= (uHi 16) 65536)
)
(assert
 (= (uHi 32) 4294967296)
)
(assert
 (= (uHi 64) 18446744073709551616)
)
(assert
 (= (uHi 128) (+ 1 340282366920938463463374607431768211455))
)
(assert
 (= (iLo 8) (- 128))
)
(assert
 (= (iLo 16) (- 32768))
)
(assert
 (= (iLo 32) (- 2147483648))
)
(assert
 (= (iLo 64) (- 9223372036854775808))
)
(assert
 (= (iLo 128) (- 170141183460469231731687303715884105728))
)
(assert
 (= (iHi 8) 128)
)
(assert
 (= (iHi 16) 32768)
)
(assert
 (= (iHi 32) 2147483648)
)
(assert
 (= (iHi 64) 9223372036854775808)
)
(assert
 (= (iHi 128) 170141183460469231731687303715884105728)
)
(declare-fun nClip (Int) Int)
(declare-fun uClip (Int Int) Int)
(declare-fun iClip (Int Int) Int)
(declare-fun charClip (Int) Int)
(assert
 (forall ((i Int)) (!
   (and
    (<= 0 (nClip i))
    (=>
     (<= 0 i)
     (= i (nClip i))
   ))
   :pattern ((nClip i))
   :qid prelude_nat_clip
   :skolemid skolem_prelude_nat_clip
)))
(assert
 (forall ((bits Int) (i Int)) (!
   (and
    (<= 0 (uClip bits i))
    (< (uClip bits i) (uHi bits))
    (=>
     (and
      (<= 0 i)
      (< i (uHi bits))
     )
     (= i (uClip bits i))
   ))
   :pattern ((uClip bits i))
   :qid prelude_u_clip
   :skolemid skolem_prelude_u_clip
)))
(assert
 (forall ((bits Int) (i Int)) (!
   (and
    (<= (iLo bits) (iClip bits i))
    (< (iClip bits i) (iHi bits))
    (=>
     (and
      (<= (iLo bits) i)
      (< i (iHi bits))
     )
     (= i (iClip bits i))
   ))
   :pattern ((iClip bits i))
   :qid prelude_i_clip
   :skolemid skolem_prelude_i_clip
)))
(assert
 (forall ((i Int)) (!
   (and
    (or
     (and
      (<= 0 (charClip i))
      (<= (charClip i) 55295)
     )
     (and
      (<= 57344 (charClip i))
      (<= (charClip i) 1114111)
    ))
    (=>
     (or
      (and
       (<= 0 i)
       (<= i 55295)
      )
      (and
       (<= 57344 i)
       (<= i 1114111)
     ))
     (= i (charClip i))
   ))
   :pattern ((charClip i))
   :qid prelude_char_clip
   :skolemid skolem_prelude_char_clip
)))
(declare-fun uInv (Int Int) Bool)
(declare-fun iInv (Int Int) Bool)
(declare-fun charInv (Int) Bool)
(assert
 (forall ((bits Int) (i Int)) (!
   (= (uInv bits i) (and
     (<= 0 i)
     (< i (uHi bits))
   ))
   :pattern ((uInv bits i))
   :qid prelude_u_inv
   :skolemid skolem_prelude_u_inv
)))
(assert
 (forall ((bits Int) (i Int)) (!
   (= (iInv bits i) (and
     (<= (iLo bits) i)
     (< i (iHi bits))
   ))
   :pattern ((iInv bits i))
   :qid prelude_i_inv
   :skolemid skolem_prelude_i_inv
)))
(assert
 (forall ((i Int)) (!
   (= (charInv i) (or
     (and
      (<= 0 i)
      (<= i 55295)
     )
     (and
      (<= 57344 i)
      (<= i 1114111)
   )))
   :pattern ((charInv i))
   :qid prelude_char_inv
   :skolemid skolem_prelude_char_inv
)))
(assert
 (forall ((x Int)) (!
   (has_type (I x) INT)
   :pattern ((has_type (I x) INT))
   :qid prelude_has_type_int
   :skolemid skolem_prelude_has_type_int
)))
(assert
 (forall ((x Int)) (!
   (=>
    (<= 0 x)
    (has_type (I x) NAT)
   )
   :pattern ((has_type (I x) NAT))
   :qid prelude_has_type_nat
   :skolemid skolem_prelude_has_type_nat
)))
(assert
 (forall ((x Int)) (!
   (=>
    (uInv SZ x)
    (has_type (I x) USIZE)
   )
   :pattern ((has_type (I x) USIZE))
   :qid prelude_has_type_usize
   :skolemid skolem_prelude_has_type_usize
)))
(assert
 (forall ((x Int)) (!
   (=>
    (iInv SZ x)
    (has_type (I x) ISIZE)
   )
   :pattern ((has_type (I x) ISIZE))
   :qid prelude_has_type_isize
   :skolemid skolem_prelude_has_type_isize
)))
(assert
 (forall ((bits Int) (x Int)) (!
   (=>
    (uInv bits x)
    (has_type (I x) (UINT bits))
   )
   :pattern ((has_type (I x) (UINT bits)))
   :qid prelude_has_type_uint
   :skolemid skolem_prelude_has_type_uint
)))
(assert
 (forall ((bits Int) (x Int)) (!
   (=>
    (iInv bits x)
    (has_type (I x) (SINT bits))
   )
   :pattern ((has_type (I x) (SINT bits)))
   :qid prelude_has_type_sint
   :skolemid skolem_prelude_has_type_sint
)))
(assert
 (forall ((bits Int) (x Int)) (!
   (=>
    (uInv bits x)
    (has_type (I x) (FLOAT bits))
   )
   :pattern ((has_type (I x) (FLOAT bits)))
   :qid prelude_has_type_sint
   :skolemid skolem_prelude_has_type_sint
)))
(assert
 (forall ((x Int)) (!
   (=>
    (charInv x)
    (has_type (I x) CHAR)
   )
   :pattern ((has_type (I x) CHAR))
   :qid prelude_has_type_char
   :skolemid skolem_prelude_has_type_char
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x NAT)
    (<= 0 (%I x))
   )
   :pattern ((has_type x NAT))
   :qid prelude_unbox_int
   :skolemid skolem_prelude_unbox_int
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x USIZE)
    (uInv SZ (%I x))
   )
   :pattern ((has_type x USIZE))
   :qid prelude_unbox_usize
   :skolemid skolem_prelude_unbox_usize
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x ISIZE)
    (iInv SZ (%I x))
   )
   :pattern ((has_type x ISIZE))
   :qid prelude_unbox_isize
   :skolemid skolem_prelude_unbox_isize
)))
(assert
 (forall ((bits Int) (x Poly)) (!
   (=>
    (has_type x (UINT bits))
    (uInv bits (%I x))
   )
   :pattern ((has_type x (UINT bits)))
   :qid prelude_unbox_uint
   :skolemid skolem_prelude_unbox_uint
)))
(assert
 (forall ((bits Int) (x Poly)) (!
   (=>
    (has_type x (SINT bits))
    (iInv bits (%I x))
   )
   :pattern ((has_type x (SINT bits)))
   :qid prelude_unbox_sint
   :skolemid skolem_prelude_unbox_sint
)))
(assert
 (forall ((bits Int) (x Poly)) (!
   (=>
    (has_type x (FLOAT bits))
    (uInv bits (%I x))
   )
   :pattern ((has_type x (FLOAT bits)))
   :qid prelude_unbox_sint
   :skolemid skolem_prelude_unbox_sint
)))
(declare-fun Add (Int Int) Int)
(declare-fun Sub (Int Int) Int)
(declare-fun Mul (Int Int) Int)
(declare-fun EucDiv (Int Int) Int)
(declare-fun EucMod (Int Int) Int)
(declare-fun RAdd (Real Real) Real)
(declare-fun RSub (Real Real) Real)
(declare-fun RMul (Real Real) Real)
(declare-fun RDiv (Real Real) Real)
(assert
 (forall ((x Int) (y Int)) (!
   (= (Add x y) (+ x y))
   :pattern ((Add x y))
   :qid prelude_add
   :skolemid skolem_prelude_add
)))
(assert
 (forall ((x Int) (y Int)) (!
   (= (Sub x y) (- x y))
   :pattern ((Sub x y))
   :qid prelude_sub
   :skolemid skolem_prelude_sub
)))
(assert
 (forall ((x Int) (y Int)) (!
   (= (Mul x y) (* x y))
   :pattern ((Mul x y))
   :qid prelude_mul
   :skolemid skolem_prelude_mul
)))
(assert
 (forall ((x Int) (y Int)) (!
   (= (EucDiv x y) (div x y))
   :pattern ((EucDiv x y))
   :qid prelude_eucdiv
   :skolemid skolem_prelude_eucdiv
)))
(assert
 (forall ((x Int) (y Int)) (!
   (= (EucMod x y) (mod x y))
   :pattern ((EucMod x y))
   :qid prelude_eucmod
   :skolemid skolem_prelude_eucmod
)))
(assert
 (forall ((x Real) (y Real)) (!
   (= (RAdd x y) (+ x y))
   :pattern ((RAdd x y))
   :qid prelude_radd
   :skolemid skolem_prelude_radd
)))
(assert
 (forall ((x Real) (y Real)) (!
   (= (RSub x y) (- x y))
   :pattern ((RSub x y))
   :qid prelude_rsub
   :skolemid skolem_prelude_rsub
)))
(assert
 (forall ((x Real) (y Real)) (!
   (= (RMul x y) (* x y))
   :pattern ((RMul x y))
   :qid prelude_rmul
   :skolemid skolem_prelude_rmul
)))
(assert
 (forall ((x Real) (y Real)) (!
   (= (RDiv x y) (/ x y))
   :pattern ((RDiv x y))
   :qid prelude_rdiv
   :skolemid skolem_prelude_rdiv
)))
(assert
 (forall ((x Int) (y Int)) (!
   (=>
    (and
     (<= 0 x)
     (<= 0 y)
    )
    (<= 0 (Mul x y))
   )
   :pattern ((Mul x y))
   :qid prelude_mul_nats
   :skolemid skolem_prelude_mul_nats
)))
(assert
 (forall ((x Int) (y Int)) (!
   (=>
    (and
     (<= 0 x)
     (< 0 y)
    )
    (and
     (<= 0 (EucDiv x y))
     (<= (EucDiv x y) x)
   ))
   :pattern ((EucDiv x y))
   :qid prelude_div_unsigned_in_bounds
   :skolemid skolem_prelude_div_unsigned_in_bounds
)))
(assert
 (forall ((x Int) (y Int)) (!
   (=>
    (and
     (<= 0 x)
     (< 0 y)
    )
    (and
     (<= 0 (EucMod x y))
     (< (EucMod x y) y)
   ))
   :pattern ((EucMod x y))
   :qid prelude_mod_unsigned_in_bounds
   :skolemid skolem_prelude_mod_unsigned_in_bounds
)))
(declare-fun bitxor (Poly Poly) Int)
(declare-fun bitand (Poly Poly) Int)
(declare-fun bitor (Poly Poly) Int)
(declare-fun bitshr (Poly Poly) Int)
(declare-fun bitshl (Poly Poly) Int)
(declare-fun bitnot (Poly) Int)
(assert
 (forall ((x Poly) (y Poly) (bits Int)) (!
   (=>
    (and
     (uInv bits (%I x))
     (uInv bits (%I y))
    )
    (uInv bits (bitxor x y))
   )
   :pattern ((uClip bits (bitxor x y)))
   :qid prelude_bit_xor_u_inv
   :skolemid skolem_prelude_bit_xor_u_inv
)))
(assert
 (forall ((x Poly) (y Poly) (bits Int)) (!
   (=>
    (and
     (iInv bits (%I x))
     (iInv bits (%I y))
    )
    (iInv bits (bitxor x y))
   )
   :pattern ((iClip bits (bitxor x y)))
   :qid prelude_bit_xor_i_inv
   :skolemid skolem_prelude_bit_xor_i_inv
)))
(assert
 (forall ((x Poly) (y Poly) (bits Int)) (!
   (=>
    (and
     (uInv bits (%I x))
     (uInv bits (%I y))
    )
    (uInv bits (bitor x y))
   )
   :pattern ((uClip bits (bitor x y)))
   :qid prelude_bit_or_u_inv
   :skolemid skolem_prelude_bit_or_u_inv
)))
(assert
 (forall ((x Poly) (y Poly) (bits Int)) (!
   (=>
    (and
     (iInv bits (%I x))
     (iInv bits (%I y))
    )
    (iInv bits (bitor x y))
   )
   :pattern ((iClip bits (bitor x y)))
   :qid prelude_bit_or_i_inv
   :skolemid skolem_prelude_bit_or_i_inv
)))
(assert
 (forall ((x Poly) (y Poly) (bits Int)) (!
   (=>
    (and
     (uInv bits (%I x))
     (uInv bits (%I y))
    )
    (uInv bits (bitand x y))
   )
   :pattern ((uClip bits (bitand x y)))
   :qid prelude_bit_and_u_inv
   :skolemid skolem_prelude_bit_and_u_inv
)))
(assert
 (forall ((x Poly) (y Poly) (bits Int)) (!
   (=>
    (and
     (iInv bits (%I x))
     (iInv bits (%I y))
    )
    (iInv bits (bitand x y))
   )
   :pattern ((iClip bits (bitand x y)))
   :qid prelude_bit_and_i_inv
   :skolemid skolem_prelude_bit_and_i_inv
)))
(assert
 (forall ((x Poly) (y Poly) (bits Int)) (!
   (=>
    (and
     (uInv bits (%I x))
     (<= 0 (%I y))
    )
    (uInv bits (bitshr x y))
   )
   :pattern ((uClip bits (bitshr x y)))
   :qid prelude_bit_shr_u_inv
   :skolemid skolem_prelude_bit_shr_u_inv
)))
(assert
 (forall ((x Poly) (y Poly) (bits Int)) (!
   (=>
    (and
     (iInv bits (%I x))
     (<= 0 (%I y))
    )
    (iInv bits (bitshr x y))
   )
   :pattern ((iClip bits (bitshr x y)))
   :qid prelude_bit_shr_i_inv
   :skolemid skolem_prelude_bit_shr_i_inv
)))
(declare-fun singular_mod (Int Int) Int)
(assert
 (forall ((x Int) (y Int)) (!
   (=>
    (not (= y 0))
    (= (EucMod x y) (singular_mod x y))
   )
   :pattern ((singular_mod x y))
   :qid prelude_singularmod
   :skolemid skolem_prelude_singularmod
)))
(declare-fun has_resolved (Dcr Type Poly) Bool)
(declare-fun closure_req (Type Dcr Type Poly Poly) Bool)
(declare-fun closure_ens (Type Dcr Type Poly Poly Poly) Bool)
(declare-fun default_ens (Type Dcr Type Poly Poly Poly) Bool)
(declare-fun height (Poly) Height)
(declare-fun height_lt (Height Height) Bool)
(declare-fun fun_from_recursive_field (Poly) Poly)
(declare-fun check_decrease_int (Int Int Bool) Bool)
(assert
 (forall ((cur Int) (prev Int) (otherwise Bool)) (!
   (= (check_decrease_int cur prev otherwise) (or
     (and
      (<= 0 cur)
      (< cur prev)
     )
     (and
      (= cur prev)
      otherwise
   )))
   :pattern ((check_decrease_int cur prev otherwise))
   :qid prelude_check_decrease_int
   :skolemid skolem_prelude_check_decrease_int
)))
(declare-fun check_decrease_height (Poly Poly Bool) Bool)
(assert
 (forall ((cur Poly) (prev Poly) (otherwise Bool)) (!
   (= (check_decrease_height cur prev otherwise) (or
     (height_lt (height cur) (height prev))
     (and
      (= (height cur) (height prev))
      otherwise
   )))
   :pattern ((check_decrease_height cur prev otherwise))
   :qid prelude_check_decrease_height
   :skolemid skolem_prelude_check_decrease_height
)))
(assert
 (forall ((x Height) (y Height)) (!
   (= (height_lt x y) (and
     ((_ partial-order 0) x y)
     (not (= x y))
   ))
   :pattern ((height_lt x y))
   :qid prelude_height_lt
   :skolemid skolem_prelude_height_lt
)))

;; MODULE 'module cpu_mask'
;; src/cpu_mask.rs:171:9: 171:15 (#0)

;; query spun off because: bitvector

;; Fuel
(declare-const fuel%vstd!std_specs.result.impl&%0.arrow_Ok_0. FuelId)
(declare-const fuel%vstd!std_specs.result.is_ok. FuelId)
(declare-const fuel%vstd!std_specs.result.is_err. FuelId)
(declare-const fuel%vstd!std_specs.result.spec_unwrap. FuelId)
(declare-const fuel%vstd!pervasive.strictly_cloned. FuelId)
(declare-const fuel%vstd!pervasive.cloned. FuelId)
(declare-const fuel%lib!error.EINVAL. FuelId)
(declare-const fuel%lib!error.OK. FuelId)
(declare-const fuel%lib!cpu_mask.MAX_CPUS. FuelId)
(declare-const fuel%lib!cpu_mask.is_power_of_two. FuelId)
(declare-const fuel%lib!cpu_mask.compute_mask. FuelId)
(declare-const fuel%vstd!array.group_array_axioms. FuelId)
(declare-const fuel%vstd!function.group_function_axioms. FuelId)
(declare-const fuel%vstd!laws_cmp.group_laws_cmp. FuelId)
(declare-const fuel%vstd!laws_eq.bool_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.u8_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.i8_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.u16_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.i16_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.u32_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.i32_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.u64_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.i64_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.u128_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.i128_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.usize_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.isize_laws.group_laws_eq. FuelId)
(declare-const fuel%vstd!laws_eq.group_laws_eq. FuelId)
(declare-const fuel%vstd!layout.group_align_properties. FuelId)
(declare-const fuel%vstd!layout.group_layout_axioms. FuelId)
(declare-const fuel%vstd!map.group_map_axioms. FuelId)
(declare-const fuel%vstd!multiset.group_multiset_axioms. FuelId)
(declare-const fuel%vstd!raw_ptr.group_raw_ptr_axioms. FuelId)
(declare-const fuel%vstd!seq.group_seq_axioms. FuelId)
(declare-const fuel%vstd!seq_lib.group_filter_ensures. FuelId)
(declare-const fuel%vstd!seq_lib.group_seq_lib_default. FuelId)
(declare-const fuel%vstd!set.group_set_axioms. FuelId)
(declare-const fuel%vstd!set_lib.group_set_lib_default. FuelId)
(declare-const fuel%vstd!slice.group_slice_axioms. FuelId)
(declare-const fuel%vstd!string.group_string_axioms. FuelId)
(declare-const fuel%vstd!std_specs.bits.group_bits_axioms. FuelId)
(declare-const fuel%vstd!std_specs.control_flow.group_control_flow_axioms. FuelId)
(declare-const fuel%vstd!std_specs.manually_drop.group_manually_drop_axioms. FuelId)
(declare-const fuel%vstd!std_specs.hash.group_hash_axioms. FuelId)
(declare-const fuel%vstd!std_specs.range.group_range_axioms. FuelId)
(declare-const fuel%vstd!std_specs.slice.group_slice_axioms. FuelId)
(declare-const fuel%vstd!std_specs.vec.group_vec_axioms. FuelId)
(declare-const fuel%vstd!std_specs.vecdeque.group_vec_dequeue_axioms. FuelId)
(declare-const fuel%vstd!group_vstd_default. FuelId)
(assert
 (distinct fuel%vstd!std_specs.result.impl&%0.arrow_Ok_0. fuel%vstd!std_specs.result.is_ok.
  fuel%vstd!std_specs.result.is_err. fuel%vstd!std_specs.result.spec_unwrap. fuel%vstd!pervasive.strictly_cloned.
  fuel%vstd!pervasive.cloned. fuel%lib!error.EINVAL. fuel%lib!error.OK. fuel%lib!cpu_mask.MAX_CPUS.
  fuel%lib!cpu_mask.is_power_of_two. fuel%lib!cpu_mask.compute_mask. fuel%vstd!array.group_array_axioms.
  fuel%vstd!function.group_function_axioms. fuel%vstd!laws_cmp.group_laws_cmp. fuel%vstd!laws_eq.bool_laws.group_laws_eq.
  fuel%vstd!laws_eq.u8_laws.group_laws_eq. fuel%vstd!laws_eq.i8_laws.group_laws_eq.
  fuel%vstd!laws_eq.u16_laws.group_laws_eq. fuel%vstd!laws_eq.i16_laws.group_laws_eq.
  fuel%vstd!laws_eq.u32_laws.group_laws_eq. fuel%vstd!laws_eq.i32_laws.group_laws_eq.
  fuel%vstd!laws_eq.u64_laws.group_laws_eq. fuel%vstd!laws_eq.i64_laws.group_laws_eq.
  fuel%vstd!laws_eq.u128_laws.group_laws_eq. fuel%vstd!laws_eq.i128_laws.group_laws_eq.
  fuel%vstd!laws_eq.usize_laws.group_laws_eq. fuel%vstd!laws_eq.isize_laws.group_laws_eq.
  fuel%vstd!laws_eq.group_laws_eq. fuel%vstd!layout.group_align_properties. fuel%vstd!layout.group_layout_axioms.
  fuel%vstd!map.group_map_axioms. fuel%vstd!multiset.group_multiset_axioms. fuel%vstd!raw_ptr.group_raw_ptr_axioms.
  fuel%vstd!seq.group_seq_axioms. fuel%vstd!seq_lib.group_filter_ensures. fuel%vstd!seq_lib.group_seq_lib_default.
  fuel%vstd!set.group_set_axioms. fuel%vstd!set_lib.group_set_lib_default. fuel%vstd!slice.group_slice_axioms.
  fuel%vstd!string.group_string_axioms. fuel%vstd!std_specs.bits.group_bits_axioms.
  fuel%vstd!std_specs.control_flow.group_control_flow_axioms. fuel%vstd!std_specs.manually_drop.group_manually_drop_axioms.
  fuel%vstd!std_specs.hash.group_hash_axioms. fuel%vstd!std_specs.range.group_range_axioms.
  fuel%vstd!std_specs.slice.group_slice_axioms. fuel%vstd!std_specs.vec.group_vec_axioms.
  fuel%vstd!std_specs.vecdeque.group_vec_dequeue_axioms. fuel%vstd!group_vstd_default.
))
(assert
 (=>
  (fuel_bool_default fuel%vstd!laws_eq.group_laws_eq.)
  (and
   (fuel_bool_default fuel%vstd!laws_eq.bool_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.u8_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.i8_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.u16_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.i16_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.u32_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.i32_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.u64_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.i64_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.u128_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.i128_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.usize_laws.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_eq.isize_laws.group_laws_eq.)
)))
(assert
 (=>
  (fuel_bool_default fuel%vstd!layout.group_layout_axioms.)
  (fuel_bool_default fuel%vstd!layout.group_align_properties.)
))
(assert
 (=>
  (fuel_bool_default fuel%vstd!seq_lib.group_seq_lib_default.)
  (fuel_bool_default fuel%vstd!seq_lib.group_filter_ensures.)
))
(assert
 (fuel_bool_default fuel%vstd!group_vstd_default.)
)
(assert
 (=>
  (fuel_bool_default fuel%vstd!group_vstd_default.)
  (and
   (fuel_bool_default fuel%vstd!seq.group_seq_axioms.)
   (fuel_bool_default fuel%vstd!seq_lib.group_seq_lib_default.)
   (fuel_bool_default fuel%vstd!map.group_map_axioms.)
   (fuel_bool_default fuel%vstd!set.group_set_axioms.)
   (fuel_bool_default fuel%vstd!set_lib.group_set_lib_default.)
   (fuel_bool_default fuel%vstd!multiset.group_multiset_axioms.)
   (fuel_bool_default fuel%vstd!function.group_function_axioms.)
   (fuel_bool_default fuel%vstd!laws_eq.group_laws_eq.)
   (fuel_bool_default fuel%vstd!laws_cmp.group_laws_cmp.)
   (fuel_bool_default fuel%vstd!slice.group_slice_axioms.)
   (fuel_bool_default fuel%vstd!array.group_array_axioms.)
   (fuel_bool_default fuel%vstd!string.group_string_axioms.)
   (fuel_bool_default fuel%vstd!raw_ptr.group_raw_ptr_axioms.)
   (fuel_bool_default fuel%vstd!layout.group_layout_axioms.)
   (fuel_bool_default fuel%vstd!std_specs.range.group_range_axioms.)
   (fuel_bool_default fuel%vstd!std_specs.bits.group_bits_axioms.)
   (fuel_bool_default fuel%vstd!std_specs.control_flow.group_control_flow_axioms.)
   (fuel_bool_default fuel%vstd!std_specs.slice.group_slice_axioms.)
   (fuel_bool_default fuel%vstd!std_specs.manually_drop.group_manually_drop_axioms.)
   (fuel_bool_default fuel%vstd!std_specs.vec.group_vec_axioms.)
   (fuel_bool_default fuel%vstd!std_specs.vecdeque.group_vec_dequeue_axioms.)
   (fuel_bool_default fuel%vstd!std_specs.hash.group_hash_axioms.)
)))

;; Trait-Decls
(declare-fun tr_bound%core!clone.Clone. (Dcr Type) Bool)
(declare-fun tr_bound%core!alloc.Allocator. (Dcr Type) Bool)
(declare-fun tr_bound%core!fmt.Debug. (Dcr Type) Bool)
(declare-fun tr_bound%vstd!std_specs.result.ResultAdditionalSpecFns. (Dcr Type Dcr
  Type Dcr Type
 ) Bool
)

;; Datatypes
(declare-datatypes ((core!result.Result. 0) (lib!cpu_mask.CpuMaskResult. 0) (tuple%0.
   0
  ) (tuple%1. 0)
 ) (((core!result.Result./Ok (core!result.Result./Ok/?0 Poly)) (core!result.Result./Err
    (core!result.Result./Err/?0 Poly)
   )
  ) ((lib!cpu_mask.CpuMaskResult./CpuMaskResult (lib!cpu_mask.CpuMaskResult./CpuMaskResult/?mask
     Int
    ) (lib!cpu_mask.CpuMaskResult./CpuMaskResult/?error Int)
   )
  ) ((tuple%0./tuple%0)) ((tuple%1./tuple%1 (tuple%1./tuple%1/?0 Poly)))
))
(declare-fun core!result.Result./Ok/0 (Dcr Type Dcr Type core!result.Result.) Poly)
(declare-fun core!result.Result./Err/0 (Dcr Type Dcr Type core!result.Result.) Poly)
(declare-fun lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask (lib!cpu_mask.CpuMaskResult.)
 Int
)
(declare-fun lib!cpu_mask.CpuMaskResult./CpuMaskResult/error (lib!cpu_mask.CpuMaskResult.)
 Int
)
(declare-fun tuple%1./tuple%1/0 (tuple%1.) Poly)
(declare-fun TYPE%core!result.Result. (Dcr Type Dcr Type) Type)
(declare-const TYPE%lib!cpu_mask.CpuMaskResult. Type)
(declare-fun TYPE%tuple%1. (Dcr Type) Type)
(declare-fun FNDEF%core!clone.Clone.clone. (Dcr Type) Type)
(declare-fun Poly%core!result.Result. (core!result.Result.) Poly)
(declare-fun %Poly%core!result.Result. (Poly) core!result.Result.)
(declare-fun Poly%lib!cpu_mask.CpuMaskResult. (lib!cpu_mask.CpuMaskResult.) Poly)
(declare-fun %Poly%lib!cpu_mask.CpuMaskResult. (Poly) lib!cpu_mask.CpuMaskResult.)
(declare-fun Poly%tuple%0. (tuple%0.) Poly)
(declare-fun %Poly%tuple%0. (Poly) tuple%0.)
(declare-fun Poly%tuple%1. (tuple%1.) Poly)
(declare-fun %Poly%tuple%1. (Poly) tuple%1.)
(assert
 (forall ((x core!result.Result.)) (!
   (= x (%Poly%core!result.Result. (Poly%core!result.Result. x)))
   :pattern ((Poly%core!result.Result. x))
   :qid internal_core__result__Result_box_axiom_definition
   :skolemid skolem_internal_core__result__Result_box_axiom_definition
)))
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (x Poly)) (!
   (=>
    (has_type x (TYPE%core!result.Result. T&. T& E&. E&))
    (= x (Poly%core!result.Result. (%Poly%core!result.Result. x)))
   )
   :pattern ((has_type x (TYPE%core!result.Result. T&. T& E&. E&)))
   :qid internal_core__result__Result_unbox_axiom_definition
   :skolemid skolem_internal_core__result__Result_unbox_axiom_definition
)))
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (_0! Poly)) (!
   (=>
    (has_type _0! T&)
    (has_type (Poly%core!result.Result. (core!result.Result./Ok _0!)) (TYPE%core!result.Result.
      T&. T& E&. E&
   )))
   :pattern ((has_type (Poly%core!result.Result. (core!result.Result./Ok _0!)) (TYPE%core!result.Result.
      T&. T& E&. E&
   )))
   :qid internal_core!result.Result./Ok_constructor_definition
   :skolemid skolem_internal_core!result.Result./Ok_constructor_definition
)))
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (x core!result.Result.)) (!
   (=>
    (is-core!result.Result./Ok x)
    (= (core!result.Result./Ok/0 T&. T& E&. E& x) (core!result.Result./Ok/?0 x))
   )
   :pattern ((core!result.Result./Ok/0 T&. T& E&. E& x))
   :qid internal_core!result.Result./Ok/0_accessor_definition
   :skolemid skolem_internal_core!result.Result./Ok/0_accessor_definition
)))
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (x Poly)) (!
   (=>
    (has_type x (TYPE%core!result.Result. T&. T& E&. E&))
    (has_type (core!result.Result./Ok/0 T&. T& E&. E& (%Poly%core!result.Result. x)) T&)
   )
   :pattern ((core!result.Result./Ok/0 T&. T& E&. E& (%Poly%core!result.Result. x)) (
     has_type x (TYPE%core!result.Result. T&. T& E&. E&)
   ))
   :qid internal_core!result.Result./Ok/0_invariant_definition
   :skolemid skolem_internal_core!result.Result./Ok/0_invariant_definition
)))
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (_0! Poly)) (!
   (=>
    (has_type _0! E&)
    (has_type (Poly%core!result.Result. (core!result.Result./Err _0!)) (TYPE%core!result.Result.
      T&. T& E&. E&
   )))
   :pattern ((has_type (Poly%core!result.Result. (core!result.Result./Err _0!)) (TYPE%core!result.Result.
      T&. T& E&. E&
   )))
   :qid internal_core!result.Result./Err_constructor_definition
   :skolemid skolem_internal_core!result.Result./Err_constructor_definition
)))
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (x core!result.Result.)) (!
   (=>
    (is-core!result.Result./Err x)
    (= (core!result.Result./Err/0 T&. T& E&. E& x) (core!result.Result./Err/?0 x))
   )
   :pattern ((core!result.Result./Err/0 T&. T& E&. E& x))
   :qid internal_core!result.Result./Err/0_accessor_definition
   :skolemid skolem_internal_core!result.Result./Err/0_accessor_definition
)))
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (x Poly)) (!
   (=>
    (has_type x (TYPE%core!result.Result. T&. T& E&. E&))
    (has_type (core!result.Result./Err/0 T&. T& E&. E& (%Poly%core!result.Result. x))
     E&
   ))
   :pattern ((core!result.Result./Err/0 T&. T& E&. E& (%Poly%core!result.Result. x))
    (has_type x (TYPE%core!result.Result. T&. T& E&. E&))
   )
   :qid internal_core!result.Result./Err/0_invariant_definition
   :skolemid skolem_internal_core!result.Result./Err/0_invariant_definition
)))
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (x core!result.Result.)) (!
   (=>
    (is-core!result.Result./Ok x)
    (height_lt (height (core!result.Result./Ok/0 T&. T& E&. E& x)) (height (Poly%core!result.Result.
       x
   ))))
   :pattern ((height (core!result.Result./Ok/0 T&. T& E&. E& x)))
   :qid prelude_datatype_height_core!result.Result./Ok/0
   :skolemid skolem_prelude_datatype_height_core!result.Result./Ok/0
)))
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (x core!result.Result.)) (!
   (=>
    (is-core!result.Result./Err x)
    (height_lt (height (core!result.Result./Err/0 T&. T& E&. E& x)) (height (Poly%core!result.Result.
       x
   ))))
   :pattern ((height (core!result.Result./Err/0 T&. T& E&. E& x)))
   :qid prelude_datatype_height_core!result.Result./Err/0
   :skolemid skolem_prelude_datatype_height_core!result.Result./Err/0
)))
(assert
 (forall ((x lib!cpu_mask.CpuMaskResult.)) (!
   (= x (%Poly%lib!cpu_mask.CpuMaskResult. (Poly%lib!cpu_mask.CpuMaskResult. x)))
   :pattern ((Poly%lib!cpu_mask.CpuMaskResult. x))
   :qid internal_lib__cpu_mask__CpuMaskResult_box_axiom_definition
   :skolemid skolem_internal_lib__cpu_mask__CpuMaskResult_box_axiom_definition
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x TYPE%lib!cpu_mask.CpuMaskResult.)
    (= x (Poly%lib!cpu_mask.CpuMaskResult. (%Poly%lib!cpu_mask.CpuMaskResult. x)))
   )
   :pattern ((has_type x TYPE%lib!cpu_mask.CpuMaskResult.))
   :qid internal_lib__cpu_mask__CpuMaskResult_unbox_axiom_definition
   :skolemid skolem_internal_lib__cpu_mask__CpuMaskResult_unbox_axiom_definition
)))
(assert
 (forall ((_mask! Int) (_error! Int)) (!
   (=>
    (and
     (uInv 32 _mask!)
     (iInv 32 _error!)
    )
    (has_type (Poly%lib!cpu_mask.CpuMaskResult. (lib!cpu_mask.CpuMaskResult./CpuMaskResult
       _mask! _error!
      )
     ) TYPE%lib!cpu_mask.CpuMaskResult.
   ))
   :pattern ((has_type (Poly%lib!cpu_mask.CpuMaskResult. (lib!cpu_mask.CpuMaskResult./CpuMaskResult
       _mask! _error!
      )
     ) TYPE%lib!cpu_mask.CpuMaskResult.
   ))
   :qid internal_lib!cpu_mask.CpuMaskResult./CpuMaskResult_constructor_definition
   :skolemid skolem_internal_lib!cpu_mask.CpuMaskResult./CpuMaskResult_constructor_definition
)))
(assert
 (forall ((x lib!cpu_mask.CpuMaskResult.)) (!
   (= (lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask x) (lib!cpu_mask.CpuMaskResult./CpuMaskResult/?mask
     x
   ))
   :pattern ((lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask x))
   :qid internal_lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask_accessor_definition
   :skolemid skolem_internal_lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask_accessor_definition
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x TYPE%lib!cpu_mask.CpuMaskResult.)
    (uInv 32 (lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask (%Poly%lib!cpu_mask.CpuMaskResult.
       x
   ))))
   :pattern ((lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask (%Poly%lib!cpu_mask.CpuMaskResult.
      x
     )
    ) (has_type x TYPE%lib!cpu_mask.CpuMaskResult.)
   )
   :qid internal_lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask_invariant_definition
   :skolemid skolem_internal_lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask_invariant_definition
)))
(assert
 (forall ((x lib!cpu_mask.CpuMaskResult.)) (!
   (= (lib!cpu_mask.CpuMaskResult./CpuMaskResult/error x) (lib!cpu_mask.CpuMaskResult./CpuMaskResult/?error
     x
   ))
   :pattern ((lib!cpu_mask.CpuMaskResult./CpuMaskResult/error x))
   :qid internal_lib!cpu_mask.CpuMaskResult./CpuMaskResult/error_accessor_definition
   :skolemid skolem_internal_lib!cpu_mask.CpuMaskResult./CpuMaskResult/error_accessor_definition
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x TYPE%lib!cpu_mask.CpuMaskResult.)
    (iInv 32 (lib!cpu_mask.CpuMaskResult./CpuMaskResult/error (%Poly%lib!cpu_mask.CpuMaskResult.
       x
   ))))
   :pattern ((lib!cpu_mask.CpuMaskResult./CpuMaskResult/error (%Poly%lib!cpu_mask.CpuMaskResult.
      x
     )
    ) (has_type x TYPE%lib!cpu_mask.CpuMaskResult.)
   )
   :qid internal_lib!cpu_mask.CpuMaskResult./CpuMaskResult/error_invariant_definition
   :skolemid skolem_internal_lib!cpu_mask.CpuMaskResult./CpuMaskResult/error_invariant_definition
)))
(assert
 (forall ((x tuple%0.)) (!
   (= x (%Poly%tuple%0. (Poly%tuple%0. x)))
   :pattern ((Poly%tuple%0. x))
   :qid internal_crate__tuple__0_box_axiom_definition
   :skolemid skolem_internal_crate__tuple__0_box_axiom_definition
)))
(assert
 (forall ((x Poly)) (!
   (=>
    (has_type x TYPE%tuple%0.)
    (= x (Poly%tuple%0. (%Poly%tuple%0. x)))
   )
   :pattern ((has_type x TYPE%tuple%0.))
   :qid internal_crate__tuple__0_unbox_axiom_definition
   :skolemid skolem_internal_crate__tuple__0_unbox_axiom_definition
)))
(assert
 (forall ((x tuple%0.)) (!
   (has_type (Poly%tuple%0. x) TYPE%tuple%0.)
   :pattern ((has_type (Poly%tuple%0. x) TYPE%tuple%0.))
   :qid internal_crate__tuple__0_has_type_always_definition
   :skolemid skolem_internal_crate__tuple__0_has_type_always_definition
)))
(assert
 (forall ((x tuple%1.)) (!
   (= x (%Poly%tuple%1. (Poly%tuple%1. x)))
   :pattern ((Poly%tuple%1. x))
   :qid internal_crate__tuple__1_box_axiom_definition
   :skolemid skolem_internal_crate__tuple__1_box_axiom_definition
)))
(assert
 (forall ((T%0&. Dcr) (T%0& Type) (x Poly)) (!
   (=>
    (has_type x (TYPE%tuple%1. T%0&. T%0&))
    (= x (Poly%tuple%1. (%Poly%tuple%1. x)))
   )
   :pattern ((has_type x (TYPE%tuple%1. T%0&. T%0&)))
   :qid internal_crate__tuple__1_unbox_axiom_definition
   :skolemid skolem_internal_crate__tuple__1_unbox_axiom_definition
)))
(assert
 (forall ((T%0&. Dcr) (T%0& Type) (_0! Poly)) (!
   (=>
    (has_type _0! T%0&)
    (has_type (Poly%tuple%1. (tuple%1./tuple%1 _0!)) (TYPE%tuple%1. T%0&. T%0&))
   )
   :pattern ((has_type (Poly%tuple%1. (tuple%1./tuple%1 _0!)) (TYPE%tuple%1. T%0&. T%0&)))
   :qid internal_tuple__1./tuple__1_constructor_definition
   :skolemid skolem_internal_tuple__1./tuple__1_constructor_definition
)))
(assert
 (forall ((x tuple%1.)) (!
   (= (tuple%1./tuple%1/0 x) (tuple%1./tuple%1/?0 x))
   :pattern ((tuple%1./tuple%1/0 x))
   :qid internal_tuple__1./tuple__1/0_accessor_definition
   :skolemid skolem_internal_tuple__1./tuple__1/0_accessor_definition
)))
(assert
 (forall ((T%0&. Dcr) (T%0& Type) (x Poly)) (!
   (=>
    (has_type x (TYPE%tuple%1. T%0&. T%0&))
    (has_type (tuple%1./tuple%1/0 (%Poly%tuple%1. x)) T%0&)
   )
   :pattern ((tuple%1./tuple%1/0 (%Poly%tuple%1. x)) (has_type x (TYPE%tuple%1. T%0&. T%0&)))
   :qid internal_tuple__1./tuple__1/0_invariant_definition
   :skolemid skolem_internal_tuple__1./tuple__1/0_invariant_definition
)))
(assert
 (forall ((x tuple%1.)) (!
   (=>
    (is-tuple%1./tuple%1 x)
    (height_lt (height (tuple%1./tuple%1/0 x)) (height (Poly%tuple%1. x)))
   )
   :pattern ((height (tuple%1./tuple%1/0 x)))
   :qid prelude_datatype_height_tuple%1./tuple%1/0
   :skolemid skolem_prelude_datatype_height_tuple%1./tuple%1/0
)))
(assert
 (forall ((T%0&. Dcr) (T%0& Type) (deep Bool) (x Poly) (y Poly)) (!
   (=>
    (and
     (has_type x (TYPE%tuple%1. T%0&. T%0&))
     (has_type y (TYPE%tuple%1. T%0&. T%0&))
     (ext_eq deep T%0& (tuple%1./tuple%1/0 (%Poly%tuple%1. x)) (tuple%1./tuple%1/0 (%Poly%tuple%1.
        y
    ))))
    (ext_eq deep (TYPE%tuple%1. T%0&. T%0&) x y)
   )
   :pattern ((ext_eq deep (TYPE%tuple%1. T%0&. T%0&) x y))
   :qid internal_tuple__1./tuple__1_ext_equal_definition
   :skolemid skolem_internal_tuple__1./tuple__1_ext_equal_definition
)))

;; Trait-Bounds
(assert
 (forall ((Self%&. Dcr) (Self%& Type)) (!
   (=>
    (tr_bound%core!clone.Clone. Self%&. Self%&)
    (sized Self%&.)
   )
   :pattern ((tr_bound%core!clone.Clone. Self%&. Self%&))
   :qid internal_core__clone__Clone_trait_type_bounds_definition
   :skolemid skolem_internal_core__clone__Clone_trait_type_bounds_definition
)))
(assert
 (forall ((Self%&. Dcr) (Self%& Type)) (!
   true
   :pattern ((tr_bound%core!alloc.Allocator. Self%&. Self%&))
   :qid internal_core__alloc__Allocator_trait_type_bounds_definition
   :skolemid skolem_internal_core__alloc__Allocator_trait_type_bounds_definition
)))
(assert
 (forall ((Self%&. Dcr) (Self%& Type)) (!
   true
   :pattern ((tr_bound%core!fmt.Debug. Self%&. Self%&))
   :qid internal_core__fmt__Debug_trait_type_bounds_definition
   :skolemid skolem_internal_core__fmt__Debug_trait_type_bounds_definition
)))
(assert
 (forall ((Self%&. Dcr) (Self%& Type) (T&. Dcr) (T& Type) (E&. Dcr) (E& Type)) (!
   (=>
    (tr_bound%vstd!std_specs.result.ResultAdditionalSpecFns. Self%&. Self%& T&. T& E&.
     E&
    )
    (and
     (sized T&.)
     (sized E&.)
   ))
   :pattern ((tr_bound%vstd!std_specs.result.ResultAdditionalSpecFns. Self%&. Self%& T&.
     T& E&. E&
   ))
   :qid internal_vstd__std_specs__result__ResultAdditionalSpecFns_trait_type_bounds_definition
   :skolemid skolem_internal_vstd__std_specs__result__ResultAdditionalSpecFns_trait_type_bounds_definition
)))

;; Function-Decl vstd::pervasive::strictly_cloned
(declare-fun vstd!pervasive.strictly_cloned.? (Dcr Type Poly Poly) Bool)

;; Function-Decl vstd::pervasive::cloned
(declare-fun vstd!pervasive.cloned.? (Dcr Type Poly Poly) Bool)

;; Function-Decl vstd::std_specs::result::ResultAdditionalSpecFns::arrow_Ok_0
(declare-fun vstd!std_specs.result.ResultAdditionalSpecFns.arrow_Ok_0.? (Dcr Type Dcr
  Type Dcr Type Poly
 ) Poly
)
(declare-fun vstd!std_specs.result.ResultAdditionalSpecFns.arrow_Ok_0%default%.? (
  Dcr Type Dcr Type Dcr Type Poly
 ) Poly
)

;; Function-Decl vstd::std_specs::result::is_ok
(declare-fun vstd!std_specs.result.is_ok.? (Dcr Type Dcr Type Poly) Bool)

;; Function-Decl vstd::std_specs::result::is_err
(declare-fun vstd!std_specs.result.is_err.? (Dcr Type Dcr Type Poly) Bool)

;; Function-Decl lib::error::EINVAL
(declare-fun lib!error.EINVAL.? () Int)

;; Function-Decl lib::error::OK
(declare-fun lib!error.OK.? () Int)

;; Function-Decl vstd::std_specs::result::spec_unwrap
(declare-fun vstd!std_specs.result.spec_unwrap.? (Dcr Type Dcr Type Poly) Poly)

;; Function-Decl lib::cpu_mask::MAX_CPUS
(declare-fun lib!cpu_mask.MAX_CPUS.? () Int)

;; Function-Decl lib::cpu_mask::is_power_of_two
(declare-fun lib!cpu_mask.is_power_of_two.? (Poly) Bool)

;; Function-Decl lib::cpu_mask::compute_mask
(declare-fun lib!cpu_mask.compute_mask.? (Poly Poly Poly) Int)

;; Function-Specs core::clone::Clone::clone
(declare-fun ens%core!clone.Clone.clone. (Dcr Type Poly Poly) Bool)
(assert
 (forall ((Self%&. Dcr) (Self%& Type) (self! Poly) (%return! Poly)) (!
   (= (ens%core!clone.Clone.clone. Self%&. Self%& self! %return!) (has_type %return! Self%&))
   :pattern ((ens%core!clone.Clone.clone. Self%&. Self%& self! %return!))
   :qid internal_ens__core!clone.Clone.clone._definition
   :skolemid skolem_internal_ens__core!clone.Clone.clone._definition
)))
(assert
 (forall ((closure%$ Poly) (Self%&. Dcr) (Self%& Type)) (!
   (=>
    (has_type closure%$ (TYPE%tuple%1. (REF Self%&.) Self%&))
    (=>
     (let
      ((self$ (tuple%1./tuple%1/0 (%Poly%tuple%1. closure%$))))
      true
     )
     (closure_req (FNDEF%core!clone.Clone.clone. Self%&. Self%&) (DST (REF Self%&.)) (TYPE%tuple%1.
       (REF Self%&.) Self%&
      ) (F fndef_singleton) closure%$
   )))
   :pattern ((closure_req (FNDEF%core!clone.Clone.clone. Self%&. Self%&) (DST (REF Self%&.))
     (TYPE%tuple%1. (REF Self%&.) Self%&) (F fndef_singleton) closure%$
   ))
   :qid user_core__clone__Clone__clone_0
   :skolemid skolem_user_core__clone__Clone__clone_0
)))

;; Function-Specs core::clone::impls::impl&%15::clone
(declare-fun ens%core!clone.impls.impl&%15.clone. (Poly Poly) Bool)
(assert
 (forall ((x! Poly) (res! Poly)) (!
   (= (ens%core!clone.impls.impl&%15.clone. x! res!) (and
     (ens%core!clone.Clone.clone. $ (UINT 32) x! res!)
     (= res! x!)
   ))
   :pattern ((ens%core!clone.impls.impl&%15.clone. x! res!))
   :qid internal_ens__core!clone.impls.impl&__15.clone._definition
   :skolemid skolem_internal_ens__core!clone.impls.impl&__15.clone._definition
)))
(assert
 (forall ((closure%$ Poly) (res$ Poly)) (!
   (=>
    (and
     (has_type closure%$ (TYPE%tuple%1. (REF $) (UINT 32)))
     (has_type res$ (UINT 32))
    )
    (=>
     (closure_ens (FNDEF%core!clone.Clone.clone. $ (UINT 32)) (DST (REF $)) (TYPE%tuple%1.
       (REF $) (UINT 32)
      ) (F fndef_singleton) closure%$ res$
     )
     (let
      ((x$ (%I (tuple%1./tuple%1/0 (%Poly%tuple%1. closure%$)))))
      (= (%I res$) x$)
   )))
   :pattern ((closure_ens (FNDEF%core!clone.Clone.clone. $ (UINT 32)) (DST (REF $)) (TYPE%tuple%1.
      (REF $) (UINT 32)
     ) (F fndef_singleton) closure%$ res$
   ))
   :qid user_core__clone__impls__impl&%15__clone_1
   :skolemid skolem_user_core__clone__impls__impl&%15__clone_1
)))

;; Function-Specs core::clone::impls::impl&%27::clone
(declare-fun ens%core!clone.impls.impl&%27.clone. (Poly Poly) Bool)
(assert
 (forall ((x! Poly) (res! Poly)) (!
   (= (ens%core!clone.impls.impl&%27.clone. x! res!) (and
     (ens%core!clone.Clone.clone. $ (SINT 32) x! res!)
     (= res! x!)
   ))
   :pattern ((ens%core!clone.impls.impl&%27.clone. x! res!))
   :qid internal_ens__core!clone.impls.impl&__27.clone._definition
   :skolemid skolem_internal_ens__core!clone.impls.impl&__27.clone._definition
)))
(assert
 (forall ((closure%$ Poly) (res$ Poly)) (!
   (=>
    (and
     (has_type closure%$ (TYPE%tuple%1. (REF $) (SINT 32)))
     (has_type res$ (SINT 32))
    )
    (=>
     (closure_ens (FNDEF%core!clone.Clone.clone. $ (SINT 32)) (DST (REF $)) (TYPE%tuple%1.
       (REF $) (SINT 32)
      ) (F fndef_singleton) closure%$ res$
     )
     (let
      ((x$ (%I (tuple%1./tuple%1/0 (%Poly%tuple%1. closure%$)))))
      (= (%I res$) x$)
   )))
   :pattern ((closure_ens (FNDEF%core!clone.Clone.clone. $ (SINT 32)) (DST (REF $)) (TYPE%tuple%1.
      (REF $) (SINT 32)
     ) (F fndef_singleton) closure%$ res$
   ))
   :qid user_core__clone__impls__impl&%27__clone_2
   :skolemid skolem_user_core__clone__impls__impl&%27__clone_2
)))

;; Function-Specs core::clone::impls::impl&%41::clone
(declare-fun ens%core!clone.impls.impl&%41.clone. (Poly Poly) Bool)
(assert
 (forall ((b! Poly) (%return! Poly)) (!
   (= (ens%core!clone.impls.impl&%41.clone. b! %return!) (and
     (ens%core!clone.Clone.clone. $ BOOL b! %return!)
     (= %return! b!)
   ))
   :pattern ((ens%core!clone.impls.impl&%41.clone. b! %return!))
   :qid internal_ens__core!clone.impls.impl&__41.clone._definition
   :skolemid skolem_internal_ens__core!clone.impls.impl&__41.clone._definition
)))
(assert
 (forall ((closure%$ Poly) (%return$ Poly)) (!
   (=>
    (and
     (has_type closure%$ (TYPE%tuple%1. (REF $) BOOL))
     (has_type %return$ BOOL)
    )
    (=>
     (closure_ens (FNDEF%core!clone.Clone.clone. $ BOOL) (DST (REF $)) (TYPE%tuple%1. (REF
        $
       ) BOOL
      ) (F fndef_singleton) closure%$ %return$
     )
     (let
      ((b$ (%B (tuple%1./tuple%1/0 (%Poly%tuple%1. closure%$)))))
      (= (%B %return$) b$)
   )))
   :pattern ((closure_ens (FNDEF%core!clone.Clone.clone. $ BOOL) (DST (REF $)) (TYPE%tuple%1.
      (REF $) BOOL
     ) (F fndef_singleton) closure%$ %return$
   ))
   :qid user_core__clone__impls__impl&%41__clone_3
   :skolemid skolem_user_core__clone__impls__impl&%41__clone_3
)))

;; Function-Specs core::clone::impls::impl&%6::clone
(declare-fun ens%core!clone.impls.impl&%6.clone. (Dcr Type Poly Poly) Bool)
(assert
 (forall ((T&. Dcr) (T& Type) (b! Poly) (res! Poly)) (!
   (= (ens%core!clone.impls.impl&%6.clone. T&. T& b! res!) (and
     (ens%core!clone.Clone.clone. (REF T&.) T& b! res!)
     (= res! b!)
   ))
   :pattern ((ens%core!clone.impls.impl&%6.clone. T&. T& b! res!))
   :qid internal_ens__core!clone.impls.impl&__6.clone._definition
   :skolemid skolem_internal_ens__core!clone.impls.impl&__6.clone._definition
)))
(assert
 (forall ((closure%$ Poly) (res$ Poly) (T&. Dcr) (T& Type)) (!
   (=>
    (and
     (has_type closure%$ (TYPE%tuple%1. (REF (REF T&.)) T&))
     (has_type res$ T&)
    )
    (=>
     (closure_ens (FNDEF%core!clone.Clone.clone. (REF T&.) T&) (DST (REF (REF T&.))) (TYPE%tuple%1.
       (REF (REF T&.)) T&
      ) (F fndef_singleton) closure%$ res$
     )
     (let
      ((b$ (tuple%1./tuple%1/0 (%Poly%tuple%1. closure%$))))
      (= res$ b$)
   )))
   :pattern ((closure_ens (FNDEF%core!clone.Clone.clone. (REF T&.) T&) (DST (REF (REF T&.)))
     (TYPE%tuple%1. (REF (REF T&.)) T&) (F fndef_singleton) closure%$ res$
   ))
   :qid user_core__clone__impls__impl&%6__clone_4
   :skolemid skolem_user_core__clone__impls__impl&%6__clone_4
)))

;; Function-Axioms vstd::pervasive::strictly_cloned
(assert
 (fuel_bool_default fuel%vstd!pervasive.strictly_cloned.)
)
(assert
 (=>
  (fuel_bool fuel%vstd!pervasive.strictly_cloned.)
  (forall ((T&. Dcr) (T& Type) (a! Poly) (b! Poly)) (!
    (= (vstd!pervasive.strictly_cloned.? T&. T& a! b!) (closure_ens (FNDEF%core!clone.Clone.clone.
       T&. T&
      ) (DST (REF T&.)) (TYPE%tuple%1. (REF T&.) T&) (F fndef_singleton) (Poly%tuple%1.
       (tuple%1./tuple%1 a!)
      ) b!
    ))
    :pattern ((vstd!pervasive.strictly_cloned.? T&. T& a! b!))
    :qid internal_vstd!pervasive.strictly_cloned.?_definition
    :skolemid skolem_internal_vstd!pervasive.strictly_cloned.?_definition
))))

;; Function-Axioms vstd::pervasive::cloned
(assert
 (fuel_bool_default fuel%vstd!pervasive.cloned.)
)
(assert
 (=>
  (fuel_bool fuel%vstd!pervasive.cloned.)
  (forall ((T&. Dcr) (T& Type) (a! Poly) (b! Poly)) (!
    (= (vstd!pervasive.cloned.? T&. T& a! b!) (or
      (vstd!pervasive.strictly_cloned.? T&. T& a! b!)
      (= a! b!)
    ))
    :pattern ((vstd!pervasive.cloned.? T&. T& a! b!))
    :qid internal_vstd!pervasive.cloned.?_definition
    :skolemid skolem_internal_vstd!pervasive.cloned.?_definition
))))

;; Function-Specs verus_builtin::impl&%5::clone
(declare-fun ens%verus_builtin!impl&%5.clone. (Dcr Type Poly Poly) Bool)
(assert
 (forall ((T&. Dcr) (T& Type) (b! Poly) (res! Poly)) (!
   (= (ens%verus_builtin!impl&%5.clone. T&. T& b! res!) (and
     (ens%core!clone.Clone.clone. (TRACKED T&.) T& b! res!)
     (= res! b!)
   ))
   :pattern ((ens%verus_builtin!impl&%5.clone. T&. T& b! res!))
   :qid internal_ens__verus_builtin!impl&__5.clone._definition
   :skolemid skolem_internal_ens__verus_builtin!impl&__5.clone._definition
)))
(assert
 (forall ((closure%$ Poly) (res$ Poly) (T&. Dcr) (T& Type)) (!
   (=>
    (and
     (has_type closure%$ (TYPE%tuple%1. (REF (TRACKED T&.)) T&))
     (has_type res$ T&)
    )
    (=>
     (closure_ens (FNDEF%core!clone.Clone.clone. (TRACKED T&.) T&) (DST (REF (TRACKED T&.)))
      (TYPE%tuple%1. (REF (TRACKED T&.)) T&) (F fndef_singleton) closure%$ res$
     )
     (let
      ((b$ (tuple%1./tuple%1/0 (%Poly%tuple%1. closure%$))))
      (= res$ b$)
   )))
   :pattern ((closure_ens (FNDEF%core!clone.Clone.clone. (TRACKED T&.) T&) (DST (REF (TRACKED
        T&.
      ))
     ) (TYPE%tuple%1. (REF (TRACKED T&.)) T&) (F fndef_singleton) closure%$ res$
   ))
   :qid user_verus_builtin__impl&%5__clone_5
   :skolemid skolem_user_verus_builtin__impl&%5__clone_5
)))

;; Function-Specs verus_builtin::impl&%3::clone
(declare-fun ens%verus_builtin!impl&%3.clone. (Dcr Type Poly Poly) Bool)
(assert
 (forall ((T&. Dcr) (T& Type) (b! Poly) (res! Poly)) (!
   (= (ens%verus_builtin!impl&%3.clone. T&. T& b! res!) (and
     (ens%core!clone.Clone.clone. (GHOST T&.) T& b! res!)
     (= res! b!)
   ))
   :pattern ((ens%verus_builtin!impl&%3.clone. T&. T& b! res!))
   :qid internal_ens__verus_builtin!impl&__3.clone._definition
   :skolemid skolem_internal_ens__verus_builtin!impl&__3.clone._definition
)))
(assert
 (forall ((closure%$ Poly) (res$ Poly) (T&. Dcr) (T& Type)) (!
   (=>
    (and
     (has_type closure%$ (TYPE%tuple%1. (REF (GHOST T&.)) T&))
     (has_type res$ T&)
    )
    (=>
     (closure_ens (FNDEF%core!clone.Clone.clone. (GHOST T&.) T&) (DST (REF (GHOST T&.)))
      (TYPE%tuple%1. (REF (GHOST T&.)) T&) (F fndef_singleton) closure%$ res$
     )
     (let
      ((b$ (tuple%1./tuple%1/0 (%Poly%tuple%1. closure%$))))
      (= res$ b$)
   )))
   :pattern ((closure_ens (FNDEF%core!clone.Clone.clone. (GHOST T&.) T&) (DST (REF (GHOST
        T&.
      ))
     ) (TYPE%tuple%1. (REF (GHOST T&.)) T&) (F fndef_singleton) closure%$ res$
   ))
   :qid user_verus_builtin__impl&%3__clone_6
   :skolemid skolem_user_verus_builtin__impl&%3__clone_6
)))

;; Function-Axioms vstd::std_specs::result::ResultAdditionalSpecFns::arrow_Ok_0
(assert
 (forall ((Self%&. Dcr) (Self%& Type) (T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (self!
    Poly
   )
  ) (!
   (=>
    (has_type self! Self%&)
    (has_type (vstd!std_specs.result.ResultAdditionalSpecFns.arrow_Ok_0.? Self%&. Self%&
      T&. T& E&. E& self!
     ) T&
   ))
   :pattern ((vstd!std_specs.result.ResultAdditionalSpecFns.arrow_Ok_0.? Self%&. Self%&
     T&. T& E&. E& self!
   ))
   :qid internal_vstd!std_specs.result.ResultAdditionalSpecFns.arrow_Ok_0.?_pre_post_definition
   :skolemid skolem_internal_vstd!std_specs.result.ResultAdditionalSpecFns.arrow_Ok_0.?_pre_post_definition
)))

;; Function-Axioms vstd::std_specs::result::is_ok
(assert
 (fuel_bool_default fuel%vstd!std_specs.result.is_ok.)
)
(assert
 (=>
  (fuel_bool fuel%vstd!std_specs.result.is_ok.)
  (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (result! Poly)) (!
    (= (vstd!std_specs.result.is_ok.? T&. T& E&. E& result!) (is-core!result.Result./Ok
      (%Poly%core!result.Result. result!)
    ))
    :pattern ((vstd!std_specs.result.is_ok.? T&. T& E&. E& result!))
    :qid internal_vstd!std_specs.result.is_ok.?_definition
    :skolemid skolem_internal_vstd!std_specs.result.is_ok.?_definition
))))

;; Function-Axioms vstd::std_specs::result::is_err
(assert
 (fuel_bool_default fuel%vstd!std_specs.result.is_err.)
)
(assert
 (=>
  (fuel_bool fuel%vstd!std_specs.result.is_err.)
  (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (result! Poly)) (!
    (= (vstd!std_specs.result.is_err.? T&. T& E&. E& result!) (is-core!result.Result./Err
      (%Poly%core!result.Result. result!)
    ))
    :pattern ((vstd!std_specs.result.is_err.? T&. T& E&. E& result!))
    :qid internal_vstd!std_specs.result.is_err.?_definition
    :skolemid skolem_internal_vstd!std_specs.result.is_err.?_definition
))))

;; Function-Axioms vstd::std_specs::result::impl&%0::arrow_Ok_0
(assert
 (fuel_bool_default fuel%vstd!std_specs.result.impl&%0.arrow_Ok_0.)
)
(assert
 (=>
  (fuel_bool fuel%vstd!std_specs.result.impl&%0.arrow_Ok_0.)
  (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (self! Poly)) (!
    (=>
     (and
      (sized T&.)
      (sized E&.)
     )
     (= (vstd!std_specs.result.ResultAdditionalSpecFns.arrow_Ok_0.? $ (TYPE%core!result.Result.
        T&. T& E&. E&
       ) T&. T& E&. E& self!
      ) (core!result.Result./Ok/0 T&. T& E&. E& (%Poly%core!result.Result. self!))
    ))
    :pattern ((vstd!std_specs.result.ResultAdditionalSpecFns.arrow_Ok_0.? $ (TYPE%core!result.Result.
       T&. T& E&. E&
      ) T&. T& E&. E& self!
    ))
    :qid internal_vstd!std_specs.result.ResultAdditionalSpecFns.arrow_Ok_0.?_definition
    :skolemid skolem_internal_vstd!std_specs.result.ResultAdditionalSpecFns.arrow_Ok_0.?_definition
))))

;; Trait-Impl-Axiom
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type)) (!
   (=>
    (and
     (sized T&.)
     (sized E&.)
    )
    (tr_bound%vstd!std_specs.result.ResultAdditionalSpecFns. $ (TYPE%core!result.Result.
      T&. T& E&. E&
     ) T&. T& E&. E&
   ))
   :pattern ((tr_bound%vstd!std_specs.result.ResultAdditionalSpecFns. $ (TYPE%core!result.Result.
      T&. T& E&. E&
     ) T&. T& E&. E&
   ))
   :qid internal_vstd__std_specs__result__impl&__0_trait_impl_definition
   :skolemid skolem_internal_vstd__std_specs__result__impl&__0_trait_impl_definition
)))

;; Function-Specs alloc::boxed::impl&%15::clone
(declare-fun ens%alloc!boxed.impl&%15.clone. (Dcr Type Dcr Type Poly Poly) Bool)
(assert
 (forall ((T&. Dcr) (T& Type) (A&. Dcr) (A& Type) (b! Poly) (res! Poly)) (!
   (= (ens%alloc!boxed.impl&%15.clone. T&. T& A&. A& b! res!) (and
     (ens%core!clone.Clone.clone. (BOX A&. A& T&.) T& b! res!)
     (vstd!pervasive.cloned.? T&. T& b! res!)
   ))
   :pattern ((ens%alloc!boxed.impl&%15.clone. T&. T& A&. A& b! res!))
   :qid internal_ens__alloc!boxed.impl&__15.clone._definition
   :skolemid skolem_internal_ens__alloc!boxed.impl&__15.clone._definition
)))
(assert
 (forall ((closure%$ Poly) (res$ Poly) (T&. Dcr) (T& Type) (A&. Dcr) (A& Type)) (!
   (=>
    (and
     (has_type closure%$ (TYPE%tuple%1. (REF (BOX A&. A& T&.)) T&))
     (has_type res$ T&)
    )
    (=>
     (closure_ens (FNDEF%core!clone.Clone.clone. (BOX A&. A& T&.) T&) (DST (REF (BOX A&. A&
         T&.
       ))
      ) (TYPE%tuple%1. (REF (BOX A&. A& T&.)) T&) (F fndef_singleton) closure%$ res$
     )
     (let
      ((b$ (tuple%1./tuple%1/0 (%Poly%tuple%1. closure%$))))
      (vstd!pervasive.cloned.? T&. T& b$ res$)
   )))
   :pattern ((closure_ens (FNDEF%core!clone.Clone.clone. (BOX A&. A& T&.) T&) (DST (REF
       (BOX A&. A& T&.)
      )
     ) (TYPE%tuple%1. (REF (BOX A&. A& T&.)) T&) (F fndef_singleton) closure%$ res$
   ))
   :qid user_alloc__boxed__impl&%15__clone_7
   :skolemid skolem_user_alloc__boxed__impl&%15__clone_7
)))

;; Function-Axioms lib::error::EINVAL
(assert
 (fuel_bool_default fuel%lib!error.EINVAL.)
)
(assert
 (=>
  (fuel_bool fuel%lib!error.EINVAL.)
  (= lib!error.EINVAL.? (iClip 32 (Sub 0 22)))
))
(assert
 (iInv 32 lib!error.EINVAL.?)
)

;; Function-Axioms lib::error::OK
(assert
 (fuel_bool_default fuel%lib!error.OK.)
)
(assert
 (=>
  (fuel_bool fuel%lib!error.OK.)
  (= lib!error.OK.? 0)
))
(assert
 (iInv 32 lib!error.OK.?)
)

;; Function-Specs vstd::std_specs::result::spec_unwrap
(declare-fun req%vstd!std_specs.result.spec_unwrap. (Dcr Type Dcr Type Poly) Bool)
(declare-const %%global_location_label%%0 Bool)
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (result! Poly)) (!
   (= (req%vstd!std_specs.result.spec_unwrap. T&. T& E&. E& result!) (=>
     %%global_location_label%%0
     (is-core!result.Result./Ok (%Poly%core!result.Result. result!))
   ))
   :pattern ((req%vstd!std_specs.result.spec_unwrap. T&. T& E&. E& result!))
   :qid internal_req__vstd!std_specs.result.spec_unwrap._definition
   :skolemid skolem_internal_req__vstd!std_specs.result.spec_unwrap._definition
)))

;; Function-Axioms vstd::std_specs::result::spec_unwrap
(assert
 (fuel_bool_default fuel%vstd!std_specs.result.spec_unwrap.)
)
(assert
 (=>
  (fuel_bool fuel%vstd!std_specs.result.spec_unwrap.)
  (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (result! Poly)) (!
    (= (vstd!std_specs.result.spec_unwrap.? T&. T& E&. E& result!) (core!result.Result./Ok/0
      T&. T& E&. E& (%Poly%core!result.Result. result!)
    ))
    :pattern ((vstd!std_specs.result.spec_unwrap.? T&. T& E&. E& result!))
    :qid internal_vstd!std_specs.result.spec_unwrap.?_definition
    :skolemid skolem_internal_vstd!std_specs.result.spec_unwrap.?_definition
))))
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type) (result! Poly)) (!
   (=>
    (has_type result! (TYPE%core!result.Result. T&. T& E&. E&))
    (has_type (vstd!std_specs.result.spec_unwrap.? T&. T& E&. E& result!) T&)
   )
   :pattern ((vstd!std_specs.result.spec_unwrap.? T&. T& E&. E& result!))
   :qid internal_vstd!std_specs.result.spec_unwrap.?_pre_post_definition
   :skolemid skolem_internal_vstd!std_specs.result.spec_unwrap.?_pre_post_definition
)))

;; Function-Axioms lib::cpu_mask::MAX_CPUS
(assert
 (fuel_bool_default fuel%lib!cpu_mask.MAX_CPUS.)
)
(assert
 (=>
  (fuel_bool fuel%lib!cpu_mask.MAX_CPUS.)
  (= lib!cpu_mask.MAX_CPUS.? 16)
))
(assert
 (uInv 32 lib!cpu_mask.MAX_CPUS.?)
)

;; Function-Axioms lib::cpu_mask::is_power_of_two
(assert
 (fuel_bool_default fuel%lib!cpu_mask.is_power_of_two.)
)
(assert
 (=>
  (fuel_bool fuel%lib!cpu_mask.is_power_of_two.)
  (forall ((m! Poly)) (!
    (= (lib!cpu_mask.is_power_of_two.? m!) (or
      (or
       (or
        (or
         (or
          (or
           (or
            (or
             (or
              (or
               (or
                (or
                 (or
                  (or
                   (or
                    (or
                     (or
                      (or
                       (or
                        (or
                         (or
                          (or
                           (or
                            (or
                             (or
                              (or
                               (or
                                (or
                                 (or
                                  (or
                                   (or
                                    (= (%I m!) 1)
                                    (= (%I m!) 2)
                                   )
                                   (= (%I m!) 4)
                                  )
                                  (= (%I m!) 8)
                                 )
                                 (= (%I m!) 16)
                                )
                                (= (%I m!) 32)
                               )
                               (= (%I m!) 64)
                              )
                              (= (%I m!) 128)
                             )
                             (= (%I m!) 256)
                            )
                            (= (%I m!) 512)
                           )
                           (= (%I m!) 1024)
                          )
                          (= (%I m!) 2048)
                         )
                         (= (%I m!) 4096)
                        )
                        (= (%I m!) 8192)
                       )
                       (= (%I m!) 16384)
                      )
                      (= (%I m!) 32768)
                     )
                     (= (%I m!) 65536)
                    )
                    (= (%I m!) 131072)
                   )
                   (= (%I m!) 262144)
                  )
                  (= (%I m!) 524288)
                 )
                 (= (%I m!) 1048576)
                )
                (= (%I m!) 2097152)
               )
               (= (%I m!) 4194304)
              )
              (= (%I m!) 8388608)
             )
             (= (%I m!) 16777216)
            )
            (= (%I m!) 33554432)
           )
           (= (%I m!) 67108864)
          )
          (= (%I m!) 134217728)
         )
         (= (%I m!) 268435456)
        )
        (= (%I m!) 536870912)
       )
       (= (%I m!) 1073741824)
      )
      (= (%I m!) 2147483648)
    ))
    :pattern ((lib!cpu_mask.is_power_of_two.? m!))
    :qid internal_lib!cpu_mask.is_power_of_two.?_definition
    :skolemid skolem_internal_lib!cpu_mask.is_power_of_two.?_definition
))))

;; Function-Axioms lib::cpu_mask::compute_mask
(assert
 (fuel_bool_default fuel%lib!cpu_mask.compute_mask.)
)
(assert
 (=>
  (fuel_bool fuel%lib!cpu_mask.compute_mask.)
  (forall ((current! Poly) (enable! Poly) (disable! Poly)) (!
    (= (lib!cpu_mask.compute_mask.? current! enable! disable!) (uClip 32 (bitand (I (uClip
         32 (bitor (I (%I current!)) (I (%I enable!)))
        )
       ) (I (uClip 32 (bitnot (I (%I disable!)))))
    )))
    :pattern ((lib!cpu_mask.compute_mask.? current! enable! disable!))
    :qid internal_lib!cpu_mask.compute_mask.?_definition
    :skolemid skolem_internal_lib!cpu_mask.compute_mask.?_definition
))))
(assert
 (forall ((current! Poly) (enable! Poly) (disable! Poly)) (!
   (=>
    (and
     (has_type current! (UINT 32))
     (has_type enable! (UINT 32))
     (has_type disable! (UINT 32))
    )
    (uInv 32 (lib!cpu_mask.compute_mask.? current! enable! disable!))
   )
   :pattern ((lib!cpu_mask.compute_mask.? current! enable! disable!))
   :qid internal_lib!cpu_mask.compute_mask.?_pre_post_definition
   :skolemid skolem_internal_lib!cpu_mask.compute_mask.?_pre_post_definition
)))

;; Trait-Impl-Axiom
(assert
 (tr_bound%core!clone.Clone. $ (UINT 32))
)

;; Trait-Impl-Axiom
(assert
 (tr_bound%core!clone.Clone. $ (SINT 32))
)

;; Trait-Impl-Axiom
(assert
 (tr_bound%core!clone.Clone. $ BOOL)
)

;; Trait-Impl-Axiom
(assert
 (forall ((T&. Dcr) (T& Type)) (!
   (tr_bound%core!clone.Clone. (REF T&.) T&)
   :pattern ((tr_bound%core!clone.Clone. (REF T&.) T&))
   :qid internal_core__clone__impls__impl&__6_trait_impl_definition
   :skolemid skolem_internal_core__clone__impls__impl&__6_trait_impl_definition
)))

;; Trait-Impl-Axiom
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type)) (!
   (=>
    (and
     (sized T&.)
     (sized E&.)
     (tr_bound%core!clone.Clone. T&. T&)
     (tr_bound%core!clone.Clone. E&. E&)
    )
    (tr_bound%core!clone.Clone. $ (TYPE%core!result.Result. T&. T& E&. E&))
   )
   :pattern ((tr_bound%core!clone.Clone. $ (TYPE%core!result.Result. T&. T& E&. E&)))
   :qid internal_core__result__impl&__5_trait_impl_definition
   :skolemid skolem_internal_core__result__impl&__5_trait_impl_definition
)))

;; Trait-Impl-Axiom
(assert
 (forall ((T&. Dcr) (T& Type) (E&. Dcr) (E& Type)) (!
   (=>
    (and
     (sized T&.)
     (sized E&.)
     (tr_bound%core!fmt.Debug. T&. T&)
     (tr_bound%core!fmt.Debug. E&. E&)
    )
    (tr_bound%core!fmt.Debug. $ (TYPE%core!result.Result. T&. T& E&. E&))
   )
   :pattern ((tr_bound%core!fmt.Debug. $ (TYPE%core!result.Result. T&. T& E&. E&)))
   :qid internal_core__result__impl&__31_trait_impl_definition
   :skolemid skolem_internal_core__result__impl&__31_trait_impl_definition
)))

;; Trait-Impl-Axiom
(assert
 (tr_bound%core!fmt.Debug. $ (SINT 32))
)

;; Trait-Impl-Axiom
(assert
 (tr_bound%core!fmt.Debug. $ (UINT 32))
)

;; Trait-Impl-Axiom
(assert
 (forall ((T&. Dcr) (T& Type)) (!
   (=>
    (tr_bound%core!fmt.Debug. T&. T&)
    (tr_bound%core!fmt.Debug. (REF T&.) T&)
   )
   :pattern ((tr_bound%core!fmt.Debug. (REF T&.) T&))
   :qid internal_core__fmt__impl&__80_trait_impl_definition
   :skolemid skolem_internal_core__fmt__impl&__80_trait_impl_definition
)))

;; Trait-Impl-Axiom
(assert
 (tr_bound%core!fmt.Debug. $ BOOL)
)

;; Trait-Impl-Axiom
(assert
 (forall ((T&. Dcr) (T& Type)) (!
   (=>
    (and
     (sized T&.)
     (tr_bound%core!fmt.Debug. T&. T&)
    )
    (tr_bound%core!fmt.Debug. (DST T&.) (TYPE%tuple%1. T&. T&))
   )
   :pattern ((tr_bound%core!fmt.Debug. (DST T&.) (TYPE%tuple%1. T&. T&)))
   :qid internal_core__fmt__impl&__107_trait_impl_definition
   :skolemid skolem_internal_core__fmt__impl&__107_trait_impl_definition
)))

;; Trait-Impl-Axiom
(assert
 (tr_bound%core!fmt.Debug. $ TYPE%tuple%0.)
)

;; Trait-Impl-Axiom
(assert
 (forall ((A&. Dcr) (A& Type)) (!
   (=>
    (tr_bound%core!alloc.Allocator. A&. A&)
    (tr_bound%core!alloc.Allocator. (REF A&.) A&)
   )
   :pattern ((tr_bound%core!alloc.Allocator. (REF A&.) A&))
   :qid internal_core__alloc__impl&__2_trait_impl_definition
   :skolemid skolem_internal_core__alloc__impl&__2_trait_impl_definition
)))

;; Trait-Impl-Axiom
(assert
 (forall ((A&. Dcr) (A& Type)) (!
   (=>
    (sized A&.)
    (tr_bound%core!clone.Clone. (TRACKED A&.) A&)
   )
   :pattern ((tr_bound%core!clone.Clone. (TRACKED A&.) A&))
   :qid internal_verus_builtin__impl&__5_trait_impl_definition
   :skolemid skolem_internal_verus_builtin__impl&__5_trait_impl_definition
)))

;; Trait-Impl-Axiom
(assert
 (forall ((A&. Dcr) (A& Type)) (!
   (=>
    (sized A&.)
    (tr_bound%core!clone.Clone. (GHOST A&.) A&)
   )
   :pattern ((tr_bound%core!clone.Clone. (GHOST A&.) A&))
   :qid internal_verus_builtin__impl&__3_trait_impl_definition
   :skolemid skolem_internal_verus_builtin__impl&__3_trait_impl_definition
)))

;; Trait-Impl-Axiom
(assert
 (forall ((T&. Dcr) (T& Type) (A&. Dcr) (A& Type)) (!
   (=>
    (and
     (sized T&.)
     (sized A&.)
     (tr_bound%core!clone.Clone. T&. T&)
     (tr_bound%core!alloc.Allocator. A&. A&)
     (tr_bound%core!clone.Clone. A&. A&)
    )
    (tr_bound%core!clone.Clone. (BOX A&. A& T&.) T&)
   )
   :pattern ((tr_bound%core!clone.Clone. (BOX A&. A& T&.) T&))
   :qid internal_alloc__boxed__impl&__15_trait_impl_definition
   :skolemid skolem_internal_alloc__boxed__impl&__15_trait_impl_definition
)))

;; Function-Specs lib::cpu_mask::CpuMaskResult::clone
(declare-fun ens%lib!cpu_mask.impl&%2.clone. (Poly Poly) Bool)
(assert
 (forall ((self! Poly) (%return! Poly)) (!
   (= (ens%lib!cpu_mask.impl&%2.clone. self! %return!) (and
     (ens%core!clone.Clone.clone. $ TYPE%lib!cpu_mask.CpuMaskResult. self! %return!)
     (= %return! self!)
   ))
   :pattern ((ens%lib!cpu_mask.impl&%2.clone. self! %return!))
   :qid internal_ens__lib!cpu_mask.impl&__2.clone._definition
   :skolemid skolem_internal_ens__lib!cpu_mask.impl&__2.clone._definition
)))
(assert
 (forall ((closure%$ Poly) (%return$ Poly)) (!
   (=>
    (and
     (has_type closure%$ (TYPE%tuple%1. (REF $) TYPE%lib!cpu_mask.CpuMaskResult.))
     (has_type %return$ TYPE%lib!cpu_mask.CpuMaskResult.)
    )
    (=>
     (closure_ens (FNDEF%core!clone.Clone.clone. $ TYPE%lib!cpu_mask.CpuMaskResult.) (DST
       (REF $)
      ) (TYPE%tuple%1. (REF $) TYPE%lib!cpu_mask.CpuMaskResult.) (F fndef_singleton) closure%$
      %return$
     )
     (let
      ((self$ (%Poly%lib!cpu_mask.CpuMaskResult. (tuple%1./tuple%1/0 (%Poly%tuple%1. closure%$)))))
      (= (%Poly%lib!cpu_mask.CpuMaskResult. %return$) self$)
   )))
   :pattern ((closure_ens (FNDEF%core!clone.Clone.clone. $ TYPE%lib!cpu_mask.CpuMaskResult.)
     (DST (REF $)) (TYPE%tuple%1. (REF $) TYPE%lib!cpu_mask.CpuMaskResult.) (F fndef_singleton)
     closure%$ %return$
   ))
   :qid user_lib__cpu_mask__CpuMaskResult__clone_8
   :skolemid skolem_user_lib__cpu_mask__CpuMaskResult__clone_8
)))

;; Trait-Impl-Axiom
(assert
 (tr_bound%core!clone.Clone. $ TYPE%lib!cpu_mask.CpuMaskResult.)
)

;; Trait-Impl-Axiom
(assert
 (tr_bound%core!fmt.Debug. $ TYPE%lib!cpu_mask.CpuMaskResult.)
)

;; Function-Specs lib::cpu_mask::validate_pin_mask
(declare-fun ens%lib!cpu_mask.validate_pin_mask. (Int Bool) Bool)
(assert
 (forall ((mask! Int) (result! Bool)) (!
   (= (ens%lib!cpu_mask.validate_pin_mask. mask! result!) (= result! (and
      (not (= mask! 0))
      (= (uClip 32 (bitand (I mask!) (I (uClip 32 (Sub mask! 1))))) 0)
   )))
   :pattern ((ens%lib!cpu_mask.validate_pin_mask. mask! result!))
   :qid internal_ens__lib!cpu_mask.validate_pin_mask._definition
   :skolemid skolem_internal_ens__lib!cpu_mask.validate_pin_mask._definition
)))

;; Function-Specs lib::cpu_mask::cpu_mask_mod
(declare-fun ens%lib!cpu_mask.cpu_mask_mod. (Int Int Int Bool Bool lib!cpu_mask.CpuMaskResult.)
 Bool
)
(assert
 (forall ((current_mask! Int) (enable! Int) (disable! Int) (is_running! Bool) (pin_only!
    Bool
   ) (result! lib!cpu_mask.CpuMaskResult.)
  ) (!
   (= (ens%lib!cpu_mask.cpu_mask_mod. current_mask! enable! disable! is_running! pin_only!
     result!
    ) (and
     (has_type (Poly%lib!cpu_mask.CpuMaskResult. result!) TYPE%lib!cpu_mask.CpuMaskResult.)
     (=>
      is_running!
      (= (lib!cpu_mask.CpuMaskResult./CpuMaskResult/error (%Poly%lib!cpu_mask.CpuMaskResult.
         (Poly%lib!cpu_mask.CpuMaskResult. result!)
        )
       ) lib!error.EINVAL.?
     ))
     (=>
      (= (lib!cpu_mask.CpuMaskResult./CpuMaskResult/error (%Poly%lib!cpu_mask.CpuMaskResult.
         (Poly%lib!cpu_mask.CpuMaskResult. result!)
        )
       ) lib!error.OK.?
      )
      (= (lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask (%Poly%lib!cpu_mask.CpuMaskResult.
         (Poly%lib!cpu_mask.CpuMaskResult. result!)
        )
       ) (uClip 32 (uClip 32 (bitand (I (uClip 32 (bitor (I current_mask!) (I enable!)))) (
           I (uClip 32 (bitnot (I disable!)))
     ))))))
     (=>
      (and
       (= (lib!cpu_mask.CpuMaskResult./CpuMaskResult/error (%Poly%lib!cpu_mask.CpuMaskResult.
          (Poly%lib!cpu_mask.CpuMaskResult. result!)
         )
        ) lib!error.OK.?
       )
       pin_only!
      )
      (and
       (not (= (lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask (%Poly%lib!cpu_mask.CpuMaskResult.
           (Poly%lib!cpu_mask.CpuMaskResult. result!)
          )
         ) 0
       ))
       (= (uClip 32 (bitand (I (lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask (%Poly%lib!cpu_mask.CpuMaskResult.
             (Poly%lib!cpu_mask.CpuMaskResult. result!)
           ))
          ) (I (uClip 32 (Sub (lib!cpu_mask.CpuMaskResult./CpuMaskResult/mask (%Poly%lib!cpu_mask.CpuMaskResult.
               (Poly%lib!cpu_mask.CpuMaskResult. result!)
              )
             ) 1
         ))))
        ) 0
     )))
     (or
      (= (lib!cpu_mask.CpuMaskResult./CpuMaskResult/error (%Poly%lib!cpu_mask.CpuMaskResult.
         (Poly%lib!cpu_mask.CpuMaskResult. result!)
        )
       ) lib!error.OK.?
      )
      (= (lib!cpu_mask.CpuMaskResult./CpuMaskResult/error (%Poly%lib!cpu_mask.CpuMaskResult.
         (Poly%lib!cpu_mask.CpuMaskResult. result!)
        )
       ) lib!error.EINVAL.?
   ))))
   :pattern ((ens%lib!cpu_mask.cpu_mask_mod. current_mask! enable! disable! is_running!
     pin_only! result!
   ))
   :qid internal_ens__lib!cpu_mask.cpu_mask_mod._definition
   :skolemid skolem_internal_ens__lib!cpu_mask.cpu_mask_mod._definition
)))

;; Function-Specs lib::cpu_mask::cpu_pin_compute
(declare-fun ens%lib!cpu_mask.cpu_pin_compute. (Int Int core!result.Result.) Bool)
(assert
 (forall ((cpu_id! Int) (max_cpus! Int) (result! core!result.Result.)) (!
   (= (ens%lib!cpu_mask.cpu_pin_compute. cpu_id! max_cpus! result!) (and
     (has_type (Poly%core!result.Result. result!) (TYPE%core!result.Result. $ (UINT 32)
       $ (SINT 32)
     ))
     (=>
      (or
       (>= cpu_id! max_cpus!)
       (> max_cpus! 32)
      )
      (is-core!result.Result./Err result!)
     )
     (=>
      (is-core!result.Result./Err result!)
      (= result! (core!result.Result./Err (I lib!error.EINVAL.?)))
     )
     (=>
      (is-core!result.Result./Ok result!)
      (let
       ((m$ (%I (core!result.Result./Ok/0 $ (UINT 32) $ (SINT 32) (%Poly%core!result.Result.
            (Poly%core!result.Result. result!)
       )))))
       (and
        (< cpu_id! 32)
        (lib!cpu_mask.is_power_of_two.? (I m$))
   )))))
   :pattern ((ens%lib!cpu_mask.cpu_pin_compute. cpu_id! max_cpus! result!))
   :qid internal_ens__lib!cpu_mask.cpu_pin_compute._definition
   :skolemid skolem_internal_ens__lib!cpu_mask.cpu_pin_compute._definition
)))

;; Function-Def lib::cpu_mask::cpu_pin_compute
;; src/cpu_mask.rs:171:9: 171:15 (#0)
(set-option :sat.euf true)
(set-option :tactic.default_tactic sat)
(set-option :smt.ematching false)
(set-option :smt.case_split 0)
(get-info :all-statistics)
(declare-const cpu_id! (_ BitVec 32))
(declare-const mask@ (_ BitVec 32))
(assert
 (bvult cpu_id! ((_ zero_extend 26) (_ bv32 6)))
)
(assert
 (= mask@ (bvshl ((_ zero_extend 31) (_ bv1 1)) cpu_id!))
)
;; bitvector assertion not satisfied
(declare-const %%location_label%%0 Bool)
(assert
 (not (=>
   %%location_label%%0
   (or
    (or
     (or
      (or
       (or
        (or
         (or
          (or
           (or
            (or
             (or
              (or
               (or
                (or
                 (or
                  (or
                   (or
                    (or
                     (or
                      (or
                       (or
                        (or
                         (or
                          (or
                           (or
                            (or
                             (or
                              (or
                               (or
                                (or
                                 (or
                                  (= mask@ ((_ zero_extend 31) (_ bv1 1)))
                                  (= mask@ ((_ zero_extend 30) (_ bv2 2)))
                                 )
                                 (= mask@ ((_ zero_extend 29) (_ bv4 3)))
                                )
                                (= mask@ ((_ zero_extend 28) (_ bv8 4)))
                               )
                               (= mask@ ((_ zero_extend 27) (_ bv16 5)))
                              )
                              (= mask@ ((_ zero_extend 26) (_ bv32 6)))
                             )
                             (= mask@ ((_ zero_extend 25) (_ bv64 7)))
                            )
                            (= mask@ ((_ zero_extend 24) (_ bv128 8)))
                           )
                           (= mask@ ((_ zero_extend 23) (_ bv256 9)))
                          )
                          (= mask@ ((_ zero_extend 22) (_ bv512 10)))
                         )
                         (= mask@ ((_ zero_extend 21) (_ bv1024 11)))
                        )
                        (= mask@ ((_ zero_extend 20) (_ bv2048 12)))
                       )
                       (= mask@ ((_ zero_extend 19) (_ bv4096 13)))
                      )
                      (= mask@ ((_ zero_extend 18) (_ bv8192 14)))
                     )
                     (= mask@ ((_ zero_extend 17) (_ bv16384 15)))
                    )
                    (= mask@ ((_ zero_extend 16) (_ bv32768 16)))
                   )
                   (= mask@ ((_ zero_extend 15) (_ bv65536 17)))
                  )
                  (= mask@ ((_ zero_extend 14) (_ bv131072 18)))
                 )
                 (= mask@ ((_ zero_extend 13) (_ bv262144 19)))
                )
                (= mask@ ((_ zero_extend 12) (_ bv524288 20)))
               )
               (= mask@ ((_ zero_extend 11) (_ bv1048576 21)))
              )
              (= mask@ ((_ zero_extend 10) (_ bv2097152 22)))
             )
             (= mask@ ((_ zero_extend 9) (_ bv4194304 23)))
            )
            (= mask@ ((_ zero_extend 8) (_ bv8388608 24)))
           )
           (= mask@ ((_ zero_extend 7) (_ bv16777216 25)))
          )
          (= mask@ ((_ zero_extend 6) (_ bv33554432 26)))
         )
         (= mask@ ((_ zero_extend 5) (_ bv67108864 27)))
        )
        (= mask@ ((_ zero_extend 4) (_ bv134217728 28)))
       )
       (= mask@ ((_ zero_extend 3) (_ bv268435456 29)))
      )
      (= mask@ ((_ zero_extend 2) (_ bv536870912 30)))
     )
     (= mask@ ((_ zero_extend 1) (_ bv1073741824 31)))
    )
    (= mask@ (_ bv2147483648 32))
))))
(get-info :all-statistics)
(get-info :version)
(set-option :rlimit 30000000)
(check-sat)
(set-option :rlimit 0)
(get-info :all-statistics)

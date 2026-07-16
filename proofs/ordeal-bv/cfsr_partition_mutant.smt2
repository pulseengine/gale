; gale#173 pilot 4 discrimination sanity — the encoding is NOT vacuously unsat.
; Break UFSR by dropping bit 31 (0xFFFF0000 -> 0x7FFF0000). The masks no longer COVER all 32
; bits, so the lossless-partition claim must FAIL. Refute the same negation with the broken
; mask over a free cfsr: ordeal must return SAT with a model in which bit 31 of cfsr is set
; (that bit is now covered by no slice, so m|b|u != cfsr). A broken/vacuous checker that
; returned unsat here would be caught.
(set-logic QF_BV)
(declare-const cfsr (_ BitVec 32))
(assert (or
  (not (= (bvand (bvand cfsr #x000000ff) (bvand cfsr #x0000ff00)) #x00000000))
  (not (= (bvand (bvand cfsr #x000000ff) (bvand cfsr #x7fff0000)) #x00000000))
  (not (= (bvand (bvand cfsr #x0000ff00) (bvand cfsr #x7fff0000)) #x00000000))
  (not (= (bvor (bvor (bvand cfsr #x000000ff) (bvand cfsr #x0000ff00)) (bvand cfsr #x7fff0000)) cfsr))))
(check-sat)

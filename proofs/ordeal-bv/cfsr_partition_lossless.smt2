; gale#173 pilot 4 (parametric) — the operational strengthening of lemma_cfsr_masks_partition.
; For ANY fault-status word cfsr, the three masked byte-slices are pairwise disjoint AND
; reassemble losslessly to cfsr — the decode loses no bit of any fault word and never
; attributes one bit to two sub-registers. This universally-quantified form IMPLIES the four
; constant conjuncts of fault_decode.rs:663-666 (take cfsr = 0xFFFFFFFF).
;   m = cfsr & 0x000000FF   b = cfsr & 0x0000FF00   u = cfsr & 0xFFFF0000
; Refute the negation of (disjoint x3 AND m|b|u == cfsr) over a free cfsr.  UNSAT => holds.
(set-logic QF_BV)
(declare-const cfsr (_ BitVec 32))
(assert (or
  (not (= (bvand (bvand cfsr #x000000ff) (bvand cfsr #x0000ff00)) #x00000000))
  (not (= (bvand (bvand cfsr #x000000ff) (bvand cfsr #xffff0000)) #x00000000))
  (not (= (bvand (bvand cfsr #x0000ff00) (bvand cfsr #xffff0000)) #x00000000))
  (not (= (bvor (bvor (bvand cfsr #x000000ff) (bvand cfsr #x0000ff00)) (bvand cfsr #xffff0000)) cfsr))))
(check-sat)

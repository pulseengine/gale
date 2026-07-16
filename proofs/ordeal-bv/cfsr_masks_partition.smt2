; gale#173 pilot 4 — fault_decode.rs:663-666 lemma_cfsr_masks_partition, re-discharged via ordeal (QF_BV, LRAT-certified)
; Lemma: the CFSR sub-register masks MMFSR/BFSR/UFSR are pairwise NON-OVERLAPPING and
;        together COVER all 32 bits — i.e. the Cortex-M fault-status decode partitions CFSR.
;   MMFSR_MASK = 0x000000FF  (bits 0-7,  MemManage)
;   BFSR_MASK  = 0x0000FF00  (bits 8-15, BusFault)
;   UFSR_MASK  = 0xFFFF0000  (bits 16-31, UsageFault)
; Premises pin the three masks to their source constants (separate asserts = conjunction).
; The final assert is the NEGATION of the 4-conjunct conclusion; UNSAT => the lemma holds.
(set-logic QF_BV)
(declare-const mmfsr (_ BitVec 32))
(declare-const bfsr (_ BitVec 32))
(declare-const ufsr (_ BitVec 32))
(assert (= mmfsr #x000000ff))
(assert (= bfsr  #x0000ff00))
(assert (= ufsr  #xffff0000))
(assert (or
  (not (= (bvand mmfsr bfsr) #x00000000))
  (not (= (bvand mmfsr ufsr) #x00000000))
  (not (= (bvand bfsr  ufsr) #x00000000))
  (not (= (bvor (bvor mmfsr bfsr) ufsr) #xffffffff))))
(check-sat)

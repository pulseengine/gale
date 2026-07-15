; DISCRIMINATION SANITY (must be SAT) — SV4 with the thread-alignment premise REMOVED.
; An unaligned thread's low bits corrupt owner&3, so owner&3 != cpu becomes satisfiable (e.g. thread=1, cpu=0).
; ordeal must return sat + model; a vacuous checker returning unsat would be caught -> proves the alignment premise is load-bearing.
(set-logic QF_BV)
(declare-const cpu (_ BitVec 32))
(declare-const thread (_ BitVec 32))
(assert (bvult cpu (_ bv4 32)))
(assert (not (= thread (_ bv0 32))))
(assert (not (= (bvand (bvor thread cpu) (_ bv3 32)) cpu)))
(check-sat)

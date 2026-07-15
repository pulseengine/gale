; gale#173 pilot 3 — src/spinlock_validate.rs SV4 (CPU recoverable) by(bit_vector) obligation, re-discharged via ordeal (QF_BV, LRAT).
; owner = thread | cpu ; decode_cpu(owner) = owner & 3. Given cpu<MAX_CPUS(4) and thread aligned (thread&3==0, thread!=0),
; prove owner&3 == cpu. Refute (premises AND owner&3 != cpu) via implicitly-conjoined asserts. UNSAT => holds for all u32.
(set-logic QF_BV)
(declare-const cpu (_ BitVec 32))
(declare-const thread (_ BitVec 32))
(assert (bvult cpu (_ bv4 32)))
(assert (not (= thread (_ bv0 32))))
(assert (= (bvand thread (_ bv3 32)) (_ bv0 32)))
(assert (not (= (bvand (bvor thread cpu) (_ bv3 32)) cpu)))
(check-sat)

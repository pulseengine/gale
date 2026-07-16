; gale#173 pilot 3 — src/spinlock_validate.rs SV5 (thread recoverable) by(bit_vector) obligation, re-discharged via ordeal (QF_BV, LRAT).
; decode_thread(owner) = owner & ~3 (0xFFFFFFFC). Same premises; prove owner & 0xFFFFFFFC == thread. UNSAT => holds for all u32.
(set-logic QF_BV)
(declare-const cpu (_ BitVec 32))
(declare-const thread (_ BitVec 32))
(assert (bvult cpu (_ bv4 32)))
(assert (not (= thread (_ bv0 32))))
(assert (= (bvand thread (_ bv3 32)) (_ bv0 32)))
(assert (not (= (bvand (bvor thread cpu) (_ bv4294967292 32)) thread)))
(check-sat)

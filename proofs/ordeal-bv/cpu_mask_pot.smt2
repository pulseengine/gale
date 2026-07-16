; gale#173 pilot — cpu_mask.rs by(bit_vector) obligation, re-discharged via ordeal (QF_BV, LRAT-certified)
; Lemma: cpu_id < 32 AND mask == (1 << cpu_id)  =>  mask is one of the 32 powers of two.
; We refute the NEGATION: premises AND NOT(mask is a power of two).  UNSAT => lemma holds.
(set-logic QF_BV)
(declare-const cpu_id (_ BitVec 32))
(declare-const mask (_ BitVec 32))
(assert (bvult cpu_id (_ bv32 32)))
(assert (= mask (bvshl (_ bv1 32) cpu_id)))
(assert (not (or (= mask (_ bv1 32)) (= mask (_ bv2 32)) (= mask (_ bv4 32)) (= mask (_ bv8 32)) (= mask (_ bv16 32)) (= mask (_ bv32 32)) (= mask (_ bv64 32)) (= mask (_ bv128 32)) (= mask (_ bv256 32)) (= mask (_ bv512 32)) (= mask (_ bv1024 32)) (= mask (_ bv2048 32)) (= mask (_ bv4096 32)) (= mask (_ bv8192 32)) (= mask (_ bv16384 32)) (= mask (_ bv32768 32)) (= mask (_ bv65536 32)) (= mask (_ bv131072 32)) (= mask (_ bv262144 32)) (= mask (_ bv524288 32)) (= mask (_ bv1048576 32)) (= mask (_ bv2097152 32)) (= mask (_ bv4194304 32)) (= mask (_ bv8388608 32)) (= mask (_ bv16777216 32)) (= mask (_ bv33554432 32)) (= mask (_ bv67108864 32)) (= mask (_ bv134217728 32)) (= mask (_ bv268435456 32)) (= mask (_ bv536870912 32)) (= mask (_ bv1073741824 32)) (= mask (_ bv2147483648 32)))))
(check-sat)

; DISCRIMINATION SANITY (must be SAT) — forward obligation with the bv2 term REMOVED from the enumeration.
; n=2 satisfies the idiom but is absent from the reduced enum, so (idiom AND not-enum) is SAT at n=2.
; ordeal must return sat + model n=2; a vacuous checker returning unsat would be caught.
(set-logic QF_BV)
(declare-const n (_ BitVec 32))
(assert (bvugt n (_ bv0 32)))
(assert (= (bvand n (bvsub n (_ bv1 32))) (_ bv0 32)))
(assert (not (or (= n (_ bv1 32)) (= n (_ bv4 32)) (= n (_ bv8 32)) (= n (_ bv16 32)) (= n (_ bv32 32)) (= n (_ bv64 32)) (= n (_ bv128 32)) (= n (_ bv256 32)) (= n (_ bv512 32)) (= n (_ bv1024 32)) (= n (_ bv2048 32)) (= n (_ bv4096 32)) (= n (_ bv8192 32)) (= n (_ bv16384 32)) (= n (_ bv32768 32)) (= n (_ bv65536 32)) (= n (_ bv131072 32)) (= n (_ bv262144 32)) (= n (_ bv524288 32)) (= n (_ bv1048576 32)) (= n (_ bv2097152 32)) (= n (_ bv4194304 32)) (= n (_ bv8388608 32)) (= n (_ bv16777216 32)) (= n (_ bv33554432 32)) (= n (_ bv67108864 32)) (= n (_ bv134217728 32)) (= n (_ bv268435456 32)) (= n (_ bv536870912 32)) (= n (_ bv1073741824 32)) (= n (_ bv2147483648 32)))))
(check-sat)

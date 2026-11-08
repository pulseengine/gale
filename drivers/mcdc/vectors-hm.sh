# MC/DC vector set for hm-thin — seven pure scalar Health-Monitor predicates,
# zero seams. Every predicate gets both outcomes; the compound ones
# (`plausible`, `vote-ok`) get one row per condition where that condition alone
# flips the result, which is what unique-cause MC/DC asks for.
F='gust:hm/detect@0.1.0#'
A=()
v(){ A+=(--invoke-with-args "${F}$1=$2"); }

# fresh: age <= limit            (1 condition, both outcomes + the boundary)
v fresh '5,10'                  # T
v fresh '15,10'                 # F
v fresh '10,10'                 # T at the boundary

# plausible: lo <= value && value <= hi          (2 conditions)
v plausible '5,0,10'            # T T -> T   base row
v plausible '-5,0,10'           # F   -> F   c0 alone flips it (vs base)
v plausible '15,0,10'           # T F -> F   c1 alone flips it (vs base)
v plausible '0,0,10'            # lower boundary
v plausible '10,0,10'           # upper boundary

# innovation-ok: innov <= k_sigma
v innovation-ok '1,5'           # T
v innovation-ok '9,5'           # F
v innovation-ok '5,5'           # T at the boundary

# budget-ok: used <= budget
v budget-ok '1,5'               # T
v budget-ok '9,5'               # F
v budget-ok '5,5'               # T at the boundary

# deadline-ok: lateness == 0     (the safety-relevant asymmetry: only zero passes)
v deadline-ok '0'               # T
v deadline-ok '7'               # F
v deadline-ok '1'               # F one microsecond late is late

# heartbeat-ok: missed == 0
v heartbeat-ok '0'              # T
v heartbeat-ok '3'              # F
v heartbeat-ok '1'              # F

# vote-ok: |s0-s1|<=tol && |s0-s2|<=tol && |s1-s2|<=tol      (3 conditions)
v vote-ok '10,10,10,5'          # T T T -> T   base row
v vote-ok '10,30,10,5'          # F     -> F   a01 alone
v vote-ok '10,10,40,5'          # T F   -> F   a02 alone
v vote-ok '10,5,15,5'           # T T F -> F   a12 alone — reachable because
                                #              |s1-s2| can reach 2*tol
# both arms of each abs() sign test: positive and negative differences
v vote-ok '10,5,10,5'           # d01>0, d02=0, d12<0
v vote-ok '5,10,5,5'            # d01<0, d02=0, d12>0
v vote-ok '0,0,0,0'             # tol=0, all equal -> T
v vote-ok '0,1,0,0'             # tol=0, one apart -> F

F='gust:switch/fsm@0.1.0#'
A=()
sw(){ A+=(--invoke-with-args "${F}set-window=$1,$2,$3,$4"); }
seal(){ A+=(--invoke-with-args "${F}seal-frame=$1"); }
call(){ A+=(--invoke "${F}$1"); }
cw(){ A+=(--invoke-with-args "${F}current-window=$1"); }
tick(){ A+=(--invoke-with-args "${F}tick=$1"); }

# -- false arms reachable only from the initial Running/zero state
sw 9 0 0 0                 # idx >= MAX_WINDOWS -> false
call mark-saved            # Running: false
call mark-swapped          # Running: false
call mark-resumed          # Running: false

base(){ sw 0 1 0 10; sw 1 2 10 10; sw 2 3 20 10; sw 3 4 30 10; }

# -- MC/DC over MajorFrame::check(): all-true, then exactly one condition false
base; seal 40              # T: all 9 conditions true
base; sw 0 1 1 10; seal 40 # c1 offset[0]==0        -> F
base; sw 0 1 0 0;  seal 40 # c2 budget[0]>0         -> F
base; sw 1 2 10 0; seal 40 # c3 budget[1]>0         -> F
base; sw 2 3 20 0; seal 40 # c4 budget[2]>0         -> F
base; sw 3 4 30 0; seal 40 # c5 budget[3]>0         -> F
base; sw 1 2 11 10; seal 40 # c6 off0+bud0==off1    -> F
base; sw 2 3 21 10; seal 40 # c7 off1+bud1==off2    -> F
base; sw 3 4 31 10; seal 40 # c8 off2+bud2==off3    -> F
base; seal 41              # c9 off3+bud3==frame_len-> F
base; seal 40              # re-seal the good frame: SW = valid, cur=0, Running

call frame-check
cw 5; cw 15; cw 25; cw 35  # all four current_window arms

tick 0                     # Running, off-boundary -> false
tick 9                     # Running, at end-1     -> true  => SaveCtx
call mark-swapped          # SaveCtx (not ProgramRegions) -> false
call mark-resumed          # SaveCtx (not Resume)         -> false
call mark-saved            # SaveCtx -> ProgramRegions    -> true
call mark-saved            # ProgramRegions               -> false
tick 9                     # phase != Running             -> false
call mark-swapped          # ProgramRegions -> Resume     -> true
call mark-resumed          # Resume -> Running, cur=1     -> true

call run-switch            # cur 1 -> 2
call run-switch            # cur 2 -> 3
call run-switch            # cur 3 -> 0  (the wrap arm)

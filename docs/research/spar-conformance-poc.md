# SPAR conformance oracle — proof of concept

Status: prototype, v1.  
Owners: gale + spar (pulseengine).  
Touches: `safety/aadl/`, `benches/engine_control/`.

## What this bridges

Three artifacts that have lived in parallel but not yet spoken to each
other:

1. **Runtime evidence** — the `engine_control` bench emits an ISR-paced
   event stream (`E,<seq>,<step>,<rpm>,<algo_cycles>,<handoff_cycles>`;
   see `benches/engine_control/README.md`). The `handoff_cycles`
   segment brackets `ring_buf_put + k_sem_give`, where `k_sem_give`
   calls gale's verified `give_decide` through the FFI.
2. **Formal source of truth** — Verus `give_decide` in `src/sem.rs`,
   `requires count <= limit` and three `ensures` clauses that
   encode the WakeThread / Increment / Saturated trichotomy.
3. **Architectural spec** — an AADL v2.3 model in
   `safety/aadl/semaphore.aadl`, in the dialect SPAR understands.

The oracle (`benches/engine_control/spar_oracle.py`) is the glue: it
parses the AADL model, parses the event stream, and checks three
properties the bench is supposed to uphold at run time.

## Architecture

```
src/sem.rs                 Verus spec (give_decide contract)
     |
     v  (exercised by FFI, observed by bench ISR)
benches/engine_control     -> events.csv  (runtime evidence)
                                |
                                v
                         spar_oracle.py <-- safety/aadl/semaphore.aadl
                                |                 ^
                                v                 |
                           pass/fail .md    spar parse/items
```

The AADL model is the shared ontology. Verus proves the C→Rust path
upholds the decision contract; the AADL model pins down the
*architectural envelope* (WCET, end-to-end latency, EMV2 error
states); the oracle closes the loop by confronting runtime observations
with the envelope.

## What the AADL model declares

Lives at `safety/aadl/semaphore.aadl`. Uses the SPAR-accepted subset:

- `data Sem` + `Sem.binary` / `Sem.counting` implementations, with
  user properties `Count_Max`, `Count_Current`,
  `Count_Lower_Bound`, `Count_Upper_Bound` from a local
  `property set Gale_Sem_Properties`.
- `subprogram Give_Decide` with
  `Compute_Execution_Time => 80 ns .. 6500 ns;` — the oracle's WCET
  envelope.
- `thread Sem_Caller` / `thread Sem_Reader` with sporadic dispatch,
  period, deadline, and flow specifications.
- `system Handoff_System.impl` with the end-to-end flow
  `isr_to_handoff` and `latency => 100 ns .. 120 us applies to
  isr_to_handoff;`.
- `annex EMV2 {** ... **};` declaring the three `GiveDecision`
  states (WakeThread initial, Increment, Saturated) and the transition
  into Saturated, plus an error type hierarchy that marks Saturated +
  CountOverflow as the error set. SPAR parses this as opaque annex
  text today (it accepts the surrounding structure but does not yet
  run EMV2 analyses on it).

Verified with:

    spar parse safety/aadl/semaphore.aadl   # OK
    spar items safety/aadl/semaphore.aadl   # lists all declarations

Analyses that would run once a companion platform file exists
(out-of-scope for this PoC): `spar analyze` for latency / budgets /
flow reachability, `spar modes` for mode reachability, EMV2
fault-tree (once SPAR promotes the annex from opaque to typed).

## How to run it end-to-end

```sh
# 1. (optional) build bench and capture a real event stream
bash benches/engine_control/run_qemu_bench.sh
# => /tmp/engine-baseline/events.csv, /tmp/engine-gale/events.csv

# 2. sanity: make sure the AADL model still parses
spar parse  safety/aadl/semaphore.aadl
spar items  safety/aadl/semaphore.aadl

# 3. run the oracle
python3 benches/engine_control/spar_oracle.py \
    --model safety/aadl/semaphore.aadl \
    --events /tmp/engine-gale/events.csv

# 4. CI-friendly alternative: synthetic smoke stream (no bench required)
python3 benches/engine_control/synth_events.py
python3 benches/engine_control/spar_oracle.py \
    --model safety/aadl/semaphore.aadl \
    --events /tmp/gale-oracle-smoke.csv
```

Exit status is 0 on all three checks passing, 1 otherwise.

## What this prototype proves

- An AADL model of a gale primitive, written in SPAR's accepted
  subset, can be kept in the tree and kept green with `spar parse`.
- Runtime evidence from the same primitive, emitted by the
  `engine_control` bench, can be automatically checked against the
  model's declared timing envelope.
- The bridge is stdlib-only — the oracle runs in the minimal CI
  container (no scipy/numpy/babeltrace2), and SPAR is used when
  available but is not a hard dependency.

## What this prototype does *not* prove

- EMV2 error-propagation chains aren't analyzed — SPAR treats the
  annex as opaque. The "no Saturated transitions" check is a
  *heuristic* on >2x-WCET outliers, not a direct observation of
  `GiveDecision::Saturated` from the FFI.
- Flow latency proxies with `max(handoff_cycles)`; true reader-side
  wall-clock wants CTF (see `ctf-tracing-evaluation.md`).
- No schedulability — would need a platform model alongside the
  application.

## Follow-up scope

Next AADL primitives to model, in priority order:

1. **mutex** — STPA H-1 (priority inversion) is the highest-severity
   unmodeled hazard. AADL has native lock/priority-ceiling properties
   (`Access_Right`, `Concurrency_Control_Protocol`).
2. **ring_buf** — the SPSC path the bench already exercises; pairs
   with sem as a complete ISR→reader chain and unlocks true
   producer-flow-path → consumer-flow-sink latency.
3. **timer** — introduces periodic dispatch, exercises
   schedulability.

Heavier SPAR analyses to enable once models exist: `spar analyze` for
latency + resource budget (needs a `processor` platform sibling);
EMV2 fault-tree export (once SPAR's EMV2 subset covers composite
behavior); `spar verify` with a TOML rules file deriving assertions
from `artifacts/stpa.yaml` + `stpa_controllers_ucas.yaml`. Promoting
the evidence source from CSV to CTF gives per-event entry/exit
timestamps, which lets the Saturated check become a direct
observation.

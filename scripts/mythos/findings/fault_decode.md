# Finding: `classify()` misclassifies HFSR.DEBUGEVT-only HardFault as `FaultCategory::None`

**File**: `/Users/r/git/pulseengine/z/gale/src/fault_decode.rs`
**Function**: `CortexMFault::classify` (lines 325-406)
**Severity**: Safety-relevant (ASIL-D diagnostic correctness)
**Category (from priors)**: *proof-code drift vs the ARM architecture reference* + *precedence when multiple fault flags set*

## Summary

When a HardFault is indicated solely by `HFSR.DEBUGEVT` (bit 31) — with
`CFSR == 0`, `HFSR.FORCED == 0`, and `HFSR.VECTTBL == 0` — the `classify()`
function returns `FaultCategory::None` instead of `FaultCategory::HardFault`.

For safety logging this produces the diagnostic statement *"no fault
occurred"* during a real HardFault exception entry. The classification
contradicts the ARMv7-M Architecture Reference Manual (B3.2.16): any set
bit in HFSR — including DEBUGEVT — indicates a HardFault.

## Root cause

`classify()` branches on

```rust
if (hfsr & HFSR_FORCED) != 0 || (hfsr & HFSR_VECTTBL) != 0 {
    ... FaultCategory::HardFault
}
```

The check inspects only `HFSR_FORCED` (bit 30) and `HFSR_VECTTBL` (bit 1).
`HFSR_DEBUGEVT` (bit 31) is declared as a public constant (line 99) but
never consulted. Consequently, the proof obligations in the `ensures`
block (lines 327-351) and `lemma_classify_exhaustive` (line 533) all treat
`(hfsr & (HFSR_FORCED | HFSR_VECTTBL)) == 0 && cfsr == 0` as equivalent to
"no fault" — which is stronger than the ARM ARM permits.

The proof obligations are internally consistent, so **the proof passes**
even though the specification itself is drifted from the ARM ARM. This is
a classic "proof-code drift" failure: the Verus property holds against the
encoded predicate, but the encoded predicate does not match the hardware.

## Concrete counter-example

```text
cfsr  = 0x0000_0000
hfsr  = 0x8000_0000   // HFSR.DEBUGEVT set, HardFault actually raised
mmfar = 0
bfar  = 0

classify()          -> FaultCategory::None       // WRONG
expected per ARM ARM -> FaultCategory::HardFault
```

A debug monitor event that is not caught (e.g., external debugger detached
mid-session, or a `BKPT` instruction executed with `C_DEBUGEN == 0`)
escalates straight to HardFault with only HFSR.DEBUGEVT asserted. The
current decoder classifies this event as "no fault detected" and the
safety-logging path following `classify()` will omit the HardFault
entirely.

## ASIL-D impact

- Breaks claim **FH1 "CFSR decode is exhaustive"**: the property holds on
  CFSR but the classifier also reads HFSR, and the HFSR axis is
  non-exhaustive (DEBUGEVT missed).
- A fault-reaction time (FTTI) requirement that depends on `classify() !=
  None` will fail to trigger the safe state for DEBUGEVT-sourced
  HardFaults.
- Production builds typically disable the debug monitor, but ASIL-D
  requires correctness under the full state space of the hardware,
  including hostile bit flips that can set only HFSR bit 31.

## Oracle (1): Verus counter-proof

Add the following proof and it fails against current `classify`:

```rust
pub proof fn lemma_debugevt_alone_is_hardfault()
    ensures ({
        let f = CortexMFault { cfsr: 0, hfsr: HFSR_DEBUGEVT,
                               mmfar: 0, bfar: 0 };
        f.classify() === FaultCategory::HardFault
    })
{ }
```

The existing `ensures` on `classify()` only promises `FaultCategory::None`
when `cfsr == 0 && (hfsr & (HFSR_FORCED | HFSR_VECTTBL)) == 0`, which is
satisfied by `hfsr = HFSR_DEBUGEVT`, so Verus will reject the lemma.

## Oracle (2): runtime test

```rust
#[test]
fn debugevt_classifies_as_hardfault() {
    let f = CortexMFault::new(0, 1u32 << 31, 0, 0);   // HFSR.DEBUGEVT
    assert_eq!(f.classify(), FaultCategory::HardFault);
}
```

Current implementation returns `FaultCategory::None`; the assertion fails.

## Suggested fix

Treat every HFSR bit that indicates a HardFault as an escalation trigger:

```rust
const HFSR_ANY: u32 = HFSR_VECTTBL | HFSR_FORCED | HFSR_DEBUGEVT;

if (hfsr & HFSR_ANY) != 0 { FaultCategory::HardFault } else { ... }
```

Update the `ensures` clauses on `classify()` and
`lemma_classify_exhaustive` to replace `(HFSR_FORCED | HFSR_VECTTBL)` with
`HFSR_ANY`, and regenerate `lemma_hfsr_split` accordingly. Add a
DEBUGEVT-specific proof obligation so the spec-to-ARM-ARM correspondence
is captured by the verifier, preventing regression.

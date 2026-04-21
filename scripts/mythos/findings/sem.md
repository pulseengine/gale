# FILE: src/sem.rs

## FUNCTION / LINES

`gale::sem::give_decide` (src/sem.rs:70-86) and its FFI wrapper
`gale_k_sem_give_decide` (ffi/src/lib.rs:381-407), plus the companion
`gale_sem_count_give` (ffi/src/lib.rs:310-324).

Precondition on the Verus spec (src/sem.rs:71-73):

```rust
pub fn give_decide(count: u32, limit: u32, has_waiter: bool) -> (result: GiveDecision)
    requires
        limit > 0,
        count <= limit,
```

FFI call site (ffi/src/lib.rs:388) — NO runtime precondition check:

```rust
let decision = give_decide(count, limit, has_waiter != 0);
```

C shim (zephyr/gale_sem.c:100-101) — passes raw `sem->count` / `sem->limit`
straight from the k_sem struct, no validation:

```c
struct gale_sem_give_decision d = gale_k_sem_give_decide(
    sem->count, sem->limit, thread != NULL ? 1U : 0U);
```

## HYPOTHESIS

**Proof/code drift between the Verus contract and FFI usage: the ASIL-D
invariant P1 (`0 <= count <= limit`) can be violated at runtime without
detection.**

The Verus proof of `give_decide` is valid ONLY when its two `requires`
(`limit > 0`, `count <= limit`) hold. Inside pure Verus code every caller
must discharge those obligations; at the C FFI boundary no such
obligation is enforceable, and no defensive check is emitted.

Consider a k_sem whose `count`/`limit` are corrupted (stack smash, DMA
overwrite, wild pointer, SEU, or a not-yet-verified C caller bypassing
`k_sem_init`) such that `count > limit` or `limit == 0`:

1. C shim reads `sem->count = 7`, `sem->limit = 5` (or `limit = 0`) and
   calls `gale_k_sem_give_decide(7, 5, 0)`.
2. `give_decide` body evaluates `has_waiter=false`, then `count < limit`
   → `7 < 5` → false → returns `GiveDecision::Saturated`.
3. FFI wrapper translates `Saturated` to
   `{ action: GALE_SEM_ACTION_INCREMENT, new_count: 7 }` (lib.rs:402-405).
4. C shim executes `sem->count = d.new_count;` → `sem->count = 7`
   (still > limit). No error returned. No trap. No trace.

Net effect: the core safety invariant advertised in the module header
(`P1: 0 <= count <= limit (always)`) is now **silently false** after a
give, and every subsequent `take` sees a count the RTOS believes is
in-range but isn't. The same reasoning shows `gale_sem_count_give` (the
Phase-1 export) inherits the identical gap — it also unconditionally
dispatches to `give_decide` (lib.rs:315) and returns `count` on the
Saturated branch (lib.rs:322), which the caller then writes back.

A dual path exists for `take_decide`: it has `requires: true` so no
precondition is violated, but `gale_k_sem_take_decide(count=0, is_no_wait=0)`
returns `{ ret: 0, new_count: 0, action: PEND }` (lib.rs:450-454). If the
C shim is ever extended to set `sem->count = d.new_count` on the PEND
branch (a plausible future refactor following the symmetry of the give
path) it would clobber an out-of-invariant count to 0, masking the
corruption and losing liveness information. Current C shim does not do
this, so PEND is latent, not live.

This maps directly to the user-provided prior "proof-code drift between
decision function and its usage in `ffi/src/lib.rs`": the decision fn
carries a `requires` contract; the FFI plumbing drops it.

## ORACLE

### (1) Failing Kani harness

Add to `ffi/src/lib.rs`'s `mod kani_sem_proofs` (Kani style matches
existing `kani_sem_*` harnesses):

```rust
/// P1 drift: FFI must refuse / trap when its inputs violate the Verus
/// precondition (limit > 0, count <= limit).  Currently it silently
/// returns a "valid-looking" Saturated decision whose new_count is still
/// out of range, so Kani finds a counter-example immediately.
#[kani::proof]
fn sem_give_decide_rejects_invariant_violation() {
    let count: u32 = kani::any();
    let limit: u32 = kani::any();
    // Attacker / corruption model: invariant violated on the struct
    // that the C shim is about to hand to us.
    kani::assume(limit == 0 || count > limit);

    let d = gale_k_sem_give_decide(count, limit, 0);

    // Safety contract: if we accept the call at all, the written-back
    // count must still satisfy P1 (count <= limit).  A correctly
    // defensive FFI would either clamp new_count to limit, or refuse
    // (signal via a dedicated error action).  Kani will exhibit a
    // trace where new_count > limit (e.g. count=7, limit=5 -> new_count=7).
    if d.action == GALE_SEM_ACTION_INCREMENT {
        assert!(d.new_count <= limit, "P1 violated: new_count > limit");
    }
}
```

Expected Kani result: **FAILED** with concrete values such as
`count = 1, limit = 0` (gives `new_count = 1 > limit = 0`) or
`count = 0xFFFF_FFFF, limit = 5` (saturated branch returns `new_count = count`).

### (2) Failing property / unit test

Drop-in test for `/tmp/sem_drift_poc.rs` (also expressible as a
`proptest!` block in `tests/proptest_sem.rs`):

```rust
//! /tmp/sem_drift_poc.rs — demonstrates the drift in native Rust.
//! Build: rustc --test /tmp/sem_drift_poc.rs --extern gale=<path>

#[test]
fn ffi_give_decide_propagates_invalid_state() {
    // Simulate a corrupted k_sem handed to the FFI by a non-verified
    // C caller (or after memory corruption).
    let count: u32 = 7;
    let limit: u32 = 5;
    assert!(count > limit, "precondition of this PoC");

    // This is exactly what zephyr/gale_sem.c:100 executes.
    let d = ffi_gale::gale_k_sem_give_decide(count, limit, /*has_waiter=*/0);

    // The C shim will blindly execute `sem->count = d.new_count;`
    // (gale_sem.c:109) whenever action != WAKE.
    assert_eq!(d.action, ffi_gale::GALE_SEM_ACTION_INCREMENT);

    // FAILURE: new_count is still 7, which the shim writes back.
    // Invariant P1 (count <= limit) remains false.
    assert!(
        d.new_count <= limit,
        "P1 drift: ffi returned new_count={} for limit={}",
        d.new_count, limit
    );
}
```

Running this prints
`P1 drift: ffi returned new_count=7 for limit=5`
and exits with failure, confirming the FFI silently propagates the
invariant violation.

## POC TEST

See oracle (2) above for the standalone unit test. A more end-to-end PoC
using the actual C shim path:

```c
/* /tmp/poc_gale_sem_drift.c — link against libgale + zephyr/gale_sem.c */
#include <stdio.h>
#include "gale_sem.h"

int main(void) {
    /* Bypass k_sem_init (which would reject limit==0); simulate post-
     * corruption state directly. */
    uint32_t count = 7, limit = 5;

    struct gale_sem_give_decision d =
        gale_k_sem_give_decide(count, limit, /*has_waiter=*/0);

    /* Mirror zephyr/gale_sem.c:108-110 */
    if (d.action != GALE_SEM_ACTION_WAKE) {
        count = d.new_count;   /* writes 7 back — still > limit */
    }

    printf("post-give: count=%u limit=%u  P1_ok=%s\n",
           count, limit, (count <= limit) ? "yes" : "NO");
    return count <= limit ? 0 : 1;   /* exits 1 — invariant broken */
}
```

## IMPACT

* **Safety property P1 (`0 <= count <= limit`) is not enforced by the
  FFI boundary** — it is only proven about the inner `give_decide`
  contract.  Any upstream corruption of a k_sem propagates through gale
  unchecked and gale writes the corrupted value back.
* **Defence in depth is absent.** ASIL-D guidance (ISO 26262 Part 6
  §7.4.14, §8.4.4; STPA-Sec) requires that an argument-checking layer
  reject or clamp out-of-contract inputs at module boundaries; here the
  FFI treats the Verus `requires` as a runtime assumption rather than
  an obligation, so the check is missing in the executable image.
* **Reference-pattern amplification.** sem.rs is declared in the prompt
  ("the reference pattern followed by every subsequent primitive").  The
  same decide/apply pattern is used across mutex, msgq, stack, pipe,
  heap, … — a quick survey of lib.rs confirms that mutex / msgq / stack
  FFI wrappers likewise pass raw struct fields to `*_decide` fns with
  non-trivial Verus `requires`. A single `debug_assert!` or explicit
  clamp at the sem FFI boundary will set the template for the rest.
* **Severity:** if upstream corruption occurs, a subsequent `k_sem_take`
  that believes `count <= limit` may hand out more "resources" than the
  limit authorizes (e.g. concurrent producers into a bounded buffer,
  over-subscription of a pool). For ASIL-D that is classifiable as
  loss-of-control of a shared resource, typically S3 E4 C3.
* **Likelihood:** low under perfect memory (spinlock + k_sem_init keep
  the invariant), but the FFI is the last line of defence for exactly
  the "memory went bad" case and it currently does nothing.  Because
  this is a template defect, fixing it in sem also hardens every
  primitive that copies the pattern.
* **Detectability:** currently zero — no error code, no trace event, no
  log.  A take against the inflated count returns `Acquired` as if
  nothing were wrong.

## CANDIDATE UCA

To append to `safety/stpa/ucas.yaml` (or the existing
`artifacts/stpa.yaml`) under the semaphore control action set:

```yaml
- id: UCA-SEM-DRIFT-01
  control_action: "gale_k_sem_give_decide / gale_sem_count_give (FFI)"
  type: "provided-incorrectly"
  description: >
    The FFI decision exports accept (count, limit) unconditionally and
    forward them to a Verus function whose contract requires
    limit > 0 and count <= limit.  When the contract is violated
    at the C boundary (memory corruption, SEU, non-verified caller),
    the FFI returns action=INCREMENT with new_count = count (saturation
    branch), and the C shim writes it back to sem->count, leaving the
    ASIL-D invariant P1 (0 <= count <= limit) silently false.
  hazards: [H-SEM-01 "Semaphore count overflow/underflow",
            H-INV-01 "Core invariant violation undetected"]
  asil: D
  linked_requirements: [P1, P2, P9]
  suggested_controls:
    - >-
      Add runtime precondition guard at the FFI boundary:
      if limit == 0 || count > limit, return a dedicated error action
      (e.g. GALE_SEM_ACTION_FAULT) and have the C shim trap via
      k_panic()/__ASSERT, matching CHECKIF() behaviour elsewhere.
    - >-
      Mirror the same guard in every primitive that follows the sem
      reference pattern (mutex, msgq, stack, pipe, heap) — a sweep of
      ffi/src/lib.rs *_decide wrappers is recommended.
    - >-
      Encode the Verus `requires` as a Kani `assume` in every
      `kani_sem_*` proof AND add a companion `kani_sem_*_rejects_*`
      proof that asserts the FFI refuses out-of-contract inputs.  This
      prevents future proof/code drift from silently re-introducing the
      gap.
  evidence:
    - src/sem.rs:70-86 (Verus requires clause)
    - ffi/src/lib.rs:381-407 (FFI wrapper, no runtime check)
    - ffi/src/lib.rs:310-324 (Phase-1 wrapper, same gap)
    - zephyr/gale_sem.c:99-111 (C shim blindly writes back new_count)

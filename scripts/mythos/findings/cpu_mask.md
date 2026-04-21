# Finding: u32→u16 truncation drift between Verus model and FFI store; unchecked CPU index in enable/disable/non-PIN_ONLY pin

## Severity
High (ASIL-D safety-critical) — thread starvation / wrong-CPU scheduling.

## Category
Proof-code drift vs FFI shim (listed prior).

## Location
- `/Users/r/git/pulseengine/z/gale/src/cpu_mask.rs` (verified Rust model, u32)
- `/Users/r/git/pulseengine/z/gale/zephyr/gale_cpu_mask.c:42,45,69-93` (FFI shim)
- `/Users/r/git/pulseengine/z/zephyr/include/zephyr/kernel/thread.h:114` (`uint16_t cpu_mask`)

## Root cause

Two independent defects compound:

### D1 — Silent u32 → u16 narrowing on store (CM4 drift)
`gale_cpu_mask.c:45`:
```c
thread->base.cpu_mask = r.mask;   // r.mask:uint32_t  →  base.cpu_mask:uint16_t
```
Verus proves CM4: `result.error == OK ==> result.mask != 0u32`. The model operates on u32. But `thread->base.cpu_mask` is a `uint16_t` (thread.h:114). High bits of `r.mask` (bits 16..31) are silently discarded on store. If the only bits set are in that range, CM4 holds in Rust but the stored mask is zero — the thread has **no permitted CPU** and is never scheduled (silent starvation).

### D2 — BIT(cpu) has no bounds check in enable/disable/non-PIN_ONLY pin paths
`gale_cpu_mask.c:71,76,91`:
```c
return gale_cpu_mask_mod_wrapper(thread, BIT(cpu), 0U, 0U);      // enable
return gale_cpu_mask_mod_wrapper(thread, 0U, BIT(cpu), 0U);      // disable
uint32_t mask = BIT(cpu);                                        // non-PIN_ONLY pin
```
`BIT(n)` expands to `(1UL << (n))`. On ARM/x86, a shift ≥ 32 or negative is **undefined behavior** (C17 §6.5.7p3). The verified helper `cpu_pin_compute` exists and enforces `cpu_id < max_cpus <= 32` (CM6), but it is only invoked from the PIN_ONLY branch of `k_thread_cpu_pin`. The other three public entry points (`k_thread_cpu_mask_enable`, `k_thread_cpu_mask_disable`, non-PIN_ONLY `k_thread_cpu_pin`) feed `BIT(cpu)` directly to the Rust model, which happily accepts any u32.

### Combined exploit
Call `k_thread_cpu_mask_enable(thread, 20)` on a thread whose current mask is 0x1 (CPU 0), not-running, PIN_ONLY disabled:
1. C: `BIT(20) = 0x100000`, passed as `enable`.
2. Rust `cpu_mask_mod(0x1, 0x100000, 0, false, false)` returns `{mask: 0x100001, err: OK}`. All Verus ensures-clauses hold.
3. C: `thread->base.cpu_mask = 0x100001` → truncated to `0x0001`. Seems benign here.

Now call `k_thread_cpu_mask_disable(thread, 0)` next:
1. `BIT(0) = 1`, `disable = 1`.
2. Rust view: previous `r.mask` was 0x100001, but the *stored* state is 0x0001. Rust reads `thread->base.cpu_mask = 0x0001`, sees `(0x0001 | 0) & ~1 = 0`, returns EINVAL. OK so far.

But call `k_thread_cpu_mask_clear(thread)` then `k_thread_cpu_mask_enable(thread, 20)`:
1. Clear → mask stored as 0.
2. Enable 20 → Rust sees current=0, enable=0x100000; returns `{mask: 0x100000, err: OK}`. CM4 satisfied in model.
3. Store truncates: `thread->base.cpu_mask = 0x0000`. **Thread is now un-scheduleable with err=OK returned to caller.**

The scheduler will skip this thread on every CPU lookup (`thread->base.cpu_mask & BIT(cpu_id)` is always 0), causing permanent starvation. On ASIL-D this can violate availability/fail-operational requirements for a bound safety thread.

## Invariant violated
- CM4 as an **end-to-end** property: "Result mask is never zero." Verus proves it for the model's return value, not for the store location.
- Implicit invariant: `result.mask < (1 << CONFIG_MP_MAX_NUM_CPUS)` — never stated or proved. `MAX_CPUS = 16` exists as a constant but is not wired into `cpu_mask_mod` or `cpu_pin_compute` bounds.

## Oracle (1) — Verus/Kani
Strengthen `cpu_mask_mod` ensures-clause with an explicit bound tied to the storage width:
```rust
ensures
    result.error == OK ==> result.mask < (1u32 << 16),  // matches uint16_t field
```
Verus will **reject** this: `enable = 0x100000` produces `result.mask = 0x100000 >= 0x10000`. The failure localises D2 (unbounded enable/disable) and D1 (storage width mismatch) simultaneously.

Equivalent Kani harness:
```rust
#[kani::proof]
fn mask_fits_storage() {
    let cur: u32 = kani::any(); kani::assume(cur < (1 << 16));
    let en: u32  = kani::any();
    let dis: u32 = kani::any();
    let r = cpu_mask_mod(cur, en, dis, false, false);
    if r.error == OK { assert!(r.mask < (1 << 16)); }   // fails
}
```

## Oracle (2) — test
Add to `/Users/r/git/pulseengine/z/gale/tests/differential_cpu_mask.rs` (or a new integration test that exercises the C shim):
```rust
#[test]
fn enable_high_cpu_does_not_silently_starve() {
    // Simulate the FFI store: uint32_t result truncated to uint16_t field.
    let (mask, err) = ffi_cpu_mask_mod(0, 1u32 << 20, 0, false, false);
    assert_eq!(err, OK);                  // model says success
    let stored: u16 = mask as u16;        // C narrowing conversion
    assert_ne!(stored, 0,
        "stored cpu_mask must not be zero after successful enable");
}

#[test]
fn enable_accepts_out_of_range_cpu() {
    // BIT(32) is UB in C; BIT(20) is legal but out of 16-CPU range.
    let (_, err) = ffi_cpu_mask_mod(0x1, 1u32 << 20, 0, false, false);
    // Currently passes with err=OK — should return EINVAL.
    assert_eq!(err, EINVAL);
}
```
Both tests fail against the current model.

## Recommended fix
1. Add a `max_cpus` parameter (or hard-code `MAX_CPUS = 16`) to `cpu_mask_mod` and enforce `result.mask < (1 << max_cpus)`. Reject with EINVAL otherwise. Prove it in Verus.
2. In the FFI shim, either widen `thread->base.cpu_mask` to `uint32_t` or add a verified assertion at the store site that the value fits in `uint16_t`.
3. Route `k_thread_cpu_mask_enable/disable` and non-PIN_ONLY `k_thread_cpu_pin` through the bounds-checked `cpu_pin_compute` helper, returning `-EINVAL` for `cpu >= CONFIG_MP_MAX_NUM_CPUS` before any shift.
4. Update `MAX_CPUS` const usage to match the `BUILD_ASSERT(CONFIG_MP_MAX_NUM_CPUS <= 16)` upstream, and thread it through ensures-clauses.

## Priors matched
- **proof-code drift vs ffi shim**: CM4 is proved on u32 model return; shim stores into u16 field with implicit truncation, breaking the property end-to-end.
- **bit-mask overflow when CPU count > bits-per-word**: bits-per-stored-word is 16, not 32; the Verus ceiling of 32 is wrong for this project.

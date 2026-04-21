# Mythos Finding: fatal.rs — KernelPanic can be `Ignore`d in test-mode ISR

- **File:** `/Users/r/git/pulseengine/z/gale/src/fatal.rs`
- **FFI export:** `gale_k_fatal_decide` (ffi/src/lib.rs:4662)
- **C consumer:** `zephyr/gale_fatal.c:136` (z_fatal_error)
- **Severity:** HIGH (safety-relevant, ASIL-D)
- **Priors matched:** (a) "missed OOPS classification", (b) "proof-code drift vs
  FFI `gale_fatal_classify`", (c) partial "re-entry into fatal from fatal handler".
- **Oracle confidence:** (1) Verus counter-instance constructible + (2) missing
  differential test coverage (see below).

## Vulnerability

In the test-mode, ISR-context branch of `FatalError::classify`
(`fatal.rs:163-179`), the `match self.reason` arm maps
**`FatalReason::KernelPanic → RecoveryAction::Ignore`** (line 172):

```rust
FatalContext::Isr => {
    match self.reason {
        FatalReason::SpuriousIrq => RecoveryAction::Ignore,
        FatalReason::StackCheckFail => RecoveryAction::AbortThread,
        FatalReason::CpuException => RecoveryAction::Ignore,
        FatalReason::KernelOops => RecoveryAction::Ignore,
        FatalReason::KernelPanic => RecoveryAction::Ignore,   // <-- BUG
    }
}
```

### Why it is a safety defect

1. **FT2 violation.** The module's invariant FT2 states *"kernel panic is always
   non-recoverable"* (docstring line 28, 48, `is_panic_spec` at line 98, and
   `lemma_panic_halts` line 251). The `classify`'s `ensures` clause
   (lines 127–128) only asserts FT2 under `!self.test_mode`. That is a
   spec-weakening — test_mode is permitted to swallow `KernelPanic`, which
   contradicts FT2 as documented.

2. **Silent ISR re-entry.** `zephyr/gale_fatal.c:158-162` consumes the decision:
   ```c
   } else if (d.action == GALE_FATAL_ACTION_IGNORE) {
       arch_irq_unlock(key);
       return;
   }
   ```
   With `CONFIG_TEST=y` and `is_isr=1`, a panic reason causes the shim to
   return to the faulting ISR. Combined with weak `k_sys_fatal_error_handler`
   overrides (which apps install under CONFIG_TEST to continue testing), this
   resumes execution after a *kernel panic* — the fatal path loops back into
   the faulted state, hitting the "re-entry into fatal from fatal handler"
   prior. Per AGENTS/ASIL-D, the fatal path must terminate cleanly.

3. **Proof-to-FFI drift.** `lemma_panic_halts` (line 251) is a stub
   (`ensures true`), so Verus never verifies the full FT2 invariant.
   The differential test `fatal_kernel_panic_always_halts_production`
   (`tests/differential_fatal.rs:156-163`) only checks `test_mode=false`.
   No test pins behaviour for `reason=4, is_isr=1, test_mode=1`.
   The FFI replica (`ffi_fatal_decide` line 31-41) hard-codes the same bug
   (`FATAL_ACTION_IGNORE` for all non-stack reasons), so the differential
   oracle passes despite the defect — classic proof/code drift.

4. **Reachability.** `gale_fatal.c:141` guards the halt-apply path with
   `!IS_ENABLED(CONFIG_TEST)`, so the `__ASSERT(reason != K_ERR_KERNEL_PANIC,…)`
   (line 142) never fires under CONFIG_TEST. The Rust decision is the sole
   gate for panic in test builds. Once the decision says `Ignore`, there is
   no downstream check.

### Reproducer (Verus counter-shape)

```rust
let err = FatalError {
    reason: FatalReason::KernelPanic,
    context: FatalContext::Isr,
    test_mode: true,
};
assert!(matches!(err.classify(), RecoveryAction::Halt));  // FAILS: returns Ignore
```

### Test-oracle proposal

Add to `tests/differential_fatal.rs`:

```rust
#[test]
fn fatal_kernel_panic_halts_even_in_test_mode() {
    for is_isr in [false, true] {
        let (action, ret) = ffi_fatal_decide(4, is_isr, true);
        assert_eq!(ret, 0);
        assert_eq!(action, FATAL_ACTION_HALT,
            "FT2: KERNEL_PANIC must halt even under CONFIG_TEST (is_isr={is_isr})");
    }
}
```

This test currently **fails** against both the Rust model and the FFI replica.

### Verus oracle proposal

Strengthen `classify`'s `ensures` clause:

```rust
// FT2 (full): kernel panic always halts, irrespective of test_mode.
self.reason === FatalReason::KernelPanic ==> result === RecoveryAction::Halt,
```

And make `lemma_panic_halts` actually assert it:

```rust
pub proof fn lemma_panic_halts()
    ensures
        forall |ctx: FatalContext, tm: bool|
            (FatalError { reason: FatalReason::KernelPanic, context: ctx, test_mode: tm })
                .classify() === RecoveryAction::Halt,
{}
```

Verus will reject the existing `classify` body (line 172) until the ISR
test-mode arm is changed to `RecoveryAction::Halt`.

### Recommended fix

In `fatal.rs:172` change:

```rust
FatalReason::KernelPanic => RecoveryAction::Ignore,
```

to

```rust
FatalReason::KernelPanic => RecoveryAction::Halt,
```

and mirror the fix in the FFI replica (`tests/differential_fatal.rs`
line 31-41) and the spec (line 127-128 / lemma_panic_halts).

### Collateral observations (not the primary finding)

- `lemma_panic_halts`, `lemma_thread_faults_recoverable`,
  `lemma_stack_check_always_abort`, `lemma_test_mode_permissive`, and
  `lemma_valid_codes_map` are all `ensures true` stubs (lines 247, 258, 270,
  291, 303). They give a false impression of proof coverage for FT1–FT4.
- `classify_decide`'s `ensures` (fatal.rs:320-321) has
  `result.is_ok() ==> true`, which is vacuous — it does not constrain the
  action, so any `Ok(_)` satisfies it. This is the formal hole through which
  the panic-ignore drift slips.
- `gale_fatal.c` calls `gale_k_fatal_decide` *after* `coredump()` and
  `k_sys_fatal_error_handler()`. If either of those re-enters `z_fatal_error`
  (e.g. a panic during coredump of a stack-corrupted thread), the IRQ lock
  `key` is already held — nested `arch_irq_lock` then `arch_irq_unlock(key)`
  on an Ignore return will drop the outer lock prematurely. Separate from
  the main finding but reinforces "fatal path must terminate cleanly."

## Status

- Written read-only per Mythos protocol; no code changes applied.
- Ready for triage under GAP-2 (STPA) and the Zephyr FV landscape
  (prior Isabelle/PiCore buddy-alloc audit found 3 bugs; this is a 4th class
  in the fatal-path).

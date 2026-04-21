# Finding: `validate_decide` ensures-clause does not cover US7 — init-check branch can silently drift past Verus verification

## Severity
High (ASIL-D safety-critical) — a regression in the initialization-flag branch of the FFI entry point that gates every user→kernel syscall would not be caught by the proof suite or by the differential test.

## Category
Proof-code drift vs FFI handlers (listed prior: "Initialization flag race" / "Proof-code drift vs ffi handlers").

## Location
- `/Users/r/git/pulseengine/z/gale/src/userspace.rs:771-798`  — `pub fn validate_decide`
- `/Users/r/git/pulseengine/z/gale/ffi/src/lib.rs:5801-5817`  — `gale_k_object_validate_decide` (only FFI path into the verified validator; `k_object_validate` in the C shim calls this function, not `KernelObject::validate`)
- `/Users/r/git/pulseengine/z/gale/zephyr/gale_userspace.c:722-745`  — C shim that invokes `gale_k_object_validate_decide` for every `k_object_validate` call (and therefore every syscall object check, including the new `z_impl_k_object_access_check` added in commit 9b99ef7)
- `/Users/r/git/pulseengine/z/gale/tests/differential_userspace.rs:57-159`  — differential test that is a tautology (see below)

## Root cause

`validate_decide` is the single ExEc-level Rust function exposed over the FFI for US1/US4/US5/US7 syscall validation. Its Verus contract is:

```rust
pub fn validate_decide(
    type_matches: bool,
    has_access: bool,
    is_initialized: bool,
    init_check: i8,
) -> (result: Result<(), i32>)
    ensures
        !type_matches ==> result === Err::<(), i32>(EBADF),
        (type_matches && !has_access) ==> result === Err::<(), i32>(EPERM),
{
    if !type_matches { return Err(EBADF); }
    if !has_access   { return Err(EPERM); }
    if init_check == 0 {
        if !is_initialized { return Err(EINVAL); }
    } else if init_check == -1 {
        if is_initialized { return Err(EADDRINUSE); }
    }
    Ok(())
}
```

The ensures clause pins down US4 (line 778) and US1/US5 (line 779). **It says nothing about US7.** The two failure postconditions it does state leave the entire initialization branch (`init_check`, `is_initialized`) completely unconstrained. In particular, neither of these hold as ensures:

- `result.is_ok() ==> (init_check != 0 || is_initialized)`                     — MustBeInit must actually be enforced
- `result.is_ok() ==> (init_check != -1 || !is_initialized)`                   — MustNotBeInit must actually be enforced
- `(type_matches && has_access && init_check == 0 && !is_initialized) ==> result === Err(EINVAL)`
- `(type_matches && has_access && init_check == -1 && is_initialized) ==> result === Err(EADDRINUSE)`

Because Verus only checks that the body entails the stated ensures, a body that inverts or drops the init check still satisfies the contract. The file-level comment block (src/userspace.rs:37-46) lists US7 as an "ASIL-D verified property", and the `KernelObject::validate` method at line 424 does carry a strong ensures for it — but the FFI shim never calls `KernelObject::validate`. It collapses the object state into four scalars (obj_type/expected_type/flags/has_access/init_check) and calls `validate_decide`, bypassing the verified US7 postcondition entirely. The chain advertised in docs/safety/stpa-gap2-audit.md:399 (`validate_decide(obj_type, expected, flags, has_access, init_check)`) does not match what is actually verified.

### Concrete drift scenario (failure silently verified)

Invert the sense on line 789 (a one-character edit):

```rust
if init_check == 0 {
    if is_initialized {                     // was !is_initialized
        return Err(EINVAL);
    }
}
```

This flips US7 (`MustBeInit`): uninitialized objects now pass, initialized objects are rejected. Calling `k_sem_take()` on a sem whose `K_OBJ_FLAG_INITIALIZED` bit is zero — either because `k_sem_init()` never ran or because `k_object_uninit()` was just called — returns OK through the FFI. A user thread can therefore operate on a half-constructed kernel object: the semaphore's wait list, atomic counter, and queue fields are all in their POST-`k_object_new()` zeroed state. This is the same class of bug the verification is claimed to prevent (the Zephyr FV prior art, 2019 PiCore buddy-allocator work, found three such decision-inversion bugs in exactly this kind of code).

Verus accepts the edit because both explicit ensures clauses remain satisfied:
- `!type_matches ==> Err(EBADF)` — still holds (line 781 unchanged)
- `(type_matches && !has_access) ==> Err(EPERM)` — still holds (line 784 unchanged)

`cargo verus` reports 0 errors. No Rocq proof covers this function. No Kani harness asserts US7 on `validate_decide` (`/Users/r/git/pulseengine/z/gale/ffi/src/lib.rs:7495-7609` has harnesses for US1/US4/US5 but the init-check assertion is only `init_check == _OBJ_INIT_ANY ==> result == OK`, which holds for both the correct and the inverted body).

### Why the differential test does not catch it

`tests/differential_userspace.rs:57-70` defines `ffi_object_validate_decide` as a wrapper that itself calls `validate_decide`. The test at line 131-159 compares the wrapper against a manual "model" that also calls `validate_decide` on lines 145-150. Both sides share the same bug; the test is a tautology that checks `validate_decide == validate_decide`, not `validate_decide == specification`. Any bug in `validate_decide` cancels out.

## Invariant violated

US7 end-to-end: "K_OBJ_FLAG_INITIALIZED required for access (when init_check == MustBeInit)". The file header claims this property is verified. It is proved on the unreachable-from-FFI `KernelObject::validate`, but the FFI path through `validate_decide` has no corresponding ensures and is therefore not verified for US7.

## Oracle (1) — Verus

Strengthen the contract of `validate_decide` so the init-check branch is part of the proof obligation:

```rust
pub fn validate_decide(
    type_matches: bool,
    has_access: bool,
    is_initialized: bool,
    init_check: i8,
) -> (result: Result<(), i32>)
    ensures
        !type_matches ==> result === Err::<(), i32>(EBADF),
        (type_matches && !has_access) ==> result === Err::<(), i32>(EPERM),
        // US7 — newly added
        (type_matches && has_access && init_check == 0 && !is_initialized)
            ==> result === Err::<(), i32>(EINVAL),
        (type_matches && has_access && init_check == -1 && is_initialized)
            ==> result === Err::<(), i32>(EADDRINUSE),
        result.is_ok() ==> type_matches && has_access
            && (init_check != 0 || is_initialized)
            && (init_check != -1 || !is_initialized),
```

With these clauses in place, the inverted body above is rejected by Verus (the `init_check == 0 && !is_initialized ==> Err(EINVAL)` clause fails because the body returns Ok). The correct body discharges all five clauses trivially.

## Oracle (2) — Test

Replace the tautological differential test with one that encodes the specification directly, independent of the function under test:

```rust
// /Users/r/git/pulseengine/z/gale/tests/differential_userspace.rs

fn expected_validate(
    type_matches: bool, has_access: bool,
    is_initialized: bool, init_check: i8,
) -> i32 {
    if !type_matches            { return EBADF; }
    if !has_access              { return EPERM; }
    match init_check {
        0  if !is_initialized   => EINVAL,
        -1 if  is_initialized   => EADDRINUSE,
        _                       => OK,
    }
}

#[test]
fn validate_decide_enforces_us7() {
    for type_matches in [false, true] {
      for has_access in [false, true] {
        for is_initialized in [false, true] {
          for init_check in [0i8, -1, 1, 2, 42] {
            let got = match validate_decide(
                type_matches, has_access, is_initialized, init_check) {
                Ok(()) => OK, Err(e) => e,
            };
            let want = expected_validate(
                type_matches, has_access, is_initialized, init_check);
            assert_eq!(got, want,
                "validate_decide drift at \
                 tm={type_matches} ha={has_access} init={is_initialized} ic={init_check}");
          }
        }
      }
    }
}
```

Applied to the correct body this passes (80 cases, all equal). Applied to the inverted body above it fails on `(true, true, false, 0)`: model returns `OK`, spec says `EINVAL`.

## Scope note on commit 9b99ef7

The new `z_impl_k_object_access_check` shim (`/Users/r/git/pulseengine/z/gale/zephyr/gale_userspace.c:668-671`) calls `k_object_validate(ko, K_OBJ_ANY, _OBJ_INIT_ANY)`, which in turn flows through `gale_k_object_validate_decide` → `validate_decide`. Because this path uses `init_check == 1` (DontCare), the specific `!is_initialized` drift scenario above does not affect the access-check syscall itself. **But every other syscall that passes `_OBJ_INIT_TRUE` (e.g. `k_sem_take`, `k_mutex_lock`, `k_msgq_put`, `k_pipe_*`, `k_mbox_*`) does hit the US7 branch on every user→kernel transition.** The new syscall is safe only incidentally; the gap is in the decision function that gates every other syscall object check.

## Priors matched
- **Proof-code drift vs ffi handlers** — the file header advertises US7 as verified; the actual FFI entry point does not verify it.
- **Initialization flag race / state-machine drift** — a one-character edit to the init branch is invisible to the verification suite, including the differential test.

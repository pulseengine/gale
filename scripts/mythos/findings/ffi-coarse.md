# Finding: `gale_sem_give_v2` proof/code drift — u32 overflow when `count > limit`

- **File**: `/Users/r/git/pulseengine/z/gale/ffi/src/coarse.rs`
- **Function**: `gale_sem_give_v2` (lines 90–105)
- **Prior matched**: proof-code drift
- **Oracles**: Kani + unit test

## Defect

```rust
pub extern "C" fn gale_sem_give_v2(state: *mut GaleSemState) -> i32 {
    unsafe {
        if state.is_null() {
            return EINVAL;
        }
        let s = &mut *state;
        if s.count != s.limit {
            // Verified: count < limit <= u32::MAX, no overflow.
            #[allow(clippy::arithmetic_side_effects)]
            {
                s.count += 1;
            }
        }
        OK
    }
}
```

The `#[allow(clippy::arithmetic_side_effects)]` comment asserts the proof
obligation `count < limit <= u32::MAX`. However, the runtime guard is
`s.count != s.limit`, not `s.count < s.limit`. Nothing in this function
(nor in the FFI contract as exposed to C) enforces the invariant
`count <= limit` on entry — `gale_sem_validate_v2` is a *separate*
function the C shim may or may not call, and the struct fields are `pub`
so a C caller can construct any `GaleSemState` in place.

### Exploit input

C caller passes `GaleSemState { count: u32::MAX, limit: 5 }`:

1. `count != limit` → branch entered.
2. `count + 1` → debug: panic (abort across FFI → UB in C);
   release: wraps to `0`, silently violating the "count up to limit"
   postcondition and producing a semaphore that now claims zero tokens
   when it previously appeared over-full.

### Why this is the drift the priors call out

The in-code "Verified" claim (`count < limit`) and the actual runtime
check (`count != limit`) diverge. Any Verus/Kani proof relying on the
commented invariant will succeed on the spec side while the emitted
binary can still overflow. This is precisely the "proof-code drift"
pattern listed in the priors.

## Oracles

### Kani harness

```rust
#[cfg(kani)]
#[kani::proof]
fn kani_sem_give_no_overflow() {
    let mut s = GaleSemState {
        count: kani::any(),
        limit: kani::any(),
    };
    // NOTE: no kani::assume(s.count <= s.limit) — the FFI boundary
    // does not enforce this.
    let before = s.count;
    let rc = gale_sem_give_v2(&mut s as *mut _);
    assert_eq!(rc, OK);
    // Must never wrap:
    assert!(s.count == before || s.count == before.wrapping_add(1));
    assert!(s.count >= before); // fails when before == u32::MAX
}
```

Kani reports overflow in `s.count += 1` for `count = u32::MAX`,
`limit ∈ [0, u32::MAX - 1]`.

### Unit test (reproduces in debug mode)

```rust
#[test]
#[should_panic] // overflow in debug; silent wrap in release
fn sem_give_overflows_when_count_exceeds_limit() {
    let mut s = GaleSemState { count: u32::MAX, limit: 5 };
    let _ = gale_sem_give_v2(&mut s);
}
```

## Fix

Replace the branch condition so the invariant the comment asserts is the
invariant actually checked:

```rust
if s.count < s.limit {
    s.count += 1;
}
```

Or, equivalently, use `saturating_add` and drop the
`arithmetic_side_effects` allow:

```rust
s.count = s.count.saturating_add(1).min(s.limit);
```

Either form makes the function total over all `(count, limit): (u32, u32)`
and brings the code back into alignment with the "verified" comment.

## Scope

Sibling functions were reviewed:

- `gale_sem_take_v2`: guard `count > 0` is sufficient for `count - 1` —
  no drift.
- `gale_stack_push_v2`: guard `count < capacity` implies
  `count <= u32::MAX - 1` — safe.
- `gale_stack_pop_v2`: guard `count > 0` — safe.
- `gale_pipe_write_v2`: `free = size - used` guarded by `used < size`;
  `used += n` with `n <= free` — safe.
- `gale_pipe_read_v2`: `used -= n` with `n <= used` — safe.

Only `gale_sem_give_v2` exhibits the drift.

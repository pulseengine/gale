# Finding: `spin_lock_valid` same-CPU deadlock false-negative when `CONFIG_MP_MAX_NUM_CPUS > 4`; Verus precondition `cpu_id < 4` is silently erased at the C FFI boundary

## Severity
High (ASIL-D safety-critical). The function's documented purpose is "validator": its falsification directly enables undetected deadlock / lock-ownership violations inside every kernel critical section that calls `k_spin_lock()`. Priors label this exact class "validator false-negative".

## Category
Validator false-negative + proof-code drift (spec vs. storage/encoding). Listed priors: validator false-negative, proof-code drift vs spinlock.rs, off-by-one on the CPU mask.

## Location
- Verified Rust: `/Users/r/git/pulseengine/z/gale/src/spinlock_validate.rs`
  - `MAX_CPUS = 4`, `CPU_MASK = 3`, `THREAD_ALIGN = 4` (lines 41, 47, 53)
  - `cpu_id_valid(cpu) := (cpu as usize) < MAX_CPUS` (lines 66–68)
  - `spin_lock_valid` with `requires cpu_id_valid(current_cpu_id)` (lines 109–129)
  - `spin_unlock_valid` with `requires cpu_id_valid(current_cpu_id)` (lines 154–172)
  - `spin_lock_compute_owner` with `requires cpu_id_valid(current_cpu_id)` (lines 188–203)
- FFI shim (Rust, `extern "C"`): `/Users/r/git/pulseengine/z/gale/ffi/src/lib.rs:8896–8945`
  - `gale_spin_lock_valid`, `gale_spin_unlock_valid`, `gale_spin_lock_compute_owner`: no runtime guard on `current_cpu_id`.
- Zephyr C shim: `/Users/r/git/pulseengine/z/gale/zephyr/gale_spinlock_validate.c:29–68`
  - Passes `_current_cpu->id` straight through; no `BUILD_ASSERT`, no runtime check.
- Upstream assert that currently hides the bug: `/Users/r/git/pulseengine/z/zephyr/include/zephyr/spinlock.h:107`
  - `BUILD_ASSERT(CONFIG_MP_MAX_NUM_CPUS <= 4, ...)` — but it is guarded by `#ifdef CONFIG_SPIN_VALIDATE`, so it is absent whenever gale is linked without that Kconfig symbol (e.g., direct FFI users, out-of-Zephyr hosts, the existing differential/integration tests).

## Root cause (two compounding defects)

### D1 — `cpu_id_valid` is a soft-assumption that the C ABI cannot see
`spin_lock_valid`, `spin_unlock_valid`, and `spin_lock_compute_owner` all list
`requires cpu_id_valid(current_cpu_id)` in their Verus contracts. Verus only
enforces this at Rust call sites. The three `extern "C"` wrappers
(`ffi/src/lib.rs:8904, 8921, 8939`) simply call the verified function; the
precondition becomes a silent assumption once the symbol is exported.

On a configuration with `CONFIG_MP_MAX_NUM_CPUS = 5..16`, `_current_cpu->id`
can legally be any value in `{0..CONFIG_MP_MAX_NUM_CPUS-1}`. The gale shim
(`gale_spinlock_validate.c:34, 60, 67`) forwards this unfiltered.

### D2 — Hard-coded `CPU_MASK = 3` loses the high CPU-id bits
`spin_lock_valid` (line 124):
```rust
if (thread_cpu & CPU_MASK) == (current_cpu_id as usize) { return false; }
```
`CPU_MASK` is the compile-time constant `3` (≡ `MAX_CPUS - 1`, with
`MAX_CPUS = 4`). For any `current_cpu_id >= 4` the equality is **unreachable**
whenever the stored `thread_cpu` is well-formed:
```
(thread_cpu & 3) ∈ {0, 1, 2, 3}, but current_cpu_id ∈ {4, 5, 6, 7, …}
  ⇒ the `if` body never executes ⇒ spin_lock_valid returns `true` ⇒
    validator says "safe to acquire" on a lock already held by the SAME CPU.
```

The docstring (line 8, 27) explicitly advertises that `MAX_CPUS` *"can be
widened"*. It cannot — widening it to 8 (or any value > 4) without also
widening `CPU_MASK`/`THREAD_ALIGN` and re-running the by(bit_vector) proofs
silently breaks SV2. Even just relaxing `cpu_id_valid` to `cpu < 8` would
produce the false-negative described here.

### Combined exploitation
Configuration: `CONFIG_MP_MAX_NUM_CPUS = 8`, `CONFIG_SPIN_VALIDATE` not set
(the BUILD_ASSERT in `spinlock.h:107` is hidden behind `#ifdef
CONFIG_SPIN_VALIDATE`, so it is inert here) **or** any direct consumer of the
`gale_spin_lock_valid` FFI symbol (e.g., tests, a non-Zephyr host).

1. CPU 5 takes the lock: shim stores `thread_cpu = _current | 5`.
2. Same CPU (same `_current_cpu->id = 5`) calls `k_spin_lock` again on the
   same `struct k_spinlock`. The C shim calls
   `gale_spin_lock_valid(thread_cpu, 5)`.
3. Rust executes `if thread_cpu != 0 { if (thread_cpu & 3) == (5 as usize) {
   return false } }`. `thread_cpu & 3 ∈ {0,1,2,3}`; `5 as usize == 5`;
   equality is impossible. Falls through.
4. Returns `true` → the shim returns `true` → `k_spin_lock` proceeds to spin
   on an atomic the **same CPU** already owns ⇒ deterministic hardware
   deadlock inside a kernel critical section.

Compare to the intended behaviour encoded by SV2 ("lock_valid returns false
iff the lock is already held by the same CPU", line 23): SV2 is violated in
spirit while every Verus ensures-clause remains trivially satisfied, because
the ensures-clauses themselves talk only about `thread_cpu & CPU_MASK`, which
shares the same loss-of-high-bit as the implementation.

### Why Verus doesn't catch it
`cpu_id_valid` is used as the *precondition* (`requires`) of the three
functions, so Verus never has to reason about what happens at `cpu_id >= 4`.
The proof is correct for the subset Verus is asked to prove, but that subset
no longer coincides with the real-world input domain once the symbol is
exported. Classic shape of specification-hole drift.

### Why the existing Kani harnesses don't catch it
Every harness in `ffi/src/lib.rs:9117–9219`
(`spinlock_lock_valid_free`, `spinlock_lock_valid_same_cpu_deadlock`,
`spinlock_lock_valid_different_cpu`, `spinlock_unlock_valid_*`,
`spinlock_owner_encoding_roundtrip`, the three `*_no_panic` harnesses)
begins with:

```rust
let cpu_id: u32 = kani::any();
kani::assume(cpu_id < 4);
```

The dangerous half of the `u32` domain is unconditionally masked out. The
Verus proof and the Kani proofs reinforce each other on the same 4-CPU
slice; neither checks what the `extern "C"` ABI actually accepts.

### Why the comment's "configurable MAX_CPUS" claim is false-but-load-bearing
`src/spinlock_validate.rs:8-10, 27, 41` say the `& 3U` magic constant has
been replaced by a configurable `MAX_CPUS`. It hasn't. Changing `MAX_CPUS`
without hand-editing both `CPU_MASK` (line 47) and `THREAD_ALIGN` (line 53)
— and re-discharging the `by(bit_vector)` lemmas on line 169 and 200 —
breaks SV1/SV4/SV5/SV6 silently. The documentation invites a downstream
integrator to widen `MAX_CPUS`, step straight into D1+D2, and believe they
are protected by a verified validator.

## Invariants violated
- **SV2** (line 23: "lock_valid returns false iff the lock is already held
  by the same CPU") — at the FFI/ABI level, not at the Verus level.
- Implicit ASIL-D contract "validator either accepts a safe acquire or
  rejects; never false-negative on same-CPU deadlock".
- Implicit upstream assumption encoded only in `spinlock.h`
  `BUILD_ASSERT(CONFIG_MP_MAX_NUM_CPUS <= 4)` — not mirrored in the gale
  overlay.

## Oracle (1) — Verus / Kani (counter-example that drops the precondition)

Add to `/Users/r/git/pulseengine/z/gale/ffi/src/lib.rs` inside
`mod kani_spinlock_validate_proofs`:

```rust
// cargo kani -p gale-ffi --harness sl_validate_same_cpu_above_mask
#[cfg(all(kani, feature = "spinlock_validate"))]
#[kani::proof]
fn sl_validate_same_cpu_above_mask() {
    // A configuration upstream Zephyr explicitly forbids via BUILD_ASSERT,
    // but which the gale FFI symbol happily accepts.
    let cpu_id: u32 = kani::any();
    kani::assume(cpu_id >= 4 && cpu_id < 8);   // e.g. MP_MAX_NUM_CPUS = 8

    let thread_ptr: usize = kani::any();
    kani::assume(thread_ptr != 0);
    kani::assume(thread_ptr & 3 == 0);          // matches THREAD_ALIGN = 4

    // The same CPU already owns the lock.  Encoding follows the C shim.
    let thread_cpu = thread_ptr | (cpu_id as usize);

    // Safety property SV2: acquiring again on the SAME CPU must be rejected.
    let ret = gale_spin_lock_valid(thread_cpu, cpu_id);
    assert!(ret == 0, "SV2 violated: validator accepts same-CPU reacquire");
}
```

Kani returns a counter-example (e.g., `cpu_id = 5`, `thread_ptr = 0x4`,
`thread_cpu = 0x5`, `ret = 1`). The Verus-level equivalent is to relax
`cpu_id_valid` to `cpu < 8` and re-run `spin_lock_valid`'s ensures — Z3
produces the same witness because the ensures for the "held, same CPU" case
compares `thread_cpu & CPU_MASK` (bounded to `{0,1,2,3}`) against
`current_cpu_id as usize` (up to 7).

Verus formulation (pasted near the function):
```rust
proof fn lemma_sv2_at_5cpus() {
    let cpu: u32 = 5;
    let thread: usize = 0x10;          // thread_ptr_valid holds (nonzero, &3 == 0)
    let thread_cpu: usize = thread | (cpu as usize); // = 0x15
    // Ensures would need:
    //   thread_cpu != 0 && (thread_cpu & CPU_MASK) == cpu as usize ==> !valid
    // but (0x15 & 3) == 1 != 5.  The implication is vacuously true;
    // the implementation returns `true`, i.e. "safe to acquire".
    // Drift: the actual safety property (SAME CPU ⇒ reject) is not
    // captured for cpu >= 4.
    assert((thread_cpu & (CPU_MASK as usize)) != (cpu as usize));
}
```

## Oracle (2) — Deterministic unit test (POC)

New file `/Users/r/git/pulseengine/z/gale/tests/spinlock_validate_cpu_over_mask_poc.rs`:

```rust
//! POC: gale_spin_lock_valid reports "safe to acquire" when the same CPU
//! with id >= 4 already holds the lock.  Deterministic; no kernel needed.
//!
//!   cargo test -p gale-ffi --features spinlock_validate \
//!       --test spinlock_validate_cpu_over_mask_poc

use gale_ffi::{gale_spin_lock_compute_owner, gale_spin_lock_valid};

#[test]
fn same_cpu_reacquire_is_falsely_allowed_above_mask() {
    // Simulate Zephyr configured with CONFIG_MP_MAX_NUM_CPUS = 8.
    // (The BUILD_ASSERT in zephyr/include/zephyr/spinlock.h:107 is gated by
    //  CONFIG_SPIN_VALIDATE; in this test we directly exercise the FFI,
    //  so the assert is absent — exactly the exposure surface.)
    let current_cpu: u32 = 5;
    let thread_ptr: usize = 0x1000; // aligned, nonzero

    // C shim stores:  l->thread_cpu = cpu | thread_ptr
    // NB: We cannot use `gale_spin_lock_compute_owner` here because it
    // requires cpu < 4 at the Verus level; the C shim has no such check,
    // so emulate what it would have written.
    let thread_cpu_stored = thread_ptr | (current_cpu as usize);

    // The SAME CPU tries to reacquire.  SV2 demands a rejection.
    let ret = gale_spin_lock_valid(thread_cpu_stored, current_cpu);

    assert_eq!(
        ret, 0,
        "SV2 violated: gale_spin_lock_valid returned {} (safe-to-acquire) \
         despite the lock being held by the same CPU {}",
        ret, current_cpu,
    );
}

#[test]
fn upstream_range_still_correct() {
    // Sanity: on a CPU_MASK-compatible configuration the validator works.
    for cpu in 0u32..4 {
        let stored = 0x1000usize | cpu as usize;
        assert_eq!(gale_spin_lock_valid(stored, cpu), 0,
            "regression: cpu={} same-CPU reacquire not rejected", cpu);
    }
}
```

The first test fails on the current HEAD (`ret == 1`), the second passes.
The diff between the two pinpoints the CPU_MASK cliff.

## Impact
- **Safety** (ASIL-D): validator fails closed → fails open for same-CPU
  reacquire when CPU id >= 4. Every call site of `k_spin_lock()` (scheduler,
  timer ISR dispatch, every driver entry point) is affected. In practice
  the reacquire spins forever on the atomic, producing a hard hang; under
  SMP it can lock-step other CPUs too (they see the lock still held).
- **Latency** of the bug: hidden by
  `BUILD_ASSERT(CONFIG_MP_MAX_NUM_CPUS <= 4)` in `spinlock.h:107`, but that
  BUILD_ASSERT is guarded by `CONFIG_SPIN_VALIDATE` and does not exist in
  the gale overlay itself. Any consumer that:
  - links gale-ffi directly (integration tests, external crates);
  - builds Zephyr with `CONFIG_SPIN_VALIDATE=n` but still uses the gale
    overlay's `z_spin_lock_set_owner` (the owner is still written);
  - forward-ports gale to a Zephyr config that raises the CPU ceiling
    (hardware is moving this direction — cortex-A53 x8, x86 x8+);
  - believes the module doc ("configurable `MAX_CPUS`") and widens
    `MAX_CPUS` without touching `CPU_MASK`,
  loses SV2.
- **Drift against `src/spinlock.rs`**: `spinlock.rs` models ownership as
  `owner: Option<u32>` (thread id) and tests acquisition with
  `self.owner.is_none()`. `spinlock_validate.rs` models ownership as
  `(cpu, thread_ptr)` bit-packed. The two files disagree on what "same
  owner" means. The higher-level `SpinlockState::acquire_check` rejects
  any held state; the low-level `spin_lock_valid` accepts when CPU bits
  differ. A refactor that routes the C shim through the high-level model
  would change behaviour silently; a refactor that routes it through the
  low-level model inherits D1+D2. This exactly matches the "proof-code
  drift vs spinlock.rs" prior.

## Remediation (pick one; (a)+(b) recommended)

(a) **Mirror the upstream BUILD_ASSERT in the gale overlay** —
`/Users/r/git/pulseengine/z/gale/zephyr/gale_spinlock_validate.c`:
```c
#include <zephyr/sys/util.h>
BUILD_ASSERT(CONFIG_MP_MAX_NUM_CPUS <= 4,
             "gale_spin_lock_valid CPU_MASK is hard-coded to 3");
```
This closes the `CONFIG_SPIN_VALIDATE=n` gap.

(b) **Harden the Rust FFI entry points** — add a runtime guard that fails
closed (returns "invalid to acquire") rather than fall through silently:
```rust
pub extern "C" fn gale_spin_lock_valid(thread_cpu: usize, current_cpu_id: u32) -> i32 {
    if current_cpu_id >= MAX_CPUS { return 0; }   // fail-closed
    …
}
```
And symmetrically for `gale_spin_unlock_valid` / `gale_spin_lock_compute_owner`.

(c) **Tighten the Kani oracle** — drop `kani::assume(cpu_id < 4)` from the
"no_panic" harnesses (`spinlock_lock_valid_no_panic`,
`spinlock_unlock_valid_no_panic`, `spinlock_compute_owner_no_panic`) and
replace with a property that the FFI symbols *reject* inputs outside the
verified domain.

(d) **Remove the misleading "configurable" docstring** on
`src/spinlock_validate.rs:8-10, 27, 41` or turn `MAX_CPUS`/`CPU_MASK`/
`THREAD_ALIGN` into a single trait/const-generic with Verus proofs
parameterised over the bit width (follow-up work; the existing TODO at
lines 211–215 already notes that the `by(bit_vector)` proofs need
arch-parameterised treatment — same generalisation would help here).

## Candidate UCA
No existing UCA in `safety/stpa/ucas.yaml` indexes the spinlock *validator*
(only the high-level `SpinlockState`). Recommend opening
**SVAL-UCA-1**: "spinlock validator returns `valid=true` while the lock is
held by the current CPU" (STPA UCA-type 2: control action provided when
safety constraint is violated). File this finding against it with
`status: draft`, severity ASIL-D.

# Finding: device_init.rs — DI4 violated by failed-dep treated as satisfied

**File:** `/Users/r/git/pulseengine/z/gale/src/device_init.rs`
**Context:** ASIL-D early-boot sequencing, invariants not yet established.
**Claimed property:** DI4 — "all deps initialized before dependent" (line 30, 316).
**Prior class:** use-before-init returns OK / state-machine drift.

## Vulnerability

`DeviceInitState::check_deps_satisfied` (lines 317–344) determines
dependency satisfaction using only `devices[dep_id].initialized`
(line 334). It does **not** check `init_res == 0`.

However, `init_device` (lines 228–280) unconditionally sets
`dev.initialized = true` (line 270) *before* branching on `success`.
On init-function failure (`success == false`), the device is left in
the state:

```
initialized = true,
init_res    = 1   // non-zero -> error
```

`is_ready_spec` (line 158–160) correctly requires both
`initialized && init_res == 0`. So the same device is simultaneously:

- **Not ready** per `is_ready_spec` / `is_device_ready` (line 349),
- **A satisfied dependency** per `check_deps_satisfied`.

A dependent driver therefore passes the DI4 dependency gate, runs
its init against an unusable predecessor (e.g. clock, regulator,
flash, MPU), and is itself marked `initialized`. In an ASIL-D boot
flow this permits cascade-init against a broken chain: the boot
sequence reports OK while safety-critical subsystems silently
operate on uninitialized hardware state. This is exactly a
"use-before-init returns OK" pattern.

Compounding issues in the same module that make this reachable:

1. `init_device` never calls `check_deps_satisfied` — DI4 is a
   documented property but is not enforced in the mutator.
2. `advance_level` (288–311) does not require that all devices of
   the current level have been initialized before moving on —
   state-machine drift: `current_level` can skip ahead while
   lower-level devices are incomplete or failed.
3. On warm boot, `DeviceInitState::init` builds a fresh tracker
   with `num_initialized == 0`, but an existing `DeviceEntry` with
   stale `initialized = true` from the previous boot is not
   cleared here; the tracker and the entries can disagree. (Re-init
   on warm boot, per the prior list.)

## Oracle

### (1) Verus — falsifying spec post-condition

Add to `impl DeviceInitState`:

```rust
pub open spec fn dep_truly_ready(
    dev: &DeviceEntry, devices: &Seq<DeviceEntry>
) -> bool {
    forall |i: int| 0 <= i < dev.num_deps as int ==> {
        let d = devices[dev.deps[i] as int];
        d.initialized && d.init_res == 0
    }
}
```

Attempting to prove:

```rust
ensures result == true ==> dep_truly_ready(dev, devices@),
```

on `check_deps_satisfied` fails: Verus constructs a counter-model
with `devices[dep_id].initialized == true` and
`devices[dep_id].init_res == 1`, where the loop returns `true` but
`dep_truly_ready` is `false`.

### (2) Unit test (concrete trigger)

```rust
#[test]
fn di4_failed_dep_marked_satisfied() {
    let mut st = DeviceInitState::init(2).unwrap();
    let mut dep = DeviceEntry {
        id: 0, level: InitLevel::PreKernel1, priority: 0,
        num_deps: 0, deps: [0; 8],
        initialized: false, init_res: 0,
    };
    let mut dependent = DeviceEntry {
        id: 1, level: InitLevel::PreKernel1, priority: 1,
        num_deps: 1, deps: [0, 0, 0, 0, 0, 0, 0, 0],
        initialized: false, init_res: 0,
    };

    // Init dep with failure.
    let _ = st.init_device(&mut dep, /*success=*/ false);
    assert!(dep.initialized);     // side-effect
    assert_ne!(dep.init_res, 0);  // failed

    // is_device_ready correctly says NOT ready.
    assert!(!DeviceInitState::is_device_ready(&dep));

    // BUT check_deps_satisfied says the dep is satisfied.
    let table = [dep, dependent];
    assert!(DeviceInitState::check_deps_satisfied(&dependent, &table));
    // ^ This assertion passes today — that is the bug.
}
```

## Zephyr reference

Mirrors the semantics of `do_device_init` in
`kernel/device.c:18-61` and `z_impl_device_is_ready` in
`kernel/device.c:186-197`: Zephyr's C code distinguishes
`DEVICE_FLAG_INIT_RES`-encoded failure from readiness, and the
in-tree dependency traversal (`device_required_foreach`) checks
readiness, not a bare `initialized` flag. The Rust model diverges
from its own C source mapping (line 11–16 of the file).

Classification: **proof-code drift** — the documented DI4 property
is stricter than the Rust implementation, and the `init_device`
mutator encodes a two-bit state (initialized, init_res) while the
dep check only inspects one bit.

## Remediation sketch

- In `check_deps_satisfied` line 334, require
  `devices[dep_id as usize].initialized &&
   devices[dep_id as usize].init_res == 0`.
- In `init_device`, either call `check_deps_satisfied` as a
  precondition (ensures DI4 enforced, not merely asserted), or
  add `requires` on a `Seq<DeviceEntry>` dep table.
- In `advance_level`, add
  `requires self.num_initialized == /* devices at current_level */`
  or at minimum bump `current_level` only after the level's
  entry section is drained.
- On warm boot, clear `initialized` / `init_res` on each
  `DeviceEntry`, or reject `DeviceInitState::init` if any entry
  has stale `initialized == true`.

## Ranking inputs

- Severity: HIGH (ASIL-D, silent cascade init over broken dep).
- Exploitability: any driver init returning non-zero — common
  on hardware faults, brown-out, missing peripheral.
- Proof gap: drift between `is_ready_spec` (correct) and
  `check_deps_satisfied` (incomplete).
- Related Zephyr class: device readiness vs. init-result
  conflation, same family as historical Zephyr issues around
  `device_is_ready` semantics pre-3.x.

**status: draft**

# gust async executor (v1 static single-partition) — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Verus is the oracle: where a step says "verify", run the verifier and iterate with `pulseengine-claude:proof-synthesis` until it passes — do NOT hand-tune the plan's proof text and assume it verifies.

**Goal:** A verified, `no_std`, allocation-free cooperative-async **scheduler core** — a fixed-priority ready-queue + tickless deadline (timer) queue over a static N-task table — that drives the gust:os inner async layer inside one partition window, with the three load-bearing properties (`no-lost-wakeups`, `bounded-poll`, `fair-ready-queue`) proven in Verus.

**Architecture:** The **verified core is scalar** — a task table, a ready-set bitmask, a deadline array, and pure selection/wake/expire functions over them (proven in Verus). The **async task bodies themselves are NOT in the verified core** (CLAUDE.md intersection rule: no trait objects / closures / async in verified code) — the executor exposes a trusted `poll_task(id)` FFI seam that dispatches into the (unverified) app state machines, exactly as the thin-seam drivers keep register pokes out of the verified FSM. This mirrors `src/priority.rs` and the driver pattern. v1 is a **single component** (no `meld --memory shared` fuse) so it dissolves clean today and is NOT blocked on synth#739.

**Tech Stack:** Rust + Verus (`verus!` macro, `src/*.rs` family), Kani (bit-level sanity), wit-bindgen (gust:os seam), the dissolve chain (loom → synth `--target cortex-m3 --relocatable`), qemu-cortex-m3 probe, rivet (typed traceability).

## Global Constraints

- **no_std, allocation-free.** All state is `static` fixed-size arrays; no heap, no `alloc`. (Matches every gust:os provider and the driver roster.)
- **Verified-code intersection.** The verified core uses NO trait objects, closures, or async. Task bodies are driven through a trusted scalar FFI (`poll_task(id) -> u32`), not stored as `dyn`/closures in the verified core.
- **Build on `src/priority.rs`.** Reuse the existing verified `Priority` type (lower value = higher priority; `is_higher_than`, `lemma_priority_total_order`, `lemma_priority_transitive`). Do not reinvent priority ordering.
- **Verus is the gate.** Proofs run under `.github/workflows/formal-verification.yml`. A task is not done until its Verus obligations verify (0 errors) in that harness.
- **Static task set, single partition (v1).** `MAX_TASKS = 8` (matches the current `spawn-provider` placeholder). No dynamic spawn, no multi-partition preemption — those are v2 and ride the meld/synth fixes (synth#739). v1 is a single-component dissolve.
- **Tickless inner, bounded by the outer window.** The executor is **deadline-driven** (a timer queue + `next_deadline()`), NOT a periodic-tick poll loop — it computes the next wakeup and idles between. The design assumption is that the *outer* partition scheduler's non-maskable window-end preemption bounds any `next_deadline()` sleep; the executor never itself blocks past its window. (Recorded in the spec's Tick Policy; see `docs/superpowers/specs/2026-07-11-gust-partition-scheduler-design.md`.)
- **Handle/return ABI (verbatim, matches `gust:os/spawn`):** `start(entry: u32) -> u32` (task handle, or `0xFFFF_FFFF` invalid); `poll(handle: u32) -> u32` (`0` = pending, `1` = done, `0xFFFF_FFFF` = invalid). The v1 executor keeps this ABI so it drops in for the placeholder provider.

---

### Task 1: Executor module scaffold + static task table with representation invariant

**Files:**
- Create: `src/executor.rs`
- Modify: `src/lib.rs` (add `pub mod executor;`)
- Test: proofs live inline in `src/executor.rs` under `verus!{}`, verified by the Verus job.

**Interfaces:**
- Produces: `pub const MAX_TASKS: usize = 8;`; `pub enum TaskState { Free, Pending, Done }`; `pub struct Tasks { state: [TaskState; MAX_TASKS], prio: [u32; MAX_TASKS], deadline: [u64; MAX_TASKS], ready: u32 }`; `spec fn inv(&self) -> bool` (the representation invariant); `pub fn new() -> Tasks` (`ensures result.inv()`).

- [ ] **Step 1: Write the module skeleton + invariant (the failing proof)**

```rust
// src/executor.rs
//! gust async executor (v1) — a verified fixed-priority + tickless-deadline
//! scheduler core over a static task table. Scalar-only (no async/closures in the
//! verified core); task bodies run through the trusted `poll_task` seam (Task 5).
//! Builds on `crate::priority::Priority`. Single-component dissolve (not meld-fused),
//! so it is not blocked on synth#739.
#![cfg_attr(not(test), no_std)]
use vstd::prelude::*;

verus! {

pub const MAX_TASKS: usize = 8;

#[derive(PartialEq, Eq, Structural)]
pub enum TaskState { Free, Pending, Done }

pub struct Tasks {
    pub state: [TaskState; MAX_TASKS],
    pub prio:  [u32; MAX_TASKS],       // lower = higher priority (Priority convention)
    pub deadline: [u64; MAX_TASKS],    // wake-by tick; u64::MAX = no timer
    pub ready: u32,                    // bit i set => task i wants to run
}

impl Tasks {
    /// Representation invariant: `ready` bits only ever set for Pending slots,
    /// and only within [0, MAX_TASKS). This is the anchor every proof rests on.
    pub open spec fn inv(&self) -> bool {
        forall|i: int| 0 <= i < MAX_TASKS ==>
            (#[trigger] self.ready_bit(i)) ==> self.state[i] == TaskState::Pending
    }

    /// Ghost: is bit i of `ready` set?
    pub open spec fn ready_bit(&self, i: int) -> bool {
        i >= 0 && i < MAX_TASKS && ((self.ready >> (i as u32)) & 1u32) == 1u32
    }

    pub fn new() -> (r: Tasks)
        ensures r.inv(), r.ready == 0u32,
    {
        Tasks {
            state: [TaskState::Free, TaskState::Free, TaskState::Free, TaskState::Free,
                    TaskState::Free, TaskState::Free, TaskState::Free, TaskState::Free],
            prio: [0; MAX_TASKS],
            deadline: [u64::MAX; MAX_TASKS],
            ready: 0u32,
        }
    }
}

} // verus!
```

- [ ] **Step 2: Wire the module + run the verifier to see it accepted (baseline green)**

Modify `src/lib.rs`: add `pub mod executor;` next to `pub mod priority;`.

Run the Verus job locally (same invocation as `formal-verification.yml` — inspect that file for the exact command; typically `cargo verus verify` or the vendored `verus` on `src/executor.rs`).
Expected: **0 verification errors** for `executor.rs` (only `new` + the spec fns exist so far). If `Structural`/`forall`/trigger complaints appear, iterate with proof-synthesis until clean.

- [ ] **Step 3: Commit**

```bash
git add src/executor.rs src/lib.rs
git commit -m "feat(executor): scaffold verified task table + representation invariant"
```

---

### Task 2: wake / is_ready / consume — the `no-lost-wakeups` property

**Files:**
- Modify: `src/executor.rs` (add methods inside the `verus!` block)

**Interfaces:**
- Consumes: `Tasks`, `inv`, `ready_bit` (Task 1).
- Produces: `fn admit(&mut self, prio: u32) -> u32` (Free→Pending, returns handle or `0xFFFF_FFFF`); `fn wake(&mut self, h: u32)` (set ready-bit iff Pending); `fn is_ready(&self, h: u32) -> bool`; `fn consume(&mut self, h: u32)` (clear ready-bit as a task is about to be polled). Key `ensures`: **`wake` then no intervening `consume` on `h` ⇒ `is_ready(h)`** and **`ready` bits are never set for non-Pending slots** (`inv` preserved).

- [ ] **Step 1: Write `admit`/`wake`/`is_ready`/`consume` with their contracts**

```rust
// inside verus! { impl Tasks { ... } }

pub fn admit(&mut self, prio: u32) -> (h: u32)
    requires old(self).inv(),
    ensures self.inv(),
        // a fresh handle is Pending and not yet ready
        h < MAX_TASKS as u32 ==> self.state[h as int] == TaskState::Pending && !self.ready_bit(h as int),
{
    let mut i = 0;
    while i < MAX_TASKS
        invariant self.inv(), 0 <= i <= MAX_TASKS,
    {
        if self.state[i] == TaskState::Free {
            self.state.set(i, TaskState::Pending);
            self.prio.set(i, prio);
            // clearing the ready bit for a just-admitted slot keeps inv trivially
            self.ready = self.ready & !(1u32 << (i as u32));
            return i as u32;
        }
        i += 1;
    }
    0xFFFF_FFFFu32
}

/// THE no-lost-wakeups primitive: waking a Pending task sets its ready bit and it
/// stays set until `consume`. Waking a non-Pending handle is a no-op (inv-safe).
pub fn wake(&mut self, h: u32)
    requires old(self).inv(),
    ensures self.inv(),
        (h < MAX_TASKS as u32 && self.state[h as int] == TaskState::Pending) ==> self.ready_bit(h as int),
        // no other task's readiness changes
        forall|j: int| 0 <= j < MAX_TASKS && j != h as int ==>
            self.ready_bit(j) == old(self).ready_bit(j),
{
    if h < MAX_TASKS as u32 && self.state[h as int] == TaskState::Pending {
        self.ready = self.ready | (1u32 << h);
    }
}

pub fn is_ready(&self, h: u32) -> (b: bool)
    requires self.inv(), h < MAX_TASKS as u32,
    ensures b == self.ready_bit(h as int),
{ ((self.ready >> h) & 1u32) == 1u32 }

pub fn consume(&mut self, h: u32)
    requires old(self).inv(), h < MAX_TASKS as u32,
    ensures self.inv(), !self.ready_bit(h as int),
        forall|j: int| 0 <= j < MAX_TASKS && j != h as int ==>
            self.ready_bit(j) == old(self).ready_bit(j),
{ self.ready = self.ready & !(1u32 << h); }
```

- [ ] **Step 2: Verify — the bit-vector reasoning is the hard part**

Run the Verus job on `src/executor.rs`.
Expected: PASS. The `ready` shift/mask facts need bit-vector reasoning — if `wake`/`consume`/`is_ready` obligations don't discharge, add `assert(...) by(bit_vector)` lemmas relating `(x | 1<<h)`, `(x & !(1<<h))`, and `(x >> h) & 1` for `h < 32`. Iterate with proof-synthesis. (These are exactly the QF_BV leaves ordeal certifies — see `proofs/ordeal-bv/`.)

- [ ] **Step 3: Add an explicit no-lost-wakeups proof harness**

```rust
// inside verus! {}
/// no-lost-wakeups: admit → wake → (any consume of a DIFFERENT handle) ⇒ still ready.
pub proof fn lemma_no_lost_wakeup(t0: Tasks, h: u32, other: u32)
    requires t0.inv(), h < MAX_TASKS as u32, other < MAX_TASKS as u32, other != h,
             t0.state[h as int] == TaskState::Pending, t0.ready_bit(h as int),
    ensures true,  // replaced by: a t1 with consume(other) still has ready_bit(h)
{ /* iterate: thread the state through consume(other); assert ready_bit(h) unchanged */ }
```

Run Verus; iterate until the lemma states and proves "consuming another handle never clears `h`'s readiness". Expected: PASS.

- [ ] **Step 4: Commit**

```bash
git add src/executor.rs
git commit -m "feat(executor): admit/wake/consume — no-lost-wakeups proven (Verus)"
```

---

### Task 3: pick_next — highest-priority ready, work-conserving (`fair-ready-queue`)

**Files:**
- Modify: `src/executor.rs`

**Interfaces:**
- Consumes: `Tasks`, `inv`, `ready_bit`, `crate::priority::Priority::is_higher_than`.
- Produces: `fn pick_next(&self) -> u32` — returns the handle of the **highest-priority ready** task (lowest `prio` value; lowest index breaks ties), or `MAX_TASKS as u32` if none ready. `ensures`: **result is ready**, and **no ready task has strictly-higher priority** than the result (work-conserving + fairness).

- [ ] **Step 1: Write `pick_next` with the selection contract**

```rust
// inside verus! { impl Tasks { ... } }
pub fn pick_next(&self) -> (h: u32)
    requires self.inv(),
    ensures
        // either nothing is ready, or h is a valid ready task...
        (h == MAX_TASKS as u32 && forall|i: int| 0 <= i < MAX_TASKS ==> !self.ready_bit(i))
        || (h < MAX_TASKS as u32 && self.ready_bit(h as int)
            // ...and no ready task outranks it (lower prio value = higher priority)
            && forall|j: int| 0 <= j < MAX_TASKS && self.ready_bit(j) ==>
                   self.prio[h as int] <= self.prio[j]),
{
    let mut best: u32 = MAX_TASKS as u32;
    let mut i: u32 = 0;
    while i < MAX_TASKS as u32
        invariant self.inv(), 0 <= i <= MAX_TASKS as u32,
            best == MAX_TASKS as u32
                || (best < i && self.ready_bit(best as int)
                    && forall|j: int| 0 <= j < i && self.ready_bit(j) ==> self.prio[best as int] <= self.prio[j]),
    {
        if ((self.ready >> i) & 1u32) == 1u32 {
            if best == MAX_TASKS as u32 || self.prio[i as int] < self.prio[best as int] {
                best = i;
            }
        }
        i += 1;
    }
    best
}
```

- [ ] **Step 2: Verify the selection invariant**

Run Verus. Expected: PASS. The loop invariant carries "best is the min-prio ready task seen so far"; the final ensures follows at `i == MAX_TASKS`. If the invariant is too weak, strengthen the `forall j < i` clause. Iterate with proof-synthesis.

- [ ] **Step 3: Kani sanity — pick_next agrees with a brute-force reference**

```rust
// src/executor.rs, #[cfg(kani)] mod exec_kani {
#[kani::proof]
fn pick_next_is_min_prio_ready() {
    let t = arbitrary_tasks();               // kani::any state/prio/ready, filtered to inv()
    let h = t.pick_next_exec();              // the non-verus executable wrapper
    // brute force: no ready j with strictly lower prio than h
    for j in 0..MAX_TASKS { if t.is_ready_exec(j as u32) && t.prio[j] < t.prio_of(h) { assert!(false); } }
}
```

Run: `cargo kani --harness pick_next_is_min_prio_ready`
Expected: VERIFICATION SUCCESSFUL. (Cross-checks the Verus contract with an independent oracle.)

- [ ] **Step 4: Commit**

```bash
git add src/executor.rs
git commit -m "feat(executor): pick_next highest-priority-ready — work-conserving + fair (Verus + Kani)"
```

---

### Task 4: Tickless deadline queue — next_deadline / expire

**Files:**
- Modify: `src/executor.rs`

**Interfaces:**
- Consumes: `Tasks`, `inv`, `wake`.
- Produces: `fn next_deadline(&self) -> u64` (min `deadline` over Pending tasks, or `u64::MAX` if none — the value the outer layer/HW timer arms a one-shot alarm for); `fn expire(&mut self, now: u64)` (wake every Pending task whose `deadline <= now`). `ensures` for `expire`: **every Pending task with `deadline <= now` is ready afterwards** (the tickless correctness property), `inv` preserved.

- [ ] **Step 1: Write `next_deadline` + `expire` with contracts**

```rust
// inside verus! { impl Tasks { ... } }
pub fn next_deadline(&self) -> (d: u64)
    requires self.inv(),
    ensures forall|i: int| 0 <= i < MAX_TASKS && self.state[i] == TaskState::Pending ==> d <= self.deadline[i],
{
    let mut d: u64 = u64::MAX;
    let mut i = 0;
    while i < MAX_TASKS
        invariant 0 <= i <= MAX_TASKS,
            forall|k: int| 0 <= k < i && self.state[k] == TaskState::Pending ==> d <= self.deadline[k],
    {
        if self.state[i] == TaskState::Pending && self.deadline[i] < d { d = self.deadline[i]; }
        i += 1;
    }
    d
}

/// Tickless expiry: on the one-shot alarm firing at `now`, mark every Pending task
/// whose deadline has passed as ready. No periodic tick — this runs only when
/// `now >= next_deadline()`.
pub fn expire(&mut self, now: u64)
    requires old(self).inv(),
    ensures self.inv(),
        forall|i: int| 0 <= i < MAX_TASKS
            && self.state[i] == TaskState::Pending && self.deadline[i] <= now ==> self.ready_bit(i),
{
    let mut i: u32 = 0;
    while i < MAX_TASKS as u32
        invariant self.inv(), 0 <= i <= MAX_TASKS as u32,
            forall|k: int| 0 <= k < i && self.state[k] == TaskState::Pending && self.deadline[k] <= now ==> self.ready_bit(k),
    {
        if self.state[i as int] == TaskState::Pending && self.deadline[i as int] <= now {
            self.ready = self.ready | (1u32 << i);
        }
        i += 1;
    }
}
```

- [ ] **Step 2: Verify**

Run Verus. Expected: PASS. The `expire` loop reuses the `wake` bit-vector lemmas from Task 2 for `ready | (1<<i)`. Iterate with proof-synthesis if the bit-set fact or the `forall k < i` invariant needs help.

- [ ] **Step 3: Commit**

```bash
git add src/executor.rs
git commit -m "feat(executor): tickless deadline queue — next_deadline + expire (Verus)"
```

---

### Task 5: poll_round — bounded, terminating, drives the trusted poll seam

**Files:**
- Modify: `src/executor.rs`

**Interfaces:**
- Consumes: everything above.
- Produces: `pub fn poll_round(&mut self)` — one scheduler round: repeatedly `pick_next`, `consume` it, dispatch it, until no task is ready. `ensures`: **terminates** and **no task is ready afterwards** (`self.ready == 0`), each ready task consumed exactly once. The dispatch itself is the trusted seam `extern "C" { fn poll_task(id: u32) -> u32; }` (declared `#[verifier::external]`), whose result maps Pending→Done. `poll_task` is NOT verified — it dispatches into the app async state machine (the intersection boundary).

- [ ] **Step 1: Declare the trusted seam + write `poll_round` with a decreases clause**

```rust
// src/executor.rs — OUTSIDE verus! (trusted FFI seam; the app's async body)
extern "C" { pub fn poll_task(id: u32) -> u32; }   // 0 = still pending, 1 = done

// inside verus! { impl Tasks { ... } }
// popcount of `ready` strictly decreases each iteration (consume clears one bit),
// so the round terminates. Verus proves termination via `decreases`.
pub fn poll_round(&mut self, dispatch: impl Fn(u32) -> u32)   // dispatch = wrapper over poll_task
    requires old(self).inv(),
    ensures self.inv(), self.ready == 0u32,
    decreases self.ready via Self::ready_popcount
{
    loop
        invariant self.inv(),
        decreases self.ready via Self::ready_popcount
    {
        let h = self.pick_next();
        if h == MAX_TASKS as u32 { return; }   // nothing ready → round done
        self.consume(h);                       // clears exactly h's bit (popcount--)
        let done = dispatch(h);                // trusted: runs the async task body once
        if done == 1u32 { self.state.set(h as int, TaskState::Done); }
    }
}
```

> Note on the intersection rule: `poll_round` takes `dispatch: impl Fn` **only in the executable wrapper**; the *verified* core reasons about the scalar state transitions (`pick_next`/`consume`/state-set), and `dispatch`'s effect is modeled as an opaque `Pending→{Pending,Done}` on one slot. If Verus balks at the closure, split: a `#[verifier::external_body]` `fn poll_round_exec(&mut self)` that calls `poll_task`, wrapping a fully-verified `fn step(&mut self, h) ` that does pick/consume/state-set. Keep the *proof* on `step` + a `decreases` on the loop.

- [ ] **Step 2: Prove termination (`ready_popcount` decreases) + ready==0 postcondition**

Add `spec fn ready_popcount(&self) -> nat` (Hamming weight of `ready`) and a lemma that `consume(h)` on a set bit strictly decreases it. Run Verus.
Expected: PASS — the loop terminates (popcount ↓) and exits only when `pick_next == MAX_TASKS` (⇒ `ready == 0`, from Task 3's ensures). This is **bounded-poll**: at most `popcount(ready) ≤ MAX_TASKS` dispatches per round, each ready task at most once. Iterate with proof-synthesis on the popcount lemma (a QF_BV fact).

- [ ] **Step 3: Kani — a round drains all ready tasks and each is polled ≤ once**

```rust
#[kani::proof]
fn poll_round_drains_and_bounds() {
    let mut t = arbitrary_tasks();
    let before = popcount(t.ready);
    let calls = t.poll_round_counted();   // exec wrapper counting dispatch calls
    assert!(t.ready == 0);
    assert!(calls <= before);             // bounded: ≤ one poll per ready task
}
```

Run: `cargo kani --harness poll_round_drains_and_bounds`
Expected: VERIFICATION SUCCESSFUL.

- [ ] **Step 4: Commit**

```bash
git add src/executor.rs
git commit -m "feat(executor): poll_round — bounded + terminating (Verus decreases + Kani)"
```

---

### Task 6: gust:os/spawn provider replacement + single-component dissolve + probe + rivet

**Files:**
- Modify: `benches/gust/drivers/spawn-provider/src/lib.rs` (back the placeholder with `crate::executor` logic via a scalar packed-state ABI, OR re-export a thin wrapper — keep the `start`/`poll` WIT ABI byte-identical)
- Create: `benches/gust/src/bin/gust_exec_probe.rs` (qemu-cortex-m3 liveness: admit 3 tasks at distinct priorities + deadlines, drive `poll_round`, assert highest-priority-ready ran first and all reach Done)
- Modify: `benches/gust/build.rs` (link the dissolved `exec-cm3.o` for `gust_exec_probe`, guarded by `.exists()` like the other os-node objects)
- Create: `artifacts/requirements/REQ-OS-EXEC-001.yaml`, `artifacts/tests/VER-OS-EXEC-001.yaml` (rivet: the executor requirement + its verification artifact, linked `verifies`)

**Interfaces:**
- Consumes: the whole verified `executor` module.
- Produces: a dissolved single-component object `exec-cm3.o` (no meld fuse → not synth#739-blocked); a green qemu probe; a closed rivet trace `REQ-OS-EXEC-001 → executor.rs → VER-OS-EXEC-001`.

- [ ] **Step 1: rivet artifact leads (feature-loop discipline) — write the requirement + test first**

```yaml
# artifacts/requirements/REQ-OS-EXEC-001.yaml
id: REQ-OS-EXEC-001
title: gust:os cooperative async executor (v1, static single-partition)
text: >
  The gust:os inner async layer schedules a static set of up to MAX_TASKS cooperative
  tasks by fixed priority with a tickless deadline queue, guaranteeing no-lost-wakeups,
  bounded-poll (each ready task polled at most once per round; round terminates), and a
  fair, work-conserving ready-queue (highest-priority ready runs; never idle while ready).
status: implemented
release: v0.4.0
```

```yaml
# artifacts/tests/VER-OS-EXEC-001.yaml
id: VER-OS-EXEC-001
title: Verus proofs + Kani + qemu probe for the async executor
verifies: [REQ-OS-EXEC-001]
evidence:
  - src/executor.rs (Verus: no-lost-wakeups, pick_next fair/work-conserving, poll_round bounded+terminating, expire)
  - cargo kani (pick_next_is_min_prio_ready, poll_round_drains_and_bounds)
  - benches/gust/src/bin/gust_exec_probe.rs (qemu-cortex-m3 liveness)
status: verified
```

Run: `rivet validate` then `rivet check`. Expected: both PASS (trace topology closes). Fix any schema/link errors before proceeding.

- [ ] **Step 2: Write the qemu probe (the executable liveness gate)**

```rust
// benches/gust/src/bin/gust_exec_probe.rs
#![no_std] #![no_main]
use cortex_m_rt::entry;
use cortex_m_semihosting::{debug, hprintln};
use panic_halt as _;
extern "C" { fn exec_admit(prio: u32, deadline: u64) -> u32; fn exec_poll_round(now: u64); fn exec_state(h: u32) -> u32; }
// task bodies (trusted seam): all complete on first poll for this liveness check
#[no_mangle] pub extern "C" fn poll_task(_id: u32) -> u32 { 1 }
#[entry] fn main() -> ! {
    let hi = unsafe { exec_admit(1, 0) };   // higher priority (lower value), due now
    let lo = unsafe { exec_admit(9, 0) };
    unsafe { exec_poll_round(0) };
    let ok = unsafe { exec_state(hi) == 2 /*Done*/ && exec_state(lo) == 2 };
    if ok && hi != 0xFFFF_FFFF && lo != 0xFFFF_FFFF {
        hprintln!("gust-exec-probe OK: both tasks Done, hi-prio first");
        debug::exit(debug::EXIT_SUCCESS);
    } else { hprintln!("gust-exec-probe FAIL"); debug::exit(debug::EXIT_FAILURE); }
    loop {}
}
```

Add the export shims (`exec_admit`/`exec_poll_round`/`exec_state`) to a small `#[no_mangle]` C-ABI surface in the dissolved crate.

- [ ] **Step 3: Dissolve the executor as a SINGLE component + wire build.rs**

Build the executor crate to wasm, then dissolve (single component — no `wac plug`, no `meld fuse`, so **not** synth#739-blocked): `loom optimize --passes inline <exec.wasm> | synth compile --target cortex-m3 --all-exports --relocatable -o benches/gust/drivers/os-node/exec-cm3.o`. Add to `build.rs`:

```rust
let eobj = Path::new(&manifest).join("drivers/os-node/exec-cm3.o");
if eobj.exists() {
    println!("cargo:rustc-link-arg-bin=gust_exec_probe={}", eobj.display());
    println!("cargo:rerun-if-changed={}", eobj.display());
}
```

- [ ] **Step 4: Run the probe (the oracle) + confirm 0-SRAM-class dissolve**

Run: `cd benches/gust && cargo run --release --bin gust_exec_probe`
Expected: `gust-exec-probe OK: both tasks Done, hi-prio first`, semihosting EXIT_SUCCESS.
Run: `arm-none-eabi-size benches/gust/drivers/os-node/exec-cm3.o` — record text/data/bss (static tables → small bounded .bss, fits the budget).

- [ ] **Step 5: Point the spawn provider at the real executor**

Replace `spawn-provider/src/lib.rs`'s placeholder `DONE[8]`/`NEXT` logic so `start`→`executor::admit`, `poll`→drives `poll_round` + returns Done/Pending — keeping the `start(entry)->u32` / `poll(handle)->u32` ABI byte-identical (Global Constraints). Re-run any existing spawn probe to confirm no ABI regression.

- [ ] **Step 6: Commit**

```bash
git add src/executor.rs benches/gust/ artifacts/requirements/REQ-OS-EXEC-001.yaml artifacts/tests/VER-OS-EXEC-001.yaml
git commit -m "feat(gust): v1 async executor — dissolved single-component, qemu-probed, rivet VER-OS-EXEC-001 (REQ-OS-EXEC-001)"
```

---

## Self-Review

**Spec coverage** (`docs/superpowers/specs/2026-07-11-gust-partition-scheduler-design.md` §7 executor + §Tick Policy):
- `no-lost-wakeups` → Task 2 (proof + lemma). ✓
- `bounded-poll` → Task 5 (decreases + Kani count). ✓
- `fair-ready-queue` (highest-ready runs, work-conserving) → Task 3 (Verus ensures + Kani). ✓
- tickless/deadline-driven (Tick Policy) → Task 4 (`next_deadline`/`expire`, no periodic tick). ✓
- v1 static single-partition, single-component dissolve (not synth#739-blocked) → Global Constraints + Task 6. ✓
- Intersection rule (no async/closures in verified core; trusted `poll_task` seam) → Global Constraints + Task 5 note. ✓
- rivet-led trace → Task 6 Step 1. ✓
- **Out of scope (v2, correctly deferred):** the outer preemptive partition switch, multi-partition, dynamic spawn, and the *fused* multi-provider node (that one rides synth#739). This plan is the *inner* executor only.

**Placeholder scan:** the Verus/Kani proof *bodies* are intentionally marked "iterate with the verifier" — that is the honest granularity (per proof-synthesis, the verifier is the oracle; pre-baked proof text is not trustworthy). Every executable step has concrete code, exact paths, and a runnable command with expected output.

**Type consistency:** `Tasks`, `TaskState{Free,Pending,Done}`, `ready:u32`, `ready_bit`, `admit`/`wake`/`is_ready`/`consume`/`pick_next`/`next_deadline`/`expire`/`poll_round`, `MAX_TASKS=8`, and the `start`/`poll` ABI are used consistently across tasks and match `src/priority.rs` (`Priority`, lower=higher) and the `gust:os/spawn` ABI.

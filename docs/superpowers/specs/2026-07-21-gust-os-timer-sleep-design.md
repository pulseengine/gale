# gust:os timer-sleep capability — design

**Status:** approved (user 2026-07-21). **Owner:** gale/gust. **Anchors:** REQ-OS-SYSCALL-001 (the gust:os seam), REQ-OS-EXEC-001 (the verified executor), the "async from kiln" thread.

## Goal

Let a dissolved component sleep for a real interval **efficiently** — tickless, no busy-poll — over the verified executor. Today the `gust:os/time` seam (`now`/`deadline`/`elapsed`) lets a component *check* time but not *register a wake*, so the only way to wait is to spin (returned Pending, re-polled every `poll_round`). And `now()` is raw HW ticks with no unit, so "one second" isn't expressible. This capability closes both gaps by **surfacing machinery that already exists and is verified** — the executor's per-task `deadline[]` + `next_deadline()` + `expire(now)` (no-lost-wakeups + bounded-poll, Verus+Kani proven, merged #189/executor).

## Resolved decisions (the design)

1. **Split seam, not folded.** `time` stays pure-mmio/0-SRAM; a **new `timer` interface** is executor-backed. Rationale: `time` is the only 0-SRAM capability (`now()` is a bare register read); folding `sleep` into it would force the time provider to reach into the executor and silently kill that property. Keep each capability single-backed — `time`=mmio, `spawn`=executor, `timer`=executor.
2. **Handle-based**, mirroring the shipped/verified `spawn` pattern: `sleep(ticks)->handle`, `slept(handle)->status`. No `pollable`/resource/`own<>` machinery — scalar, 0-SRAM on the timer side too.
3. **Units via `resolution()`** on `time` (ticks/sec = the timer Hz). `now()` stays raw; the component converts once (`sleep(resolution())` = 1s). WASI monotonic-clock shape; no hot-path multiply.
4. **One new verified-core function:** `set_deadline(h, d)` (set an already-Pending task's wake deadline — the `deadline[]` field the executor carries but never surfaces). Everything downstream (`next_deadline`, `expire`, the no-lost-wakeups/bounded-poll lemmas) is reused unchanged. *(This supersedes the `admit_timer` framing from the pitch — same idea, minimal verified setter for the deadline — but `sleep` operates on the already-admitted **calling** task, so a setter is simpler and more faithful than admitting a fresh slot.)*

## WIT changes (`benches/gust/drivers/wit-os/gust-os.wit`)

`time` gains one function (mmio provider, still 0-SRAM):
```wit
interface time {
    now: func() -> u64;
    deadline: func(now: u64, ticks: u64) -> u64;
    elapsed: func(now: u64, deadline: u64) -> bool;
    resolution: func() -> u64;                     // NEW: ticks per second (timer Hz)
}
```

New interface (executor-backed):
```wit
/// Tickless one-shot timers over the verified executor's deadline table. A
/// component registers a wake and polls it — no host Future, no busy-poll.
interface timer {
    /// Arm a one-shot wake `ticks` from now on task `handle` (held from spawn.start;
    /// `ticks < 2^31`). Returns 0 on success, 0xFFFF_FFFF if invalid/not-Pending/out-of-range.
    /// (User decision 2026-07-21: explicit handle — the cooperative executor has no ambient
    /// current task, so sleep targets the handle the app already holds.)
    sleep: func(handle: u32, ticks: u32) -> u32;
    /// Poll a timer handle: 0 = pending, 1 = elapsed, 0xFFFF_FFFF = invalid.
    slept: func(handle: u32) -> u32;
}
```

New worlds (mirroring `spawn`):
```wit
world app-timer { import time; import timer; export run: func() -> u32; }
world timer-provider { import taskdisp; import gust:hal/mmio@0.1.0; export timer; }
```
The full `app` world gains `import timer;`.

## Verified-core change (`src/executor.rs` + `plain/` mirror)

One new function — a minimal setter for the per-task deadline the executor already tracks:
```rust
/// Set the wake-by deadline of an already-admitted Pending task. This is the
/// only surface that lets a caller register a timed wake into the tickless path.
pub fn set_deadline(&mut self, h: u32, d: u64)
    requires old(self).inv(),
    ensures
        self.inv(),
        (h < MAX_TASKS as u32 && old(self).state[h as int] === TaskState::Pending)
            ==> self.deadline[h as int] == d,
        // touches only deadline[h]: state, ready, and every other deadline unchanged
        self.state === old(self).state,
        self.ready == old(self).ready,
        forall|j: int| 0 <= j < MAX_TASKS && j != h as int
            ==> self.deadline[j as int] == old(self).deadline[j as int],
```
`inv()` constrains `ready ↔ Pending` and `ready < 256`; it does **not** constrain `deadline`, so writing any `u64` deadline preserves it — the proof is a single guarded array write. **No change** to `next_deadline`/`expire`/`poll_round` or their lemmas. Kani: `set_deadline_sets_only_h` (`kani::any` h/d → `deadline[h]==d`, all else unchanged, `inv()` holds).

`slept(handle)` is a pure recompute: `elapsed(now(), deadline[handle])` for an in-range Pending/ready handle → 0/1, out-of-range → 0xFFFF_FFFF — a small verified accessor, no new state.

## The mechanism (data flow)

1. Component (mid-poll of its task, handle `h`): `let t = timer.sleep(one_sec);` where `one_sec = time.resolution()`.
2. `timer` provider (knows the **calling task** `h` — kiln is dispatching it): `d = deadline(now(), ticks)`; `executor.set_deadline(h, d)`; return `h` as the timer token. The task then returns Pending.
3. `next_deadline()` (unchanged, verified) now includes `d` → the outer layer arms a **one-shot HW alarm** at the soonest deadline; CPU WFIs. No periodic tick, no re-poll spin (the task is Pending, not ready).
4. Alarm fires at `now`: `expire(now)` (unchanged, verified — inherits no-lost-wakeups) re-readies every task whose `deadline <= now`.
5. The task is re-polled; `timer.slept(h)` → 1 (`elapsed`); it proceeds. (A still-early spurious poll returns 0 and the task re-Pends — idempotent.)

## Provider (`drivers/timer-provider/`)

Mirrors `spawn-provider`: imports the executor `taskdisp`-class seam (extended with a `set-deadline` FFI + a `timer-now`/state read) + `gust:hal/mmio` (for `now()`), exports `timer`. Dissolves like the other providers (component new → wac plug → meld fuse --memory shared → loom → synth). The trusted seam adds `set_deadline` alongside `poll_task` — same TCB class (the executor dispatch already crosses `taskdisp`), no new TCB *kind*.

## Bounded edge (called out, v1 scope)

The HW one-shot alarm is 32-bit; a `sleep` longer than one timer wrap needs the outer arm-logic to re-arm across wraps. **v1 bounds `sleep(ticks)` to `ticks < 2^31`** (< one wrap, documented + asserted at the seam) — a multi-wrap re-arming timer is a follow-on. Deadline/`elapsed` math is already wrap-safe over the 32-bit domain widened to u64, so a within-bound sleep is correct across a single counter wrap.

## Oracle gates (per change)

- **Kani:** `set_deadline_sets_only_h` (deadline[h]==d, all else unchanged, `inv()`); the accessor for `slept` (state → status total).
- **Verus:** `//:verus_test` re-verifies with `set_deadline` added (obligation count rises by the new function; `next_deadline`/`expire` proofs unchanged). verus-strip gate 2/2 (regenerate `plain/`).
- **qemu demonstrator** `benches/gust/src/bin/gust_timer_probe.rs`: `sleep(resolution())`; assert `slept(h)==0` before the deadline, drive the executor `expire(now)` at the deadline, assert `slept(h)==1` after; **tickless assertion** — exactly ONE `next_deadline()` alarm value is armed and the sleeper is NOT re-polled before expiry (no spin). Non-vacuity: a not-yet-due handle stays 0; an invalid handle → 0xFFFF_FFFF.
- **SRAM budget:** the timer path adds no `.bss` beyond the executor's existing `deadline[]` (already counted); `nm` TCB-atom count unchanged in *kind* (set_deadline is the same dispatch class as poll_task).
- **`rivet validate`** PASS.

## rivet artifacts (lead the code)

- **REQ-OS-TIMER-001** (sw-req, release v0.4.x/next): "A component SHALL express a bounded sleep interval over the verified executor's tickless deadline path — register a wake and poll it, no busy-poll — with a stated time unit (`resolution()`). `sleep(ticks)` (ticks < 2^31) registers a one-shot wake; `slept(handle)` reports pending/elapsed/invalid; the wake is delivered through the executor's proven no-lost-wakeups `expire`, not a periodic tick." `related-to` REQ-OS-SYSCALL-001, REQ-OS-EXEC-001.
- **VER-OS-TIMER-001** (sw-verification): Verus/Kani on `set_deadline` (only deadline[h] set, `inv()` preserved) + the reused no-lost-wakeups/bounded-poll proofs compose over timer entries; the qemu `gust_timer_probe` demonstrator (tickless-arm + expire-wake + non-vacuity). Kill-criterion: a Verus/Kani harness fails, a due timer is not readied by `expire`, or the demonstrator re-polls the sleeper before its deadline (spin, not tickless). `verifies` REQ-OS-TIMER-001.

## Honest scope / non-goals (v1)

- **v1 = one partition / one executor** (the inner cooperative layer). Multi-partition timer accounting is the outer scheduler's concern (v0.6 switch), out of scope here.
- **No real `.await` syntax** — the component polls the handle (like `spawn`). A kiln timer-future that `.await`s this handle is the ergonomic layer on top, tracked with the async-executor epic, not this spec.
- `sleep >= 2^31 ticks` (multi-wrap) is a follow-on.
- Sub-tick resolution is out of scope — resolution is the timer's Hz.
- Demonstrator is qemu logic, not silicon.

## Files

- **Modify:** `benches/gust/drivers/wit-os/gust-os.wit` (time += resolution; new timer interface + worlds), `src/executor.rs` (+set_deadline, +slept accessor), `plain/src/executor.rs` (regen), the verus-strip FILES list already includes executor.
- **Create:** `drivers/timer-provider/` (crate), `benches/gust/src/bin/gust_timer_probe.rs`, rivet REQ/VER-OS-TIMER-001.

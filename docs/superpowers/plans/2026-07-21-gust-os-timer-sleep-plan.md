# gust:os timer-sleep — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Give a dissolved component an efficient tickless `sleep(interval)` over the verified executor, closing the two gaps (no wake-registration → busy-poll; raw ticks → no time unit).

**Architecture:** Surface the executor's already-verified per-task `deadline[]` + `next_deadline()` + `expire()` path through a new executor-backed `gust:os/timer` interface (`sleep`/`slept`), add `resolution()` to the mmio-backed `time`, and add one verified-core setter (`set_deadline`). Reuse the proven no-lost-wakeups/bounded-poll lemmas unchanged.

**Tech Stack:** Verus + Kani (tri-track verified core), verus-strip (plain mirror), WIT / wit-bindgen / wac / meld / loom / synth (dissolve), qemu lm3s6965evb (demonstrator), rivet (traceability).

**Spec:** `docs/superpowers/specs/2026-07-21-gust-os-timer-sleep-design.md`.

## Global Constraints

- **rivet leads:** `REQ/VER-OS-TIMER-001` land before the code that closes them (Task 1 first).
- **Verified core is sacred:** the only new function in `src/executor.rs` is `set_deadline` (+ a `slept`-supporting accessor). `next_deadline`/`expire`/`poll_round`/`admit`/`wake` and their lemmas are **not modified**. NO `assume`/`admit`/`external_body`/broadened-`requires` to pass a proof.
- **Intersection discipline:** scalar/`u32`/`u64` only, no trait objects/closures/dyn/async/heap in verified code.
- **plain mirror is generated:** never hand-edit `plain/src/executor.rs`; regenerate via `tools/verus-strip`. `executor` is already in the strip-gate FILES list.
- **Every dissolve is probe- or Kani-gated** before it counts (dissolves ≠ verified).
- **No third-party company/product/person names** in committed content.
- **v1 scope:** single partition/executor; `sleep(ticks)` bounded `ticks < 2^31`; qemu demonstrator (not silicon); polled handle (no `.await` syntax).
- Commit trailers: `Co-Authored-By: Claude Fable 5 <noreply@anthropic.com>` / `Claude-Session: https://claude.ai/code/session_011QG86sovTbfnPNY9SfhSmo`.

## File Structure

- `artifacts/gust_os_timer.yaml` — **create**: `REQ-OS-TIMER-001` + `VER-OS-TIMER-001`.
- `src/executor.rs` — **modify**: add `set_deadline` (Verus) + `slept_status` accessor + a `#[cfg(kani)]` harness.
- `plain/src/executor.rs` — **regenerate** (verus-strip); do not hand-edit.
- `benches/gust/drivers/wit-os/gust-os.wit` — **modify**: `time` += `resolution`; new `timer` interface; `app-timer` + `timer-provider` worlds; `app` += `import timer`.
- `benches/gust/drivers/timer-provider/` — **create**: the executor-backed `timer` provider crate.
- `benches/gust/src/bin/gust_timer_probe.rs` — **create**: qemu tickless demonstrator.

---

### Task 1: rivet artifacts (traceability leads)

**Files:**
- Create: `artifacts/gust_os_timer.yaml`

**Interfaces:**
- Produces: `REQ-OS-TIMER-001` (sw-req), `VER-OS-TIMER-001` (sw-verification, `verifies` REQ-OS-TIMER-001). Both `status: proposed`, `release: v0.8.0` (next line item). `related-to` REQ-OS-SYSCALL-001, REQ-OS-EXEC-001.

- [ ] **Step 1: Write the artifacts** (`artifacts/gust_os_timer.yaml`) — copy REQ text from the spec's "rivet artifacts" section verbatim: REQ-OS-TIMER-001 (bounded sleep over the tickless deadline path, `resolution()` unit, `sleep(ticks<2^31)`→wake, `slept`→pending/elapsed/invalid, wake via proven `expire`) + VER-OS-TIMER-001 (Verus/Kani on `set_deadline` + reused no-lost-wakeups compose + qemu `gust_timer_probe`; kill-criterion: harness fails / due timer not readied / sleeper re-polled before deadline). Use the `gale@0.1.0` schema shape of the sibling `gust_os_executor.yaml`.
- [ ] **Step 2: Validate** — Run: `rivet validate`. Expected: `Result: PASS`.
- [ ] **Step 3: Commit** — `git add artifacts/gust_os_timer.yaml && git commit -m "rivet(gust): REQ/VER-OS-TIMER-001 — tickless component sleep (proposed)"`

---

### Task 2: verified-core `set_deadline` + `slept_status` (the load-bearing change)

**Files:**
- Modify: `src/executor.rs`
- Regenerate: `plain/src/executor.rs`

**Interfaces:**
- Consumes: existing `Sched` struct (`state: [TaskState; MAX_TASKS]`, `ready: u32`, `deadline: [u64; MAX_TASKS]`, `inv()`), `TaskState::{Free,Pending,Done}`, `admit`.
- Produces: `pub fn set_deadline(&mut self, h: u32, d: u64)` (ensures per spec) and `pub fn slept_status(&self, h: u32, now: u64) -> u32` (0 pending / 1 elapsed / 0xFFFF_FFFF invalid), both consumed by the timer provider seam.

- [ ] **Step 1: Write the Kani harness first (red)** — in `src/executor.rs` under the existing `#[cfg(kani)]` module:

```rust
#[cfg(kani)]
#[kani::proof]
fn set_deadline_sets_only_h() {
    let mut s: Sched = kani::any();
    kani::assume(s.inv());
    let h: u32 = kani::any();
    let d: u64 = kani::any();
    let old = s.clone();
    s.set_deadline(h, d);
    assert!(s.inv());
    assert_eq!(s.ready, old.ready);
    if (h as usize) < MAX_TASKS && matches!(old.state[h as usize], TaskState::Pending) {
        assert_eq!(s.deadline[h as usize], d);
    }
    let mut j = 0usize;
    while j < MAX_TASKS {
        assert!(matches!(s.state[j], _) && s.state[j] as u8 == old.state[j] as u8);
        if j != h as usize { assert_eq!(s.deadline[j], old.deadline[j]); }
        j += 1;
    }
}
```

- [ ] **Step 2: Run it — fails to compile** — `cargo kani --harness set_deadline_sets_only_h`. Expected: FAIL (`set_deadline` not defined).
- [ ] **Step 3: Implement `set_deadline` + `slept_status` in `verus! { }`** — model the `ensures` on the spec verbatim; body is a single guarded write:

```rust
pub fn set_deadline(&mut self, h: u32, d: u64)
    requires old(self).inv(),
    ensures
        self.inv(),
        (h < MAX_TASKS as u32 && old(self).state[h as int] === TaskState::Pending)
            ==> self.deadline[h as int] == d,
        self.state === old(self).state,
        self.ready == old(self).ready,
        forall|j: int| 0 <= j < MAX_TASKS && j != h as int
            ==> self.deadline[j as int] == old(self).deadline[j as int],
{
    if h < MAX_TASKS as u32 && matches!(self.state[h as usize], TaskState::Pending) {
        self.deadline[h as usize] = d;
    }
}

/// 0 = pending (deadline not yet reached), 1 = elapsed, 0xFFFF_FFFF = invalid handle.
pub fn slept_status(&self, h: u32, now: u64) -> (r: u32)
    requires self.inv(),
    ensures
        (h >= MAX_TASKS as u32) ==> r == 0xFFFF_FFFFu32,
{
    if h >= MAX_TASKS as u32 { return 0xFFFF_FFFFu32; }
    // elapsed once `now` has passed the deadline (wrap-safe form reused from time)
    if now >= self.deadline[h as usize] { 1 } else { 0 }
}
```
(If `inv()` needs the write guarded to stay provable, the `matches!(Pending)` guard already ensures no Free/Done slot's deadline is disturbed; adjust the `ensures` `now >= deadline` form to the module's existing wrap-safe `elapsed` predicate if one is factored out.)

- [ ] **Step 4: Verus green** — `bazel test //:verus_test --test_output=all --cache_test_results=no`. Expected: `verified, 0 errors`, PASSED; obligation count rises by the new functions.
- [ ] **Step 5: Regenerate the plain mirror** — run the verus-strip generator for `executor` (see `tools/verus-strip` README / how sibling modules regen). Do NOT hand-edit `plain/src/executor.rs`.
- [ ] **Step 6: Strip gate green** — `cargo test --manifest-path tools/verus-strip/Cargo.toml --test gate`. Expected: `2 passed; 0 failed`.
- [ ] **Step 7: Kani green** — `cargo kani --harness set_deadline_sets_only_h`. Expected: `VERIFICATION:- SUCCESSFUL`. (Also re-run one existing executor harness, e.g. the no-lost-wakeups one, to confirm no regression.)
- [ ] **Step 8: Commit** — `git add src/executor.rs plain/src/executor.rs && git commit -m "feat(gust): executor set_deadline + slept_status — surface the tickless deadline (REQ-OS-TIMER-001)"`

---

### Task 3: WIT seam (`time` += `resolution`, new `timer` interface)

**Files:**
- Modify: `benches/gust/drivers/wit-os/gust-os.wit`

**Interfaces:**
- Produces: `time.resolution: func() -> u64`; `interface timer { sleep: func(ticks: u64) -> u32; slept: func(handle: u32) -> u32; }`; worlds `app-timer`, `timer-provider`; `app` += `import timer`.

- [ ] **Step 1: Add `resolution` to `time`, the `timer` interface, and the worlds** — copy the WIT blocks from the spec's "WIT changes" section verbatim.
- [ ] **Step 2: Validate WIT parses** — `wasm-tools component wit benches/gust/drivers/wit-os/gust-os.wit` (or the repo's WIT check). Expected: parses, no error.
- [ ] **Step 3: Commit** — `git add benches/gust/drivers/wit-os/gust-os.wit && git commit -m "wit(gust): time.resolution + gust:os/timer interface + worlds (REQ-OS-TIMER-001)"`

---

### Task 4: `timer-provider` crate (dissolve)

**Files:**
- Create: `benches/gust/drivers/timer-provider/{Cargo.toml,.cargo/config.toml,src/lib.rs}`

**Interfaces:**
- Consumes: the executor seam (`set_deadline`, `slept_status`) via a `taskdisp`-class extern + `gust:hal/mmio` for `now()`.
- Produces: exports `gust:os/timer`; when dissolved, its undefined symbols are the executor `set_deadline`/`slept`/`poll_task` seam + `read32`/`write32` — the same TCB class as `spawn-provider`.

- [ ] **Step 1: Scaffold the crate** — mirror `drivers/spawn-provider/` (wit-bindgen `generate!` with `generate_all`, `#![no_std]`, the zero-state trapping `GlobalAlloc`, the `.cargo/config.toml` `--allow-undefined`). Read `spawn-provider/RESULTS.md` for the exact recipe.
- [ ] **Step 2: Implement the two exports** — `sleep(ticks)`: assert `ticks < (1u64<<31)`; compute `d = now() + ticks` (wrap-safe u64); call the extern `set_deadline(current_task_handle, d)`; return the handle. `slept(handle)`: call the extern `slept_status(handle, now())`. `current_task_handle` comes from the executor dispatch context (same mechanism spawn-provider uses to know its task); if not available as an FFI, add a `current_task()` extern to the seam.
- [ ] **Step 3: Build to wasm** — `cargo build --release --target wasm32-unknown-unknown` (needs the `--allow-undefined` config). Expected: builds; `wasm-tools component new` yields a valid component (embedded world).
- [ ] **Step 4: Dissolve** — `wasm-tools component new` → (compose with an app-timer test app via `wac plug`) → `meld fuse --memory shared` → `loom optimize --passes inline` → `synth compile --target cortex-m3 --all-exports --relocatable`. Expected: one `.o`, `timer` exports present, undefined syms = executor seam + mmio only (`nm` check). Record sizes in `RESULTS.md`.
- [ ] **Step 5: Commit** — `git add benches/gust/drivers/timer-provider && git commit -m "feat(gust): timer-provider — executor-backed gust:os/timer, dissolves clean"`

---

### Task 5: `gust_timer_probe` qemu demonstrator (the end-to-end oracle)

**Files:**
- Create: `benches/gust/src/bin/gust_timer_probe.rs`

**Interfaces:**
- Consumes: `gale::executor` (`Sched`, `set_deadline`, `slept_status`, `next_deadline`, `expire`) directly (native probe, like `gust_hm_probe`), plus a stub `now()`.

- [ ] **Step 1: Write the probe** — build a `Sched`, `admit` a task (Pending), `set_deadline(h, now+ONE_SEC)` where `ONE_SEC` is a chosen tick constant. Assert: (a) `slept_status(h, now) == 0` before; (b) `next_deadline()` returns exactly that deadline (the single armed alarm — tickless); (c) the task is NOT readied by `expire(now_before)` (no spin); (d) after `expire(now+ONE_SEC)` the task is ready and `slept_status(h, now+ONE_SEC) == 1`. Non-vacuity: an invalid handle → `0xFFFF_FFFF`; a not-yet-due handle stays `0`. Print one OK line; explicit FAIL with detail on any deviation (no silent fall-through). Model structure/exit on `gust_hm_probe.rs`.
- [ ] **Step 2: Run it** — `cd benches/gust && cargo run --bin gust_timer_probe` (qemu runner). Expected: the OK line, exit 0. SHOW the output.
- [ ] **Step 3: Regression** — `cargo run --bin gust_hm_probe` still OK.
- [ ] **Step 4: Commit** — `git add benches/gust/src/bin/gust_timer_probe.rs && git commit -m "feat(gust): gust_timer_probe — tickless sleep/expire/slept demonstrated on qemu (VER-OS-TIMER-001)"`

---

### Task 6: close — flip `VER-OS-TIMER-001` → verified

**Files:**
- Modify: `artifacts/gust_os_timer.yaml`

- [ ] **Step 1: Flip status** — `VER-OS-TIMER-001` `proposed` → `verified`; append a DELIVERED paragraph: `set_deadline` Verus + Kani (`set_deadline_sets_only_h`), reused no-lost-wakeups/bounded-poll compose over timer entries, and the `gust_timer_probe` demonstrator (tickless single-alarm + expire-wake + non-vacuity), all green. Note honest scope (single-partition, `sleep < 2^31`, qemu not silicon). Keep `REQ-OS-TIMER-001` at `implemented` unless every REQ clause is met — it is, so flip it to `verified` too.
- [ ] **Step 2: Validate** — `rivet validate`. Expected: PASS.
- [ ] **Step 3: Commit** — `git add artifacts/gust_os_timer.yaml && git commit -m "release(gust): VER-OS-TIMER-001 verified — tickless component sleep closed"`

---

## Self-Review (author checklist — done)

- **Spec coverage:** resolution (T3) ✓, timer interface (T3) ✓, set_deadline verified (T2) ✓, slept (T2) ✓, provider dissolve (T4) ✓, tickless demonstrator (T5) ✓, bounded `<2^31` (T4 sleep assert + T5) ✓, rivet lead+close (T1/T6) ✓.
- **Placeholders:** none — each code step carries the actual signature/body.
- **Type consistency:** `set_deadline(u32,u64)`, `slept_status(u32,u64)->u32`, `sleep(u64)->u32`, `slept(u32)->u32` consistent across tasks.
- **Ambiguity resolved:** `current_task_handle` provenance (T4 Step 2) — from executor dispatch context; add a `current_task()` extern if not already exposed. This is the one integration unknown; the implementer confirms it against `spawn-provider` before writing T4.

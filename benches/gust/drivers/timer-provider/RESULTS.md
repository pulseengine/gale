# timer-provider — gust:os `timer` backed by the real executor (Task 4, v0.4.0 timer-sleep)

Backs the `gust:os/timer` WIT interface (world `timer-provider`, `wit-os/gust-os.wit`:
`sleep: func(handle: u32, ticks: u32) -> u32; slept: func(handle: u32) -> u32;`)
with the node's ONE verified executor deadline table — not a hand-written
placeholder, and no longer a private copy. `src/lib.rs` holds no state at all: it
reaches `Tasks::set_deadline`/`Tasks::slept_status` (the Task 2 additions) over the
`gust:sched/tasks` import that `exec-provider`, the executor's single owner, exports.

RESOLVED SEAM (task-4 brief, exact contract implemented):
- `sleep(handle, ticks)`: reject `ticks >= 2^31` (return `0xFFFF_FFFF`); else
  `d = time.deadline(time.now(), ticks)`; call `tasks.set-deadline(handle, d)`;
  return `0`. `set_deadline` itself is a no-op on a non-Pending/out-of-range handle
  (Kani-framed by `set_deadline_sets_only_h`), so this marshalling layer
  re-implements no admission decision.
- `slept(handle)`: return `tasks.slept-status(handle, time.now())` directly
  (0 pending / 1 elapsed / 0xFFFF_FFFF invalid).
- `handle` is the app's handle, used UNCHANGED — there is no mapping table here.
- The clock is `gust:os/time`, not a local MMIO read. This crate has NO
  `gust:hal` import; `time-provider` owns the register.

Neither `sleep` nor `slept` drives a scheduler round, so this crate never touches
the trusted `taskdisp`/`poll_task` dispatch seam either — that belongs to the
executor's owner. Its complete import set is `gust:sched/tasks` +
`gust:os/time`, which the built component confirms.

**Resolved (was a v1 known limitation):** this crate used to keep its OWN
`MaybeUninit<Tasks>` table, separate from `spawn-provider`'s, so a task admitted by
`spawn.start` was invisible to `timer.sleep` (both would silently no-op against a
`Free` slot's default `u64::MAX` deadline). It also read `TIM2_CNT` (`0x4000_0024`)
directly, agreeing with `time.now()` only because the two hardcoded constants
matched. Both are gone: one executor instance, one clock, both imported.

## Build

    cargo build --release --target wasm32-unknown-unknown

No `.cargo/config.toml` / `--allow-undefined` is needed: the crate has no
`extern "C"` declaration at all any more, only WIT imports.

    $ cargo build --release --target wasm32-unknown-unknown
        Finished `release` profile [optimized] target(s) in 0.11s
      (no warnings — the executor include, and its dead `poll_task` decl, are gone)

## ABI: confirmed

    $ wasm-tools print target/wasm32-unknown-unknown/release/gust_timer_provider.wasm | grep -E "\(import|\(export"
      (import "gust:os/time@0.1.0" "now" (func ...))
      (import "gust:os/time@0.1.0" "deadline" (func ...))
      (import "gust:sched/tasks@0.1.0" "set-deadline" (func ...))
      (import "gust:sched/tasks@0.1.0" "slept-status" (func ...))
      (export "memory" (memory 0))
      (export "gust:os/timer@0.1.0#sleep" (func $gust:os/timer@0.1.0#sleep))
      (export "gust:os/timer@0.1.0#slept" (func $gust:os/timer@0.1.0#slept))
      (export "cabi_realloc_wit_bindgen_0_52_0" (func $cabi_realloc_wit_bindgen_0_52_0))
      (export "cabi_realloc" (func $cabi_realloc))

Four imports, no `gust:hal` among them: the clock comes from `gust:os/time` and the
deadline table from `gust:sched/tasks`. Both `timer` exports are present with the
WIT-declared names/shapes.

    $ wasm-tools component new target/wasm32-unknown-unknown/release/gust_timer_provider.wasm -o /tmp/timer-provider.comp.wasm
    $ wasm-tools validate /tmp/timer-provider.comp.wasm
    (exit 0 — valid component, embedded world)

## Dissolve

    loom-1.2.0 optimize /tmp/timer-provider.comp.wasm --passes inline --attestation false -o /tmp/timer-provider.loom.wasm
      Component:    2867 → 2242 bytes (21.8% reduction)
      Module size:  1463 → 1463 bytes (0.0% reduction)
      Status:       Successfully optimized 1 of 1 core modules

    synth-0.49.0 compile /tmp/timer-provider.loom.wasm --target cortex-m3 --all-exports --relocatable -o /tmp/tp-cm3.o
      Compiling function 'gust:os/timer@0.1.0#sleep' via backend 'arm'...
      Compiling function 'gust:os/timer@0.1.0#slept' via backend 'arm'...
      Compiled 5 functions to /tmp/tp-cm3.o

    $ arm-none-eabi-size /tmp/tp-cm3.o
       text    data     bss     dec     hex
        552       0       0     552     228

Both exports compile; the four seam calls are the object's only undefined symbols
(`now`, `deadline`, `set-deadline`, `slept-status`).

An earlier revision of this file reported `sleep` as BLOCKED on synth#518 (an i64
param in a frame-backing function). That text described the older WIT shape
`sleep(handle: u32, ticks: u64)`; it is stale, and NOT something this change fixed.
Measured both ways at synth-0.49.0: the pre-change source (embedded executor,
`gust:hal` clock, `ticks: u32`) also compiles — `Compiled 5 functions`, text 844 B —
so the difference here is size (844 → 552 B text, the executor no longer being
duplicated into this object), not lowering.

The checked-in `timer-provider-cm3.o` predates all of this and was deliberately NOT
regenerated; the measurements above were taken into a temp path.

## Historical note: the synth#518 `sleep` blocker

Everything from here down in earlier revisions of this file described the Task-4
WIT shape `sleep(handle: u32, ticks: u64) -> u32`, whose i64 param plus in-body
call hit synth#518 and was dropped from the object ("1 of 5 functions were
skipped"). That signature no longer exists — the seam narrowed `ticks` to `u32`
(< 2^31 fits) and every 64-bit value now crosses as lo/hi u32 halves, so the
buggy lowering path is never entered. The blocker is not reachable from this
crate any more and the "Full app-timer compose: NOT attempted" reasoning that
followed from it is obsolete: `sleep` compiles (see the Dissolve section above).

The upstream limitation itself is unchanged and still real for any interface that
combines a `u64` WIT param with an in-body call; it is recorded in
`exec-provider/RESULTS.md`, which carries the same lo/hi split for the same reason.

## Summary

| Export | Component (wasm) | loom optimize | synth compile (cortex-m3) |
|---|---|---|---|
| `sleep(handle: u32, ticks: u32) -> u32` | present, correct ABI | passes through | compiles |
| `slept(handle: u32) -> u32` | present, correct ABI | passes through | compiles |

Undefined symbols of the standalone object: `now`, `deadline` (the clock, from
`gust:os/time`) and `set-deadline`, `slept-status` (the task table, from
`gust:sched/tasks`). No `gust:hal`, no `poll_task`, no scheduler state — this
crate holds none.

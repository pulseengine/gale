# exec-provider — the v1 async executor, dissolved single-component (Task 6)

Dissolves `src/executor.rs` (Verus 1081/0: no-lost-wakeups, fair/work-conserving
`pick_next`, bounded+terminating `poll_round`, tickless `next_deadline`/`expire`;
Kani `pick_next_is_min_prio_ready` + `poll_round_drains_and_bounds`, both
SUCCESSFUL) to ONE relocatable Cortex-M3 object, with NO wac plug and NO meld
fuse — a single-component dissolve.

## Provenance

`src/lib.rs` includes `plain/src/executor.rs` **verbatim** via `#[path]` — the
exact verus-strip output `cargo kani` already builds against
(`tools/verus-strip/tests/gate.rs` enforces the two stay convergent). Everything
else in this crate is scalar C-ABI marshalling only (`exec_admit`/
`exec_poll_round`/`exec_state`): no admit/wake/pick_next/poll_round/expire
decision is re-implemented at the wasm boundary.

## Build

    cargo build --release --target wasm32-unknown-unknown
    loom optimize <wasm> --passes inline --attestation false -o loom.wasm
    synth compile loom.wasm --target cortex-m3 --all-exports --relocatable \
      --native-pointer-abi -o exec-cm3.o

`--native-pointer-abi` is REQUIRED here (unlike the purely-scalar thin drivers):
this crate carries real static state (the `Tasks` table), and synth's plain
`--relocatable` path (no `--native-pointer-abi`) does not materialize any real
`.bss`/`.data` for a stateful module — verified empirically: without the flag,
`exec-cm3.o` reported `bss=0` and every write to the `TASKS` singleton was
silently lost (each `exec_admit` call saw a fresh, empty table). `.cargo/config.toml`
also sets `-z stack-size=1024` (a plain wasm-ld flag, not synth's
`--shadow-stack-size`) so the default ~1 MiB wasm32 shadow-stack reservation
doesn't get carried into `.bss` — this crate has no recursion and a ~80 B max
frame, so 1 KiB is generous. NOT using `--shadow-stack-size` keeps this outside
the specific synth#746 combination (which needs `--native-pointer-abi` +
`--shadow-stack-size` + a meld-fused node — none of the last two apply here).

## Measured (synth 0.42.0 + loom 1.1.18)

`exec-cm3.o`: **text 1860 / data 428 / bss 1448 = 3736 B**, one undefined
symbol (`poll_task`, the trusted FFI seam, resolved at final native link by the
qemu probe's own `poll_task` export — same pattern every thin-seam driver in
this repo uses for its mmio externs). `.bss` is the `Tasks` table
(`state[8]+prio[8]+deadline[8]+ready`, MAX_TASKS=8) plus wasm-ld's data-segment
bookkeeping — small and bounded, not proportional to anything unbounded.

## Oracle: `gust_exec_probe` (qemu-cortex-m3)

    cd benches/gust && cargo run --release --bin gust_exec_probe
    gust-exec-probe OK: both tasks Done, hi-prio first   (exit 0, EXIT_SUCCESS)

Admits two tasks at priorities 1 and 9 with a due-now deadline (0), drives one
`exec_poll_round(0)`, and asserts both reach `Done` — exercising `admit`,
`expire` (tickless: deadline `<=` now marks ready), `pick_next`
(fairness — not separately observable from a 2-task drain-to-completion, but
the SAME proven scan the Kani harness checks), `consume`, and `dispatch_one`
end to end on the dissolved object. Reproducible across repeated clean runs.

## ABI deviations from the brief's literal probe code (both are marshalling-only)

1. **`exec_poll_round(now: u64)` -> `exec_poll_round(now_lo: u32, now_hi: u32)`.**
   A wasm export combining a 64-bit parameter with a (post loom-`--passes
   inline`) call hit a synth ARM-backend gap: `synth compile` printed
   `warning: skipping function 'exec_poll_round': ... an i64/f64 param in a
   frame-backing function ... is not yet lowered` and produced only 4 of 5
   functions — silently dropping the whole function from the object. Splitting
   `now` into lo/hi halves at the wasm-export boundary avoids it (no exported
   function has an i64 parameter). `now` is reconstructed by a shift+or before
   ever reaching `expire`/`poll_round`, which run unmodified.
2. **`exec_admit(prio: u32, deadline: u64) -> u32` -> `exec_admit(prio: u32,
   deadline_lo: u32, deadline_hi: u32) -> u32`.** The mixed-width
   `(u32, u64)` form compiled with NO synth warning, but empirically read a
   garbage `deadline` — traced to AAPCS's 64-bit register-pair alignment
   (`deadline` should land in r2:r3, skipping r1) not being reproduced by
   synth's simple sequential register lowering. All three functions were moved
   to plain sequential u32 params so both sides of the FFI boundary (this
   crate's wasm export and the qemu probe's native `extern "C"` declaration)
   use the identical convention, with no dependence on synth reproducing AAPCS
   padding for any mixed-width parameter list.

Both are pure register/argument marshalling changes; `Tasks::{admit, expire,
poll_round}` are called with the same reconstructed values and run unmodified.

## A separate synth finding NOT blocking this deliverable (disclosed, not worked around)

While instrumenting extra temporary debug exports (`exec_debug_ready`,
`exec_debug_deadline_lo`, `exec_debug_wake`, `exec_debug_deadline_eq_now`,
`exec_debug_expire_only` — since removed) during root-cause debugging, two
further synth 0.42.0 gaps surfaced around **wide (`i64`) static loads under
`--native-pointer-abi`**, both apparently sensitive to the module's overall
data-segment layout (i.e., the SAME `self.deadline[i] <= now` comparison
inside `expire()`, unmodified, sometimes compiled correctly and sometimes
did not, depending on which OTHER functions were also exported):
  - **Explicit decline** on an isolated single-purpose debug export: `warning:
    skipping function 'exec_debug_deadline_eq_now': ... #739: i64.load with
    static-region memarg offset 1376 (>= wasm_data_base 1024) under the
    native-pointer ABI is not yet relocated — declining rather than baking an
    un-rebased linmem offset`.
  - **Silent miscompilation** (no warning at all) on a slightly different
    export set: `exec_poll_round` compiled with no warning, but `expire()`'s
    deadline comparison inside it never fired (`ready` stayed 0 after
    `exec_poll_round`, confirmed via temporary `exec_debug_ready`/
    `exec_debug_expire_only` probes) — i.e., the SAME `i64.load` bug, present
    but NOT caught by whatever heuristic produces the explicit-decline warning
    above.
  This did not reproduce with the FINAL, minimal 3-export crate shipped here
  (`exec_admit`/`exec_poll_round`/`exec_state` only) — `gust_exec_probe`
  passes reliably across repeated clean rebuilds — but it is a real,
  reproducible synth defect class (wide static loads under
  `--native-pointer-abi`, independent of meld-fusion or `--shadow-stack-size`,
  contradicting the narrower "only affects the shrink+meld-fused combo"
  characterization of the adjacent, already-tracked synth#746). Worth a
  precise upstream report with the repro above; NOT worked around by
  re-implementing any scheduling decision — `expire`'s comparison runs
  unmodified in the shipped object.

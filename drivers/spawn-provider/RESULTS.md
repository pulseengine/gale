# spawn-provider — gust:os `spawn` backed by the real executor (Task 6, Step 5)

Backs the `gust:os/spawn` WIT interface (world `spawn-provider`,
`wit-os/gust-os.wit`: `start: func(entry: u32) -> u32; poll: func(handle: u32)
-> u32;`) with the SAME verified executor `exec-provider` owns — not a
hand-written placeholder, and no longer a private copy of it. This crate is
STATELESS: no `Tasks` table, no `#[path]` include of `plain/src/executor.rs`, no
handle mapping. It reaches the one executor instance over the `gust:sched/tasks`
import that `exec-provider` exports.

`start(entry)`: `admit(0)` then `wake(h)` — spawn is "ready now" (this WIT ABI
carries no priority/deadline, unlike `exec-provider`'s C-ABI probe surface, so
v1 admits at a fixed neutral priority and wakes immediately rather than
threading a deadline through). The handle is returned UNCHANGED, so it is the
executor's own handle and means the same task to `gust:os/timer.sleep` and
`gust:os/exec.state`. `poll(handle)`: queries the handle's state first (an
invalid handle is rejected without driving anything), then drives one
`poll-round` (every poll is cooperative — it drains whatever else is ready too,
not only `handle`) and reports `handle`'s resulting state as the WIT-documented
code (`0`=pending, `1`=done, `0xFFFF_FFFF`=invalid). Both functions are
marshalling only; `admit`/`wake`/`poll_round` run unmodified in their owner.

## ABI: confirmed

    $ wasm-tools print target/wasm32-unknown-unknown/release/gust_spawn_provider.wasm | grep -E "\(import|\(export"
      (import "gust:sched/tasks@0.1.0" "state" (func ...))
      (import "gust:sched/tasks@0.1.0" "poll-round" (func ...))
      (import "gust:sched/tasks@0.1.0" "admit" (func ...))
      (import "gust:sched/tasks@0.1.0" "wake" (func ...))
      (export "memory" (memory 0))
      (export "gust:os/spawn@0.1.0#poll" (func $gust:os/spawn@0.1.0#poll))
      (export "gust:os/spawn@0.1.0#start" (func $gust:os/spawn@0.1.0#start))

Same two exported function names/shapes the WIT world declared — `start`/`poll`
are unchanged. What changed is the import side: this crate no longer carries the
trusted `gust:os/taskdisp.poll-task` dispatch either. Dispatch belongs to the
component that runs the scheduler, and that is now exclusively `exec-provider`.

## Build (same shape as exec-provider — see its RESULTS.md for the
`--native-pointer-abi` / stack-size rationale, which applies identically here)

    cargo build --release --target wasm32-unknown-unknown
    loom optimize <wasm> --passes inline --attestation false -o loom.wasm
    synth compile loom.wasm --target cortex-m3 --all-exports --relocatable \
      --native-pointer-abi -o spawn-provider-cm3.o

Measured (synth 0.45.1 + loom 1.2.0), into a temp path — the checked-in
`spawn-provider-cm3.o` predates this change and was deliberately NOT regenerated:

| | text | data | bss | total |
|---|---|---|---|---|
| with the executor embedded (previous) | 1108 | 16 | 1168 | 2292 B |
| stateless, over `gust:sched/tasks` | 376 | 32 | 1060 | 1468 B |

No skipped functions; undefined symbols are exactly the four seam calls
(`admit`, `wake`, `poll-round`, `state`). The text drop is the second copy of the
executor leaving this object; the remaining `.bss` is the shadow stack
(`-zstack-size=1024`), not scheduler state — this crate has none.

## ts-node compose (v0.4.0 step-3)

`drivers/build-os-ts.sh` composes `app-ts` + `time-provider` + `spawn-provider` +
`exec-provider` and dissolves them to `os-node/os-ts-cm3.o`. exec-provider is in
that list now BECAUSE this crate is stateless: something has to export
`gust:sched/tasks`. The composition also moved from `wac plug` to a `wac compose`
script — `plug` wires plugs into the socket's imports only, never plug→plug, so
the spawn→exec edge has to be named (verified: passing exec-provider as a fourth
`--plug` still leaves `gust:sched/tasks` as a residual import).

Measured through the updated script (into a temp `OSNODE`): text 4756 / data 496 /
bss 2584 = 7836 B / 8192 budget, undefined = `read32` + `poll-task` only.
Previously 4164 B; the growth is exec-provider joining the node. The checked-in
`os-node/os-ts-cm3.o` was NOT regenerated, so it and `gust_os_ts_probe` still
reflect the pre-change composition.

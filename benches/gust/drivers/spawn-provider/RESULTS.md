# spawn-provider — gust:os `spawn` backed by the real executor (Task 6, Step 5)

Backs the `gust:os/spawn` WIT interface (world `spawn-provider`,
`wit-os/gust-os.wit`: `start: func(entry: u32) -> u32; poll: func(handle: u32)
-> u32;`) with the SAME verified executor `exec-provider` dissolves — not a
hand-written placeholder. `src/lib.rs` includes `plain/src/executor.rs`
verbatim, exactly as `exec-provider` does.

`start(entry)`: `admit(0)` then `wake(h)` — spawn is "ready now" (this WIT ABI
carries no priority/deadline, unlike `exec-provider`'s C-ABI probe surface, so
v1 admits at a fixed neutral priority and wakes immediately rather than
threading a deadline through). `poll(handle)`: drives one `poll_round` (every
poll is cooperative — it drains whatever else is ready too, not only
`handle`) and reports `handle`'s resulting state as the WIT-documented code
(`0`=pending, `1`=done, `0xFFFF_FFFF`=invalid). Both functions are marshalling
only; `admit`/`wake`/`poll_round` run unmodified.

## ABI: byte-identical, confirmed

    wasm-tools print target/wasm32-unknown-unknown/release/gust_spawn_provider.wasm
      (import "gust:os/taskdisp@0.1.0" "poll-task" (func ...))
      (export "gust:os/spawn@0.1.0#poll" (func $gust:os/spawn@0.1.0#poll))
      (export "gust:os/spawn@0.1.0#start" (func $gust:os/spawn@0.1.0#start))

Same two exported function names/shapes the WIT world declared — `start`/`poll`
are unchanged. The trusted dispatch import moved from raw `env::poll_task` to
the WIT-typed `gust:os/taskdisp.poll-task` (see "ts-node compose" below); the
contract (dispatch task `id` once; 1 = completed) is identical, and
`plain/src/executor.rs` is still included verbatim — the forwarding
`#[no_mangle] poll_task` in this crate resolves the executor's extern in-module
and calls the WIT import.

## Build (same shape as exec-provider — see its RESULTS.md for the
`--native-pointer-abi` / stack-size rationale, which applies identically here)

    cargo build --release --target wasm32-unknown-unknown
    loom optimize <wasm> --passes inline --attestation false -o loom.wasm
    synth compile loom.wasm --target cortex-m3 --all-exports --relocatable \
      --native-pointer-abi -o spawn-provider-cm3.o

Measured (synth 0.45.1 + loom 1.2.0): `spawn-provider-cm3.o` = text 1108 /
data 16 / bss 1168 = 2292 B, no skipped functions, sole undefined symbol
`poll-task` (the taskdisp seam). Smaller than `exec-provider`'s object:
`start`/`poll` never call `expire`/`next_deadline` (spawn has no deadline
concept), so this crate never exercises the wide (`i64`)
static-load-under-`--native-pointer-abi` codegen path noted in exec-provider's
RESULTS.md at all. (Earlier, synth 0.42.0 + loom 1.1.18 measured
1068/124/1152 = 2344 B on the pre-taskdisp source; data shrank 124 -> 16
because the lazily-initialized table is now `MaybeUninit` + flag instead of a
niche-encoded `Option<Tasks>`, whose `None` discriminant byte was initialized
data — see src/lib.rs.)

## ts-node compose (v0.4.0 step-3 — resolves the seam this section deferred)

The blocker documented here previously — `wasm-tools component new` rejecting
the raw `env::poll_task` core import — is RESOLVED by the WIT-typed
task-dispatch seam: `gust:os/taskdisp { poll-task: func(id: u32) -> u32 }`
(wit-os/gust-os.wit), imported by `world spawn-provider`. This crate forwards
the executor's `extern "C" poll_task` to that import, so no raw `env` import
survives, the module componentizes, and `wac` plugs it (with time-provider)
into the `app-ts` node: `drivers/build-os-ts.sh` ->
`os-node/os-ts-cm3.o` (text 1540 / data 40 / bss 2584 = 4164 B / 8192,
undefined = `read32` + `poll-task` only). Liveness oracle:
`gust_os_ts_probe` (qemu) — the app's spawn.start/poll round-trip through the
dissolved executor returns 1 (done); `gust_exec_probe` still covers the raw
C-ABI surface of the SAME executor.

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
      (import "env" "poll_task" (func $poll_task ...))
      (export "gust:os/spawn@0.1.0#poll" (func $gust:os/spawn@0.1.0#poll))
      (export "gust:os/spawn@0.1.0#start" (func $gust:os/spawn@0.1.0#start))

Same two exported function names/shapes the WIT world declared before this
change (the WIT file itself was not touched) — `start`/`poll` are unchanged.
No prior spawn-provider probe existed in this repo to re-run for a regression
check (the directory held only a build leftover, no committed source); this
`wasm-tools print` comparison against the WIT source is the ABI-parity check
in its place.

## Build (same shape as exec-provider — see its RESULTS.md for the
`--native-pointer-abi` / stack-size rationale, which applies identically here)

    cargo build --release --target wasm32-unknown-unknown
    loom optimize <wasm> --passes inline --attestation false -o loom.wasm
    synth compile loom.wasm --target cortex-m3 --all-exports --relocatable \
      --native-pointer-abi -o spawn-provider-cm3.o

Measured (synth 0.42.0 + loom 1.1.18): `spawn-provider-cm3.o` = text 1068 /
data 124 / bss 1152 = 2344 B, no skipped functions. Smaller than
`exec-provider`'s object: `start`/`poll` never call `expire`/`next_deadline`
(spawn has no deadline concept), so this crate never exercises the wide
(`i64`) static-load-under-`--native-pointer-abi` codegen path noted in
exec-provider's RESULTS.md at all.

## Deferred (v2 / not in Task 6 scope)

`wasm-tools component new` cannot componentize this module as-is: the trusted
`poll_task` FFI seam (inside the included `executor` module) is a raw
`extern "C"` import (`env::poll_task`), not a WIT-typed one, and the
`spawn-provider` world declares no import for it — so full wac-plug
composition into an `app-ts` node (the way `time-provider`/`log-provider`
compose into the `os-node` step-1/2 nodes) needs a WIT-typed task-dispatch
seam design first. Out of scope for Task 6 (v1 static single-partition); the
executable liveness oracle for this task is `gust_exec_probe`, which drives
the SAME executor through its raw C-ABI end to end and passes.

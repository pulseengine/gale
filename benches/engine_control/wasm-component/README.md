# engine-control as a WebAssembly **Component** — the full-story leg

The bench already runs the engine-control algorithm as **core wasm dissolved to
native** (`../wasm-dissolve/`, 2.1× vs LLVM) and the verified gale primitives in
three runtimes. This adds the missing layer: the same algorithm as a real
**Component Model** component with a typed WIT boundary — the layer nobody else
puts on this device class (gale#74 / gale#63).

## What it is

One component (`engine-control` world), two exports:

| export | kind | status |
|---|---|---|
| `gale:engine-control/control` → `step` | **sync** `func(sensors) -> actuators` | **runs on wasmtime today**, functionally identical to `../src/control.c` |
| `gale:engine-control/crank-stream` → `process` | **async** `func(stream<sensors>) -> u32` | built on the forked bindgen's amortized `StreamReader::next`; live-run blocked (see below) |

The algorithm is a faithful port of `../src/control.c` (table lookups + integer
corrections, no float, no alloc); `src/tables.rs` is generated from
`../src/tables.c` by `gen-tables.sh` (single source of truth).

## Why the forked wit-bindgen

Built on **`pulseengine/wit-bindgen`** (branch
`perf/async-stream-amortize-single-item-alloc`), not stock. The fork amortizes
per-item heap allocation in the single-item stream helpers: `StreamReader::next`
reuses one cached buffer across reads — the **first** read allocates it, the rest
allocate **zero**. That bounded-heap behaviour is exactly what an embedded node
(wohl, a drone failsafe) needs to process a live crank-sample stream, which is
why `crank-stream.process` reads via `next().await` in a loop.

The fork proves the amortization mechanically with a host-side alloc-counting
test (`crates/guest-rust/src/rt/async_support/stream_support.rs`): first
`next()`/`write_one()` allocates once, the next eight allocate zero.

## Honest status — what runs vs what's blocked

- ✅ **Builds** against the forked bindgen; valid Component (`wasm-tools validate`).
- ✅ **Sync `step` runs on wasmtime** and matches the C bench on every vector
  (`./run.sh` → 5/5 `[OK]`).
- ⛔ **Async `crank-stream.process` cannot be driven at runtime yet** — a
  component-model async-lift stream export can't be exercised by the harness;
  this is **pulseengine/witness#107**, the same blocker the fork's own authors
  hit (they fell back to the host-side alloc proof above). When witness#107
  lands, `process` gets a live wasmtime run + a runtime alloc-count here.
- Note: even invoking the sync `step` needs `wasmtime -W component-model-async=y`,
  because the component carries async constructs for the streaming export.

## Reproduce

```sh
export PATH="/opt/homebrew/opt/llvm/bin:$PATH"   # macOS: llvm-* for the C ref
./run.sh
```

## The full story, in one line

`control_step` now runs as: **native (LLVM)** · **core-wasm dissolved to
Cortex-M (`../wasm-dissolve`)** · **core-wasm on wasmtime** · **a typed
Component Model component (here)** — same algorithm, one source of truth, with
the embedded async-stream shape staged on the fork for when witness#107 unblocks
the live path.

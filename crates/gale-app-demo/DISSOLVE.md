# The library-OS backing — dissolving the composed component to native ELF

`run.sh` proves the component runs on **wasmtime** (hosted backing).
`dissolve.sh` produces the **lean, grow-free native** backing — the MCU
library-OS image.

## Lean dissolve (working) — `dissolve.sh`

Built on **`pulseengine/wit-bindgen@integration/embedded-rt-no-grow`** with the
**`cabi-realloc-extern`** feature: the canonical-ABI `cabi_realloc` is routed to
an embedder symbol **`__cabi_arena_realloc`** (the reference TCB arena in
`tcb/cabi_arena.c` — fixed, trap-on-exhaustion), so the component links **no
growing allocator** and emits **no `memory.grow`**. Built for
`wasm32-unknown-unknown` (the feature is gated `not(target_env=p2)`), then
`loom → synth → native .o`.

| object | before (wasip2/default) | **lean (no-grow)** |
|---|---|---|
| gale-kiln core `.text` | ~24,848 B (adapter + dlmalloc + grow) | **972 B**, 0 `memory.grow` |
| gale-app-demo core `.text` | ~24,162 B | **316 B**, 0 `memory.grow` |

Both import `__cabi_arena_realloc`, resolved by the TCB arena at native link.
**~25× leaner, single-address-space, grow-free** — the MCU library-OS object.
This unblocks gale#89.

> Pins an **unmerged** wit-bindgen branch (#4 `cabi-realloc-extern`, #5 inline
> zero-heap, #6 scalar-elide). Re-pin to a tag when it merges.

## The two backings stay one component

| backing | build | status |
|---|---|---|
| wasmtime (hosted, dev/test) | `wasm32-wasip2` (feature is a no-op here) | ✅ `run.sh` → `run-demo()=53` |
| dissolved-native (library OS) | `wasm32-unknown-unknown` + `cabi-realloc-extern` | ✅ `dissolve.sh` → 972 B / 316 B, grow-free |
| kiln-runtime (hosted, target) | — | ⛔ kiln#344 (kilnd component-model disabled) |

Note: a grow-free core uses an `env::__cabi_arena_realloc` import, which
`wasm-tools component new` rejects — so the grow-free build is the **core
dissolve** path (synth + native link to the TCB arena), not the
component/`meld --memory shared` path (that needs wit-bindgen#6's full elide or
`component new` to allow the `env` arena import). For the single-image MCU
dissolve we don't need meld.

Isolation between dissolved components is the **opt-in** MPU/PMP layer (gale#86).

## Reproduce

```sh
export PATH="/opt/homebrew/opt/llvm/bin:/Users/r/.cargo/bin:$PATH"
./run.sh        # hosted backing on wasmtime (run-demo()=53)
./dissolve.sh   # lean grow-free MCU library-OS object (972 B / 316 B)
```

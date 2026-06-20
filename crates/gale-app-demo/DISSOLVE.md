# The library-OS backing — dissolving the composed component to native ELF

`run.sh` proves the composed component runs on **wasmtime** (the hosted backing).
`dissolve.sh` proves the **same composed component dissolves to native ELF** —
the library-OS backing of the kiln component-host path (SAC-BYOOS-LIBOS,
gale#74/#63).

## Pipeline

```
gale-app-demo (imports gale:kernel)  +  gale-kiln (provides gale:kernel over gale::*)
        │  wac plug
        ▼
   composed component  ──wasm-tools component unbundle──▶  per-crate core modules
        │                                                    module0 = kernel exports
        │                                                    module1 = app (imports kernel) + run-demo
        ▼  per core:  loom optimize --passes inline  →  synth compile --relocatable
   native .o  (the component glue's import/export wiring becomes native linking)
```

## Proven (`./dissolve.sh`)

Both core modules dissolve, **synth exit 0** on each:

| core | synth | .text |
|---|---|---|
| module0 (kernel exports) | exit 0 | ~24.8 KB / 24 fns |
| module1 (app + run-demo) | exit 0 | ~24.2 KB / 16 fns |

So the *same* component running on wasmtime today also lowers to native with no
wasm runtime resident — the library-OS image.

## Honest caveat (FIND-BYOOS-006 · synth#401)

Those ~24 KB **include the component-adapter / `cabi_realloc` canonical-ABI
machinery** carried in the unbundled core — the dev/test runtime layer, not the
logic. Our kernel interfaces are scalar-in / enum-out, so the canonical ABI
needs no `realloc` at the call boundary; the bare gale-logic core dissolves to
hundreds of bytes (cf. the wasm-dist `sem` module at **544 B**). The **lean**
image links the bare gale-logic cores; stripping the unused adapter from a
dissolved core is filed as synth#401. This is the std/alloc "runtime layer
evaporates on dissolve" story made measurable (see `../../wit/README.md`).

## The three backings, one component

| backing | status |
|---|---|
| wasmtime (hosted, dev/test) | ✅ `run.sh` → `run-demo()=53` |
| dissolved-native (library OS) | ✅ `dissolve.sh` → both cores synth exit 0 (lean strip pending synth#401) |
| kiln-runtime (hosted, target) | ⛔ kiln#344 (kilnd component-model disabled) |

Isolation between dissolved components is the **opt-in** MPU/PMP layer
(gale#86), not the dissolve — verification is the primary line.

## Reproduce

```sh
export PATH="/opt/homebrew/opt/llvm/bin:/Users/r/.cargo/bin:$PATH"
./run.sh        # hosted backing on wasmtime
./dissolve.sh   # native library-OS backing (SYNTH_TARGET=cortex-m3 ./dissolve.sh)
```

# The library-OS backing — dissolving the composed component to native ELF

`run.sh` proves the composed component runs on **wasmtime** (the hosted backing).
`dissolve.sh` exercises the **native** backing via the canonical maximal-wasm
pipeline and is honest about where it's currently blocked.

## The canonical pipeline: `meld → loom → synth`

> *Meld fuses. Loom weaves. Synth transpiles. Kiln fires.*

```
gale-app-demo (imports gale:kernel)  +  gale-kiln (provides gale:kernel over gale::*)
        │  meld fuse   ← static component fusion: import resolution + index-space
        │              merge + canonical-ABI at BUILD time → ONE core module
        ▼
   fused core (gale:kernel imports resolved to 0)
        │  loom optimize --passes inline   (whole-program weave / DCE)
        ▼
   synth compile --relocatable             (native transpile)
        ▼
   native .o
```

**meld is the fusion stage** — it replaces runtime linking with a single
monolithic module. (An earlier revision wrongly used `wac` compose +
`wasm-tools unbundle`, which *preserves* per-component adapters and yields two
adapter-laden cores. That was a mis-step; meld is the correct stage.)

## Honest status — the lean MCU image is BLOCKED (gale#89)

| step | result |
|---|---|
| `meld fuse` (multi-memory, auto) | ✅ single fused core, `gale:kernel` imports resolved to 0 |
| `meld fuse --memory shared --address-rebase` (the MCU mode) | ⛔ **`memory.grow not supported with address rebasing`** |
| multi-memory fused → synth | partial: 2 memories, synth **loud-skips** the cross-memory copies (`#369` — correct, *not* a miscompile); not an MCU image |

The `memory.grow` is **not gale code**: it's `cabi_realloc` (exported,
wit-bindgen canonical ABI) → `__rust_alloc` → `dlmalloc` → `sbrk` →
`memory.grow`. After fusion the app↔kernel boundary is internal (0 imports
left), yet the fused core **still exports `cabi_realloc` ×2 / keeps
`memory.grow` ×2** — meld leaves the vestigial canonical-ABI adapter in place,
and being *exported* loom can't DCE it. That dead allocator is what blocks
`--memory shared`.

**Primary fix — meld#298:** on fusion with a scalar external surface, drop the
now-internal `cabi_realloc`/adapter so the allocator+grow DCE → `--memory shared`
→ one lean core (wasm-dist **544 B** class, not tens of KB) for loom+synth.
Building the components `#![no_std]` no-grow is a secondary belt-and-suspenders,
not the root cause. Gale-side tracker: **gale#89**.

## The three backings, one component

| backing | status |
|---|---|
| wasmtime (hosted, dev/test) | ✅ `run.sh` → `run-demo()=53` |
| dissolved-native (library OS) | 🔶 pipeline wired (`meld → loom → synth`); lean MCU image blocked on **gale#89** (no-grow components) |
| kiln-runtime (hosted, target) | ⛔ kiln#344 (kilnd component-model disabled) |

Isolation between dissolved components is the **opt-in** MPU/PMP layer (gale#86),
not the dissolve — verification is the primary line.

## Reproduce

```sh
export PATH="/opt/homebrew/opt/llvm/bin:/Users/r/.cargo/bin:$PATH"
./run.sh        # hosted backing on wasmtime (run-demo()=53)
./dissolve.sh   # canonical meld→loom→synth; shows the gale#89 MCU block honestly
```

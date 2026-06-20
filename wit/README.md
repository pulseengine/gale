# `wit/gale.wit` — the gale kernel-primitives Component Model world

The dependency-root of the **kiln component-host** path (Phase 2,
`SWREQ-KILN-001`; gale#63 / gale#74). It defines the verified gale kernel
primitives as WebAssembly **Component Model** interfaces so that a gale
application *component* can `import` them and the **kiln host provides them as
direct Rust calls into `gale::*`** — eliminating the C FFI boundary.

## Interfaces

`gale:kernel@0.1.0` — `sem`, `msgq`, `mutex`, `event`. Each function is the
SAME machine-checked decision the crate already ships (Verus/Rocq/Lean + Kani):
`sem.give` ↔ `gale::sem::give_decide`, `msgq.put` ↔ `gale::msgq::put_decide`,
etc. The WIT `enum` discriminants are in declaration order to match the crate's
`Decision` enums.

Two worlds:
- **`app`** — what a gale application component imports (the kernel it runs on).
- **`host`** — what the kiln host (or the Zephyr drop-in) exports; `gale-kiln`
  registers `gale::*` behind these.

## Status (honest)

- ✅ **WIT authored + validated** — round-trips through `wasm-tools component
  wit`; `wit-bindgen rust --world app` generates clean guest bindings.
- ⬜ **`gale-kiln` host crate** (`SWREQ-KILN-002`: register `gale::*` as kiln
  host functions) — not built yet; the next slice.
- ⬜ **End-to-end run** — instantiate an `app` component on `kiln-runtime` with
  the `host` world wired to `gale::*`. kiln-runtime *has* component-model
  support (`component_impl.rs`, `component_unified.rs`, async example) but
  hosting an external component this way needs validation.

## `std` / `alloc` reality — be precise about which parts

The dissolved **payload** (what synth turns into native ELF and what actually
runs on-device) is already `no_std` + `no_alloc`:

| part | status |
|---|---|
| gale verified primitives (`gale`, `src/lib.rs`) | **`#![no_std]`, no alloc** — pure decision fns |
| kiln-async scheduler core (`kiln-async`) | **`#![no_std]`, no `Box`/`Vec`/alloc** in core |
| browser demo crate | **`#![no_std]`** |
| wasm-dissolve C shim | **`-nostdlib`, freestanding** |

The `std`/`alloc` that exists is in the **host/dev-time runtime machinery**, not
the payload:

| part | uses | note |
|---|---|---|
| forked wit-bindgen async stream runtime | **alloc** (`Vec` stream buffers) | the amortized-alloc work; a wasm-host concern |
| `engine_control` component crate | `std` (not yet `#![no_std]`); async path pulls the alloc above | sync `step` is no_std/no_alloc-capable |
| `kiln-error` `recovery.rs` | **alloc** | gated out by kiln#338 (`no_alloc` feature) |

**Why this is mostly fine — synth puts the wasm to ELF.** Because the component
is *dissolved* (wasm → loom → synth → native ELF), the wasm async/runtime layer
and its allocations are **elided or replaced by the thin native TCB** — they do
not land on the device. So the `no_std`/`no_alloc` obligation is on the
*payload* (above, already met), and the runtime `alloc` is a testing/host
artifact that falls away on dissolve. Where we *do* keep alloc on-target (e.g.
the scheduler's recovery path), the plan is to introduce it as `no_alloc`
deliberately (kiln#338) — not leave it implicit.

## Sequence

1. **`wit/gale.wit`** (this file) — the contract. ✅
2. **`gale-kiln`** crate — implement the `host` world over `gale::*`
   (`SWREQ-KILN-002`).
3. **Run an `app` component on kiln** — e.g. the engine-control component
   (`benches/engine_control/wasm-component`) re-pointed at `gale:kernel`, or a
   sem/msgq smoke component — closing the no-C-FFI loop.
4. Then the `embedded-hal-async` driver leg (`SAC-BYOOS-HALSEAM`) sits on top.

## Regenerate / check

```sh
wasm-tools component wit wit/gale.wit          # validate
wit-bindgen rust --world app wit/gale.wit --out-dir /tmp/g   # guest bindings
```

# gale-kiln — the gale kernel as a WebAssembly Component (no C FFI)

Slice 2 of the kiln component-host path (Phase 2 / **SWREQ-KILN-002**; gale#63).

Implements `world host` from `../../wit/gale.wit` —
`gale:kernel/{sem,msgq,mutex,event}` — **directly over the verified `gale::*`
decision functions**. An application component that imports `gale:kernel` gets
its kernel decisions resolved by this component (or, on kiln, registered as host
functions), with **no C FFI / no `extern "C"` / no `#[no_mangle]`** boundary —
just the proven Rust (Verus/Rocq/Lean + Kani).

## Proven (`./run.sh`)

Builds to a wasip2 component, validates, and asserts the verified decision for
representative inputs run behind the WIT — e.g.:

```
sem.give(0,3,false) -> increment   sem.give(3,3,false) -> saturated
sem.give(0,3,true)  -> wake        mutex.lock(0,null,_,_) -> acquire
msgq.put(0,4,4,_,_) -> full        event.post(0,0b101,0b1111) -> 5
```

## std/alloc

The **payload is `gale` (`#![no_std]`, no alloc)** — the decisions. The wasip2
component wrapper around it is host/dev-time scaffolding; when synth dissolves
this to native ELF the wasm-runtime layer is elided/replaced by the thin TCB
(see `../../wit/README.md`).

## Next (slice 3)

Compose an `app` component (imports `gale:kernel`) with this host — e.g. a sem/
msgq smoke component, or the engine-control component re-pointed at the kernel —
and run the composed component on `kiln-runtime`, closing the no-C-FFI loop on
the actual kiln host.

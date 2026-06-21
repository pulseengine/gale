# gale-app-demo — the no-C-FFI component loop (kiln component-host slice 3)

A gale **application component** that *imports* `gale:kernel` (sem, msgq) — it
contains **no kernel logic**. Composed with [`gale-kiln`](../gale-kiln) (which
provides `gale:kernel` over the verified `gale::*` decisions), the app's kernel
calls resolve to proven Rust across the Component Model boundary — **no C FFI,
no `extern "C"`, no `#[no_mangle]`**.

## Proven (`./run.sh`)

Builds app + host, **composes them with `wac`**, runs the result on wasmtime:

```
composed imports remaining: (none — fully satisfied)
run-demo() = 53            # would-block(1) | increment(1)<<2 | full(3)<<4
```

i.e. the app called `sem.take/give` and `msgq.put`; each resolved to the
machine-checked `gale::*` decision in the composed host.

## Status — host backings

| host | status |
|---|---|
| **wasmtime** (dev/test) | ✅ composed loop runs (`run.sh`) |
| **kiln-runtime** (target) | ⛔ kilnd component-model support is *temporarily disabled*; runtime component API is internal/test-grade — tracked as a kiln-host gap. The composition is host-agnostic, so it lands on kiln unchanged once kilnd enables components. |
| **dissolved-native** | future: synth the composed core to ELF (the library-OS backing) |

## The loop, one line

`app (imports gale:kernel)` + `gale-kiln (gale::* behind gale:kernel)` --wac-->
`composed` → runs → verified decisions. The same composition is what kiln will
host and what synth will dissolve to a native library-OS image.

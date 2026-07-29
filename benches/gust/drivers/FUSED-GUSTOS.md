# Fused `gust:os` component (gale#224)

One wasm **component** that exports the `gust:os` capability set and whose only
residual imports are the `gust:hal/mmio` hardware seam and the app-supplied
`gust:os/taskdisp` dispatch. Produced by composing the five existing provider
components — no provider Rust logic and no WIT was changed to make this compose.

Reproduce: `bash benches/gust/drivers/build-fused-gustos.sh` (exits non-zero if any
invariant below is violated).

## Tool versions

| tool | version |
| --- | --- |
| `wasm-tools` | 1.245.1 |
| `wac` | wac-cli 0.10.1 |
| target | `wasm32-unknown-unknown` |

## Composition graph

The five providers form a **flat antichain**: each one exports a `gust:os`
capability, and none of them consumes another's export. The only import any of them
carries is a seam the *node* resolves — `gust:hal/mmio` (hardware) or
`gust:os/taskdisp` (the application's trusted task dispatch).

```
                          composite: gust:os
 gust:hal/mmio ──┬─► time-provider   ──► gust:os/time
                 ├─► log-provider    ──► gust:os/log
                 └─► timer-provider  ──► gust:os/timer
 gust:os/taskdisp┬─► spawn-provider  ──► gust:os/spawn
                 └─► exec-provider   ──► gust:os/exec
```

`gust:os/taskdisp` surviving as an import is **correct, not a gap**: `poll-task` is
provided by the application, not by any provider (see `wit-os/gust-os.wit`).

One discrepancy worth recording: `world timer-provider` in `wit-os/gust-os.wit`
declares `import taskdisp`, but the built `timer-provider` component imports **only**
`gust:hal/mmio`. The timer path uses the executor's `set_deadline`/`slept_status`
only and never reaches `poll_task`, so the symbol is dropped at link and
`wasm-tools component new` records no `taskdisp` import. The WIT world is therefore
wider than the component actually needs. Not changed here (no WIT edits in scope),
but it means the graph above is derived from the built artifacts, not from the
declared worlds.

### `wac plug` does not apply here — `wac compose` does

The task framing assumed a `wac plug` chain. There is no import/export edge between
any two providers for `plug` to connect, so every plug invocation fails:

```
$ wac plug exec-provider.component.wasm \
    --plug time-provider.component.wasm --plug log-provider.component.wasm \
    --plug spawn-provider.component.wasm --plug timer-provider.component.wasm \
    -o fused.wasm
error: the socket component had no matching imports for the plugs that were provided
```

The same error occurs for every pairwise ordering tried (`exec`←`spawn`,
`spawn`←`exec`, `timer`←`time`+`exec`). The needed operation is a **union merge**
with import forwarding, which is `wac compose` over a WAC script. `{ ... }` in each
`new` forwards that instance's unsatisfied imports to the composite's own imports,
which is what leaves exactly `gust:hal/mmio` + `gust:os/taskdisp` as residuals:

```wac
package gale:fused-gustos@0.1.0;

let t = new gust:time-provider  { ... };
let l = new gust:log-provider   { ... };
let s = new gust:spawn-provider { ... };
let x = new gust:exec-provider  { ... };
let m = new gust:timer-provider { ... };

export t["gust:os/time@0.1.0"];
export l["gust:os/log@0.1.0"];
export s["gust:os/spawn@0.1.0"];
export x["gust:os/exec@0.1.0"];
export m["gust:os/timer@0.1.0"];
```

## The composite's exact WIT

Verbatim `wasm-tools component wit fused-gustos.component.wasm`:

```wit
package root:component;

world root {
  import gust:hal/mmio@0.1.0;
  import gust:os/taskdisp@0.1.0;

  export gust:os/time@0.1.0;
  export gust:os/log@0.1.0;
  export gust:os/spawn@0.1.0;
  export gust:os/exec@0.1.0;
  export gust:os/timer@0.1.0;
}
package gust:hal@0.1.0 {
  interface mmio {
    read32: func(addr: u32) -> u32;

    write32: func(addr: u32, val: u32);
  }
}


package gust:os@0.1.0 {
  interface taskdisp {
    poll-task: func(id: u32) -> u32;
  }
  interface time {
    now: func() -> u64;

    deadline: func(now: u64, ticks: u64) -> u64;

    elapsed: func(now: u64, deadline: u64) -> bool;

    resolution: func() -> u64;
  }
  interface log {
    line: func(msg: list<u8>);
  }
  interface spawn {
    start: func(entry: u32) -> u32;

    poll: func(handle: u32) -> u32;
  }
  interface exec {
    admit: func(prio: u32, deadline-lo: u32, deadline-hi: u32) -> u32;

    poll-round: func(now-lo: u32, now-hi: u32);

    state: func(h: u32) -> u32;
  }
  interface timer {
    sleep: func(handle: u32, ticks: u32) -> u32;

    slept: func(handle: u32) -> u32;
  }
}
```

## Size

| artifact | bytes |
| --- | --- |
| `time-provider.component.wasm` | 2 518 |
| `log-provider.component.wasm` | 1 835 |
| `spawn-provider.component.wasm` | 2 614 |
| `exec-provider.component.wasm` | 7 097 |
| `timer-provider.component.wasm` | 2 220 |
| sum of the five | 16 284 |
| **`fused-gustos.component.wasm`** | **16 905** |

The composite is 621 B larger than the concatenated inputs — the WAC-generated
instantiation/adapter wiring. Nothing is deduplicated, because nothing is shared:
the composite holds 5 core modules with 5 independent linear memories.

## What is verified

`build-fused-gustos.sh` gates all of these and exits non-zero on any violation:

1. **Is a component** — `wasm-tools component wit` decodes.
2. **Validates** — `wasm-tools validate --features=all` passes.
3. **Exports** — exactly 5 exports, one per `gust:os` capability the providers supply.
4. **Residual imports** — an exact allow-list of `gust:hal/*` and `gust:os/taskdisp`.
   Note this is *stricter* than the per-provider gate in
   `build-gustos-components.sh` (which allows any `gust:os/*` import): after fusing,
   any other surviving `gust:os/*` import would mean a capability the composite
   failed to satisfy in place.
5. **No symbol leak** — a deny-list sweep over every raw core-wasm import string in
   the binary (not just the component-level world) for `env`, `wasi*`, `GOT.*`,
   `malloc`/`calloc`/`sbrk`/`__heap*`, and `poll_task`/`scheduler`/`task_*`. The only
   raw core imports present are `gust:hal/mmio` (`read32`/`write32`),
   `gust:os/taskdisp` (`poll-task`), and `wit-component`'s internal
   `import-func-*` adapter shims.

## What is NOT done

Read this section before repeating any claim from this file.

- **Not lowered to native.** This is a wasm component only. It has not been put
  through synth/meld dissolve, so there is no `.o`, no code size, no cycle count and
  no SRAM figure for it. The 16 905 B above is the size of a **wasm** artifact and
  says nothing about the eventual firmware footprint.
- **Not run.** Not on hardware, not in Renode, not in qemu, not in a wasm runtime.
  Composition and validation are static checks only; no `gust:os` call has been
  executed through this composite.
- **The three executor instances are not one scheduler.** `exec-provider`,
  `spawn-provider` and `timer-provider` each `#[path]`-include the same
  `plain/src/executor.rs` verbatim, and each keeps its own `static mut TASKS` table.
  In the composite these land in three *separate* component instances with three
  separate linear memories, so a handle from `spawn.start()` is meaningless to
  `timer.sleep()` and to `exec.state()`. The composite is structurally fused but
  **semantically triplicated**; a single shared scheduler needs a WIT/architecture
  change (the three would have to route through one executor instance, e.g. by
  making `spawn` and `timer` import `exec` rather than embed it) and is deliberately
  out of scope here — no provider logic or WIT was changed for this task.
- **Memory footprint is unshaped.** The 5 core modules declare 17, 17, 1, 1 and 17
  wasm pages respectively (`time`, `log`, `timer` still carry the default 1 MiB
  shadow-stack reservation; `exec` and `spawn` are at 1 page thanks to their
  `.cargo/config.toml` `-zstack-size=1024`). This is only meaningful after lowering,
  but it is the shape the lowering step will start from.
- **`log` is not 0-SRAM.** `log.line` takes `list<u8>`, so the canonical ABI needs a
  real shared linear memory. The composite as a whole is therefore bounded-SRAM, not
  0-SRAM.
- **No verification claim is added by this composition.** The providers' existing
  Kani/Verus results are unchanged and unextended; composing components proves
  nothing about their contents.

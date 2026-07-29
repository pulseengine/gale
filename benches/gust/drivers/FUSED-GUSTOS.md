# Fused `gust:os` component (gale#224)

One wasm **component** that exports the `gust:os` capability set and whose only
residual imports are the `gust:hal/mmio` hardware seam and the app-supplied
`gust:os/taskdisp` dispatch. Composed from the five provider components.

Reproduce: `bash benches/gust/drivers/build-fused-gustos.sh` (exits non-zero if any
invariant below is violated).

## Tool versions

| tool | version |
| --- | --- |
| `wasm-tools` | 1.245.1 |
| `wac` | wac-cli 0.10.1 |
| target | `wasm32-unknown-unknown` |

## One scheduler, one clock

The governing rule: **a provider must import every capability it does not own.**

| provider | owns | imports | exports |
| --- | --- | --- | --- |
| `exec` | the executor (the only `Tasks`) | `gust:os/taskdisp` | `gust:os/exec`, `gust:sched/tasks` |
| `spawn` | nothing | `gust:sched/tasks` | `gust:os/spawn` |
| `timer` | nothing | `gust:sched/tasks`, `gust:os/time` | `gust:os/timer` |
| `time` | the clock (TIM2 via mmio) | `gust:hal/mmio` | `gust:os/time` |
| `log` | — | `gust:hal/mmio` | `gust:os/log` |

```
                                   composite: gust:os
 gust:hal/mmio ──┬─► time-provider ──┬──────────────────────► gust:os/time
                 │                   └──► timer-provider ───► gust:os/timer
                 └─► log-provider  ─────────────────────────► gust:os/log
                                            ▲ gust:sched/tasks
 gust:os/taskdisp ─► exec-provider ─────────┼──────────────► gust:os/exec
                          └── gust:sched/tasks ─► spawn-provider ─► gust:os/spawn
```

`gust:os/taskdisp` surviving as an import is **correct, not a gap**: `poll-task` is
provided by the application, not by any provider. `gust:sched/tasks` NOT surviving is
equally load-bearing — see below.

### What was wrong before

`exec-provider`, `spawn-provider` and `timer-provider` each `#[path]`-included the
same `plain/src/executor.rs` and each kept its own `static mut TASKS`. Composed, that
was three component instances with three separate scheduler states: a handle from
`spawn.start()` named a different task in `timer.sleep()` and in `exec.state()`. The
composite was structurally fused but **semantically triplicated**. Separately,
`timer-provider` and `time-provider` each hardcoded `const TIM2_CNT = 0x4000_0024`
and read it independently — one clock only because two constants happened to match.

`wit-os/gust-os.wit` already promised otherwise: `timer.sleep`'s doc calls its
argument "the handle the app holds from spawn.start". The WIT specified a shared
handle space; the implementation did not provide one.

### What is true now

`exec-provider` is the single owner. `spawn-provider` and `timer-provider` are
**stateless** — no `static mut`, no task table, no mapping table, no executor
include. A spawn handle IS the executor's handle, returned unchanged; timer operates
on the app's handle unchanged. `timer-provider` reads the clock through
`gust:os/time` and has **no `gust:hal` import at all**. `plain/src/executor.rs` is
untouched and still embedded in exec-provider only; it carries the Verus/Kani proofs.

### `gust:sched@0.1.0` — a separate package on purpose

The internal seam lives in `wit-os/deps/sched/gust-sched.wit`, NOT in `gust:os`.
`world app` is the published surface; a tenant that imports `gust:os` must not be
able to reach scheduler mutation (`admit`, `wake`, `set-deadline`, …). Because the
composite satisfies `gust:sched/tasks` in place, the name never appears in the fused
component's world, so there is nothing for a downstream to bind to. That is gated,
not asserted — gate 3 below, with a negative test.

## Composition: still `wac compose`, but no longer a union

`wac plug` DOES apply now, and did not before: the providers are no longer a flat
antichain, so plug finds matching imports instead of exiting with "the socket
component had no matching imports for the plugs that were provided". Verified:

```
$ wac plug timer-provider.component.wasm \
    --plug exec-provider.component.wasm --plug time-provider.component.wasm -o t2.wasm
$ wasm-tools component wit t2.wasm
world root {
  import gust:hal/mmio@0.1.0;
  import gust:os/taskdisp@0.1.0;
  export gust:os/timer@0.1.0;
}
```

It still cannot build this deliverable: **a plug result exports only the SOCKET's
exports**, and the composite must export five interfaces that live in five different
components. There is no socket whose exports are the union. `wac plug` also wires
plugs into the socket's imports ONLY, never plug→plug — passing exec-provider as an
extra `--plug` alongside spawn-provider leaves `gust:sched/tasks` unsatisfied, which
is why `build-os-ts.sh` moved off `plug` too.

So the deliverable is `wac compose` over a WAC script, as before — but the script now
names real edges instead of forwarding everything outward with `{ ... }`:

```wac
package gale:fused-gustos@0.1.0;

let x = new gust:exec-provider  { ... };
let t = new gust:time-provider  { ... };
let l = new gust:log-provider   { ... };
let s = new gust:spawn-provider { "gust:sched/tasks@0.1.0": x["gust:sched/tasks@0.1.0"], ... };
let m = new gust:timer-provider { "gust:sched/tasks@0.1.0": x["gust:sched/tasks@0.1.0"],
                                  "gust:os/time@0.1.0":     t["gust:os/time@0.1.0"], ... };

export t["gust:os/time@0.1.0"];
export l["gust:os/log@0.1.0"];
export s["gust:os/spawn@0.1.0"];
export x["gust:os/exec@0.1.0"];
export m["gust:os/timer@0.1.0"];
```

`{ ... }` still forwards each instance's REMAINING unsatisfied imports outward, which
is what leaves `gust:hal/mmio` + `gust:os/taskdisp` — and only those — as residuals.

## The composite's exact WIT

Verbatim `wasm-tools component wit fused-gustos.component.wasm`:

```wit
package root:component;

world root {
  import gust:os/taskdisp@0.1.0;
  import gust:hal/mmio@0.1.0;

  export gust:os/time@0.1.0;
  export gust:os/log@0.1.0;
  export gust:os/spawn@0.1.0;
  export gust:os/exec@0.1.0;
  export gust:os/timer@0.1.0;
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


package gust:hal@0.1.0 {
  interface mmio {
    read32: func(addr: u32) -> u32;

    write32: func(addr: u32, val: u32);
  }
}
```

Note `gust:sched` does not appear anywhere in it.

## Size

| artifact | bytes (before) | bytes (now) |
| --- | --- | --- |
| `time-provider.component.wasm` | 2 518 | 2 518 |
| `log-provider.component.wasm` | 1 835 | 1 835 |
| `spawn-provider.component.wasm` | 2 614 | 2 598 |
| `exec-provider.component.wasm` | 7 097 | 10 246 |
| `timer-provider.component.wasm` | 2 220 | 2 867 |
| sum of the five | 16 284 | 20 064 |
| **`fused-gustos.component.wasm`** | **16 905** | **20 741** |

The composite grew by 3 836 B. Almost all of it is exec-provider (+3 149 B): it now
exports a second interface with six functions, and `wasm-tools component new`'s
canonical-ABI wiring plus the embedded WIT type section pay for that. spawn and timer
did NOT shrink at the component level even though each lost an entire copy of the
executor — the executor's core code was small next to the per-component WIT metadata,
and timer gained a whole imported interface's types. The saving shows up in the
DISSOLVED objects, where metadata is gone: spawn-provider-cm3 text 1108 → 376 B,
timer-provider-cm3 text 844 → 552 B (measured into temp paths; the checked-in `.o`
files were deliberately not regenerated).

## What is verified

`build-fused-gustos.sh` gates all of these and exits non-zero on any violation:

1. **Is a component / validates** — `wasm-tools component wit` decodes,
   `wasm-tools validate --features=all` passes.
2. **Exports** — exactly 5, one per `gust:os` capability.
3. **Residual imports** — an exact allow-list of `gust:hal/*` and `gust:os/taskdisp`.
   `gust:sched/*` is deliberately NOT on it (the per-provider gate in
   `build-gustos-components.sh` does allow it — before composition it is a legitimate
   import). A `gust:sched` residual means spawn/timer were never actually bound to
   exec-provider's executor, i.e. the whole point of this change did not happen.
   **Negative-tested in-band, every run**: the script also composes the same five
   components with an unwired script (`{ ... }` everywhere, no edges) and requires the
   identical check to REJECT it. Real output:

   ```
   == negative test: the residual gate must reject a mis-composed input ==
           unexpected residual: import gust:sched/tasks@0.1.0
           unexpected residual: import gust:os/time@0.1.0
     ok: gate rejected the unwired composite (as it must) — the gate above is live
   ```

4. **Exactly one scheduler instance.** The script unbundles the composite back into
   its five core modules (`wasm-tools component unbundle --threshold 0`) and asserts
   the ROUTING from each module's import list: exactly one module imports the trusted
   `poll-task` dispatch (and imports no `gust:sched`, because it OWNS the table rather
   than calling one); exactly one routes spawn's `admit`/`wake` over `gust:sched`; and
   exactly one routes timer's `set-deadline`/`slept-status` over it. A provider
   holding a private table would do those operations locally and therefore would not
   import them. Real output:

   ```
   == one scheduler ==
     unbundled-module0.wasm     7573 B  dispatch=1 spawn-route=0 timer-route=0  imports: gust:os/taskdisp poll-task
     unbundled-module1.wasm     1219 B  dispatch=0 spawn-route=0 timer-route=0  imports: gust:hal/mmio read32
     unbundled-module2.wasm     1078 B  dispatch=0 spawn-route=0 timer-route=0  imports: gust:hal/mmio write32
     unbundled-module3.wasm     1452 B  dispatch=0 spawn-route=1 timer-route=0  imports: gust:sched/tasks state gust:sched/tasks poll-round gust:sched/tasks admit gust:sched/tasks wake
     unbundled-module4.wasm     1463 B  dispatch=0 spawn-route=0 timer-route=1  imports: gust:os/time now gust:os/time deadline gust:sched/tasks set-deadline gust:sched/tasks slept-status
     ok: one dispatcher, and both spawn's and timer's scheduler operations route to it — one task table
   ```

   That table doubles as the import ledger: module4 (timer) shows no `gust:hal` entry.

   **A signal that was tried and REJECTED**, recorded so nobody re-adds it: looking
   for the executor's source path inside each module (rustc bakes it into the
   bounds-check panic location of code that indexes the task table). It is not a
   detector. Rebuilt from `HEAD`, neither the pre-change spawn-provider nor the
   pre-change timer-provider core module contains that string — both bounds-check
   before indexing, so the panic path is elided — yet both DID hold a full private
   `Tasks`. A gate built on it would have passed the triplicated composite while
   reporting "exactly one". A `Tasks` table lives in wasm `.bss`, which a module never
   declares, so there is no reliable binary signal for "this module holds one"; hence
   the routing check here, plus the exact source-level ownership check in
   `build-gustos-components.sh` (only one crate may `#[path]`-include the executor).

   **Negative-tested** by rebuilding the pre-change spawn/timer providers from `HEAD`,
   composing them the old way, and running this gate's logic on the result:

   ```
   ### gate 4 applied to the PRE-CHANGE (triplicated) composite:
     unbundled-module2.wasm   dispatch=1 spawn-route=0 timer-route=0  imports: gust:os/taskdisp poll-task
     unbundled-module3.wasm   dispatch=1 spawn-route=0 timer-route=0  imports: gust:os/taskdisp poll-task
     FAIL: expected exactly 1 module importing the poll-task dispatch, got 2
     FAIL: expected exactly 1 module routing spawn's admit/wake over gust:sched, got 0
     FAIL: expected exactly 1 module routing timer's set-deadline/slept-status over gust:sched, got 0
   ```

   This one runs by hand (it needs pre-change sources), unlike gate 3's negative test
   which is in-band on every run.

5. **No symbol leak** — a deny-list sweep over every raw core-wasm import string in
   the binary (not just the component-level world) for `env`, `wasi*`, `GOT.*`,
   `malloc`/`calloc`/`sbrk`/`__heap*`, and `poll_task`/`scheduler`/`task_*`.

`build-gustos-components.sh` additionally gates, at source, that exactly one provider
crate carries a `#[path]` include of `plain/src/executor.rs` and that it is
`exec-provider` — the exact form of "only one component may hold scheduler state".

## What is NOT done

Read this section before repeating any claim from this file.

- **Not lowered to native.** This is a wasm component only. The composite has not
  been through synth/meld dissolve, so there is no `.o`, no code size, no cycle count
  and no SRAM figure for it. The 20 741 B above is a **wasm** artifact's size and says
  nothing about firmware footprint. (The step-3 `os-ts` node, a different and smaller
  composition, does dissolve — `build-os-ts.sh`, 7 836 B / 8 192 budget.)
- **Not run.** Not on hardware, not in Renode, not in qemu, not in a wasm runtime.
  Composition and validation are static checks only; no `gust:os` call has been
  executed through this composite. In particular, "one scheduler" is verified
  STRUCTURALLY (one instance holds the state, and the other two are wired to it) —
  nobody has executed `spawn.start()` and then observed the same handle in
  `timer.sleep()` through this composite. The qemu probes that do run
  (`gust_exec_probe`, `gust_timer_probe`) exercise the executor natively and do not
  go through these components at all.
- **Linear memories: still five, and that is not the defect.** The composite declares
  5 memories, one per component instance, at 1/17/17/1/17 pages — unchanged by this
  work. The Component Model is shared-nothing; every instance gets its own memory by
  design. The defect was three copies of the *scheduler state*, not the memory count,
  and no memory-count reduction is claimed here because none was measured.
- **`log` is not 0-SRAM.** `log.line` takes `list<u8>`, so the canonical ABI needs a
  real shared linear memory. The composite as a whole is bounded-SRAM, not 0-SRAM.
- **No verification claim is added by this composition.** The providers' existing
  Kani/Verus results are unchanged and unextended. `plain/src/executor.rs` was not
  edited, so its proofs stand exactly as before; nothing here proves anything new
  about its contents. What changed is which components may hold its state.
- **Checked-in objects are stale by choice.** `os-node/os-ts-cm3.o`,
  `spawn-provider-cm3.o` and `timer-provider-cm3.o` were NOT regenerated, so they and
  the probes that link them reflect the pre-change composition.

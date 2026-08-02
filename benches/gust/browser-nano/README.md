# gale-nano in a browser tab — the third runtime

The **same signed component** an STM32 runs a dissolved version of, executing in a browser.

    wkg oci pull ghcr.io/pulseengine/gale-nano:0.6.0     one signed artifact, 20 741 B
      ├── jco transpile      -> this page
      ├── wasmtime host      -> drivers/gustos-hostrun  (REQ-OS-COMPOSITE-EXEC-001)
      └── meld+loom+synth    -> STM32, no runtime present at all

`build.sh` starts from `wkg oci pull`, not from `cargo build`, on purpose: the demo's
whole claim is that these are the *same bytes*, not a browser-shaped rebuild of the same
source. It also runs `cosign verify`, so the artifact's provenance is checkable here too.

## What differs between the three

Only who answers `gust:hal/mmio` — a JS array here, a Rust array under wasmtime, real
registers on silicon. That substitution is the thesis: **change the tires, not the car.**

## What the page asserts

The same script the wasmtime host runs: a handle from `spawn.start()` is accepted by
`timer.sleep()`, dispatched by `exec.poll-round()`, and agreed done by all three. Plus the
one-clock checks — arming performs exactly one clock read, `time.now()` tracks a jump in
the register, and every read went to one address.

10/10 in Chrome as of 2026-08-02.

## Two things this page learned the hard way

**Do not call an interface inside an assertion's message argument.** The first version
called `timer.sleep()` twice — once in the condition, once in the string — which re-armed
the deadline and produced a failure that was the test's, not the component's.

**The app is the time source.** `exec.poll-round` takes `now` as a caller argument while
`timer.slept` reads the clock register. The composite has no internal clock driving expiry.
An app that advances one without the other is being two clocks, and the page caught itself
doing exactly that. `tick()` moves both together.

## Run

    ./build.sh
    cd web && python3 -m http.server 8000     # http://localhost:8000/

# timer-provider — gust:os `timer` backed by the real executor (Task 4, v0.4.0 timer-sleep)

Backs the `gust:os/timer` WIT interface (world `timer-provider`, `wit-os/gust-os.wit`:
`sleep: func(handle: u32, ticks: u64) -> u32; slept: func(handle: u32) -> u32;`)
with the SAME verified executor deadline table that `spawn-provider`/`exec-provider`
dissolve — not a hand-written placeholder. `src/lib.rs` includes
`plain/src/executor.rs` verbatim, exactly as `spawn-provider` does, and calls the
Task 2 additions (`Tasks::set_deadline`, `Tasks::slept_status`) directly as plain
(non-FFI) methods.

RESOLVED SEAM (task-4 brief, exact contract implemented):
- `sleep(handle, ticks)`: reject `ticks >= 2^31` (return `0xFFFF_FFFF`); else
  `d = now().wrapping_add(ticks)`; call `Tasks::set_deadline(handle, d)`; return `0`.
  `set_deadline` itself is a no-op on a non-Pending/out-of-range handle
  (Kani-framed by `set_deadline_sets_only_h`), so this marshalling layer
  re-implements no admission decision.
- `slept(handle)`: return `Tasks::slept_status(handle, now())` directly (0 pending /
  1 elapsed / 0xFFFF_FFFF invalid).
- `now()` reads `TIM2_CNT` (`0x4000_0024`) via `gust:hal/mmio.read32` — the SAME
  register `time-provider` reads for `gust:os/time.now`.

Unlike `spawn-provider`'s `poll`, neither `sleep` nor `slept` calls
`poll_round`/`dispatch_one`, so this crate never crosses the trusted
`taskdisp`/`poll_task` FFI seam — confirmed empirically (see "ABI" below):
`poll_task` is dead-code-eliminated (`cargo build` itself warns `function
'poll_task' is never used`) and does not appear as an import in the compiled
wasm. The world `timer-provider` still declares `import taskdisp` in the WIT
(`wit-os/gust-os.wit`), but it is unused — this crate's only cross-module
symbol is `gust:hal/mmio.read32`.

**Known limitation (v1):** this crate's lazily-initialized `Tasks` table (the
`MaybeUninit<Tasks>` + `TASKS_INIT` flag, same straddle-avoidance pattern as
`spawn-provider`) is a SEPARATE instance from `spawn-provider`'s. v1 does not
share the deadline table across providers, so in a real composed node a task
admitted by `spawn-provider` would not be visible to `timer-provider`'s
`set_deadline`/`slept_status` (both would silently no-op / never reach `1`
against a `Free` slot 0's default `u64::MAX` deadline). Reconciling this needs
either a shared-state design (mirroring the dma-own `own<buffer>` handoff
precedent) or moving `sleep`/`slept` into the same provider as `spawn`. Out of
scope for Task 4; flagged for the executor-integration follow-on.

## Build

    cargo build --release --target wasm32-unknown-unknown

No `.cargo/config.toml` / `--allow-undefined` is needed (unlike `spawn-provider`):
since `sleep`/`slept` never reach `poll_round`, the raw `extern "C" poll_task`
declared inside the included `executor.rs` is dead code and the linker never
sees an unresolved reference to it.

    $ cargo build --release --target wasm32-unknown-unknown
    warning: function `poll_task` is never used
       --> src/../../../../../plain/src/executor.rs:33:12
    warning: `gust-timer-provider` (lib) generated 11 warnings
        Finished `release` profile [optimized] target(s) in 8.96s

## ABI: confirmed

    $ wasm-tools print target/wasm32-unknown-unknown/release/gust_timer_provider.wasm | grep -E "import|export"
      (import "gust:hal/mmio@0.1.0" "read32" (func ...))
      (export "memory" (memory 0))
      (export "gust:os/timer@0.1.0#sleep" (func $gust:os/timer@0.1.0#sleep))
      (export "gust:os/timer@0.1.0#slept" (func $gust:os/timer@0.1.0#slept))
      (export "cabi_realloc_wit_bindgen_0_52_0" (func $cabi_realloc_wit_bindgen_0_52_0))
      (export "cabi_realloc" (func $cabi_realloc))

Sole import is `gust:hal/mmio.read32` — no `taskdisp`/`poll_task` at all. Both
`timer` exports are present with the WIT-declared names/shapes.

    $ wasm-tools component new target/wasm32-unknown-unknown/release/gust_timer_provider.wasm -o /tmp/timer-provider.comp.wasm
    $ wasm-tools validate /tmp/timer-provider.comp.wasm
    (exit 0 — valid component, embedded world)

## Dissolve: `slept` clean, `sleep` BLOCKED on a synth ARM-backend limitation

    loom-1.2.0 optimize /tmp/timer-provider.comp.wasm --passes inline --attestation false -o /tmp/timer-provider.loom.wasm
      Component:    2225 -> 1765 bytes (20.7% reduction)
      Module size:  1256 -> 1256 bytes (0.0% reduction)
      Status:       Successfully optimized 1 of 1 core modules

    synth-0.49.0 compile /tmp/timer-provider.loom.wasm --target cortex-m3 --all-exports --relocatable -o timer-provider-cm3.o
      Compiling function 'gust:os/timer@0.1.0#sleep' via backend 'arm'...
      warning: skipping function 'gust:os/timer@0.1.0#sleep': backend 'arm' failed: compilation
        failed: instruction selection failed: Synthesis failed: #518: an i64/f64 param in a
        frame-backing function (contains a call, or the register-pair-exhaustion retry) is not
        yet lowered — the param_slots path drops the high half
      Compiling function 'gust:os/timer@0.1.0#slept' via backend 'arm'...
        346 bytes of machine code
      warning: 1 of 5 functions were skipped (not in output): gust:os/timer@0.1.0#sleep
      Compiled 4 functions to timer-provider-cm3.o
        Total code size: 426 bytes / ELF size: 1286 bytes / 3 relocations (ET_REL)

**`gust:os/timer@0.1.0#sleep` is dropped from the output object.** Root cause,
per synth's own diagnostic: `sleep(handle: u32, ticks: u64) -> u32` has a `u64`
(core-wasm `i64`) parameter AND its body makes a call (`read32` for `now()`,
plus the internal `set_deadline` call) — synth's ARM backend does not yet
lower an i64 parameter through a function that also needs a call-preserving
stack frame ("frame-backing"). This is a genuine, reproducible backend
limitation, not a symptom of our source shape or of loom's optimization:

- `--native-pointer-abi` added: identical failure (`timer-provider-cm3-npabi.o`
  attempt, same #518 message).
- `--no-optimize` (bypasses synth's own optimizer): identical failure.
- Compiling the PRE-loom component directly (skip loom entirely): identical
  failure — rules out loom's `--passes inline` as the cause.
- synth 0.45.1 (the version `spawn-provider`/`exec-provider` pin) on the SAME
  input: identical failure — not a 0.49.0 regression; the limitation predates
  this task (nothing shipped before Task 4 combined a u64 WIT param with an
  in-body call: `time-provider`'s `deadline(now: u64, ticks: u64) -> u64` has
  two i64 params but its body is pure register arithmetic, `now.wrapping_add
  (ticks)` — no call, so it never hit this path).

No source-level workaround exists: `sleep`'s WIT signature (`ticks: u64`) is
fixed by the already-committed Task 3 interface, and the call to `read32`
(`now()`) cannot be inlined away — it is a genuine cross-module wasm import
call, so the function is unavoidably "frame-backing" under synth's own
definition regardless of how the Rust body is written.

## What DID dissolve clean: `slept`

    $ arm-none-eabi-nm timer-provider-cm3.o
    00000178 T cabi_realloc
    00000160 T cabi_realloc_wit_bindgen_0_52_0
    00000000 t func_1
    00000004 t func_4
    00000160 t func_5
    00000178 t func_6
    00000004 T gust:os/timer@0.1.0#slept
             U read32

    $ arm-none-eabi-size timer-provider-cm3.o
       text    data     bss     dec     hex filename
        428       0       0     428     1ac timer-provider-cm3.o

`slept` (a `u32`-only-param function; no i64 anywhere in its signature) compiles
cleanly. Its sole undefined symbol is `read32` — the same mmio TCB class every
other thin/provider driver in this repo uses, no new TCB kind introduced.
`sleep` does not appear in the symbol table at all (skipped, not emitted as a
stub) — this object does **not** implement the full `gust:os/timer` interface.

## Full `app-timer` compose: NOT attempted

`world app-timer { import time; import timer; export run: func() -> u32; }`
would need a test app + `wac plug` (with `time-provider`) + `meld fuse
--memory shared` + `synth compile`. This was **not attempted**: with `sleep`
entirely absent from the dissolved object, a wac-plug compose of `timer-
provider` into an `app-timer` node would either fail to satisfy the app's
`sleep` import or silently link against nothing for it — there is no honest
"it composed" claim to make while half the interface doesn't exist in native
code. Per the task's own guidance ("do NOT claim a fuse worked that you
didn't see succeed"), this step is deferred until `sleep` itself dissolves.

## Summary

| Export | Component (wasm) | loom optimize | synth compile (cortex-m3) |
|---|---|---|---|
| `sleep(handle: u32, ticks: u64) -> u32` | present, correct ABI | passes through | **SKIPPED** — synth ARM backend #518 (i64 param + in-body call not lowered) |
| `slept(handle: u32) -> u32` | present, correct ABI | passes through | compiles clean, 346 B machine code, sole undefined symbol `read32` |

Deliverable status: the crate builds and componentizes cleanly end-to-end
(Steps 1-3 of the task complete, confirmed by command output above). The
dissolve step (Step 4) is **partial**: `slept` alone reaches a clean
relocatable object with the expected TCB-class undefined symbol; `sleep` is
blocked on a reproducible synth ARM-backend limitation, independent of synth
version (0.45.1, 0.49.0), loom, and compile flags tried. This is new friction
— nothing upstream of this task combined a WIT `u64` parameter with an
in-body call — and is the first concrete blocker for `gust:os/timer` reaching
full dissolve; it belongs to the synth backend, not to this crate's source.

## CORRECTION 2026-07-21: sleep DISSOLVES via u32 ticks (u64-param decline worked around)

The initial u64 `ticks` param made synth's ARM backend loud-DECLINE `sleep` (a 64-bit
param in a frame-backing function that makes a call — synth references the closed #518
i64-param class; it declines rather than miscompiles, the safe behaviour). Since v1 bounds
`ticks < 2^31` (fits u32), the seam is `sleep(handle: u32, ticks: u32)`, widened to u64
internally for the deadline. synth 0.49.0 now compiles BOTH exports:

    Compiling function 'gust:os/timer@0.1.0#sleep' via backend 'arm'...
    Compiling function 'gust:os/timer@0.1.0#slept' via backend 'arm'...
    Compiled 5 functions to timer-cm3.o

Dissolved object: 1783 B, exports `gust:os/timer#sleep` + `#slept` (both T symbols).
UNDEFINED symbols = `read32` ONLY (the mmio now() TCB) — set_deadline/slept_status inline
from the included executor module, poll_task DCE'd. **TCB = 1 atom (read32)**, same class
as every thin driver, no new TCB kind.

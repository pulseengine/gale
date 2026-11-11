# gust-fused — the full-pipeline demonstrator

**The claim:** the *same* Component-Model composition that runs on wasmtime today
dissolves to a native bare-metal image and produces the *identical* result — no
wasm runtime on the target. "Components on top, which we drive; meld them down
into one module we fuse; run on the gust stack."

```
  gale-app-demo  (component: imports gale:kernel)          ← the app we drive
  gale-kiln      (component: provides gale:kernel
                  over the verified gale::* decisions)      ← the verified OS
        │  wac plug / meld fuse
        ▼
  ── runs on wasmtime today ──────────────────────────────  run-demo() = 53
        │  (crates/gale-app-demo/run.sh)
        │
        │  DISSOLVE (build-fused.sh):
        │   component new --import-passthrough env::__cabi_arena_realloc
        │   meld fuse --memory shared --address-rebase   → one merged memory
        │   loom optimize --passes inline
        │   strip exports → {memory, run-demo}           → DCE the realloc path
        │   synth compile --target cortex-m3 --all-exports --relocatable
        ▼
  wasm-kernel/fused.o   (ET_REL, .text ~668 B, 0 undefined symbols)
        │  build.rs links it into the gust_fused TCB bin
        ▼
  ── runs bare-metal on Cortex-M3 (no wasm runtime) ──────  run-demo() = 53
        (cargo run --release --bin gust_fused / run-fused.sh)
```

## Parity (the oracle)

| Surface | How it runs | `run-demo()` |
|---|---|---|
| wasmtime | composed P2 component, host runtime | **53** |
| Cortex-M3 (qemu lm3s6965evb) | meld-fused → synth-dissolved native, TCB shim, **no runtime** | **53** |

`53` = `take(0,true)`=would-block(1) │ `give(0,3,false)`=increment(1)<<2 │
`put(0,4,4,_,true)`=full(3)<<4. Verify both sides:

```sh
( cd ../../crates/gale-app-demo && ./run.sh )   # wasmtime → 53
./run-fused.sh                                    # bare-metal Cortex-M3 → 53
```

Kill-criterion: either side yielding a value other than 53 falsifies the
dissolve's semantic preservation.

## Footprint

`text ~3.5 KB, bss 8 B` for the whole image (TCB + fused composition). It links
and boots under an **8 KB-SRAM** map (verified with a forced relink at
`RAM LENGTH = 8K`; the committed `memory.x` stays at 64 K for the other bins).
This is the 8 KB-class node target behind synth#383.

## Reproducing fused.o

`fused.o` is checked in so the bench builds with no dissolve toolchain on PATH.
To regenerate it from the components:

```sh
WASM_TOOLS=…/wasm-tools SYNTH=…/synth ./build-fused.sh
```

Requires the (currently unmerged) tool forks noted at the top of
`build-fused.sh`: `wasm-tools@feat/import-passthrough` (wasm-tools#2) and
`wit-bindgen@integration/embedded-rt-no-grow` (#4/#6), plus `meld`, `loom`,
`synth`, LLVM. Re-pin to released tags once those land.

## Distinct from `dissolve.sh`

`crates/gale-app-demo/dissolve.sh` dissolves *each* component to its own `.o`,
linked against a TCB arena (`__cabi_arena_realloc`) — the per-component
library-OS form. This demonstrator instead **fuses** the two components into one
merged-memory core *before* dissolving, so the result is a single self-contained
object with no cross-component imports — the form that drops straight onto the
gust TCB. Both are valid lowerings of gale#89; this one is the "one fused module"
the BYO-OS vision asks for.

## gust_stack — the composition driven ON the stack (the north-star, completed)

`gust_fused` proves the *dissolve* (CM → fuse → synth → `run-demo()=53`, called
once). `gust_stack` proves the other half — **running on our stack**: the same
dissolved composition is the body of a **kiln-async task**, re-polled every
scheduler round. The kiln-async executor (gust's scheduler) drives the verified
gale components as scheduled work — not a one-shot call.

```
cargo run --release --bin gust_stack   # qemu lm3s6965evb / Cortex-M3
→ gust-stack: 5000 poll rounds; dissolved run-demo() = 53 each round; mismatches = 0
```

So the whole north-star path executes bare-metal, no runtime:
**components on top (CM) → meld fuse → one module → synth dissolve → driven by the
kiln-async scheduler on gust.** Rung 1 (DONE): a richer driven example — gust_control runs the dissolved
**engine_control** control loop (sensors -> control_step -> actuators) as the kiln
task body, one tick per round; gate matches C/wasmtime (spark 33deg/fuel 2300us),
5000 ticks bare-metal. Needs the r11=0 TCB trampoline (control_step is
--native-pointer-abi: it reads its tables off the linmem base the scheduler
clobbers). Next: the same on real silicon (G474RE/F100).

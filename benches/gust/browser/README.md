# gale + gust in the browser

**One wasm module (~3.3 KB)** running two things, clearly separated, both the *same code*
that targets the Cortex-M3:

1. **gale verified components** — the actual formally-verified decision functions
   (`gale::sem::give_decide`/`take_decide`, `gale::msgq::put_decide`/`get_decide`),
   proven with **Verus + Rocq + Lean + Kani**. Interactive semaphore + message-queue
   panels: you drive `give/take` and `put/get`, the **verified decision**
   (INCREMENT/WAKE/SATURATED, STORE/WAKE_READER/PEND/FULL/EMPTY) is computed by the
   proven gale code, and the page applies it. This is the *same* logic that ships in
   the Zephyr drop-in and the wasm-cross-LTO modules.
2. **kiln-async scheduler** — the cooperative fuel-bounded executor that boots gust on
   Cortex-M3 and dissolves to native via `wasm → loom → synth`. Live poll loop driving a
   failsafe mixer. *(This is kiln, the executor — not a verified gale primitive; labeled
   as such.)*

This is the browser leg of "one verified component, three runtimes" — browser ·
wasmtime/kiln · dissolved-native (gale#74 · `FIND-BYOOS-002`).

## Run
```sh
./build.sh                       # cargo -> wasm32, copies web/gust.wasm
cd web && python3 -m http.server # open http://localhost:8000/
```

## What's gale vs what's not (the honest map)
- **gale (verified):** `gale_sem_give/take`, `gale_msgq_put/get` → call the proven
  `gale::sem` / `gale::msgq` deciders.
- **kiln (executor):** `gust_boot/gust_poll` → kiln-async `Scheduler`.
- **gust app (not verified):** `gust_mix` → the ~10-line fixed-point failsafe mixer.

## Validation
All verified deciders checked in wasmtime (same engine as the browser):
`sem_give 0/10/0→INCREMENT, 5/10/1→WAKE, 10/10/0→SATURATED`; `sem_take 0/no-wait→WOULD_BLOCK`;
`msgq_put STORE/WAKE_READER/FULL`; `msgq_get EMPTY`. Deploys to GitHub Pages on merge to main.

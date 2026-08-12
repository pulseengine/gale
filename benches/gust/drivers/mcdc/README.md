# mcdc — evidence-on-wasm for the thin-seam drivers

The leg `REQ-OS-OBJVERIFY-001` demands and every stage-2 `RESULTS.md` lists as
missing: witness MC/DC on the WASM the dissolve actually compiles, plus the
WASM→object disposition join against synth's provenance map.

- `run-mcdc.sh` — the whole chain, one command. `DRV=…` points it at another driver.
- `vectors.sh` — the designed vector set for switch-thin (84 invocations): every
  export, every FSM phase, the window wrap, and a 10-vector sweep of
  `MajorFrame::check`'s 9-condition conjunction.
- `ctx-stub/` — a component exporting `gust:switch/ctx` whose three atoms return
  success. The same substitution the Kani harness makes; `run_switch`'s FFI calls
  cannot be linked, so the seam must be stood in for to execute anything at all.
- `evidence/` — the committed output of the run recorded in
  `../measurements/switch-thin-mcdc.md`.

Two traps this harness exists to avoid:

1. **`witness report` without `--format mcdc` reports branches *reached*, not
   MC/DC.** The percentage it prints is not a coverage result.
2. **`meld fuse` without `--preserve-names` drops the name section**, and every
   gap row becomes `(anon)` — unattributable, so the gaps cannot be triaged.

Two things to know when writing vectors. The harness runs the **fused core**, so
argument types come from the core signature, not the WIT:

- a WIT `bool` is an `i32` — write `1` / `0`, not `true` / `false`;
- a WIT `u32` is an `i32` — values at or above 2^31 need the two's-complement
  form (`0xFFFF_FFE0` is `-32`), or witness rejects the spec as out of range.

And a seam stub is **part of the artefact under measurement**: fusing it adds one
more copy of the canonical-ABI glue, so anything counted per-component is
inflated by one. See the `cabi_realloc` correction in
`../measurements/switch-thin-mcdc.md`.

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

Two traps this harness exists to avoid. **witness 0.40.0 now warns about both**
(REQ-063/REQ-064, from pulseengine/witness#177 and #178 filed off this work), but
the harness still handles them by default so the warnings should never fire here:

1. **`witness report` without `--format mcdc` reports branches *reached*, not
   MC/DC.** As of 0.40.0 the default output says so itself — `branches reached:
   30/98 (30.6%) — reached at least once, NOT MC/DC` — but the number is still
   not a coverage result. `run-mcdc.sh` writes the `--format mcdc` truth table.
2. **`meld fuse` without `--preserve-names` drops the name section**, and every
   gap row becomes `(anon)` — unattributable, so the gaps cannot be triaged.
   0.40.0 warns at instrument time; `run-mcdc.sh` always passes the flag.

A third, only visible in the manifest: a build **without DWARF** still reports
`attribution_source: "dwarf"`, because `meld` emits a synthetic `<meld-adapter>`
unit regardless — so decisions collapse from 23 to 3, all of them the adapter's,
with none from user code. 0.40.0 warns on this too. `run-mcdc.sh` always builds
with `debuginfo=2`.

Two things to know when writing vectors. The harness runs the **fused core**, so
argument types come from the core signature, not the WIT:

- a WIT `bool` is an `i32` — write `1` / `0`, not `true` / `false`;
- a WIT `u32` is an `i32` — values at or above 2^31 need the two's-complement
  form (`0xFFFF_FFE0` is `-32`), or witness rejects the spec as out of range.

And a seam stub is **part of the artefact under measurement**: fusing it adds one
more copy of the canonical-ABI glue, so anything counted per-component is
inflated by one. See the `cabi_realloc` correction in
`../measurements/switch-thin-mcdc.md`.

## witness 0.41.0

Two things landed off reports filed from this harness; one is usable, one is not.

**REQ-065 (inline attribution, witness#179) — partially.** Decisions inlined
through stdlib now attribute to the driver: switch-thin's `lib.rs` row went 4→5
decisions and 2→3 proved, and the `option.rs` row disappeared. The overall
verdict is unchanged (`3/22; 13 proved, 11 gap, 51 dead`) across 0.39/0.40/0.41,
so it is re-attribution only and no committed evidence moves.
**hm-thin is unchanged** — both its decisions are still booked to
`wit_bindgen_cabi_realloc.rs` with no `lib.rs` row, because there the outermost
frame is itself a generated cabi wrapper. For the thin-driver shape that is the
common case, not an edge case; tracked on witness#179.

**REQ-066 (`--stub-imports`, witness#180) — not usable yet.** It would delete
`ctx-stub/` and `regs-stub/` entirely, but it only applies to
`--backend-wasmtime-component`, and no export name resolves on that backend — not
even the documented `pkg:ns/iface@ver#func` form that works on the core backend.
Filed as witness#194. Until it is fixed the stub crates and the `wac plug` +
`meld fuse` steps stay.

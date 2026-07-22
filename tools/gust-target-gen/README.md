# gust-target-gen

Generate per-target build artifacts from an [AADL](https://www.sae.org/standards/content/as5506d/)
hardware model: a Rust constants module, a linker `memory.x`, and a
[WIT](https://component-model.bytecodealliance.org/design/wit.html) world.

The idea: describe a board's processor, memory regions, and peripheral devices
once, in an architecture model, and derive everything the firmware needs from it —
instead of hand-scattering base addresses and register offsets across source files.
Retargeting across a chip family becomes *selecting a model*, not editing code, and
the generated values bake in at build time, so there is **zero runtime cost**.

## Input

`spar items --format json <model>.aadl` output. The model declares a board as a
`system implementation` with `processor` / `memory` / `device` subcomponents, each
carrying integer properties (base address, length, register offsets, reset-flag bit
positions) and an optional presence flag.

## Output (per board)

```
gust-target-gen --items <spar-items.json> --board <Package::Type.impl> --out <dir>
```

writes three `// GENERATED — DO NOT EDIT` files into `<dir>`:

- **`gust_target_<stem>.rs`** — `pub const` module: peripheral bases, the memory
  map, and derived register constants (e.g. a control/status register address from
  `base + offset`, a flag mask from `1 << bit`).
- **`memory-<stem>.x`** — a linker `MEMORY { … }` block from the model's memory
  regions (length rendered `K`/`M` when it divides evenly).
- **`world-<stem>.wit`** — a WIT world importing the driver-seam interfaces the
  board's *present* devices require (peripheral presence → interface set), so a
  board that gains a peripheral imports its interface and one that lacks it does not.

## Design

Build-time only; the generator never runs on the target. Commit the generated tree
and gate it in CI by regenerating and asserting no diff — the committed artifacts
can then never drift from the model. The generator panics loudly on any shape the
model tool did not actually produce (unknown board, unmapped device class,
malformed input): a build-time codegen tool should fail early and visibly, not emit
a silently wrong constant.

## License

Apache-2.0.

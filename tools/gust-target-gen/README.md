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

Optional: `--reference <peripherals.tsv>` cross-checks every SoC device base
before anything is written, and `--board-reference <board.tsv>` (with
`--reference`) also cross-checks the board pins. `--help` and `--version`
exit 0.

On success it writes three `// GENERATED — DO NOT EDIT` files into `<dir>`:

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
can then never drift from the model. The generator refuses any shape the model
tool did not actually produce (unknown board, unmapped device class, malformed
input), and writes nothing when it does: a build-time codegen tool should fail
early and visibly, not emit a silently wrong constant.

## Exit codes

Every failure prints `error: <what went wrong>` on stderr and exits with one code
per failure class, so a caller can tell bad input from a broken environment:

| code | meaning |
|---|---|
| 0 | success (also `--help`, `--version`) |
| 1 | input rejected: malformed items JSON or reference TSV, a board or classifier the items do not contain, a model missing a required device or property, or a `--reference` mismatch |
| 2 | usage error: unknown flag, missing required flag, flag without a value, flag given twice (usage is printed on stderr) |
| 3 | I/O error: an input file cannot be read, or the output directory or a file in it cannot be created or written |

A panic (exit 101) is a bug in the tool, not a verdict on the input.

The WASI build (`wasm32-wasip2`) reports the same codes through
`wasi:cli/exit`'s `exit-with-code`; wasmtime releases that still gate that API need
`wasmtime run -S cli-exit-with-code`.

## License

Apache-2.0.

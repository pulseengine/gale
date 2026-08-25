# Committed evidence

Rendered reports **and the raw manifests they came from**. The raw
`*.witness.json` is here deliberately: it carries `branch_inline_chains`, which
the rendered rollup discards, and witness needs real chains (not synthetic
fixtures) to work on inlined-decision attribution — pulseengine/witness#179.

| file | what |
|---|---|
| `switch-thin.witness.json` | manifest: 98 branches, 23 decisions, inline chains |
| `switch-thin.provenance.json` | synth `synth-provenance-v1` map, 33 functions |
| `switch-thin-mcdc-rollup.txt` | 3/22 decisions; 13 proved / 11 gap / 51 dead |
| `switch-thin-mcdc-truthtables.txt` | per-decision truth tables + gap-row hints |
| `switch-thin-object-disposition.txt` | 42 stands / 56 no-prov / 9 only-in-synth |
| `switch-thin-maskonly-mcdc-rollup.txt` | the bounds-check-removal control (below) |
| `hm-thin.witness.json` | manifest: 9 branches, 2 decisions |
| `hm-thin-*` | hm-thin's rollup / truth tables / disposition |
| `vector-set.txt` | the 84 invocations |

The manifests are joined by `(func_index, byte_offset)` to the provenance map;
both must come from **one** artefact — `debuginfo=2` permutes function indices.

## The mask-only control

`switch-thin-maskonly-*` is the variant with **only** the array-index bounds
checks removed (the proven `cur < MAX_WINDOWS` communicated as a mask); the lazy
`Option` statics and their `unwrap` are retained, so any delta is attributable to
the bounds checks alone. It answers loom#303's question about what strands what:

| | branches | unreached | of which fmt | of which `cabi_realloc` |
|---|---|---|---|---|
| shipped | 98 | 68 | 53 | 6 |
| mask-only | 43 | 14 | **0** | 6 |

Both `cabi_realloc` copies survive the mask with all 6 unreached branches intact,
while every formatting function disappears. So the formatter is stranded by
`panic_bounds_check` in the driver's own code — **not** by `cabi_realloc`.

## Per-file attribution: FIXED in witness 0.43.0 (witness#179)

These rollups were regenerated on **witness 0.43.0**. Everything committed here
before that was produced by 0.39–0.42, which never rebased branch offsets into
DWARF space: branches past the last line-table row were silently **clamped** to
it, usually onto `wit_bindgen_cabi_realloc.rs:11`. Two bugs — v4
`.debug_ranges` never extracted, and offsets never rebased — both found by
upstream reproducing the manifests in *this directory*, and fixed in witness#197.

The re-run moved the totals, not just the file column (75 → 70 conditions on
switch-thin, 3 → 2 full MC/DC), and hm-thin — the fixture committed here
specifically to catch the case — now attributes to `lib.rs` instead of generated
glue.

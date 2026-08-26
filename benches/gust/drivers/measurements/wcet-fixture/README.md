# T4 WCET fixture — the `synth-wcet-v1` sidecar for the whole dissolved OS

The input scry asked for on [scry#144](https://github.com/pulseengine/scry/issues/144),
pinned as a committed artefact rather than a reproduction script, so the decline
set cannot drift under whoever regenerates it.

| file | | |
|---|---|---|
| `gustos.loom.wasm` | 11 321 B | the loom-optimized module synth compiles |
| `gustos-dissolved-cm3.o.wcet.json` | 9 690 B | `synth-wcet-v1`, **3 bounded / 28 declined** |
| the object | 11 132 B | **not duplicated here** — see below |

## The object is the one gale ships

The `.o` is deliberately **not** copied into this directory. It is byte-identical
to the committed

    benches/gust/drivers/os-node/gustos-dissolved-cm3.o
    sha256 d3b0bca88d7de4a6241791d664d9c7f4b41e5810772e9b5d095eecccfb7e868c

Verified with `cmp`. Duplicating a tracked binary would create two copies that
drift the moment one is regenerated, and the whole point of a fixture is that it
does not drift. It also confirms synth's documented claim that `--emit-wcet` is
**purely additive** — the same bytes with and without it.

    gustos.loom.wasm
    sha256 62acd3a96e35c9831329b0b8689818047022e8cdc20d16294bac9475bb55c031

## Provenance

    meld 0.48.0  fuse --memory shared --pack-rebase --share-stack
      -> loom 1.2.0  optimize --passes inline --attestation false
      -> synth 0.57.0 compile --target cortex-m3 --all-exports --relocatable
                              --native-pointer-abi --emit-wcet

Regenerate with `../../build-dissolve-gustos.sh` plus `--emit-wcet`.

## What it says

**BOUNDED — 3 of 31**, all leaves in `gust:os/time`:

    gust:os/time@0.1.0#deadline       38 cyc
    gust:os/time@0.1.0#elapsed       109 cyc
    gust:os/time@0.1.0#resolution     68 cyc

**DECLINED — 28**:

| reason | n | owner |
|---|---|---|
| `loop` | **13** | scry loop-bound inference (scry#144) |
| `callee-unbounded` | 11 | cascade — resolves when the leaves do |
| `call` | 2 | **by design** — both reach the native seam |
| `unmodeled-op` | 2 | synth cycle model |

The `call` pair is `gust:os/time@0.1.0#now` and `poll_task`. An intra-procedural
bound *should* stop at an import whose cost belongs to the host, so these are
correct rather than missing.

## The join-key problem, which this fixture exists to make visible

**7 of the 13 `loop` declines have no name**:

    gust:os/log@0.1.0#line     cabi_realloc_wit_bindgen_0_52_0
    exec_admit                 gust:sched/tasks@0.1.0#state
    exec_poll_round            exec_state
    func_22 func_24 func_25 func_26 func_27 func_28 func_32   <- 7

Measured: the module has **26 exports** and synth compiled **31** functions; all
**23** named entries in the sidecar are exports and all **8** anonymous ones are
not. So `synth-wcet-v1` takes its name from the export section and falls back to
`func_<index>` otherwise.

That matters because `synth --wcet-hints` (schema `synth-wcet-hints-v1`, the scry
oracle seam) keys on the function name. A hint cannot be written for those seven,
and one written against `func_22` silently retargets when the index space shifts.

**It is not caused by a missing `--preserve-names`.** That was the obvious
suspect — `mcdc/run-mcdc.sh` passes it and documents the trap, while
`build-dissolve-gustos.sh` does not. Rebuilding the whole chain with it produces
a **byte-identical** `.wcet.json` while populating the name section from **0 to
59 of 68** functions. The names exist; the sidecar does not consult them. Raised
as [synth#1063](https://github.com/pulseengine/synth/issues/1063).

Adding `--preserve-names` to the dissolve chain is therefore correct but
currently inert, and is deliberately **not** bundled here: it would force
regenerating the committed object for no measurable change. Worth doing when
synth#1063 lands.

## Read the qualifiers before quoting a cycle count

The sidecar carries `wait_states` and a `memory_assumption`. Every bound is
**conditional on a zero-wait-state precondition**. A cycle figure quoted without
it is not the number synth emitted.

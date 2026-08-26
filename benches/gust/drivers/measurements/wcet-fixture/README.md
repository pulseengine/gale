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

---

## `gustos.loom.named.wasm` — the same module WITH a name section

Added after scry pointed out (scry#144) that their identity-churn corpus is
**legacy**-mangled and the **v0** path has unit coverage but no corpus
measurement. The fixture above has **zero** named functions — it is the default
dissolve build, and `meld fuse` drops the name section without `--preserve-names`.
So it could not serve that purpose. This file can.

    gustos.loom.named.wasm    meld fuse … --preserve-names   -> 59 of 68 named
    gustos.loom.wasm          (default)                      ->  0 of 68 named

Both produce a **byte-identical** `.wcet.json`, which is the point of synth#1063:
`synth-wcet-v1` names from the export section and does not consult the name
section either way.

### v0 identity churn, measured on this module

scry's `#146` two-tier fix strips the v0 crate disambiguator (`Cs<base62>_`) where
the stripped name is unique in the module, and marks it `id_build_local` where it
is not. On this module:

    38 of 62 distinct names are v0-mangled (`_R…`); 0 legacy; 24 unmangled
    after stripping Cs<disambiguator>_ :  38 unique, 0 collisions

So every v0 name here qualifies for a stable key — **no `id_build_local`
fallbacks**.

Two-build churn, replicating scry's experiment. The perturbation must change
**crate metadata**, not source: a comment added to `exec-provider/src/lib.rs`
churned **nothing** (100% raw survival), because the v0 disambiguator derives from
crate name/version/flags rather than content. Bumping `exec-provider`'s version
`0.1.0 -> 0.1.1` is the honest perturbation:

    RAW v0 names                  26 / 38 survived   = 68.4%   (12 vanished)
    STRIPPED (scry #146 identity) 38 / 38 survived   = 100.0%  (0 vanished)

The 12 that vanish raw are exactly `exec-provider`'s functions — the crate whose
metadata moved. Compare scry's legacy corpus: ~52% -> ~74%.

**Scope.** One module, one perturbation kind (a version bump). It does not cover
rustc upgrades, dependency-graph changes, or the collision case — this module has
no collisions to exercise `id_build_local` with, which is a gap in what it can
demonstrate, not evidence that the flag is unnecessary.

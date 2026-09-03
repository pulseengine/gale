# Toolchain pin — varve layer `2026.08.4`

Every dissolve step invokes a tool, and by default gets whatever is on `PATH`.
That is the mixed-toolchain hazard: half the pipeline on one version and half on
another, producing an artefact no single toolchain ever built.

**gale has hit it.** The scripts moved to `meld 0.48.0` while
`gustos-dissolve.yml` still installed `0.41.3`; the scripts fell through to
whatever was on `PATH`, and it surfaced as `unexpected argument '--pack-rebase'`
deep inside a fuse rather than as a version problem. A pin closes that by
construction.

    varve install          # fetch + verify + lay down the layer
    varve which meld       # which binary runs here, and from which layer

The realm supplies the registry and the trust root, so **no environment variable
is needed**. `varve-realms.toml` is the canonical file shipped with each varve
release, downloaded rather than pasted.

## What this layer carries

| tool | layer 2026.08.4 | gale's floor | |
|---|---|---|---|
| **meld** | 0.52.0 | ≥ 0.48.0 | `--pack-rebase` / `--share-stack` |
| **witness** | 0.43.0 | ≥ 0.43.0 | the DWARF branch-offset fix |
| synth | 0.58.0 | 0.57.0 pinned | see below |
| loom | 1.4.0 | 1.2.0 used | see below |
| rivet | 0.34.0 | — | |

## Why adopting it was safe — measured, not assumed

The layer is **ahead of gale's pins on two tools that feed committed objects**:
synth `0.57.0 -> 0.58.0` and loom `1.2.0 -> 1.4.0`. That is exactly the situation
where a pin bump silently invalidates evidence, so it was tested before adoption:

    build-iso-core.sh        with meld 0.52.0 / loom 1.4.0 / synth 0.58.0
      -> text 8700  data 1096  bss 3140   SRAM 4 236 B (51%)
      -> BYTE-IDENTICAL to the committed iso-core-fused-cm3.o

    build-dissolve-gustos.sh with the same toolchain
      -> text 5384  data 1580  bss 3620   SRAM 5 200 B (63%), 1-page arena
      -> BYTE-IDENTICAL to the committed os-node/gustos-dissolved-cm3.o

    gust_iso_probe      ALL CHECKS PASSED   (9/9)
    gust_osfused_probe  ALL CHECKS PASSED   (6/6)

So **no measurement in this repo is invalidated by the pin**, and no committed
object needed regenerating. Had either object differed, adoption would have
required re-measuring every figure derived from it — which is the reason to check
rather than assume.

## What the pin does NOT change

The build scripts still resolve tools through `$MELD` / `$LOOM` / `$SYNTH` with
`~/pe-toolchain` defaults, and CI still installs pinned tarballs via
`gustos-dissolve.yml`. **This pin is additive**: it makes the intended toolchain
declarative and verifiable, and does not yet replace those paths. Moving the
scripts onto varve shims is a separate change, and should be measured the same
way.

## Known caveat

`loom v1.4.0` publishes no `aarch64-unknown-linux-gnu` tarball upstream, so the
layer omits loom on that platform rather than substituting something. gale's CI
is `ubuntu-22.04` (x86_64) and development is darwin/arm64, so neither is
affected — but an aarch64 Linux runner would find `varve which loom` refusing,
by design rather than by accident.

## Why the pin carries a digest (2026-08-28)

The pin above names a layer **and a manifest digest**. The digest is not
decoration — without it the pin does not resolve at all:

    $ varve which synth
    error: layer 2026.08.4 is installed more than once under different
    digests (2 entries) and the pin carries no digest to disambiguate

The rolling channel republished `2026.08.4` under a second manifest digest.
Both copies are in the local store:

| manifest digest | installed |
|---|---|
| `sha256:7e48ccc3…` | 2026-08-27 00:35 |
| `sha256:c1e6a418…` | 2026-08-27 23:45 |

**The two carry identical payloads.** `varve inspect` under each pin returns
the same 47-line inventory, byte-identical apart from the layer's own digest
line, and every dispatched tool hashes the same under both:

| tool | version | sha256 (first 16) |
|---|---|---|
| synth | 0.58.0 | `033d7c61118e7093` |
| loom | 1.4.0 | `9583f530baa0d6f0` |
| meld | 0.52.0 | `88fb38bd1e796632` |
| witness | 0.43.0 | `3c54bf7bde25f284` |
| rivet | 0.34.0 | `72b1b81dab0b8a78` |

So this is a re-publication, not a content change, and not a compromise. The
digest recorded above is the later one — what the channel serves now.

The lesson is the one varve's design already states: **on the rolling channel a
layer name is not a stable identifier.** Any reproducibility claim anchored to
`layer = "2026.08.4"` alone is falsifiable, and was — the name resolved to one
manifest on 27 Aug and to two by 28 Aug. Only the digest pins the artifact.

varve's behaviour here is correct and is the reason this was caught rather than
silently mixed: it refused to guess, exited non-zero, and named the fix. A
shim that had fallen back to an ambient binary would have produced a build
attributable to no layer at all.

Negative control for this section: delete the `digest` line from `varve.toml`
and re-run `varve which synth`. It must exit 1 with the message above. If it
succeeds, the duplicate has been pruned from the store and this section's
premise no longer holds locally — the digest pin stays regardless.

## What this pin does NOT cover: rustc

The varve layer pins synth, loom, meld, witness and rivet. Several build scripts
pin their own synth on top of that (`build-os-ts.sh` uses synth 0.45.1). Every
crate's `Cargo.lock` is committed — 13 thin drivers and 5 providers, all tracked.

**rustc is not pinned.** `rust-toolchain.toml` says:

    [toolchain]
    channel = "stable"

A floating channel. So the compiler that produces the wasm — the input to the
entire pinned pipeline — is whatever `stable` happens to be on the machine.

### This is not hypothetical; four committed objects have drifted

Re-running each builder on the current toolchain, with the synth version each
script pins:

| object | committed | rebuilt | delta |
|---|---|---|---|
| `breadth/breadth-cm3.o` | 4368 | 4616 | +248 |
| `os-node/os-time-cm3.o` | 1609 | 1932 | +323 |
| `os-node/os-tl-cm3.o` | 4317 | 4003 | −314 |
| `os-node/os-ts-cm3.o` | 3638 | **8857** | **+5219 (2.4×)** |

Undefined-symbol sets are identical in all four, so the seam contracts hold and
nothing is functionally wrong. Only code size moved.

Since the synth version is pinned per-script and the dependency versions are
pinned by committed lockfiles, rustc is the remaining unpinned variable in the
path from source to committed object.

### CORRECTION (2026-09-03): rustc is NOT the cause

The section above named rustc as the remaining unpinned variable and said so was
not the same as proving it. It is now ruled out. Completing the bisect:

| rustc | `os-ts-cm3.o` |
|---|---|
| 1.90.0 | 9293 B |
| 1.94.0 | 9317 B |
| 1.97.0 (stable) | 8857 B |
| **committed** | **3638 B** |

Going *back* three versions makes the object larger, not smaller. No rustc
reproduces 3638.

**The real cause is source drift.** Every one of the four objects predates weeks
of changes to its own inputs:

| object | committed | inputs changed through |
|---|---|---|
| `breadth-cm3.o` | 2026-07-09 | 2026-08-28 |
| `os-time-cm3.o` | 2026-07-09 | 2026-08-28 |
| `os-tl-cm3.o` | 2026-07-23 | 2026-08-28 |
| `os-ts-cm3.o` | 2026-07-23 | 2026-08-28 |

For `os-ts-cm3.o` specifically, committed 2026-07-23: `build-os-ts.sh` changed
07-29, `time-provider` and `exec-provider` 08-07, `timer-provider` 08-26,
`wit-os` 08-28. Five to seven weeks of input changes, never regenerated.

So this is not a toolchain-pinning problem. **Nothing rebuilds a committed object
when its sources change, and nothing notices.** That is a simpler defect and a
more tractable one: it is mechanically checkable — for each committed `*-cm3.o`,
is any input newer in git history than the object itself?

The rustc observation stands on its own (`channel = "stable"` does float, and
that is worth deciding about), but it is not what made these four drift, and this
document previously implied it was.

### Stated precisely

rustc is the remaining *unpinned* variable. That is not the same as having proved
it is the cause. A bisect across installed stables (1.90.0, 1.94.0) could not run:
`rust-std-wasm32-unknown-unknown` is installed only for `stable`, so those
toolchains cannot build the wasm at all, and the failure is silent — cargo exits
101 with no output under the scripts' `set -euo pipefail`.

To settle it, install the wasm32 std for an older stable and rebuild:

    rustup target add wasm32-unknown-unknown --toolchain 1.94.0
    RUSTUP_TOOLCHAIN=1.94.0 bash benches/gust/drivers/build-os-ts.sh

If that reproduces 3638 bytes, rustc is confirmed as the cause and the version
that built the committed objects is identified.

### Why it matters here

varve exists to close the mixed-toolchain hazard: half a pipeline on one version,
half on another, producing an artifact no single toolchain ever built. That hazard
is closed downstream of the wasm and open upstream of it. A committed object that
no toolchain reproduces is a measurement waiting to be taken from the wrong
artifact — the shape already seen in `dma-own-cm3.o`, which was 41 synth releases
stale and whose `RESULTS.md` numbers no current build produces.

Pinning `channel` to an exact version would close it. That is a real cost
(deliberate bumps, CI churn) and a decision rather than an obvious fix, so it is
recorded here rather than taken.

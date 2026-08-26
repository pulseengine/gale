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

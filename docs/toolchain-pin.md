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

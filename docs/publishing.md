# Publishing decision record

What of this repository is published to public package registries, and why. This is
an auditable, revisitable record — not a one-off.

## Decision

**Publish `gust-target-gen` (the AADL target-model generator). Do not publish gale
or gust as libraries.**

### `gust-target-gen` → crates.io (ready) + npm (documented follow-on)

A self-contained build-time codegen tool with registry-only dependencies — the
clean case for publication.

- **crates.io: wired and preflight-green.** `tools/gust-target-gen/Cargo.toml` has
  full metadata (Apache-2.0, repository, keywords, categories, README);
  `cargo publish --dry-run` packages and verifies cleanly. The name
  `gust-target-gen` is free on crates.io. Publishing is one maintainer action:
  push a `gust-target-gen-vX.Y.Z` tag → `.github/workflows/publish-gust-target-gen.yml`
  guards tag==version and runs `cargo publish` with the org `CRATES_IO_TOKEN`
  secret (the same one synth/loom use).
- **npm: `@pulseengine/gust-target-gen` (scoped name free), via the synth wrapper
  pattern — a documented follow-on, not yet committed.** synth distributes its Rust
  CLI on npm as a hand-written binary installer (`npm/` = `package.json` +
  `install.js`/`run.js`/`uninstall.js`): `postinstall` downloads the host-platform
  archive from the GitHub release and verifies it against the release's
  `SHA256SUMS.txt` before extracting; `bin` → `run.js`. To add it here:
  1. a release workflow that builds `gust-target-gen` for `{darwin,linux}×{x64,arm64}`,
     tars each, and writes `SHA256SUMS.txt` to the release (the installer's dependency);
  2. `tools/gust-target-gen/npm/` cloned from `pulseengine/synth:npm/`, re-scoped to
     `@pulseengine/gust-target-gen`, download URLs pointed at gale's release assets;
  3. a `publish-npm` workflow: `jq` the tag into `package.json`, `npm publish
     --access public` with an **Automation** `NPM_TOKEN` (a Publish token fails under
     the org's 2FA — synth's `npm whoami` preflight exists to catch this).
  A non-functional installer (downloading release assets that don't exist yet) would
  be worse than this recipe, so the wrapper is deferred until npm distribution is
  actually wanted. For a Rust CLI, crates.io is the natural primary channel.

### gale and gust → not published as libraries

Grounded in the registry + dependency facts, not preference:

- **`gale`** is a standalone internal library (`publish = false`, consumed by path).
  The flat name `gale` is already taken on crates.io + npm by unrelated crates. If
  ever published it would ship as `pulseengine-gale` (free) — but there is no
  near-term reason to.
- **`gust`** (`benches/gust`) has hard blockers: a `path` dependency on the
  unpublished `gale` and a `git` dependency on `kiln-async` — either makes crates.io
  refuse it — plus the taken `gust` name and no `license` field. It is a
  board-specific mini-OS bench, not a reusable library.

## Naming convention (for any future tool)

- crates.io: hyphen-prefixed member/tool crates — `gust-target-gen`, and
  `pulseengine-<x>` where a flat name is taken.
- npm: the owned scope `@pulseengine/<tool>`.

This mirrors synth (it routes around the taken flat `synth` name with `synth-*`
crates + the `@pulseengine` npm scope; it set **no** placeholder-reservation
precedent).

## Optional: defensive name reservation

`pulseengine`, `pulseengine-gust`, and `pulseengine-gale` are all free on crates.io.
Reserving them with `0.0.0` placeholders would guard the namespace, but that is a
*new* policy (not something synth did) and an outward-facing publish — it needs an
explicit maintainer decision and is intentionally **not** done automatically here.

## Prerequisites for any live publish

1. `license` field present on every crate to be published (done for `gust-target-gen`).
2. No `path`/`git` dependencies in a published crate (the gust blocker).
3. `CRATES_IO_TOKEN` (org secret, exists) and — for npm — an **Automation**
   `NPM_TOKEN` scoped to `@pulseengine/*`, published from the `@pulseengine`-owning
   account.

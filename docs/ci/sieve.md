# Which CI jobs a pull request runs (sieve)

gale's heavy CI jobs decide whether a pull request needs them through
[sieve](https://github.com/pulseengine/sieve), pulseengine's shared CI path
classifier. It replaced three hand-written detectors:
`tools/ci/detect-changes.sh` (docs-only), the "V-closure needed?" path regex
and the drv-cross-arch step guard.

| file | role |
|---|---|
| `sieve.toml` | the classes, what runs everything, the never-skip jobs |
| `.github/actions/setup-sieve/action.yml` | installs the pinned sieve (version + sha256) |
| `tools/ci/sieve-gate-probe.sh` | hands temper's gate probe to sieve |
| `cargo xtask check sieve-pin` | the pin agrees with itself; every job that runs sieve installs it |

## The classes

| class | replaces | read by |
|---|---|---|
| `code` | docs-only detector: false only when every changed file is docs | Rust Coverage, Zephyr C Coverage, gust dissolved on Renode, LLVM LTO build + size comparison, Binary size comparison (and their advisory siblings) |
| `cross_arch` | drv-cross-arch step guard | seam survives lowering on ARM and RISC-V (all thin drivers) |
| `v_closure` | "V-closure needed?" regex | no requirement lags its closed V |

There is no proofs class. **Proofs never skip** (owner, 2026-11-20): Verus, Kani,
Rocq and Lean run in full on every pull request, in the merge queue and on push
to main. `[workflows] never_skip_jobs` lists them, so `sieve check-workflows`
fails if one gains a classifier `if:`, a step-level skip or a skippable
`needs:`. "proofs needed?" stays as a job only because it is a required context.

Everything runs when sieve is unsure: any event but `pull_request` (push,
merge_group, schedule), no base, a failed or empty diff, a path no class
claims, a change to `.github/**`, `tools/**`, `bzl/**`, `sieve.toml`, any
`Cargo.lock` or a toolchain or Bazel file, and a classifier job that failed, was
cancelled or wrote nothing.

## How a job is gated

Each gated workflow has one job that runs

```sh
sieve classify --config sieve.toml --base "$BASE_SHA" --event "$EVENT_NAME" --format github-output
```

and a required job skips only through

```yaml
if: ${{ !cancelled() && (needs.<classifier>.result != 'success' || needs.<classifier>.outputs.<class> != 'false') }}
```

A skipped job still reports its required context. No workflow that hosts a
required context has a `paths:` filter. `sieve check-workflows` (run by the
"every required context can actually report" job) enforces this, and
`sieve check-graph` checks that every class input really selects its class.

## Evidence for the switch

`sieve equivalence` replays gale's last 400 first-parent commits (to a0e9d47c)
through each old detector in `GATE_PROBE_FILES` mode and through sieve, class
by class. A narrowing (sieve selecting less than the old detector) fails it.

| old detector | compared | narrower | exit |
|---|---|---|---|
| `tools/ci/detect-changes.sh` (`code`) | 400/400 | 0 | 0 |
| the inline V-closure and cross-arch regexes, copied verbatim (`v_closure`, `cross_arch`) | 400/400 | 0 | 0 |
| the Python reference `classify-changes.py` (4050000f), each of its 5 heavy-job outputs mapped to `code`, plus `cross_arch` | 400/400 each | 0 | 0 |

The first run against the Python reference narrowed `cross_arch` on 2 commits
that changed `os/gust/Cargo.lock`: sieve scopes a nested workspace's lock to the
classes that build in it (sieve REQ-027), the reference ran everything.
`[run_all] basenames = ["Cargo.lock"]` restores the reference's behaviour; turning
the scoping on is a separate decision.

## Bumping sieve

1. Download the release's `SHA256SUMS.txt` and its signature bundle, and verify:

   ```sh
   cosign verify-blob \
     --certificate-identity-regexp '^https://github.com/pulseengine/sieve/.github/workflows/release.yml@refs/tags/v.*$' \
     --certificate-oidc-issuer 'https://token.actions.githubusercontent.com' \
     --bundle SHA256SUMS.txt.cosign.bundle SHA256SUMS.txt
   ```

2. Copy the `x86_64-unknown-linux-musl` and `aarch64-unknown-linux-musl` entries
   into the action, change `SIEVE_VERSION` and the step name, and run
   `cargo xtask check sieve-pin`.
3. Run `sieve check-workflows` and `sieve check-graph` with the new binary, and
   `sieve equivalence` against the old one if the classifier's rules changed.

The independent false-skip controls against sieve live in
[plant](https://github.com/pulseengine/plant), apart from the decider by design.

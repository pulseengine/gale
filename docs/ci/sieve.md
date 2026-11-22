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
| `cargo xtask check sieve-pin` | the pin agrees with itself; every job that runs sieve installs it; the verdict record stays intact |
| `.github/workflows/sieve-verdict.yml`, `tools/ci/sieve-verdict.jq` | records and validates sieve's verdict on every run |

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

## The verdict record

Every run (pull_request, merge_group, push to main) records sieve's verdict in
one repo-wide job, "record sieve's verdict" in
`.github/workflows/sieve-verdict.yml`. It runs
`sieve classify --format json --explain` once, checks the record with
`jq -e -f tools/ci/sieve-verdict.jq` against `sieve graph` (it must parse,
name `sieve.classify.v1` and carry a boolean for every class in sieve.toml),
and uploads it as `sieve-verdict-<run_attempt>`, kept 90 days, with
`if-no-files-found: error`. `sieve observe` can compare these records with
the jobs' results later.

It is a separate job because a classifier job may run `sieve classify` only
once (check-workflows gate b) and sieve 0.3.0's github-output mode writes no
JSON. Moving the record into that single call is tracked in sieve#25. The
sieve#25 branch (PR #26, 30b587ee, built locally) was tested in a copy of this
tree. With `--record "$RUNNER_TEMP/sieve-verdict.json"` added to all six
classifier steps, its check-workflows exits 0 (sieve 0.3.0 exits 1). The record
passes this validation. An unwritable record path exits 2 and writes no
outputs, so every gated job runs. The swap waits for a cosign-verified v0.4.0. On a pull
request it classifies the PR against its base. On merge_group it is a run_all
verdict (the event is not a pull request). On push to main it uses sieve's
observe form, `--event pull_request --base HEAD~1 --head HEAD`: the pushed
commit's own diff (main merges by squash, so HEAD~1 is the previous main).
The checkout fetches full history; a checkout without HEAD~1 fails the step
loudly rather than recording nothing quietly. The validation step fails with a
clear message when jq is missing.

The job is not a required context. It is on `never_skip_jobs`, so a
job-level `if:` fails check-workflows, and `cargo xtask check sieve-pin`
fails if the validation or the upload step is removed, reordered, guarded or
pointed at another path.

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

## Paths that still run everything

A replay of gale's last 80 merged PRs found PRs that ran everything only
because a changed path was claimed by no class:

- `drivers/dma-own/**`: its own Cargo workspace, which no class root reached (it
  is not a `*-thin` driver, and `os/gust/build.rs` links only its committed
  object). It is now a `code` root.
- `drivers/CROSS-ARCH.md`: the cross-arch gate's policy record, named by its
  scripts, so not docs-only. It is now claimed by `code`.
- `README.md`, `docs/safety/verification-honesty.md`,
  `docs/architecture/three-modes-two-axes.md`, `docs/releases/v0.11.0-plan.md`:
  no job reads them, but a non-markdown file names each one (artifact prose,
  proof comments, a `README.md` basename in `binaries.toml`), so the docs rule
  treats them as code. They stay unclaimed, so they run everything. Claiming them
  in `code` would narrow `cross_arch` on 4 of the 400 commits against the Python
  reference, which ran everything on them. Listing them `[inert]` would narrow
  `code` against both `detect-changes.sh` and the reference. Either way needs an
  owner decision.
- Paths under the old `benches/gust/` (moved to `os/gust/`): only history. No
  class root points at the old location, and the paths no longer exist.

Every job that runs `sieve classify --base` checks out with `fetch-depth: 0`
(`cargo xtask check sieve-pin` enforces 0 or at least 2). A depth-1 checkout
cannot resolve the PR base, so sieve runs everything, with the reason
"git diff failed against <base>". The saving disappears without any error.
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

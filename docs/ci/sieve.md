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
| `.github/workflows/sieve-verdict.yml`, `tools/ci/sieve-verdict.jq` | record and validate sieve's verdict on every run |
| `tools/ci/sieve-gate-probe.sh` | hands temper's gate probe to sieve |
| `cargo xtask check sieve-pin` | keeps the pin, the verdict record and the sieve gates intact (below) |

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
required context has a `paths:` filter.

Every job that runs `sieve classify --base` checks out with `fetch-depth: 0`. A
depth-1 checkout cannot resolve the PR base, so sieve runs everything with the
reason "git diff failed against <base>": safe, but the saving disappears
without any error.

## What keeps it honest

The required job "every required context can actually report"
(kill-criteria.yml) runs four sieve gates, so each one blocks the merge:

- `sieve check-workflows`: triggers, the exact classifier `if:`, fail-open, and
  the never-skip jobs.
- `sieve check-graph`: every class input selects its class.
- `cargo xtask check sieve-pin --self-test`, then `cargo xtask check sieve-pin`.
  This gate checks:
  - the action, the verdict workflow (from `on:` on) and the jq filter equal
    canonical texts held in the gate, line for line, ignoring blank lines and
    comment-only lines. Only the version and the two sha256 values may differ,
    so a check that is present but neutralised fails (`|| true`, a reassigned
    sha, `set +e`, a dropped `exit 1`, `--event push`, `ref: main`, `or true`);
  - every job that runs sieve installs the pin and proves PATH finds it;
  - every `sieve classify --base` job checks out with fetch-depth 0 or at
    least 2;
  - the required job keeps all four sieve steps, unguarded and with nothing
    appended.

What sieve-pin cannot see is listed in REQ-CI-SIEVE-002.

temper's gate probe (`.github/gate-probe.json`, report-only) also runs sieve on
one positive and one negative control per class: `src/sem.rs` for `code`,
`drivers/uart-thin/src/lib.rs` for `cross_arch` and
`artifacts/requirements.yaml` for `v_closure`. Dropping a class's claim turns
the probe red.

## The verdict record

Every run (pull_request, merge_group, push to main) records sieve's verdict in
one repo-wide job, "record sieve's verdict" in
`.github/workflows/sieve-verdict.yml`:

- **pull_request:** the PR classified against its base.
- **merge_group:** a run_all verdict (the event is not a pull request).
- **push to main:** sieve's observe form, `--event pull_request --base <base>
  --head HEAD`, so the record covers the push's whole diff. `<base>` is
  `github.event.before` when it is a real commit in the checkout, otherwise
  `HEAD~1` (the first parent). Without either, the step fails loudly and nothing
  is recorded. A push run cancelled by the concurrency group records nothing.

The job runs `sieve classify --format json --explain` once. It checks the
record with `jq -e -f tools/ci/sieve-verdict.jq` against `sieve graph`: the
record must parse, name `sieve.classify.v1` and carry a boolean for every class
in sieve.toml. A missing jq fails the step with a clear message. The record is
uploaded as `sieve-verdict-<run_attempt>`, kept 90 days, with
`if-no-files-found: error`. `sieve observe` can later compare these records
with the jobs' results.

It is a separate job because a classifier job may run `sieve classify` only
once (check-workflows gate b), and sieve 0.3.0's github-output mode writes no
JSON. Moving the record into that single call is tracked in sieve#25.

The sieve#25 branch (PR #26, 30b587ee, built locally) was tested in a copy of
this tree:

- with `--record "$RUNNER_TEMP/sieve-verdict.json"` added to all six classifier
  steps, its check-workflows exits 0 (sieve 0.3.0 exits 1);
- the record passes this validation;
- an unwritable record path exits 2 and writes no outputs, so every gated job
  runs.

The swap waits for a cosign-verified v0.4.0.

The job is not a required context. It is on `never_skip_jobs`, so a job-level
`if:` fails check-workflows.

## Evidence for the switch

`sieve equivalence` replays gale's last 400 first-parent commits (to a0e9d47c)
through each old detector in `GATE_PROBE_FILES` mode and through sieve, class
by class. A narrowing (sieve selecting less than the old detector) fails it.

| old detector | compared | narrower | identical / wider | exit |
|---|---|---|---|---|
| `tools/ci/detect-changes.sh` (`code`) | 400/400 | 0 | 400 / 0 | 0 |
| the inline V-closure and cross-arch regexes, copied verbatim (`v_closure`, `cross_arch`) | 400/400 | 0 | 63 / 337 | 0 |
| Python reference, `--map zephyr=code` (and `cross_arch`) | 400/400 | 0 | 360 / 40 | 0 |
| Python reference, `--map zephyr_renode=code` | 400/400 | 0 | 360 / 40 | 0 |
| Python reference, `--map rust_cov=code` | 400/400 | 0 | 356 / 44 | 0 |
| Python reference, `--map proofs=code` | 400/400 | 0 | 367 / 33 | 0 |
| Python reference, `--map gust_renode=code` | 400/400 | 0 | 356 / 44 | 0 |

`--map` keeps the last old output written for a class. That is why each
reference output gets its own run.

The first run against the Python reference narrowed `cross_arch` on 2 commits
that changed `os/gust/Cargo.lock`. sieve scopes a nested workspace's lock to the
classes that build in it (sieve REQ-027); the reference ran everything.
`[run_all] basenames = ["Cargo.lock"]` restores the reference's behaviour.
Turning the scoping on is a separate decision.

### Reproducing it

Run from a checkout of this branch with the pinned sieve on PATH, and with
`$T` a scratch directory outside the tree. The Python reference needs Python
3.11 or later (`tomllib`); an older one, or a missing Bazel snapshot, makes it
run everything on every commit, and that shows up as narrowing.

```sh
# the old docs-only detector, from the last commit that had it
git show a0e9d47c:tools/ci/detect-changes.sh > "$T/detect-changes.sh"
sieve equivalence --config sieve.toml --ref a0e9d47c --commits 400 \
  --old bash --old-arg "$T/detect-changes.sh"

# the Python reference, gale reference/l7-ci-classifier at 4050000f, with the
# Bazel snapshot it reads from the tree (untracked, so sieve does not see it)
git show 4050000f:tools/ci/classify-changes.py > "$T/classify-changes.py"
git show 4050000f:tools/ci/inputs-bazel.json > tools/ci/inputs-bazel.json
sieve equivalence --config sieve.toml --ref a0e9d47c --commits 400 \
  --old python3.12 --old-arg -I --old-arg "$T/classify-changes.py" --map zephyr=code
rm tools/ci/inputs-bazel.json
```

Repeat the second command with `--map zephyr_renode=code`, `rust_cov=code`,
`proofs=code` and `gust_renode=code` for the other rows.

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
- Paths under gust's pre-rework location (now `os/gust/`): only history. No
  class root points at the old location, and the paths no longer exist.

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
   `cargo xtask check sieve-pin`. A change to the install script also needs the
   canonical text in `tools/xtask/src/gates/sieve_pin.rs` changed in the same
   commit.
3. Run `sieve check-workflows` and `sieve check-graph` with the new binary, and
   `sieve equivalence` against the old one if the classifier's rules changed.

The independent false-skip controls against sieve live in
[plant](https://github.com/pulseengine/plant), apart from the decider by design.

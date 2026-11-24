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
| `cargo xtask check sieve-pin` | keeps the pin, each classifier job's verdict record and the sieve gates intact (below) |

## The classes

| class | replaces | read by |
|---|---|---|
| `code` | docs-only detector: false only when every changed file is docs | Rust Coverage, Zephyr C Coverage, gust dissolved on Renode, LLVM LTO build + size comparison, Binary size comparison (and their advisory siblings) |
| `cross_arch` | drv-cross-arch step guard | seam survives lowering on ARM and RISC-V (all thin drivers) |
| `v_closure` | "V-closure needed?" regex | no requirement lags its closed V |

There is no proofs class. **Proofs never skip** (owner, 2026-11-20): Verus,
Kani (including the driver crates), Rocq and Lean run in full on every pull
request, in the merge queue and on push to main. `[workflows] never_skip_jobs`
lists them, so `sieve check-workflows` fails if one gains a classifier `if:`, a
step-level skip or a skippable `needs:`. "proofs needed?" stays as a job only
because it is a required context. The driver-crate Kani job is not a required
context, but it is on the list too; it has no path guard, and
`cargo xtask check kani-coverage` counts each of its steps unguarded.

Everything runs when sieve is unsure: any event but `pull_request` (push,
merge_group, schedule), no base, a failed or empty diff, a path no class
claims, a change to `.github/**` (except a scoped leaf-workflow edit, below),
`tools/**`, `bzl/**`, `sieve.toml`, any `Cargo.lock` or a toolchain or Bazel
file, and a classifier job that failed, was cancelled or wrote nothing.

## How a job is gated

Each gated workflow has one job that runs

```sh
sieve classify --config sieve.toml --base "$BASE_SHA" --event "$EVENT_NAME" --format github-output --record "$RUNNER_TEMP/sieve-verdict.json"
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
  - the action equals a canonical text held in the gate from `runs:` on, line
    for line, ignoring blank lines and comment-only lines. Only the version and
    the two sha256 values may differ, so a check that is present but
    neutralised fails (`|| true`, a reassigned sha, `set +e`, a dropped
    `exit 1`);
  - every classifier job runs exactly the classify call above, and its very
    next step uploads the record (SHA-pinned upload-artifact, the
    workflow-and-job artifact name, the record's path, `if-no-files-found:
    error`, 90 days, `if:` only `always()`);
  - every job that runs sieve installs the pin and proves PATH finds it;
  - every `sieve classify --base` job checks out with fetch-depth 0 or at
    least 2;
  - the required job keeps all four sieve steps, unguarded and with nothing
    appended.
  - `sieve.toml` keeps exactly one `scope_leaf_edits = true` under
    `[workflows]` (dropped, `false`, commented out, moved or overridden fails).

What sieve-pin cannot see is listed in REQ-CI-SIEVE-002.

temper's gate probe (`.github/gate-probe.json`, report-only) also runs sieve on
one positive and one negative control per class: `src/sem.rs` for `code`,
`drivers/uart-thin/src/lib.rs` for `cross_arch` and
`artifacts/requirements.yaml` for `v_closure`. Dropping a class's claim turns
the probe red.

## The verdict of record

Each classifier job's one classify call also writes the JSON verdict
(`--record "$RUNNER_TEMP/sieve-verdict.json"`, sieve v0.4.0, sieve#25). Its very
next step uploads it as `sieve-verdict-<workflow>-<job>-<run_attempt>`, kept 90
days, with `if-no-files-found: error`. sieve writes the record before its
outputs; if it cannot write it, sieve writes no output and exits 2, so every
gated job runs.

On a pull request, the last record of each classifier job is **the verdict of
record**: the verdict that actually skipped jobs. Push and merge_group records
say run_all, which is right, because gating on push stays `--event push` and
main pushes never skip anything.

There is no separate jq validation step. The record is sieve's own output,
written before any gating output. A validation step in the gating job would
tie every PR's gating to jq on the runner. The gate holds the shape instead:
`sieve-pin` requires the exact classify call and the upload right after it.

### Follow-up: counting misses (not built in L17)

`sieve observe` will compare the verdict of record with the jobs of the push run
on main after the merge. gale has no merge queue, so that push run runs
everything. The steps are:

1. find the PR for the merge commit;
2. download that PR's last `sieve-verdict-*` artifacts;
3. run `sieve observe` with the push run's job results.

A commit with no PR record is **uncompared**, never "no miss". Recomputing a
verdict from the merge commit's own diff is not a substitute: when main moved
while the PR was open, that is a different verdict from the one that gated.

## Evidence for the switch

`sieve equivalence` replays gale's last 400 first-parent commits (to a0e9d47c)
through each old detector in `GATE_PROBE_FILES` mode and through sieve, class
by class. A narrowing (sieve selecting less than the old detector) fails it.
Numbers with sieve 0.4.0 (rerun in lane L18 with leaf scoping on, to f0ac6e3e,
with 0.4.2 and again with 0.4.3, the same each time: `detect-changes.sh`
400/400, 0 narrower, 400 identical; the Python reference union 400/400,
0 narrower, 371 identical, 29 wider; both exit 0):

| old detector | compared | narrower | identical / wider | exit |
|---|---|---|---|---|
| `tools/ci/detect-changes.sh` (`code`) | 400/400 | 0 | 400 / 0 | 0 |
| the inline V-closure and cross-arch regexes, copied verbatim (`v_closure`, `cross_arch`) | 400/400 | 0 | 63 / 337 | 0 |
| Python reference, its five heavy-job outputs all mapped to `code` (a union), plus `cross_arch` | 400/400 | 0 | 371 / 29 | 0 |

Under sieve 0.3.0, `--map` kept only the last old output written for a class, so
each reference output got its own run (wider 40, 40, 44, 33, 44; 0 narrower each).

The first run against the Python reference narrowed `cross_arch` on 2 commits
that changed `os/gust/Cargo.lock`. sieve scopes a nested workspace's lock to the
classes that build in it (sieve REQ-027); the reference ran everything.
`[run_all] basenames = ["Cargo.lock"]` restores the reference's behaviour.
Turning the scoping on is a separate decision.

### Reproducing it

Run from a checkout of this branch with the pinned sieve on PATH, and with
`$T` a scratch directory outside the tree. The Python reference needs Python
3.11 or later (`tomllib`); an older one, or a missing Bazel snapshot, makes it
run everything on every commit, and that shows up as narrowing. The inline
guards were three `grep -E` regexes in the old workflows; they have no script to
rerun from here.

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
  --old python3.12 --old-arg -I --old-arg "$T/classify-changes.py" \
  --map zephyr=code --map zephyr_renode=code --map rust_cov=code \
  --map proofs=code --map gust_renode=code
rm tools/ci/inputs-bazel.json
```
## Scoped leaf-workflow edits (sieve 0.4.3, lane L18)

`[workflows] scope_leaf_edits = true` lets an edit to one file directly under
`.github/workflows/` select less than everything (sieve README, "Scoped
leaf-workflow edits"). The README is authoritative. In short:

- A job-body edit selects the classes that the classifier `if:` reads, for the
  changed jobs, the jobs that need them and the jobs those need (REQ-030).
- A workflow that hosts no classifier selects no class, because its jobs all
  run anyway (REQ-035).
- The files GitHub's own services read (the Dependabot config, a code-owners
  file, the issue and pull-request templates) select nothing unless another
  tracked file spells out their file name (REQ-036). A doc counts: this page
  once spelled the Dependabot file's name, and that alone made a
  Dependabot-only edit run everything, so it describes them instead.
- Everything else under `.github/` still runs everything: composite actions,
  `.github/zephyr-fork-pin` (which `code` jobs read), `required-contexts.txt`
  and `gate-probe.json`. So do these workflow edits:
  - a change outside `jobs:`;
  - a change to a job's `if:`, `needs:`, `outputs:`, `strategy:` or
    `continue-on-error:`, or to a step's `if:`;
  - any edit to a job that runs `sieve classify`;
  - an edit made with no workflow blob at the merge base.

Scoping only works if the workflows have the shape sieve can read. gale's
workflows keep these rules (L18 made them true):

- every gating job `if:` is exactly the classifier form. The dead
  `github.event_name != 'merge_group'` terms are gone, because gale has no
  merge queue;
- no step `if:` reads a context other than `github` or `runner`. The Codecov
  uploads test `github.event.pull_request.head.repo.fork` and `github.actor`,
  and the TSan sysctl tests its matrix value inside the script;
- no job reads `needs` outside its `if:`. sieve 0.4.2 also counted the bare
  word "needs" in `run:` script text, comments included, so L18 reworded those
  comments. That rule was a workaround for sieve#37: 0.4.3 reads `needs` only
  inside `${{ }}` or an `if:` value, so the rule is no longer required. The
  rewording stays (it costs nothing and keeps 0.4.2-era replays comparable).

`cargo xtask check sieve-pin` fails if the `scope_leaf_edits = true` line is
dropped. Turning scoping off is safe (everything runs again) but gives the
saving back, so it has to be a visible change to the gate.

### Blocker: `uses: ./gale/.github/actions/setup-renode` (still open under 0.4.3)

Five Renode workflows check gale out under `path: gale` and call
`uses: ./gale/.github/actions/setup-renode`. sieve scans every workflow for
whole-repository linters on every scoped edit (and on every scoped
GitHub-service file edit), so one job it cannot resolve makes every scoped edit
run everything.

- **sieve 0.4.2** could not resolve `./gale/` at all.
- **sieve 0.4.3** (sieve#36) resolves `./<p>/` through an earlier same-repository
  `actions/checkout` with a plain `path: <p>`, no `ref:`, no other repository,
  and no later step that rewrites `<p>`. On gale it still refuses all five
  jobs, with the reason "`uses: ./gale/.github/actions/setup-renode` is under
  `gale`, which a step checked out from another repository or ref, or
  rewrote". None of the five rewrites `gale/`. Measured on the flight job by
  deleting steps from a scratch copy: the rewrite check fires on the bare word
  `gale` in `run:` script text. A shell comment `(gale#289)` is enough by
  itself; so are a read-only `grep` of
  `${GITHUB_WORKSPACE}/gale/.github/zephyr-fork-pin` and the
  `GALE_ROOT: ${{ github.workspace }}/gale` builds. Only deleting every step
  between the checkout and the `uses:` cleared it, and then the next Renode
  workflow refused for the same reason.

**On this branch every scoped edit therefore still runs everything** (safe; no
saving). The same false-positive class as sieve#37, now in the #36 rewrite
check: it should count only write-like commands whose path argument is `<p>` or
under it, and ignore comments and reads. That fix belongs in sieve. Rewording
gale's scripts cannot reach it: the jobs have to name their own checkout to
build from it. The alternative is still moving those five jobs' checkout to the
workspace root, which is an owner decision.

### Measurements

Scripts and raw JSONL live in the lane's scratch area. The method follows jig's
replay: for each of the last 80 first-parent commits M on main (to f0ac6e3e),
one `sieve classify --event pull_request --base M^ --head M` in git mode. It
uses the tree's `sieve.toml` and files for old diffs, and models no merge_group
or push run. Leaf scoping reads each workflow as it was at M^ and M, so this
replay mostly measures the workflow shapes of that time. The per-job probe
measures today's shapes: one harmless step is inserted into each of the 82
jobs, and the edit is classified against the tree. "Simulated" means the five
`uses: ./gale/.github/actions/setup-renode` lines rewritten to
`./.github/actions/setup-renode` in a scratch commit: what sieve would see if
it resolved them. It is not a proposed gale change (at runtime `./.github/` does
not hold gale in those jobs).

Measured with sieve 0.4.3 (current):

| replay (80 PRs), sieve 0.4.3 | run_all | est. min/PR saved |
|---|---|---|
| f0ac6e3e, scoping off | 75 | 0.2 |
| f0ac6e3e, scoping on | 75 | 0.2 |
| this branch | 75 | 0.2 |
| this branch, `./gale/` simulated resolvable | 71 | 29.5 |

| per-job probe (82 jobs), sieve 0.4.3 | scoped | run_all |
|---|---|---|
| this branch | 0 | 82 (6 classifier jobs; gustos-publish's tag/dispatch `if:`, 2 jobs; the other 74: the `./gale/` rewrite check above) |
| this branch, `./gale/` simulated resolvable | 74 | 8 (6 classifier jobs; gustos-publish's tag/dispatch `if:`, 2 jobs) |

In the simulated replay, #456 and #447 (one-line compliance.yml edits) scope
again under 0.4.3: 0.4.2 refused them for the word "needs" in a script comment
(sieve#37). #537 (a Dependabot-config-only edit) scopes too, but only after this
page stopped spelling that file's name: measured on the tree before that
rewording, the simulated replay gave 72 run_all and an estimated 22.2 min/PR.

History (sieve 0.4.0 and 0.4.2, the same method, tree b6179e01 for "this
branch"):

| replay (80 PRs) | run_all | est. min/PR saved |
|---|---|---|
| f0ac6e3e, sieve 0.4.0, scoping off | 75 | 0.2 |
| f0ac6e3e, sieve 0.4.0, scoping on | 73 | 14.9 |
| f0ac6e3e, sieve 0.4.2, scoping off | 75 | 0.2 |
| f0ac6e3e, sieve 0.4.2, scoping on | 75 | 0.2 |
| this branch, sieve 0.4.2 | 75 | 0.2 |
| this branch, sieve 0.4.2, `./gale/` simulated resolvable | 73 | 14.9 |

| per-job probe (82 jobs, sieve 0.4.2) | scoped | run_all |
|---|---|---|
| this branch | 0 | 82 (all: `./gale/` resolution) |
| this branch, `./gale/` simulated resolvable | 74 | 8 (6 classifier jobs; gustos-publish's tag/dispatch `if:`, 2 jobs) |

Minutes are an **estimate**, not a measurement of any run. Each skipped job
costs its median measured duration over 8 successful push runs on main, with
matrix legs counted one by one: `code` 581.5 min, `cross_arch` 5.0,
`v_closure` 0.2. The classifier jobs' own cost (about 0.2 min each) is not
subtracted. The replay cannot see queueing, cache effects, retries, or what a
PR's author would have done differently.

Safety plants (git mode, scratch commits). With sieve 0.4.3 on the real branch
every row runs everything; the rows that expect a class therefore run
everything too, for the `./gale/` reason, so on the real tree they show only
that nothing narrows. On the branch with `./gale/` simulated resolvable:

| plant | expected | observed (0.4.3, simulated) |
|---|---|---|
| a step added to rust-coverage / cross-arch / v-closure / zephyr-test | its class | `code` / `cross_arch` / `v_closure` / `code` |
| a step added to a proof job (verus) | no class (proofs never skip) | none |
| rust-coverage `if:`, `needs:`, `outputs:`, a step `if:` changed | everything | everything |
| coverage.yml `concurrency`, drv-cross-arch.yml top-level `env` | everything | everything ("changed outside `jobs:`"; the `env` plant makes the file unparseable, so it runs everything for that reason) |
| a step added to a classifier job (coverage, rivet-v-closure) | everything | everything |
| a depth-1 checkout | everything | everything ("git diff failed against <base>") |
| a depth-2 checkout (base = HEAD^) | its class | `code` |
| a `blob:none` clone, full history | everything or its class | everything ("cannot read ... at the merge base") |
| `scope_leaf_edits` removed or `false` | everything | everything |

The 0.4.2 plants (simulated) gave the same verdict on every row but the `blob:none` one, which scoped to `code` there. Both verdicts are safe.

A shallow or blobless checkout fails safe and silently. Nothing goes red: the
saving disappears, and the only trace is the verdict of record's `reason`. sieve-pin
keeps every `sieve classify --base` checkout at fetch-depth 0 (or at least 2).

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

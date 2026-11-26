# Counting sieve's misses (`cargo xtask observe`)

sieve decides which heavy jobs a pull request skips ([sieve.md](sieve.md)). A
skip is wrong when the skipped job would have failed. This page describes the
counter that looks for that after the merge. Requirement:
REQ-CI-SIEVE-OBSERVE-001; verification: VER-CI-SIEVE-OBSERVE-001.

| file | role |
|---|---|
| `tools/xtask/src/observe.rs` | the counter, its planted cases and their unit tests |
| `.github/workflows/sieve-observe.yml` | runs it daily and on demand; not a required context |

## What it compares

The classifier workflows carry `merge_group` triggers, but the counter never
reads a merge-group run. It compares the PR's verdict of record with the push
run on main after the merge, where the classifier runs every gated job (any
event but `pull_request` runs them all). For each first-parent commit of
`origin/main` in the window (see [Running it](#running-it)) the counter:

1. finds the pull request merged as that commit (`commits/{sha}/pulls`; a PR
   that only lists the commit does not count);
2. takes, for each workflow with a classifier job (a workflow running
   `sieve classify`), the latest `pull_request` run of that workflow on the PR's
   head SHA, and in it the LAST record per classifier job: the
   `sieve-verdict-<workflow>-<job>-<run_attempt>` artifact with the highest
   attempt. An older attempt's record is never used in its place, even when the
   newest one has expired;
3. takes the latest `push` run of the same workflow on the merge commit, and its
   job conclusions over every attempt. A job that failed in any attempt counts
   as failed, so a green re-run does not erase a red;
4. runs `sieve observe --config sieve.toml --verdict <record> --results <jobs.json> --format json`.
   sieve lists each job that failed on main although every class it reads
   (`[workflows.job_outputs]`) was false on the PR. Each such job is a miss.

## Honest accounting

Each (commit, workflow record) pair is either compared or **uncompared**, and
an uncompared pair is never counted as "no miss". The report gives
`m miss(es) of N compared, U uncompared` and the reason for each uncompared pair:

- no pull request was merged as the commit (no PR record exists);
- no `pull_request` run of the workflow on the PR head;
- the PR run has no `sieve-verdict-*` artifact. Records exist only for PRs
  whose runs carried the `--record` wiring (#543, f0ac6e3e, onwards);
- the last record has expired (records are kept 90 days);
- no `push` run of the workflow on the commit yet;
- the push run, or a job in it, is not completed;
- sieve compared no skipped job of the pair: the PR skipped nothing
  (`run_all`, or every class true), or every skipped job concluded on main as
  something sieve cannot compare (`cancelled`, `skipped`, `startup_failure`,
  `neutral`, `stale`, or no conclusion). sieve lists those under
  `uncompared_skipped_jobs` with `compared_skipped_jobs: 0`; the counter reads
  both fields, and a pair with nothing compared is never "compared, 0 miss";
- a GitHub call or the download failed (a **failed call**);
- `sieve observe` refused (exit 3), or its output lacks a well-formed
  `misses` array, integer `compared_skipped_jobs` or `uncompared_skipped_jobs`
  array (also a failed call: a malformed field is never read as 0 or "none").

A GitHub record missing a field the counter reads (a run's `status`, a PR's
head SHA, an artifact's `expired`, ...) is a failed call too, and so is an
unreadable workflow file when the classifier workflows are listed (that
refuses the whole command).

The report also counts the skipped jobs sieve judged, and lists each skipped
job sieve could not compare on a pair that compared others.

| exit | meaning |
|---|---|
| 0 | no miss, at least one skipped job compared, and no failed call. Uncompared pairs for a benign reason (no PR, no record, an expired record, no push run yet, a run in progress, nothing skipped) are warnings |
| 1 | at least one miss (takes precedence over a refusal) |
| 2 | usage error |
| 3 | refused, not a pass: no skipped job was compared at all, a GitHub call, a download or sieve failed for any pair, or sieve or `sieve.toml` is missing |

## What it cannot see

- A job that passed on main reveals nothing. A miss needs a failure on main.
- A job sieve skipped that would have passed is invisible by design. The
  counter can show that a skip was wrong, never that a skip was right.
- The push run tests the merged tree; the PR's runs tested its head. A job can
  fail on main only because of commits merged while the PR was open. The
  counter still reports that as a miss of the PR's verdict. Read each miss
  against the run before calling it sieve's.
- A PR merged without any run on its last head SHA has no comparable record.
- It judges only the workflows that run `sieve classify` today; a classifier
  removed since a commit was merged is not looked for.

## Running it

```sh
cargo xtask observe                       # the last 10 first-parent commits
cargo xtask observe --commits 20 --format json
cargo xtask observe --since-days 7
cargo xtask observe --self-test           # planted cases through the real sieve
```

It needs `gh` with a token that can read the repository's actions, pull
requests and contents, `git` history of `origin/main`, and `sieve` on PATH (or
`--sieve PATH`). `--repo OWNER/NAME` overrides `GITHUB_REPOSITORY` and
`gh repo view`; `--ref` overrides `origin/main`.

The workflow installs the pinned sieve through `.github/actions/setup-sieve`
(sieve 0.4.4), runs `--self-test`, then judges the last 20 first-parent
commits and writes the text report to the job summary.

The window is what is judged, not every skip ever made. Each daily run looks
only at the last 20 first-parent commits of main (or `--commits N`). A commit
is judged only while it is in that window; nothing carries over between runs,
and a failed call is not retried within a run (it refuses the run instead). A
commit that leaves the window while still uncompared is never judged, and
when more than 20 merges land between two runs some commits are never in a
window at all.

A miss (exit 1) or a
refusal (exit 3) fails the job. It is not a required context and has no
`pull_request` trigger: it judges main after the fact.

## Planted cases

`cargo xtask observe --self-test` and the unit tests run the same fixtures. The
unit tests use a stand-in for sieve's documented rule; `--self-test` uses the
real binary. Each case comes with a positive control:

| case | planted | verdict | control |
|---|---|---|---|
| (a) | record `code=false`, the code-gated job failed on main | 1 miss, exit 1 | the job passed on main: 0 misses |
| (b) | record `code=true`, the job failed on main | 0 misses; nothing skipped, so that pair is uncompared | (a) |
| (c) | no PR for the commit | uncompared | the same commit with a PR: compared |
| (d) | PR runs exist, the record has expired (the job failed on main) | uncompared | not expired: compared, 1 miss |
| (e) | two attempts, attempt 1 `false`, attempt 2 `true`, the job failed on main | 0 misses (the last record is used) | swapped: 1 miss |
| (f) | nothing compared | refused, exit 3 | one compared pair: exit 0 |
| (g) | the push run, or one of its jobs, still in progress | uncompared, not judged | completed: compared, 1 miss |
| (h) | the skipped gated job concluded `cancelled`, `skipped`, `startup_failure`, `neutral`, `stale` or null on main (6 cases) | uncompared; alone, refused (exit 3); beside a compared pair, exit 0 | the job failed on main: 1 miss |
| (i) | the only commit with a skip has its jobs call fail; the other PR skipped nothing | refused (exit 3), not "0 miss, exit 0" | (j+) |
| (k) | every compared PR ran everything | refused (exit 3) | (a+) |
| (j) | each GitHub call fails in turn: pulls, PR runs, push runs, artifacts, push jobs, record download (6 cases) | uncompared and refused (exit 3) | no call fails: 2 compared, exit 0 |
| (m) | sieve output without `compared_skipped_jobs`, with it as a string or non-integer, without `misses`, with `misses` not an array, without `uncompared_skipped_jobs` (6 cases) | refused (exit 3) | well-formed output: compared, exit 0 |
| (n) | a GitHub run without `status`, with a null `id`; a PR without its head SHA; a job without `name`; an artifact without `expired` | a failed call, not a default | each well-formed record parses |
| (w) | a workflow file that cannot be read | refused | readable workflows are listed |

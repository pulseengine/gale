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

gale has no merge queue, so the push run on main after a merge runs every job.
For each recent first-parent commit of `origin/main` the counter:

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
- a GitHub call or the download failed;
- `sieve observe` refused (exit 3), for example when a record is not a
  `sieve.classify.v1` verdict.

The report also counts the skipped jobs sieve judged. A compared pair whose PR
ran everything (`run_all`, or every class true) skipped nothing, so it cannot
show a miss. When no compared pair skipped anything, the report says so.

| exit | meaning |
|---|---|
| 0 | at least one pair compared, no miss |
| 1 | at least one miss |
| 2 | usage error |
| 3 | refused: nothing compared, or sieve or `sieve.toml` is missing. Not a pass. |

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
(sieve 0.4.0, which has `observe`), runs `--self-test`, then judges the last 20
commits and writes the text report to the job summary. A miss (exit 1) or a
refusal (exit 3) fails the job. It is not a required context and has no
`pull_request` trigger: it judges main after the fact.

## Planted cases

`cargo xtask observe --self-test` and the unit tests run the same fixtures. The
unit tests use a stand-in for sieve's documented rule; `--self-test` uses the
real binary. Each case comes with a positive control:

| case | planted | verdict | control |
|---|---|---|---|
| (a) | record `code=false`, the code-gated job failed on main | 1 miss, exit 1 | the job passed on main: 0 misses |
| (b) | record `code=true`, the job failed on main | 0 misses | (a) |
| (c) | no PR for the commit | uncompared | the same commit with a PR: compared |
| (d) | PR runs exist, the record has expired (the job failed on main) | uncompared | not expired: compared, 1 miss |
| (e) | two attempts, attempt 1 `false`, attempt 2 `true`, the job failed on main | 0 misses (the last record is used) | swapped: 1 miss |
| (f) | nothing compared | refused, exit 3 | one compared pair: exit 0 |
| (g) | the push run, or one of its jobs, still in progress | uncompared, not judged | completed: compared, 1 miss |

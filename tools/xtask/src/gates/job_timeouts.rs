//! Every CI job declares a `timeout-minutes`.
//!
//! gale's CI bottleneck is runner starvation: ~60 jobs per PR against one or two
//! concurrent runners is why the repo holds a 2-PR ceiling. A job with no
//! timeout runs to GitHub's 6-hour default, so one guest that never reaches its
//! exit path holds a runner for six hours against that queue (gale#382). At the
//! time of writing that was most of the repo: the census below is what this
//! gate reported on its first run.
//!
//! JOB level, not step level. A step-level timeout bounds one step; a job with
//! one bounded step and an unbounded checkout or setup step is still unbounded,
//! and "is every step covered" is a harder question than this gate should have
//! to answer. A job that calls a reusable workflow (`uses:` at job level) cannot
//! take `timeout-minutes` -- GitHub rejects the key there -- so it is reported
//! as exempt, and the CALLED workflow's own jobs are checked where they live.
//!
//! The numbers themselves are not this gate's business: they were sized from
//! measured durations (gale#382 records the measurement), because a guessed
//! timeout is a new source of red.

use crate::verdict::Verdict;
use std::path::Path;

pub const NAME: &str = "job-timeouts";

#[derive(Debug, PartialEq)]
enum Job {
    Bounded,
    Reusable,
    Unbounded,
}

/// (job id, verdict) for every job in one workflow. Hand-scanned: a job is a
/// two-space-indented key under a column-0 `jobs:`, and its own keys are at
/// four spaces. Indentation is how YAML scopes these, and every workflow in
/// the repo uses two-space indentation -- the self-test pins that assumption
/// by refusing a workflow in which `jobs:` finds no jobs.
fn jobs(text: &str) -> Vec<(String, Job)> {
    let mut out: Vec<(String, Job)> = Vec::new();
    let mut in_jobs = false;
    for line in text.lines() {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        let indent = line.len() - line.trim_start().len();
        if indent == 0 {
            in_jobs = line.trim_end() == "jobs:";
            continue;
        }
        if !in_jobs {
            continue;
        }
        let t = line.trim();
        if indent == 2 && t.ends_with(':') && !t.contains(' ') {
            out.push((t.trim_end_matches(':').to_string(), Job::Unbounded));
            continue;
        }
        if indent == 4 {
            if let Some(last) = out.last_mut() {
                if t.starts_with("timeout-minutes:") {
                    last.1 = Job::Bounded;
                } else if t.starts_with("uses:") && last.1 == Job::Unbounded {
                    last.1 = Job::Reusable;
                }
            }
        }
    }
    out
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let wf = repo.join(".github/workflows");
    let Ok(rd) = std::fs::read_dir(&wf) else {
        return Verdict::Refused(format!("cannot read {}", wf.display()));
    };
    let mut files: Vec<_> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| matches!(p.extension().and_then(|e| e.to_str()), Some("yml" | "yaml")))
        .collect();
    files.sort();
    if files.is_empty() {
        return Verdict::Refused("no workflows found -- the sweep would be vacuous".into());
    }

    let mut lines = Vec::new();
    let mut unbounded = Vec::new();
    let (mut total, mut reusable) = (0usize, 0usize);
    for f in &files {
        let name = f.file_name().unwrap().to_string_lossy().to_string();
        let Ok(text) = std::fs::read_to_string(f) else {
            return Verdict::Refused(format!("cannot read {name}"));
        };
        let js = jobs(&text);
        if text.lines().any(|l| l.trim_end() == "jobs:") && js.is_empty() {
            return Verdict::Refused(format!(
                "{name} has `jobs:` but the scanner found no jobs -- indentation it does \
                 not understand, so it cannot answer"
            ));
        }
        for (id, j) in js {
            total += 1;
            match j {
                Job::Bounded => {}
                Job::Reusable => reusable += 1,
                Job::Unbounded => unbounded.push(format!("{name}: {id}")),
            }
        }
    }

    if !unbounded.is_empty() {
        lines.push(format!(
            "FAIL: {} of {total} job(s) have no timeout-minutes and run to the 6-hour default:",
            unbounded.len()
        ));
        lines.extend(unbounded.iter().map(|u| format!("  {u}")));
        return Verdict::Fail(lines);
    }
    lines.push(format!(
        "{total} job(s) in {} workflow(s): every one bounded ({reusable} call a reusable \
         workflow and are bounded where it lives).",
        files.len()
    ));
    Verdict::Pass(lines)
}

/// Each case is a way the scanner could be WRONG: a step-level timeout must not
/// count for the job, a nested key must not be read as a job, and a reusable
/// call must not be reported as unbounded.
pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };
    let wf = "\
on:
  push:
    branches: [main]
env:
  timeout-minutes: 5
jobs:
  bounded:
    runs-on: x
    timeout-minutes: 10
    steps:
      - run: true
  step-only:
    runs-on: x
    steps:
      - run: true
        timeout-minutes: 3
  # a comment at job indent is not a job
  reusable:
    uses: ./.github/workflows/other.yml
  nested:
    runs-on: x
    strategy:
      matrix:
        timeout-minutes: [1]
";
    let js = jobs(wf);
    let get = |n: &str| js.iter().find(|(i, _)| i == n).map(|(_, j)| j);
    ck("four jobs found, the comment and nested keys are not jobs", js.len() == 4);
    ck("a job-level timeout bounds the job", get("bounded") == Some(&Job::Bounded));
    ck("a STEP-level timeout does not bound the job", get("step-only") == Some(&Job::Unbounded));
    ck("a reusable-workflow call is exempt, not unbounded", get("reusable") == Some(&Job::Reusable));
    ck("a timeout nested deeper than the job does not count", get("nested") == Some(&Job::Unbounded));
    ck("a top-level env timeout is not a job's", !js.iter().any(|(i, _)| i == "env"));
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

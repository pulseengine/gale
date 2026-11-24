//! `cargo xtask observe`: the observed-miss counter for sieve's change
//! detection (REQ-CI-SIEVE-OBSERVE-001, docs/ci/sieve-observe.md).
//!
//! On a pull request a classifier job writes sieve's verdict of record and
//! uploads it as `sieve-verdict-<workflow>-<job>-<run_attempt>`. gale has no
//! merge queue, so the push run on main after the merge runs every job. For
//! each recent first-parent commit on main this command takes the merged PR's
//! LAST record per classifier job (latest pull_request run of the workflow on
//! the PR head, highest attempt), the push run of the same workflow on the
//! commit, and hands both to `sieve observe`. sieve lists every job that failed
//! on main although all its classes were false on the PR: a miss.
//!
//! Accounting is the point. A pair this command cannot compare (no PR, no PR
//! run, no record, an expired record, no push run yet, a run still in progress,
//! a failed download, sieve refusing) is UNCOMPARED with its reason, never "no
//! miss". Zero compared pairs is a refusal (exit 3), not a pass.
//!
//! What it cannot see: a job that passed on main reveals nothing, and a job
//! sieve skipped that would have passed is invisible by design. A miss needs a
//! failure on main.
//!
//! GitHub I/O sits behind [`Forge`] and the sieve call behind [`Observer`], so
//! the planted cases below run on fixtures (unit tests, fake sieve) and, with
//! `--self-test`, through the real sieve binary.

use crate::gates::target_reference::{parse_json, Json};
use crate::verdict::{escape, EXIT_FAIL, EXIT_PASS, EXIT_REFUSED, EXIT_USAGE};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

const RECORD_PREFIX: &str = "sieve-verdict-";
const RECORD_FILE: &str = "sieve-verdict.json";

// ------------------------------------------------------------------ the data

#[derive(Debug, Clone)]
pub struct Commit {
    pub sha: String,
    pub subject: String,
}

#[derive(Debug, Clone)]
pub struct Pull {
    pub number: u64,
    pub head_sha: String,
    pub merge_commit_sha: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Run {
    pub id: u64,
    pub path: String,
    pub status: String,
    pub run_attempt: u64,
    pub run_number: u64,
}

#[derive(Debug, Clone)]
pub struct Job {
    pub name: String,
    pub status: String,
    pub conclusion: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Artifact {
    pub name: String,
    pub expired: bool,
}

/// The GitHub reads this command needs. Every error is a reason to leave a
/// pair uncompared, never to call it clean.
pub trait Forge {
    fn pulls(&self, sha: &str) -> Result<Vec<Pull>, String>;
    /// Runs on `sha` for one event (`pull_request` or `push`).
    fn runs(&self, sha: &str, event: &str) -> Result<Vec<Run>, String>;
    fn jobs(&self, run_id: u64, attempt: u64) -> Result<Vec<Job>, String>;
    fn artifacts(&self, run_id: u64) -> Result<Vec<Artifact>, String>;
    /// The record's JSON text from one artifact of one run.
    fn record(&self, run_id: u64, artifact: &str) -> Result<String, String>;
}

#[derive(Debug, Clone, PartialEq)]
pub enum Observed {
    /// sieve compared: these jobs (rendered) are misses, and it judged
    /// `skipped` jobs that the verdict had skipped.
    Compared { misses: Vec<String>, skipped: u64 },
    /// sieve refused (exit 3) or could not be run.
    Refused(String),
}

/// `sieve observe --verdict <record> --results <jobs.json>`.
pub trait Observer {
    fn observe(&self, record: &str, results: &str) -> Observed;
}

// --------------------------------------------------------------- the report

#[derive(Debug, Clone)]
pub struct Pair {
    pub commit: String,
    pub subject: String,
    pub pr: Option<u64>,
    /// Workflow path, or `*` when the whole commit could not be compared.
    pub workflow: String,
    pub record: Option<String>,
    pub push_run: Option<u64>,
    /// None = compared.
    pub uncompared: Option<String>,
    pub misses: Vec<String>,
    pub skipped: u64,
}

#[derive(Debug, Default)]
pub struct Report {
    pub commits: usize,
    pub workflows: usize,
    pub pairs: Vec<Pair>,
}

impl Report {
    pub fn compared(&self) -> usize {
        self.pairs.iter().filter(|p| p.uncompared.is_none()).count()
    }
    pub fn uncompared(&self) -> usize {
        self.pairs.len() - self.compared()
    }
    pub fn misses(&self) -> usize {
        self.pairs.iter().map(|p| p.misses.len()).sum()
    }
    pub fn skipped(&self) -> u64 {
        self.pairs.iter().filter(|p| p.uncompared.is_none()).map(|p| p.skipped).sum()
    }
    /// 1 any miss; 3 nothing compared (not a pass); else 0.
    pub fn exit(&self) -> i32 {
        if self.misses() > 0 {
            EXIT_FAIL
        } else if self.compared() == 0 {
            EXIT_REFUSED
        } else {
            EXIT_PASS
        }
    }
    fn verdict(&self) -> &'static str {
        match self.exit() {
            EXIT_FAIL => "miss",
            EXIT_REFUSED => "refused",
            _ => "pass",
        }
    }

    pub fn headline(&self) -> String {
        format!(
            "sieve observe: {} miss(es) of {} compared, {} uncompared ({} commit(s) x {} classifier workflow(s); {} skipped job(s) judged)",
            self.misses(),
            self.compared(),
            self.uncompared(),
            self.commits,
            self.workflows,
            self.skipped()
        )
    }

    pub fn text(&self) -> String {
        let mut s = format!("{}\n", self.headline());
        for p in &self.pairs {
            let pr = p.pr.map(|n| format!("#{n}")).unwrap_or_else(|| "no PR".into());
            let c = &p.commit[..p.commit.len().min(8)];
            match &p.uncompared {
                Some(r) => s.push_str(&format!("  UNCOMPARED {c} {pr} {}: {r}\n", p.workflow)),
                None => {
                    for m in &p.misses {
                        s.push_str(&format!("  MISS       {c} {pr} {}: {m}\n", p.workflow));
                    }
                    s.push_str(&format!(
                        "  COMPARED   {c} {pr} {}: record {}, push run {}, {} miss(es), {} skipped job(s) judged\n",
                        p.workflow,
                        p.record.as_deref().unwrap_or("?"),
                        p.push_run.map(|n| n.to_string()).unwrap_or_else(|| "?".into()),
                        p.misses.len(),
                        p.skipped
                    ));
                }
            }
        }
        s.push_str(match self.exit() {
            EXIT_FAIL => "verdict: MISS -- a job failed on main that sieve skipped on its PR (exit 1)\n",
            EXIT_REFUSED => "verdict: REFUSED -- nothing was compared; that is not a pass (exit 3)\n",
            _ => "verdict: no miss among the compared pairs (exit 0); uncompared pairs say nothing\n",
        });
        if self.compared() > 0 && self.skipped() == 0 {
            s.push_str("note: no compared pair skipped any gated job (every compared PR ran everything), so this run could not have seen a miss\n");
        }
        s.push_str(
            "limits: a job that passed on main reveals nothing, and a skipped job that would have passed is invisible by design\n",
        );
        s
    }

    pub fn json(&self) -> String {
        let pairs: Vec<String> = self
            .pairs
            .iter()
            .map(|p| {
                let opt = |o: &Option<String>| match o {
                    Some(v) => format!("\"{}\"", escape(v)),
                    None => "null".into(),
                };
                let misses: Vec<String> = p.misses.iter().map(|m| format!("\"{}\"", escape(m))).collect();
                format!(
                    "{{\"commit\":\"{}\",\"subject\":\"{}\",\"pr\":{},\"workflow\":\"{}\",\"status\":\"{}\",\"record\":{},\"push_run\":{},\"reason\":{},\"misses\":[{}],\"skipped_judged\":{}}}",
                    escape(&p.commit),
                    escape(&p.subject),
                    p.pr.map(|n| n.to_string()).unwrap_or_else(|| "null".into()),
                    escape(&p.workflow),
                    if p.uncompared.is_some() { "uncompared" } else { "compared" },
                    opt(&p.record),
                    p.push_run.map(|n| n.to_string()).unwrap_or_else(|| "null".into()),
                    opt(&p.uncompared),
                    misses.join(","),
                    p.skipped
                )
            })
            .collect();
        format!(
            "{{\"schema\":\"gale.observe.v1\",\"verdict\":\"{}\",\"exit\":{},\"misses\":{},\"compared\":{},\"uncompared\":{},\"commits\":{},\"workflows\":{},\"skipped_judged\":{},\"pairs\":[{}]}}",
            self.verdict(),
            self.exit(),
            self.misses(),
            self.compared(),
            self.uncompared(),
            self.commits,
            self.workflows,
            self.skipped(),
            pairs.join(",")
        )
    }
}

// ----------------------------------------------------------------- the core

/// The attempt number at the end of a record name, and the name without it:
/// `sieve-verdict-coverage-changes-2` -> (`sieve-verdict-coverage-changes`, 2).
fn record_key(name: &str) -> Option<(&str, u64)> {
    if !name.starts_with(RECORD_PREFIX) {
        return None;
    }
    let (key, n) = name.rsplit_once('-')?;
    Some((key, n.parse().ok()?))
}

/// Latest run of `path`: highest run_number, then highest id.
fn latest<'a>(runs: &'a [Run], path: &str) -> Option<&'a Run> {
    runs.iter().filter(|r| r.path == path).max_by_key(|r| (r.run_number, r.id))
}

fn failed(c: &str) -> bool {
    c == "failure" || c == "timed_out"
}

/// The push run's job conclusions as `[{name, conclusion}]`, over every attempt:
/// a job that failed in any attempt counts as failed (a later green re-run does
/// not erase the failure); otherwise the latest attempt's conclusion. Err when
/// any job is not completed.
fn push_results(forge: &dyn Forge, run: &Run) -> Result<String, String> {
    let mut merged: BTreeMap<String, String> = BTreeMap::new();
    for attempt in 1..=run.run_attempt.max(1) {
        let jobs = forge.jobs(run.id, attempt).map_err(|e| format!("cannot list jobs of push run {} attempt {attempt}: {e}", run.id))?;
        for j in jobs {
            if j.status != "completed" {
                return Err(format!("push run {} attempt {attempt}: job `{}` is {}, not judged", run.id, j.name, j.status));
            }
            let c = j.conclusion.unwrap_or_else(|| "none".into());
            match merged.get(&j.name) {
                Some(prev) if failed(prev) => {}
                _ => {
                    merged.insert(j.name, c);
                }
            }
        }
    }
    if merged.is_empty() {
        return Err(format!("push run {} has no jobs", run.id));
    }
    let items: Vec<String> = merged
        .iter()
        .map(|(n, c)| format!("{{\"name\":\"{}\",\"conclusion\":\"{}\"}}", escape(n), escape(c)))
        .collect();
    Ok(format!("[{}]", items.join(",")))
}

pub fn observe_commits(forge: &dyn Forge, sieve: &dyn Observer, commits: &[Commit], workflows: &[String]) -> Report {
    let mut rep = Report { commits: commits.len(), workflows: workflows.len(), pairs: Vec::new() };
    for c in commits {
        let base = Pair {
            commit: c.sha.clone(),
            subject: c.subject.clone(),
            pr: None,
            workflow: "*".into(),
            record: None,
            push_run: None,
            uncompared: None,
            misses: Vec::new(),
            skipped: 0,
        };
        let whole = |why: String| Pair { uncompared: Some(why), ..base.clone() };
        let pulls = match forge.pulls(&c.sha) {
            Ok(p) => p,
            Err(e) => {
                rep.pairs.push(whole(format!("cannot list the commit's pull requests: {e}")));
                continue;
            }
        };
        let Some(pr) = pulls.iter().find(|p| p.merge_commit_sha.as_deref() == Some(c.sha.as_str())) else {
            let why = if pulls.is_empty() {
                "no pull request for this commit (no PR record exists)".to_string()
            } else {
                let ns: Vec<String> = pulls.iter().map(|p| format!("#{}", p.number)).collect();
                format!("pull request(s) {} list this commit, but none was merged as it", ns.join(", "))
            };
            rep.pairs.push(whole(why));
            continue;
        };
        let base = Pair { pr: Some(pr.number), ..base };
        let pr_runs = forge.runs(&pr.head_sha, "pull_request");
        let push_runs = forge.runs(&c.sha, "push");
        for wf in workflows {
            let pair = Pair { workflow: wf.clone(), ..base.clone() };
            let un = |why: String| Pair { uncompared: Some(why), ..pair.clone() };
            let pr_runs = match &pr_runs {
                Ok(r) => r,
                Err(e) => {
                    rep.pairs.push(un(format!("cannot list the PR head's runs: {e}")));
                    continue;
                }
            };
            let Some(pr_run) = latest(pr_runs, wf) else {
                rep.pairs.push(un(format!("no pull_request run of this workflow on the PR head {}", &pr.head_sha[..pr.head_sha.len().min(8)])));
                continue;
            };
            let push_runs = match &push_runs {
                Ok(r) => r,
                Err(e) => {
                    rep.pairs.push(un(format!("cannot list the commit's push runs: {e}")));
                    continue;
                }
            };
            let Some(push_run) = latest(push_runs, wf) else {
                rep.pairs.push(un("no push run of this workflow on the commit (not yet, or the push did not trigger it)".into()));
                continue;
            };
            let pair = Pair { push_run: Some(push_run.id), ..pair };
            let un = |why: String| Pair { uncompared: Some(why), ..pair.clone() };
            if push_run.status != "completed" {
                rep.pairs.push(un(format!("push run {} is {}, not judged", push_run.id, push_run.status)));
                continue;
            }
            let arts = match forge.artifacts(pr_run.id) {
                Ok(a) => a,
                Err(e) => {
                    rep.pairs.push(un(format!("cannot list artifacts of PR run {}: {e}", pr_run.id)));
                    continue;
                }
            };
            // The LAST record per classifier job: the highest attempt, expired or
            // not. An older attempt's record is never a fallback.
            let mut last: BTreeMap<&str, (u64, &Artifact)> = BTreeMap::new();
            for a in &arts {
                if let Some((key, n)) = record_key(&a.name) {
                    if last.get(key).map_or(true, |(m, _)| n > *m) {
                        last.insert(key, (n, a));
                    }
                }
            }
            if last.is_empty() {
                rep.pairs.push(un(format!("PR run {} has no {RECORD_PREFIX}* artifact (no record was made)", pr_run.id)));
                continue;
            }
            let results = match push_results(forge, push_run) {
                Ok(r) => r,
                Err(e) => {
                    rep.pairs.push(un(e));
                    continue;
                }
            };
            for (_, (_, a)) in last {
                let p = Pair { record: Some(a.name.clone()), ..pair.clone() };
                if a.expired {
                    rep.pairs.push(Pair { uncompared: Some(format!("record {} of PR run {} has expired", a.name, pr_run.id)), ..p });
                    continue;
                }
                let text = match forge.record(pr_run.id, &a.name) {
                    Ok(t) => t,
                    Err(e) => {
                        rep.pairs.push(Pair { uncompared: Some(format!("cannot download record {}: {e}", a.name)), ..p });
                        continue;
                    }
                };
                match sieve.observe(&text, &results) {
                    Observed::Compared { misses, skipped } => rep.pairs.push(Pair { misses, skipped, ..p }),
                    Observed::Refused(r) => rep.pairs.push(Pair { uncompared: Some(format!("sieve observe refused: {r}")), ..p }),
                }
            }
        }
    }
    rep
}

// ------------------------------------------------------- the real sieve call

pub struct SieveCli {
    pub sieve: String,
    pub config: PathBuf,
    pub work: PathBuf,
}

/// Parse `sieve observe --format json` output plus its exit code.
pub fn parse_observe(code: Option<i32>, stdout: &str, stderr: &str) -> Observed {
    let err = stderr.trim();
    match code {
        Some(0) | Some(1) => {}
        Some(3) => return Observed::Refused(if err.is_empty() { "exit 3".into() } else { err.to_string() }),
        other => return Observed::Refused(format!("sieve observe exited {other:?}: {err}")),
    }
    let v = match parse_json(stdout) {
        Ok(v) => v,
        Err(e) => return Observed::Refused(format!("unparseable sieve observe output: {e}")),
    };
    if v.get("schema").and_then(Json::str) != Some("sieve.observe.v1") {
        return Observed::Refused("sieve observe output is not sieve.observe.v1".into());
    }
    if v.get("refused") == Some(&Json::Bool(true)) {
        return Observed::Refused("sieve observe output says refused".into());
    }
    let misses: Vec<String> = v
        .get("misses")
        .map(Json::arr)
        .unwrap_or(&[])
        .iter()
        .map(|m| {
            let classes: Vec<&str> = m.get("classes").map(Json::arr).unwrap_or(&[]).iter().filter_map(Json::str).collect();
            format!(
                "{} ({}): sieve selected none of {:?} on the PR",
                m.get("job").and_then(Json::str).unwrap_or("?"),
                m.get("conclusion").and_then(Json::str).unwrap_or("?"),
                classes
            )
        })
        .collect();
    // Exit code and list must agree, or the output is not trusted.
    if (code == Some(1)) != !misses.is_empty() {
        return Observed::Refused(format!("sieve observe exit {code:?} disagrees with its {} listed miss(es)", misses.len()));
    }
    let skipped = match v.get("compared_skipped_jobs") {
        Some(Json::Num(n)) if *n >= 0.0 => *n as u64,
        _ => 0,
    };
    Observed::Compared { misses, skipped }
}

impl Observer for SieveCli {
    fn observe(&self, record: &str, results: &str) -> Observed {
        let r = self.work.join("verdict.json");
        let j = self.work.join("jobs.json");
        if let Err(e) = std::fs::write(&r, record).and_then(|_| std::fs::write(&j, results)) {
            return Observed::Refused(format!("cannot write sieve inputs under {}: {e}", self.work.display()));
        }
        let out = Command::new(&self.sieve)
            .arg("observe")
            .arg("--config")
            .arg(&self.config)
            .arg("--verdict")
            .arg(&r)
            .arg("--results")
            .arg(&j)
            .args(["--format", "json"])
            .output();
        match out {
            Ok(o) => parse_observe(o.status.code(), &String::from_utf8_lossy(&o.stdout), &String::from_utf8_lossy(&o.stderr)),
            Err(e) => Observed::Refused(format!("cannot run `{}`: {e}", self.sieve)),
        }
    }
}

// ------------------------------------------------------------ GitHub via gh

pub struct GhForge {
    pub repo: String,
    pub work: PathBuf,
}

fn gh(args: &[&str]) -> Result<String, String> {
    let o = Command::new("gh").args(args).output().map_err(|e| format!("cannot run gh: {e}"))?;
    if !o.status.success() {
        return Err(format!("gh {} failed: {}", args.first().unwrap_or(&""), String::from_utf8_lossy(&o.stderr).trim()));
    }
    String::from_utf8(o.stdout).map_err(|_| "gh output is not UTF-8".into())
}

/// `gh api --paginate <path> --jq <jq>`: one compact JSON value per line.
fn gh_lines(path: &str, jq: &str) -> Result<Vec<Json>, String> {
    let out = gh(&["api", "--paginate", path, "--jq", jq])?;
    out.lines().filter(|l| !l.trim().is_empty()).map(|l| parse_json(l).map_err(|e| format!("bad gh output: {e}"))).collect()
}

fn s(v: &Json, k: &str) -> String {
    v.get(k).and_then(Json::str).unwrap_or("").to_string()
}

fn n(v: &Json, k: &str) -> u64 {
    match v.get(k) {
        Some(Json::Num(x)) if *x >= 0.0 => *x as u64,
        _ => 0,
    }
}

impl Forge for GhForge {
    fn pulls(&self, sha: &str) -> Result<Vec<Pull>, String> {
        let v = gh_lines(
            &format!("repos/{}/commits/{sha}/pulls?per_page=100", self.repo),
            ".[] | {number, merge_commit_sha, merged_at, head: .head.sha}",
        )?;
        Ok(v.iter()
            .map(|p| Pull {
                number: n(p, "number"),
                head_sha: s(p, "head"),
                // Only a merged PR has a merge commit that counts.
                merge_commit_sha: if p.get("merged_at").and_then(Json::str).is_some() { p.get("merge_commit_sha").and_then(Json::str).map(String::from) } else { None },
            })
            .collect())
    }
    fn runs(&self, sha: &str, event: &str) -> Result<Vec<Run>, String> {
        let v = gh_lines(
            &format!("repos/{}/actions/runs?head_sha={sha}&event={event}&per_page=100", self.repo),
            ".workflow_runs[] | {id, path, status, run_attempt, run_number}",
        )?;
        Ok(v.iter()
            .map(|r| Run { id: n(r, "id"), path: s(r, "path"), status: s(r, "status"), run_attempt: n(r, "run_attempt"), run_number: n(r, "run_number") })
            .collect())
    }
    fn jobs(&self, run_id: u64, attempt: u64) -> Result<Vec<Job>, String> {
        let v = gh_lines(
            &format!("repos/{}/actions/runs/{run_id}/attempts/{attempt}/jobs?per_page=100", self.repo),
            ".jobs[] | {name, status, conclusion}",
        )?;
        Ok(v.iter().map(|j| Job { name: s(j, "name"), status: s(j, "status"), conclusion: j.get("conclusion").and_then(Json::str).map(String::from) }).collect())
    }
    fn artifacts(&self, run_id: u64) -> Result<Vec<Artifact>, String> {
        let v = gh_lines(&format!("repos/{}/actions/runs/{run_id}/artifacts?per_page=100", self.repo), ".artifacts[] | {name, expired}")?;
        Ok(v.iter().map(|a| Artifact { name: s(a, "name"), expired: a.get("expired") == Some(&Json::Bool(true)) }).collect())
    }
    fn record(&self, run_id: u64, artifact: &str) -> Result<String, String> {
        let dir = self.work.join(format!("{run_id}-{artifact}"));
        if dir.exists() {
            // Downloaded earlier in this invocation (another commit cannot share a run).
            return std::fs::read_to_string(dir.join(RECORD_FILE)).map_err(|e| e.to_string());
        }
        std::fs::create_dir_all(&dir).map_err(|e| format!("cannot create {}: {e}", dir.display()))?;
        let d = dir.to_string_lossy().to_string();
        gh(&["run", "download", &run_id.to_string(), "-R", &self.repo, "-n", artifact, "-D", &d])?;
        std::fs::read_to_string(dir.join(RECORD_FILE)).map_err(|e| format!("artifact {artifact} holds no {RECORD_FILE}: {e}"))
    }
}

// ------------------------------------------------------------ the inputs

/// First-parent commits of `git_ref`, newest first.
pub fn main_commits(root: &Path, git_ref: &str, sel: &Select) -> Result<Vec<Commit>, String> {
    let mut cmd = Command::new("git");
    cmd.arg("-C").arg(root).args(["log", "--first-parent", "--format=%H%x09%s"]);
    match sel {
        Select::Commits(k) => {
            cmd.arg(format!("-n{k}"));
        }
        Select::SinceDays(d) => {
            cmd.arg(format!("--since={d} days ago"));
        }
    }
    cmd.arg(git_ref).arg("--");
    let o = cmd.output().map_err(|e| format!("cannot run git: {e}"))?;
    if !o.status.success() {
        return Err(format!("git log {git_ref} failed: {}", String::from_utf8_lossy(&o.stderr).trim()));
    }
    Ok(String::from_utf8_lossy(&o.stdout)
        .lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(sha, subject)| Commit { sha: sha.to_string(), subject: subject.to_string() })
        .collect())
}

/// The workflows with a classifier job: a non-comment line running `sieve classify`.
pub fn classifier_workflows(root: &Path) -> Result<Vec<String>, String> {
    let dir = root.join(".github/workflows");
    let rd = std::fs::read_dir(&dir).map_err(|e| format!("cannot read {}: {e}", dir.display()))?;
    let mut out = Vec::new();
    for e in rd.flatten() {
        let name = e.file_name().to_string_lossy().to_string();
        if !(name.ends_with(".yml") || name.ends_with(".yaml")) {
            continue;
        }
        let text = std::fs::read_to_string(e.path()).unwrap_or_default();
        if text.lines().any(|l| {
            let t = l.trim_start();
            !t.starts_with('#') && t.contains("sieve classify")
        }) {
            out.push(format!(".github/workflows/{name}"));
        }
    }
    out.sort();
    Ok(out)
}

// ------------------------------------------------------------ the command

#[derive(Debug, Clone, PartialEq)]
pub enum Select {
    Commits(u64),
    SinceDays(u64),
}

const USAGE: &str = "usage: cargo xtask observe [--since-days N | --commits N] [--format text|json]\n\
                     \x20                        [--ref REF (origin/main)] [--repo OWNER/NAME] [--sieve PATH (sieve)]\n\
                     \x20      cargo xtask observe --self-test [--sieve PATH]\n\
                     exit: 0 no miss among >= 1 compared pair, 1 a miss, 2 usage, 3 refused (nothing compared, or a tool is missing)";

fn work_dir() -> Result<PathBuf, String> {
    let base = std::env::var_os("RUNNER_TEMP").map(PathBuf::from).unwrap_or_else(std::env::temp_dir);
    let d = base.join(format!("gale-observe-{}", std::process::id()));
    std::fs::create_dir_all(&d).map_err(|e| format!("cannot create {}: {e}", d.display()))?;
    Ok(d)
}

fn repo_name() -> Result<String, String> {
    if let Ok(r) = std::env::var("GITHUB_REPOSITORY") {
        if !r.is_empty() {
            return Ok(r);
        }
    }
    Ok(gh(&["repo", "view", "--json", "nameWithOwner", "--jq", ".nameWithOwner"])?.trim().to_string())
}

#[derive(Debug, PartialEq)]
pub struct Opts {
    pub sel: Select,
    pub json: bool,
    pub self_test: bool,
    pub git_ref: String,
    pub repo: Option<String>,
    pub sieve: String,
}

pub fn parse_args(args: &[String]) -> Result<Opts, String> {
    let mut o = Opts { sel: Select::Commits(10), json: false, self_test: false, git_ref: "origin/main".into(), repo: None, sieve: "sieve".into() };
    let mut sel_set = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let a = a.as_str();
        let mut val = || it.next().cloned().ok_or_else(|| format!("{a} needs a value"));
        match a {
            "--commits" | "--since-days" => {
                if sel_set {
                    return Err("--commits and --since-days are exclusive, and each is given once".into());
                }
                sel_set = true;
                let v = val()?;
                let k = v.parse::<u64>().ok().filter(|n| *n > 0).ok_or_else(|| format!("{a} needs a positive integer, got '{v}'"))?;
                o.sel = if a == "--commits" { Select::Commits(k) } else { Select::SinceDays(k) };
            }
            "--format" => match val()?.as_str() {
                "json" => o.json = true,
                "text" => o.json = false,
                f => return Err(format!("unknown --format '{f}' (text|json)")),
            },
            "--ref" => o.git_ref = val()?,
            "--repo" => o.repo = Some(val()?),
            "--sieve" => o.sieve = val()?,
            "--self-test" => o.self_test = true,
            other => return Err(format!("unknown argument '{other}'")),
        }
    }
    Ok(o)
}

pub fn cmd(root: &Path, args: &[String]) -> i32 {
    let o = match parse_args(args) {
        Ok(o) => o,
        Err(e) => {
            eprintln!("xtask observe: {e}\n{USAGE}");
            return EXIT_USAGE;
        }
    };
    let Opts { sel, json, self_test, git_ref, repo, sieve } = o;

    let work = match work_dir() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("xtask observe: {e}");
            return EXIT_REFUSED;
        }
    };
    let config = root.join("sieve.toml");
    if !config.is_file() {
        eprintln!("xtask observe: refused: {} not found", config.display());
        return EXIT_REFUSED;
    }
    let observer = SieveCli { sieve: sieve.clone(), config, work: work.clone() };
    if self_test {
        return run_self_test(&observer);
    }

    let commits = match main_commits(root, &git_ref, &sel) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("xtask observe: refused: {e}");
            return EXIT_REFUSED;
        }
    };
    let workflows = match classifier_workflows(root) {
        Ok(w) if !w.is_empty() => w,
        Ok(_) => {
            eprintln!("xtask observe: refused: no workflow runs `sieve classify`; nothing to compare");
            return EXIT_REFUSED;
        }
        Err(e) => {
            eprintln!("xtask observe: refused: {e}");
            return EXIT_REFUSED;
        }
    };
    let repo = match repo.map(Ok).unwrap_or_else(repo_name) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("xtask observe: refused: cannot tell the repository: {e}");
            return EXIT_REFUSED;
        }
    };
    let forge = GhForge { repo, work };
    let rep = observe_commits(&forge, &observer, &commits, &workflows);
    if json {
        println!("{}", rep.json());
    } else {
        print!("{}", rep.text());
    }
    rep.exit()
}

// ------------------------------------------------- fixtures + planted cases

/// An in-memory forge: the fixtures behind the unit tests and `--self-test`.
#[derive(Default, Clone)]
pub struct FakeForge {
    pub pulls: BTreeMap<String, Vec<Pull>>,
    pub runs: BTreeMap<(String, String), Vec<Run>>,
    pub jobs: BTreeMap<(u64, u64), Vec<Job>>,
    pub artifacts: BTreeMap<u64, Vec<Artifact>>,
    pub records: BTreeMap<(u64, String), String>,
}

impl Forge for FakeForge {
    fn pulls(&self, sha: &str) -> Result<Vec<Pull>, String> {
        Ok(self.pulls.get(sha).cloned().unwrap_or_default())
    }
    fn runs(&self, sha: &str, event: &str) -> Result<Vec<Run>, String> {
        Ok(self.runs.get(&(sha.to_string(), event.to_string())).cloned().unwrap_or_default())
    }
    fn jobs(&self, run_id: u64, attempt: u64) -> Result<Vec<Job>, String> {
        self.jobs.get(&(run_id, attempt)).cloned().ok_or_else(|| format!("no attempt {attempt} of run {run_id}"))
    }
    fn artifacts(&self, run_id: u64) -> Result<Vec<Artifact>, String> {
        Ok(self.artifacts.get(&run_id).cloned().unwrap_or_default())
    }
    fn record(&self, run_id: u64, artifact: &str) -> Result<String, String> {
        self.records.get(&(run_id, artifact.to_string())).cloned().ok_or_else(|| "artifact not found".into())
    }
}

pub const WF: &str = ".github/workflows/coverage.yml";
/// A job gale's sieve.toml gates on `code` ([workflows.job_outputs]).
pub const CODE_JOB: &str = "Rust Coverage";

pub fn record_json(code: bool) -> String {
    format!(
        "{{\"schema\":\"sieve.classify.v1\",\"classes\":{{\"code\":{code},\"cross_arch\":false,\"v_closure\":false}},\"run_all\":false,\"reason\":null,\"explain\":{{}},\"unknown_paths\":[],\"unresolved\":{{}}}}"
    )
}

/// One merged commit `m<i>` (PR #i, head `h<i>`) with a completed PR run and push run of
/// [`WF`]; its record says `code`, and on main the code-gated job concluded `main`.
pub fn plant_commit(f: &mut FakeForge, i: u64, code: bool, main: &str) -> Commit {
    let (m, h) = (format!("m{i}"), format!("h{i}"));
    let (pr_run, push_run) = (100 + i, 200 + i);
    f.pulls.insert(m.clone(), vec![Pull { number: i, head_sha: h.clone(), merge_commit_sha: Some(m.clone()) }]);
    f.runs.insert((h, "pull_request".into()), vec![Run { id: pr_run, path: WF.into(), status: "completed".into(), run_attempt: 1, run_number: i }]);
    f.runs.insert((m.clone(), "push".into()), vec![Run { id: push_run, path: WF.into(), status: "completed".into(), run_attempt: 1, run_number: i }]);
    let name = "sieve-verdict-coverage-changes-1".to_string();
    f.artifacts.insert(pr_run, vec![Artifact { name: name.clone(), expired: false }]);
    f.records.insert((pr_run, name), record_json(code));
    f.jobs.insert(
        (push_run, 1),
        vec![
            Job { name: "Detect changes".into(), status: "completed".into(), conclusion: Some("success".into()) },
            Job { name: CODE_JOB.into(), status: "completed".into(), conclusion: Some(main.into()) },
        ],
    );
    Commit { sha: m, subject: format!("planted #{i}") }
}

pub struct Planted {
    pub name: &'static str,
    pub forge: FakeForge,
    pub commits: Vec<Commit>,
    /// (misses, compared, uncompared, exit)
    pub want: (usize, usize, usize, i32),
}

fn case(name: &'static str, forge: FakeForge, commits: Vec<Commit>, want: (usize, usize, usize, i32)) -> Planted {
    Planted { name, forge, commits, want }
}

/// The planted bad inputs (a)-(g), each with its positive control.
pub fn planted() -> Vec<Planted> {
    let mut v = Vec::new();

    // (a) code=false on the PR, the code-gated job failed on main -> 1 miss.
    let mut f = FakeForge::default();
    let c = plant_commit(&mut f, 1, false, "failure");
    v.push(case("(a) code=false, gated job failed on main -> 1 miss", f, vec![c], (1, 1, 0, EXIT_FAIL)));
    let mut f = FakeForge::default();
    let c = plant_commit(&mut f, 1, false, "success");
    v.push(case("(a+) control: code=false, gated job passed on main -> 0 misses", f, vec![c], (0, 1, 0, EXIT_PASS)));

    // (b) code=true: the job ran on the PR, so its failure on main is no miss.
    let mut f = FakeForge::default();
    let c = plant_commit(&mut f, 1, true, "failure");
    v.push(case("(b) code=true, gated job failed on main -> 0 misses", f, vec![c], (0, 1, 0, EXIT_PASS)));
    // (b+) is (a): the same failure with code=false is a miss.

    // (c) a commit with no PR -> uncompared (never "no miss"); a clean second commit keeps it from refusing.
    let mut f = FakeForge::default();
    let c2 = plant_commit(&mut f, 2, false, "success");
    let lone = Commit { sha: "m9".into(), subject: "direct push".into() };
    v.push(case("(c) no PR for the commit -> uncompared", f.clone(), vec![lone, c2.clone()], (0, 1, 1, EXIT_PASS)));
    let c9 = plant_commit(&mut f, 9, false, "success");
    v.push(case("(c+) control: the same commit with a PR -> compared", f, vec![c9, c2], (0, 2, 0, EXIT_PASS)));

    // (d) PR runs exist but the record artifact has expired -> uncompared, even though the job failed on main.
    let mut f = FakeForge::default();
    let c = plant_commit(&mut f, 3, false, "failure");
    let c2 = plant_commit(&mut f, 2, false, "success");
    let mut fd = f.clone();
    fd.artifacts.get_mut(&103).unwrap()[0].expired = true;
    v.push(case("(d) expired record -> uncompared (not judged, not a miss)", fd, vec![c.clone(), c2.clone()], (0, 1, 1, EXIT_PASS)));
    v.push(case("(d+) control: the record not expired -> compared, 1 miss", f, vec![c, c2], (1, 2, 0, EXIT_FAIL)));

    // (e) two attempts on the PR: the LAST record is the verdict of record.
    let two = |first: bool, second: bool| {
        let mut f = FakeForge::default();
        let c = plant_commit(&mut f, 4, first, "failure");
        f.runs.get_mut(&("h4".to_string(), "pull_request".to_string())).unwrap()[0].run_attempt = 2;
        let n2 = "sieve-verdict-coverage-changes-2".to_string();
        // Listed before attempt 1 on purpose: order must not matter.
        f.artifacts.get_mut(&104).unwrap().insert(0, Artifact { name: n2.clone(), expired: false });
        f.records.insert((104, n2), record_json(second));
        (f, c)
    };
    let (f, c) = two(false, true);
    v.push(case("(e) attempt 1 false, attempt 2 true, job failed on main -> 0 misses", f, vec![c], (0, 1, 0, EXIT_PASS)));
    let (f, c) = two(true, false);
    v.push(case("(e+) swapped: attempt 1 true, attempt 2 false -> 1 miss", f, vec![c], (1, 1, 0, EXIT_FAIL)));

    // (f) zero compared -> refusal (exit 3), distinct from a miss.
    let f = FakeForge::default();
    let lone = Commit { sha: "m9".into(), subject: "direct push".into() };
    v.push(case("(f) zero compared -> refused (exit 3)", f, vec![lone], (0, 0, 1, EXIT_REFUSED)));
    let mut f = FakeForge::default();
    let c = plant_commit(&mut f, 5, false, "success");
    v.push(case("(f+) control: one compared -> exit 0", f, vec![c], (0, 1, 0, EXIT_PASS)));

    // (g) the push run still in progress -> uncompared, not judged (its failed job included).
    let mut f = FakeForge::default();
    let c = plant_commit(&mut f, 6, false, "failure");
    let c2 = plant_commit(&mut f, 2, false, "success");
    let mut fg = f.clone();
    fg.runs.get_mut(&("m6".to_string(), "push".to_string())).unwrap()[0].status = "in_progress".into();
    v.push(case("(g) push run in_progress -> uncompared", fg, vec![c.clone(), c2.clone()], (0, 1, 1, EXIT_PASS)));
    let mut fj = f.clone();
    fj.jobs.get_mut(&(206, 1)).unwrap()[1].status = "in_progress".into();
    fj.jobs.get_mut(&(206, 1)).unwrap()[1].conclusion = None;
    v.push(case("(g2) a push job in_progress -> uncompared", fj, vec![c.clone(), c2.clone()], (0, 1, 1, EXIT_PASS)));
    v.push(case("(g+) control: the push run completed -> compared, 1 miss", f, vec![c, c2], (1, 2, 0, EXIT_FAIL)));

    v
}

/// Run every planted case through `sieve`; the lines and whether all held.
pub fn check_planted(sieve: &dyn Observer) -> (Vec<String>, bool) {
    let mut lines = Vec::new();
    let mut ok = true;
    for p in planted() {
        let rep = observe_commits(&p.forge, sieve, &p.commits, &[WF.to_string()]);
        let got = (rep.misses(), rep.compared(), rep.uncompared(), rep.exit());
        let held = got == p.want;
        ok &= held;
        lines.push(format!(
            "{} {}: got {} miss / {} compared / {} uncompared / exit {}{}",
            if held { "ok  " } else { "FAIL" },
            p.name,
            got.0,
            got.1,
            got.2,
            got.3,
            if held { String::new() } else { format!(" (want {:?})", p.want) }
        ));
    }
    (lines, ok)
}

fn run_self_test(sieve: &SieveCli) -> i32 {
    // The planted cases need the real sieve to answer, not to be missing.
    match Command::new(&sieve.sieve).arg("--version").output() {
        Ok(o) if o.status.success() => {
            println!("observe self-test with {}", String::from_utf8_lossy(&o.stdout).lines().next().unwrap_or("?"));
        }
        _ => {
            eprintln!("xtask observe: refused: cannot run `{} --version`", sieve.sieve);
            return EXIT_REFUSED;
        }
    }
    let (lines, ok) = check_planted(sieve);
    for l in &lines {
        println!("  {l}");
    }
    println!("observe self-test: {}/{} planted cases held", lines.iter().filter(|l| l.starts_with("ok")).count(), lines.len());
    if ok { EXIT_PASS } else { EXIT_FAIL }
}

// ------------------------------------------------------------------- tests

#[cfg(test)]
mod tests {
    use super::*;

    /// sieve observe's documented rule, for fixtures: a job in the map that
    /// failed while every class it reads is false is a miss.
    struct FakeSieve;
    impl Observer for FakeSieve {
        fn observe(&self, record: &str, results: &str) -> Observed {
            let map: &[(&str, &[&str])] = &[(CODE_JOB, &["code"]), ("no requirement lags its closed V", &["v_closure"])];
            let Ok(r) = parse_json(record) else { return Observed::Refused("bad record".into()) };
            if r.get("schema").and_then(Json::str) != Some("sieve.classify.v1") {
                return Observed::Refused("not a verdict".into());
            }
            let run_all = r.get("run_all") == Some(&Json::Bool(true));
            let class = |c: &str| run_all || r.get("classes").and_then(|o| o.get(c)) != Some(&Json::Bool(false));
            let Ok(j) = parse_json(results) else { return Observed::Refused("bad results".into()) };
            let mut gated = 0;
            let mut skipped = 0;
            let mut misses = Vec::new();
            for job in j.arr() {
                let name = job.get("name").and_then(Json::str).unwrap_or("");
                let concl = job.get("conclusion").and_then(Json::str).unwrap_or("");
                let Some((_, cls)) = map.iter().find(|(n, _)| *n == name) else { continue };
                gated += 1;
                if cls.iter().all(|c| !class(c)) {
                    skipped += 1;
                    if failed(concl) {
                        misses.push(format!("{name} ({concl})"));
                    }
                }
            }
            if gated == 0 {
                return Observed::Refused("no job matches [workflows.job_outputs]".into());
            }
            Observed::Compared { misses, skipped }
        }
    }

    fn run(p: &Planted) -> Report {
        observe_commits(&p.forge, &FakeSieve, &p.commits, &[WF.to_string()])
    }

    fn by_name(prefix: &str) -> Planted {
        planted().into_iter().find(|p| p.name.starts_with(prefix)).unwrap_or_else(|| panic!("no planted case {prefix}"))
    }

    fn check(prefix: &str) -> Report {
        let p = by_name(prefix);
        let rep = run(&p);
        assert_eq!((rep.misses(), rep.compared(), rep.uncompared(), rep.exit()), p.want, "{}\n{}", p.name, rep.text());
        rep
    }

    #[test]
    fn a_code_false_failed_job_is_a_miss() {
        let rep = check("(a)");
        assert!(rep.pairs[0].misses[0].contains(CODE_JOB));
    }
    #[test]
    fn a_control_passed_job_is_no_miss() {
        check("(a+)");
    }
    #[test]
    fn b_code_true_failed_job_is_no_miss() {
        check("(b)");
    }
    #[test]
    fn c_no_pr_is_uncompared() {
        let rep = check("(c)");
        let u = rep.pairs.iter().find(|p| p.uncompared.is_some()).unwrap();
        assert!(u.uncompared.as_deref().unwrap().contains("no pull request"), "{}", rep.text());
    }
    #[test]
    fn c_control_with_pr_is_compared() {
        check("(c+)");
    }
    #[test]
    fn d_expired_record_is_uncompared() {
        let rep = check("(d)");
        assert!(rep.text().contains("has expired"), "{}", rep.text());
    }
    #[test]
    fn d_control_live_record_is_compared() {
        check("(d+)");
    }
    #[test]
    fn e_last_attempt_record_is_used() {
        let rep = check("(e)");
        assert_eq!(rep.pairs[0].record.as_deref(), Some("sieve-verdict-coverage-changes-2"));
    }
    #[test]
    fn e_control_swapped_attempts_is_a_miss() {
        check("(e+)");
    }
    #[test]
    fn f_zero_compared_refuses() {
        let rep = check("(f)");
        assert_eq!(rep.exit(), EXIT_REFUSED);
        assert_ne!(rep.exit(), EXIT_FAIL);
        assert!(rep.json().contains("\"verdict\":\"refused\""));
    }
    #[test]
    fn f_control_one_compared_passes() {
        check("(f+)");
    }
    #[test]
    fn g_in_progress_push_run_is_uncompared() {
        let rep = check("(g)");
        assert!(rep.text().contains("in_progress, not judged"), "{}", rep.text());
    }
    #[test]
    fn g_in_progress_push_job_is_uncompared() {
        check("(g2)");
    }
    #[test]
    fn g_control_completed_push_run_is_compared() {
        check("(g+)");
    }

    #[test]
    fn every_planted_case_holds() {
        let (lines, ok) = check_planted(&FakeSieve);
        assert!(ok, "{}", lines.join("\n"));
        assert_eq!(lines.len(), 14);
    }

    #[test]
    fn no_pr_run_and_no_push_run_and_no_record_are_uncompared() {
        let mut f = FakeForge::default();
        let c = plant_commit(&mut f, 1, false, "failure");
        let mut a = f.clone();
        a.runs.remove(&("h1".to_string(), "pull_request".to_string()));
        let mut b = f.clone();
        b.runs.remove(&("m1".to_string(), "push".to_string()));
        let mut d = f.clone();
        d.artifacts.insert(101, vec![Artifact { name: "coverage-report".into(), expired: false }]);
        for (fx, why) in [(a, "no pull_request run"), (b, "no push run"), (d, "no sieve-verdict-* artifact")] {
            let rep = observe_commits(&fx, &FakeSieve, &[c.clone()], &[WF.to_string()]);
            assert_eq!((rep.compared(), rep.uncompared(), rep.exit()), (0, 1, EXIT_REFUSED), "{why}");
            assert!(rep.text().contains(why), "{why}: {}", rep.text());
        }
    }

    #[test]
    fn a_pr_not_merged_as_this_commit_does_not_count() {
        let mut f = FakeForge::default();
        let c = plant_commit(&mut f, 1, false, "failure");
        f.pulls.get_mut("m1").unwrap()[0].merge_commit_sha = Some("other".into());
        let rep = observe_commits(&f, &FakeSieve, &[c], &[WF.to_string()]);
        assert_eq!(rep.uncompared(), 1);
        assert!(rep.text().contains("none was merged as it"));
    }

    #[test]
    fn a_failure_in_an_earlier_push_attempt_still_counts() {
        let mut f = FakeForge::default();
        let c = plant_commit(&mut f, 1, false, "failure");
        f.runs.get_mut(&("m1".to_string(), "push".to_string())).unwrap()[0].run_attempt = 2;
        f.jobs.insert((201, 2), vec![Job { name: CODE_JOB.into(), status: "completed".into(), conclusion: Some("success".into()) }]);
        let rep = observe_commits(&f, &FakeSieve, &[c.clone()], &[WF.to_string()]);
        assert_eq!(rep.misses(), 1, "{}", rep.text());
        // Control: green in both attempts -> no miss.
        f.jobs.get_mut(&(201, 1)).unwrap()[1].conclusion = Some("success".into());
        assert_eq!(observe_commits(&f, &FakeSieve, &[c], &[WF.to_string()]).misses(), 0);
    }

    #[test]
    fn sieve_refusal_is_uncompared() {
        let mut f = FakeForge::default();
        let c = plant_commit(&mut f, 1, false, "failure");
        f.records.insert((101, "sieve-verdict-coverage-changes-1".into()), "{\"classes\":{}}".into());
        let rep = observe_commits(&f, &FakeSieve, &[c], &[WF.to_string()]);
        assert_eq!((rep.compared(), rep.uncompared(), rep.exit()), (0, 1, EXIT_REFUSED));
        assert!(rep.text().contains("sieve observe refused"));
    }

    #[test]
    fn args_parse_and_reject() {
        let a = |v: &[&str]| parse_args(&v.iter().map(|s| s.to_string()).collect::<Vec<_>>());
        assert_eq!(a(&[]).unwrap().sel, Select::Commits(10));
        assert_eq!(a(&["--since-days", "7"]).unwrap().sel, Select::SinceDays(7));
        assert!(a(&["--format", "json"]).unwrap().json);
        assert!(a(&["--commits", "3", "--since-days", "7"]).is_err());
        assert!(a(&["--commits", "0"]).is_err());
        assert!(a(&["--commits"]).is_err());
        assert!(a(&["--format", "yaml"]).is_err());
        assert!(a(&["--bogus"]).is_err());
    }

    #[test]
    fn record_key_takes_the_trailing_attempt() {
        assert_eq!(record_key("sieve-verdict-coverage-changes-12"), Some(("sieve-verdict-coverage-changes", 12)));
        assert_eq!(record_key("sieve-verdict-x"), None);
        assert_eq!(record_key("coverage-1"), None);
    }

    #[test]
    fn parse_observe_reads_misses_and_refusals() {
        let miss = r#"{"schema":"sieve.observe.v1","refused":false,"compared_skipped_jobs":3,"misses":[{"classes":["code"],"conclusion":"failure","job":"Rust Coverage"}]}"#;
        match parse_observe(Some(1), miss, "") {
            Observed::Compared { misses, skipped } => {
                assert_eq!(skipped, 3);
                assert!(misses[0].starts_with("Rust Coverage (failure)"));
            }
            o => panic!("{o:?}"),
        }
        let none = r#"{"schema":"sieve.observe.v1","refused":false,"compared_skipped_jobs":0,"misses":[]}"#;
        assert_eq!(parse_observe(Some(0), none, ""), Observed::Compared { misses: vec![], skipped: 0 });
        assert!(matches!(parse_observe(Some(3), "", "no gated job"), Observed::Refused(_)));
        assert!(matches!(parse_observe(Some(2), "", "usage"), Observed::Refused(_)));
        // Exit and list disagree: not trusted.
        assert!(matches!(parse_observe(Some(0), miss, ""), Observed::Refused(_)));
        assert!(matches!(parse_observe(Some(1), none, ""), Observed::Refused(_)));
        assert!(matches!(parse_observe(Some(0), "{}", ""), Observed::Refused(_)));
    }

    #[test]
    fn json_report_is_parseable_and_counts_match() {
        let rep = run(&by_name("(d)"));
        let v = parse_json(&rep.json()).expect("valid JSON");
        assert_eq!(v.get("compared"), Some(&Json::Num(1.0)));
        assert_eq!(v.get("uncompared"), Some(&Json::Num(1.0)));
        assert_eq!(v.get("pairs").map(|p| p.arr().len()), Some(2));
    }
}

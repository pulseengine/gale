//! Every crate with a `#[kani::proof]` harness is run by some CI `cargo kani`
//! invocation that can fail.
//!
//! Found 2026-11-19: formal-verification.yml and nightly.yml ran `cargo kani`
//! at the repo root and in `ffi/` only. The harnesses in `drivers/*` (dma-own,
//! wdg-thin, spi-thin, i2c-v2-thin, sen66-thin, ...) were checked by nothing:
//! "Kani-proven" in their docs was a local result. This is renode-targets'
//! shape (a target defined, never run) for Kani.
//!
//! The gate finds each tracked `.rs` file with `#[kani::proof]` in code (not a
//! comment), maps it to its owning package (nearest `Cargo.toml` with
//! `[package]`), and reads every workflow with a strict YAML reader
//! (`gates::yaml`: structure from indentation, anything outside the subset it
//! reads is an error, and a workflow it cannot read counts nothing).
//!
//! ALLOWLIST, not denylist (gale lane L11, round 3: three rounds of denylist
//! patches each leaked). A step counts only when ALL of this holds; anything
//! else that mentions `cargo ... kani` is VOID (listed, not counted):
//!
//! - its keys are only `name`, `id`, `if`, `run`, `working-directory` and
//!   `timeout-minutes` (so no `env:` -- `BASH_ENV` --, `shell:`,
//!   `continue-on-error:`, `uses:`);
//! - its `if:` (and its job's) is absent or exactly the path-guard form
//!   `needs.guard.outputs.K == 'true'`, on a job whose `needs:` is exactly
//!   `guard`, where the `guard` job's keys are on `GUARD_JOB_KEYS` (no `if:`,
//!   `needs:`, `container:`, `defaults:`, `concurrency:`; `env:` names
//!   allowlisted) and it maps K to `${{ steps.ID.outputs.K }}` of a step (keys
//!   `id`, `name`, `run` only; only `actions/checkout` steps before it)
//!   whose script starts with the byte-exact guard template
//!   (`guard_template_outputs`) setting `K=true` on every non-PR event -- and a
//!   guarded invocation needs a push-to-main or schedule trigger, since on a
//!   pull_request the guard may write K=false every time;
//! - its `run:` is a literal or one-line scalar whose script is exactly one
//!   line `cargo kani ARGS`, optionally preceded by the one line
//!   `set -o pipefail`; ARGS are allowlisted flags (`KANI_FLAGS`, and
//!   `KANI_VALUE_FLAGS` with a literal value), then optionally `2>&1`, then
//!   optionally `| tee PATH` (only after the `set -o pipefail` line). Every
//!   word is made of `[A-Za-z0-9_./,=:+-]` (those three operators aside): no
//!   quote, `$`, `;`, `&&`, `||`, backslash or glob can appear;
//! - its job's keys are on `JOB_KEYS` (no `continue-on-error:`, `container:`,
//!   `services:`), job and workflow `env:` names are on `ENV_ALLOW`, and
//!   `defaults: run:` sets only `shell: bash` and `working-directory`;
//! - the workflow has a trigger directly under `on:` that its text says runs for
//!   main: `schedule` whose crons all parse and fire every year (names
//!   allowed; Feb counts 28 days; weekday 0-7), or `push`/`pull_request` with
//!   no filter but a `branches:` list naming main and no negated (`!`)
//!   pattern (a `paths:` filter is not read as covering main; the gate reads the YAML,
//!   it does not observe a run);
//! - a directory from `${{ matrix.K }}` comes from a matrix with no `exclude:`.
//!
//! A crate whose harnesses all sit outside its compiled roots fails even when a
//! step runs there: `cargo kani` would build none of them.
//!
//! The directory comes from `working-directory:` (step, else job, else workflow
//! `defaults`: the most specific one REPLACES the others and a relative one
//! resolves from the workspace, as GitHub documents -- never joined) or
//! `--manifest-path`; `${{ matrix.K }}` expands over the job's
//! matrix list. `cargo kani setup`, `list`, `--version` and `--help` lines are
//! not invocations.
//!
//! A harness is `#[kani::proof]` or `#[kani::proof_for_contract(f)]` (also
//! inside `cfg_attr`). Harness figures: "in source" counts them in a package's files;
//! "compiled" drops files outside the package's compiled roots (the `[lib]`
//! path's directory, `tests/`, `benches/`, `examples/`, `src/bin/`). The root
//! package's lib is `plain/src`, so `src/`'s harnesses (the Verus originals of
//! plain/'s copies) are in source but never compiled by `cargo kani`.
//!
//! What it cannot see is `CANNOT_SEE`, printed on every run.

use crate::gates::trust_ledger::{kani_contract_proofs, kani_proofs};
use crate::gates::yaml::{self, Node, Style};
use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const NAME: &str = "kani-coverage";

pub const CANNOT_SEE: &[&str] = &[
    "the environment a counted step inherits from EARLIER steps in its job and the runner (before the guard step only `actions/checkout` is allowed): a step that writes `$GITHUB_ENV` / `$GITHUB_PATH` (BASH_ENV, PATH, RUSTFLAGS), replaces or shadows `cargo` or `cargo-kani`, or a `.cargo/config.toml` alias, env or rustflags in the crate or the repo",
    "whether a harness in a compiled root is reachable (`mod` declared, `#[cfg]` enabled); `#[cfg(test)]` harnesses need `--tests`, and only tests/ harnesses are required to have it",
    "what an allowlisted flag's value does (`--default-unwind 1` narrows: trust-ledger's cannot-see list), and `--features` sets",
    "whether the path guard's filter matches a given PR: a guarded invocation counts only in a workflow with a push-to-main or a firing schedule trigger, on which the guard template writes K=true; on pull_request it may never run",
    "`concurrency:` cancellation of a counted job (workflow- or job-level `cancel-in-progress`); the guard job may not set `concurrency:`",
    "a job a `needs:` chain skips, and workflows that call other workflows",
    "harnesses produced by macros",
];

#[derive(Debug, Clone, PartialEq)]
pub struct Invocation {
    pub workflow: String,
    pub job: String,
    pub dir: String,
    pub tests: bool,
    /// None = counts; Some(reason) = the gate cannot affirm it runs and fails the job.
    pub void: Option<String>,
    /// The path-guard output this invocation is conditioned on, if any.
    pub guard: Option<String>,
}

fn norm_dir(d: &str) -> String {
    let mut d = d.trim().to_string();
    for p in ["${{ github.workspace }}/", "$GITHUB_WORKSPACE/", "./"] {
        if let Some(r) = d.strip_prefix(p) {
            d = r.to_string();
        }
    }
    let d = d.trim_end_matches('/');
    if d == "." || d == "${{ github.workspace }}" || d == "$GITHUB_WORKSPACE" {
        String::new()
    } else {
        join_dir("", d)
    }
}

fn join_dir(base: &str, rel: &str) -> String {
    let rel = rel.trim();
    if rel.starts_with('/') {
        return rel.to_string();
    }
    let mut parts: Vec<&str> = base.split('/').filter(|p| !p.is_empty()).collect();
    for p in rel.split('/').filter(|p| !p.is_empty()) {
        match p {
            "." => {}
            ".." => {
                parts.pop();
            }
            p => parts.push(p),
        }
    }
    parts.join("/")
}

/// The step keys a counted step may have.
const STEP_KEYS: &[&str] = &["name", "id", "if", "run", "working-directory", "timeout-minutes"];
/// The job keys a counted step's job may have.
const JOB_KEYS: &[&str] = &["name", "runs-on", "timeout-minutes", "needs", "if", "steps", "strategy", "defaults", "env", "permissions", "outputs", "concurrency"];
/// The keys the `guard` job may have.
const GUARD_JOB_KEYS: &[&str] = &["name", "runs-on", "timeout-minutes", "outputs", "steps", "permissions", "env"];
/// Environment names a workflow or job `env:` may set around a counted step.
const ENV_ALLOW: &[&str] = &["GITHUB_TOKEN", "GH_TOKEN", "CARGO_TERM_COLOR", "CARGO_INCREMENTAL", "RUST_BACKTRACE"];

/// `cargo kani` arguments the gate can affirm do not narrow what runs or stop
/// verification: flags, and flags that take one literal value. Anything else
/// (`--harness`, `-p`, `--exclude`, `--only-codegen`, `--dry-run`, `--help`,
/// a `$VARIABLE`, ...) voids the invocation.
const KANI_FLAGS: &[&str] = &["--tests", "--all-features", "--no-default-features", "--verbose", "-v", "--quiet", "-q"];
const KANI_VALUE_FLAGS: &[&str] = &["--features", "-F", "--manifest-path", "--default-unwind", "--object-bits", "--jobs", "-j", "--output-format"];

/// Lines that name `cargo kani` but run no harness.
const NOT_INVOCATIONS: &[&str] = &["cargo kani setup", "cargo kani list", "cargo kani --version", "cargo kani -V", "cargo kani --help", "cargo kani -h"];

fn safe_word(w: &str) -> bool {
    !w.is_empty() && w.chars().all(|c| c.is_ascii_alphanumeric() || "_./,=:+-".contains(c))
}

fn literal_value(v: &str) -> bool {
    safe_word(v) && !v.starts_with('-')
}

/// Does this script mention `cargo ... kani` on a line that is not a known
/// non-invocation? (Such a step is judged; anything else is not a Kani step.)
fn mentions_kani(script: &str) -> bool {
    script.lines().map(str::trim).any(|l| {
        let w: Vec<&str> = l.split_whitespace().collect();
        let bare = |x: &str| x.chars().filter(|c| !"\"'\\;&|()".contains(*c)).collect::<String>();
        let c = w.iter().position(|x| bare(x).ends_with("cargo"));
        let named = c.is_some_and(|c| w[c + 1..].iter().any(|x| bare(x) == "kani")) || w.iter().any(|x| x.contains("cargo-kani"));
        named && !NOT_INVOCATIONS.contains(&w.join(" ").as_str())
    })
}

/// The allowlisted script shape: Ok((manifest directory, --tests)) or the
/// reason it is not that shape.
fn affirm_script(script: &str) -> Result<(Option<String>, bool), String> {
    let mut lines: Vec<&str> = script.lines().map(str::trim_end).collect();
    while lines.last().is_some_and(|l| l.is_empty()) {
        lines.pop();
    }
    let shape = "the script is not exactly `cargo kani ARGS` (optionally after a `set -o pipefail` line)";
    let (pipefail, cmd) = match lines.as_slice() {
        [c] => (false, c.trim()),
        [p, c] if p.trim() == "set -o pipefail" => (true, c.trim()),
        _ => return Err(shape.into()),
    };
    let w: Vec<&str> = cmd.split_whitespace().collect();
    if w.len() < 2 || w[0] != "cargo" || w[1] != "kani" {
        return Err(format!("{shape}: `{cmd}`"));
    }
    let mut dir = None;
    let mut tests = false;
    let mut k = 2;
    while k < w.len() {
        let x = w[k];
        if x == "2>&1" {
            k += 1;
            continue;
        }
        if x == "|" {
            if !pipefail {
                return Err("a pipe without the `set -o pipefail` line (GitHub's default shell is `bash -e`, no pipefail)".into());
            }
            return match &w[k + 1..] {
                ["tee", path] if literal_value(path) => Ok((dir, tests)),
                rest => Err(format!("after `|` only `tee <literal path>` is allowed, not `{}`", rest.join(" "))),
            };
        }
        if !safe_word(x) {
            return Err(format!("`{x}` is not a literal word (quotes, `$`, `;`, `&&`, `||`, `\\`, globs and other operators are not allowed)"));
        }
        if KANI_FLAGS.contains(&x) {
            tests |= x == "--tests";
            k += 1;
            continue;
        }
        let (flag, value, used) = match x.split_once('=') {
            Some((f, v)) => (f, Some(v), 1),
            None => (x, w.get(k + 1).copied(), 2),
        };
        if !KANI_VALUE_FLAGS.contains(&flag) {
            return Err(format!("argument `{x}` is not on the allowlist (it may select a subset, skip verification or come from a variable)"));
        }
        match value {
            Some(v) if literal_value(v) => {
                if flag == "--manifest-path" {
                    dir = Some(v.rsplit_once('/').map_or("", |(d, _)| d).to_string());
                }
                k += used;
            }
            _ => return Err(format!("`{flag}` without a literal value")),
        }
    }
    Ok((dir, tests))
}

/// The output K of a path-guard `if:`, or None if it is not exactly that form.
fn guard_if(v: &Node) -> Option<String> {
    let s = v.str()?.trim();
    let inner = s.strip_prefix("${{").and_then(|x| x.strip_suffix("}}")).map(str::trim).unwrap_or(s);
    let norm = inner.split_whitespace().collect::<Vec<_>>().join(" ");
    let k = norm.strip_prefix("needs.guard.outputs.")?.strip_suffix(" == 'true'")?;
    (!k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')).then(|| k.to_string())
}

fn scalar_list(n: &Node) -> Vec<String> {
    match n {
        Node::Str(s, _) => vec![s.clone()],
        Node::Seq(v) => v.iter().filter_map(|x| x.str().map(str::to_string)).collect(),
        _ => Vec::new(),
    }
}

/// `env:` names outside `ENV_ALLOW`, as a void reason.
fn env_void(where_: &str, env: Option<&Node>) -> Option<String> {
    let env = env?;
    let Node::Map(m) = env else { return Some(format!("{where_} `env:` is not a mapping the gate reads")) };
    m.iter().find(|(k, _)| !ENV_ALLOW.contains(&k.as_str())).map(|(k, _)| format!("{where_} `env:` sets `{k}`, which is not on the allowlist (BASH_ENV, PATH and the like change what runs)"))
}

/// `defaults:` -> its working-directory, or a void reason (only `run: shell: bash`
/// and `run: working-directory` are read; pipefail comes from the script line).
fn defaults_of(where_: &str, d: Option<&Node>) -> Result<Option<String>, String> {
    let Some(d) = d else { return Ok(None) };
    let mut wd = None;
    for (k, v) in d.entries() {
        if k != "run" {
            return Err(format!("{where_} `defaults: {k}:` is not read"));
        }
        for (rk, rv) in v.entries() {
            match (rk.as_str(), rv.str()) {
                ("shell", Some("bash")) => {}
                ("working-directory", Some(w)) => wd = Some(w.to_string()),
                _ => return Err(format!("{where_} `defaults: run: {rk}:` is not `shell: bash` or a working-directory")),
            }
        }
    }
    Ok(wd)
}

/// The repo's guard step condition, byte-exact: events other than pull_request
/// and merge_group (push, schedule, workflow_dispatch) take the first branch.
const GUARD_IF: &str = "if [ \"${{ github.event_name }}\" != \"pull_request\" ] && [ \"${{ github.event_name }}\" != \"merge_group\" ]; then";

#[derive(Debug, Clone, Copy, PartialEq)]
enum Op {
    End,
    And,
    Or,
    Seq,
    Pipe,
    Bg,
}

/// Split one logical shell line into simple commands and the operator after
/// each. Quotes are respected; `2>&1`, `>&2` and `&>` are redirections, not `&`;
/// an unquoted `#` at a word start ends the line. The flag is true when a
/// quote is still open at the end (a multi-line quoted string).
fn segments(line: &str) -> (Vec<(String, Op)>, bool) {
    let c: Vec<char> = line.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    let (mut sq, mut dq) = (false, false);
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        if sq {
            sq = ch != '\'';
            cur.push(ch);
            i += 1;
            continue;
        }
        if dq {
            if ch == '\\' {
                cur.push(ch);
                if let Some(n) = next {
                    cur.push(n);
                }
                i += 2;
                continue;
            }
            dq = ch != '"';
            cur.push(ch);
            i += 1;
            continue;
        }
        let at_word_start = cur.chars().next_back().is_none_or(char::is_whitespace);
        match ch {
            '\'' => sq = true,
            '"' => dq = true,
            '\\' => {
                cur.push(ch);
                if let Some(n) = next {
                    cur.push(n);
                }
                i += 2;
                continue;
            }
            '#' if at_word_start => break,
            _ => {}
        }
        let op = match (ch, next) {
            ('&', Some('&')) => Some((Op::And, 2)),
            ('|', Some('|')) => Some((Op::Or, 2)),
            ('|', Some('&')) => Some((Op::Pipe, 2)),
            ('|', _) => Some((Op::Pipe, 1)),
            (';', _) => Some((Op::Seq, 1)),
            ('&', n) => {
                let prev = if i > 0 { Some(c[i - 1]) } else { None };
                if prev == Some('>') || prev == Some('<') || n == Some('>') {
                    None
                } else {
                    Some((Op::Bg, 1))
                }
            }
            _ => None,
        };
        if let Some((op, len)) = op {
            out.push((std::mem::take(&mut cur), op));
            i += len;
            continue;
        }
        cur.push(ch);
        i += 1;
    }
    out.push((cur, Op::End));
    (out, sq || dq)
}

/// The outputs a guard script sets to `true`, UNCONDITIONALLY, on every
/// non-PR event -- or nothing, unless the script starts with exactly this
/// template (leading whitespace aside):
///
/// ```text
/// set -euo pipefail
/// if [ "${{ github.event_name }}" != "pull_request" ] && [ "${{ github.event_name }}" != "merge_group" ]; then
///   echo "K1=true" >> "$GITHUB_OUTPUT"; ...; echo "<message>"; exit 0
/// fi
/// ```
///
/// The third line is split with the same shell segmenter as `run:` scripts:
/// every segment must be `echo "K=true" >> "$GITHUB_OUTPUT"` (each K at most
/// once), or `echo "<text>"` with no redirection, expansion or substitution,
/// joined by `;`, ending in `exit 0`. A `K=true` in a comment, a string, a
/// function or a later conditional is not this shape and affirms nothing.
pub fn guard_template_outputs(script: &str) -> BTreeSet<String> {
    let none = BTreeSet::new();
    let l: Vec<&str> = script.lines().map(str::trim).filter(|x| !x.is_empty()).collect();
    if l.len() < 4 || l[0] != "set -euo pipefail" || l[1] != GUARD_IF || l[3] != "fi" {
        return none;
    }
    let (segs, open) = segments(l[2]);
    if open || segs.is_empty() {
        return none;
    }
    let mut out = BTreeSet::new();
    let last = segs.len() - 1;
    for (k, (text, op)) in segs.iter().enumerate() {
        let t = text.trim();
        if k == last {
            if t != "exit 0" || *op != Op::End {
                return none;
            }
            continue;
        }
        if *op != Op::Seq {
            return none;
        }
        if let Some(name) = t.strip_prefix("echo \"").and_then(|r| r.strip_suffix("=true\" >> \"$GITHUB_OUTPUT\"")) {
            if name.is_empty() || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') || !out.insert(name.to_string()) {
                return none;
            }
            continue;
        }
        let msg = t.strip_prefix("echo \"").and_then(|r| r.strip_suffix('"'));
        if msg.is_some_and(|m| !m.contains(['"', '$', '`', '\\', '>', '<'])) {
            continue;
        }
        return none;
    }
    out
}

/// The `guard` job's outputs the gate can AFFIRM are `true` on push and
/// schedule: the job has no `if:`, the output maps to `${{ steps.ID.outputs.K }}`
/// of a step with `id: ID` in that job, that step's keys are only `id`, `name`
/// and `run` (no `if:`, `env:`, `shell:`, `continue-on-error:`), and its
/// literal `run: |` script matches `guard_template_outputs` with K among the
/// outputs it sets. Anything else is not affirmed.
fn guard_outputs(jobs: &Node) -> BTreeSet<String> {
    let mut s = BTreeSet::new();
    let Some(g) = jobs.get("guard") else { return s };
    // The guard job itself must be one the gate can affirm runs its template
    // unaltered (L11 round 4, K3-K5): only GUARD_JOB_KEYS (no `if:`, `needs:`,
    // `container:`, `continue-on-error:`, `services:`), allowlisted `env:`
    // names, and no `defaults:` (the workflow-level `defaults:` is checked for every
    // invocation).
    if g.entries().iter().any(|(k, _)| !GUARD_JOB_KEYS.contains(&k.as_str())) || env_void("guard job", g.get("env")).is_some() {
        return s;
    }
    let mut sets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    // Only `actions/checkout` steps may run before a template step: any other
    // earlier step could write BASH_ENV to $GITHUB_ENV and stop the template
    // (round 5 follow-up; was a cannot-see item).
    let mut clean_prefix = true;
    for step in g.get("steps").map(Node::items).unwrap_or(&[]) {
        let keys_ok = |allowed: &[&str]| step.entries().iter().all(|(k, _)| allowed.contains(&k.as_str()));
        // Exactly `actions/checkout@<ref>`. The ref may contain `/` (a branch
        // `releases/v1`, round 7) but never `..`, `@`, an empty segment or a
        // leading `/` (`@v4/../../evil/act@v1` is another action: round 6).
        let checkout_ref = |u: &str| {
            u.strip_prefix("actions/checkout@").is_some_and(|r| {
                !r.is_empty()
                    && !r.contains("..")
                    && r.split('/').all(|seg| !seg.is_empty())
                    && r.chars().all(|c| c.is_ascii_alphanumeric() || "._-/".contains(c))
            })
        };
        let checkout = keys_ok(&["uses", "with", "name", "id", "timeout-minutes"]) && step.get("uses").and_then(Node::str).is_some_and(checkout_ref);
        let template = match (step.get("id").and_then(Node::str), step.get("run")) {
            (Some(id), Some(Node::Str(script, Style::Literal))) if keys_ok(&["id", "name", "run"]) => Some((id, script)),
            _ => None,
        };
        match template {
            Some((id, script)) if clean_prefix => {
                sets.insert(id.to_string(), guard_template_outputs(script));
            }
            _ if checkout => {}
            _ => clean_prefix = false,
        }
    }
    for (name, v) in g.get("outputs").map(Node::entries).unwrap_or(&[]) {
        let v: String = v.str().unwrap_or("").split_whitespace().collect();
        let Some(id) = v.strip_prefix("${{steps.").and_then(|r| r.strip_suffix(&format!(".outputs.{name}}}}}"))) else { continue };
        if sets.get(id).is_some_and(|set| set.contains(name)) {
            s.insert(name.clone());
        }
    }
    s
}

const CRON_MONTHS: &[&str] = &["JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC"];
const CRON_DAYS: &[&str] = &["SUN", "MON", "TUE", "WED", "THU", "FRI", "SAT"];

/// One cron field: `*`, `N`, `A-B`, either with `/S`, comma lists; numbers, or
/// the standard month / weekday names (`JAN`, `MON-FRI`; round 5, C1). The
/// set it matches.
fn cron_field(f: &str, lo: u32, hi: u32, names: &[&str], first: u32) -> Result<BTreeSet<u32>, String> {
    let mut s = BTreeSet::new();
    for part in f.split(',') {
        let (range, step) = match part.split_once('/') {
            Some((r, st)) => (r, st.parse::<u32>().ok().filter(|x| *x > 0 && st.chars().all(|c| c.is_ascii_digit())).ok_or(format!("step `{st}`"))?),
            None => (part, 1),
        };
        let num = |x: &str| {
            let up = x.to_ascii_uppercase();
            let v = names.iter().position(|n| *n == up).map(|i| i as u32 + first).or_else(|| x.chars().all(|c| c.is_ascii_digit()).then(|| x.parse::<u32>().ok()).flatten());
            v.filter(|n| (lo..=hi).contains(n)).ok_or(format!("`{x}` outside {lo}-{hi}"))
        };
        let (a, b) = if range == "*" {
            (lo, hi)
        } else if let Some((a, b)) = range.split_once('-') {
            (num(a)?, num(b)?)
        } else {
            let a = num(range)?;
            (a, if part.contains('/') { hi } else { a })
        };
        if a > b {
            return Err(format!("range `{range}` is empty"));
        }
        s.extend((a..=b).step_by(step as usize));
    }
    Ok(s)
}

/// Ok if a 5-field POSIX cron (GitHub's `schedule`, UTC) fires at least once
/// EVERY year: each field parses (day-of-week 0-7, 7 = Sunday as in cron), and
/// unless both day fields are restricted (cron then fires on either, so the
/// weekday suffices), some listed month has a listed day -- February counts 28
/// days, so `0 0 29 2 *` (leap years only) is not provable. A day field
/// starting with `*` (`*/2`) counts as unrestricted, as cron reads it.
fn cron_can_fire(expr: &str) -> Result<(), String> {
    let f: Vec<&str> = expr.split_whitespace().collect();
    if f.len() != 5 {
        return Err(format!("`{expr}` is not 5 fields"));
    }
    cron_field(f[0], 0, 59, &[], 0)?;
    cron_field(f[1], 0, 23, &[], 0)?;
    let dom = cron_field(f[2], 1, 31, &[], 1)?;
    let months = cron_field(f[3], 1, 12, CRON_MONTHS, 1)?;
    let dow = cron_field(f[4], 0, 7, CRON_DAYS, 0)?; // 7 is Sunday, as in cron
    let dom_star = f[2].starts_with('*');
    let dow_star = f[4].starts_with('*');
    if !dom_star && !dow_star {
        return if dow.is_empty() && dom.is_empty() { Err(format!("`{expr}` names no day")) } else { Ok(()) };
    }
    if !dom_star {
        let days_in = |m: u32| match m {
            2 => 28,
            4 | 6 | 9 | 11 => 30,
            _ => 31,
        };
        if !months.iter().any(|m| dom.iter().any(|d| *d <= days_in(*m))) {
            return Err(format!("`{expr}` names no day that exists every year in its months"));
        }
    }
    Ok(())
}

/// The triggers directly under `on:` whose text says they run for main (read
/// from the YAML, not observed):
/// `schedule` whose crons all parse and can fire, and `push` / `pull_request`
/// with no filter other than a `branches:` list naming `main` (no `!`
/// pattern). Only keys and list items DIRECTLY under `on:` are triggers. With
/// the reasons the others were not.
fn provable_triggers(root: &Node) -> (BTreeSet<&'static str>, Vec<String>) {
    let mut set = BTreeSet::new();
    let mut why = Vec::new();
    let Some(on) = root.get("on") else { return (set, vec!["the workflow has no `on:`".into()]) };
    let canon = |w: &str| ["push", "pull_request"].into_iter().find(|x| *x == w);
    match on {
        Node::Str(..) | Node::Seq(_) => {
            // `on: [push]`: no filter. A bare `schedule` names no cron.
            for n in scalar_list(on) {
                match canon(&n) {
                    Some(t) => {
                        set.insert(t);
                    }
                    None => why.push(format!("`on: {n}` is not a trigger the gate reads as running for main")),
                }
            }
        }
        Node::Map(m) => {
            for (name, val) in m {
                if name == "schedule" {
                    // A schedule runs only if it names at least one cron (K7) and
                    // every cron can fire (`0 0 31 2 *`, Feb 31, never does).
                    let crons = val.items().iter().filter(|c| c.get("cron").and_then(Node::str).is_some_and(|x| cron_can_fire(x).is_ok())).count();
                    if crons > 0 && crons == val.items().len() {
                        set.insert("schedule");
                    } else {
                        why.push("`schedule` names no cron that can fire (or a non-cron entry)".into());
                    }
                    continue;
                }
                let Some(t) = canon(name) else { continue };
                match val {
                    Node::Null => {
                        set.insert(t);
                    }
                    Node::Str(s, _) if s == "null" || s == "~" => {
                        set.insert(t);
                    }
                    Node::Map(f) => {
                        let mut ok = true;
                        for (fk, fv) in f {
                            if fk != "branches" {
                                why.push(format!("`{name}` has a `{fk}:` filter the gate cannot prove covers main"));
                                ok = false;
                                break;
                            }
                            // A negated pattern (`!main`, `!ma*`) can exclude main
                            // after it is listed (K6): any `!` entry is unprovable.
                            let bs = scalar_list(fv);
                            if bs.iter().any(|b| b.starts_with('!')) {
                                why.push(format!("`{name}` has a negated `branches:` pattern the gate does not evaluate"));
                                ok = false;
                                break;
                            }
                            if !bs.iter().any(|b| b == "main") {
                                why.push(format!("`{name}` is limited to branches that do not include main"));
                                ok = false;
                                break;
                            }
                        }
                        if ok {
                            set.insert(t);
                        }
                    }
                    other => why.push(format!("`{name}: {other:?}` is a value the gate does not read")),
                }
            }
        }
        Node::Null => why.push("`on:` is empty".into()),
    }
    (set, why)
}

/// `None` if some trigger reads as running for main; `Some(reason)` otherwise.
fn trigger_void(root: &Node) -> Option<String> {
    let (set, why) = provable_triggers(root);
    (set.is_empty()).then(|| {
        if why.is_empty() {
            "the workflow has no push, schedule or pull_request trigger directly under `on:`".into()
        } else {
            format!("no trigger the gate reads as running for main: {}", why.join("; "))
        }
    })
}

/// Every `cargo kani` invocation in one workflow file. A workflow the YAML
/// reader refuses counts nothing: if it mentions `cargo kani`, one VOID
/// invocation says why.
pub fn workflow_invocations(name: &str, text: &str) -> Vec<Invocation> {
    let root = match yaml::parse(text) {
        Ok(r) => r,
        Err(e) => {
            if !mentions_kani(text) {
                return Vec::new();
            }
            return vec![Invocation {
                workflow: name.into(),
                job: "?".into(),
                dir: format!("<unread {name}>"),
                tests: false,
                void: Some(format!("the workflow is not in the YAML subset the gate reads: {e}")),
                guard: None,
            }];
        }
    };
    let mut out = Vec::new();
    let wf_triggers = provable_triggers(&root).0;
    let mut wf_void = trigger_void(&root).or_else(|| env_void("workflow", root.get("env")));
    let wf_wd = match defaults_of("workflow", root.get("defaults")) {
        Ok(w) => w.map(|w| norm_dir(&w)).unwrap_or_default(),
        Err(e) => {
            wf_void = wf_void.or(Some(e));
            String::new()
        }
    };
    let Some(jobs) = root.get("jobs") else { return out };
    let guards = guard_outputs(jobs);
    let set_void = |v: &mut Option<String>, r: String| {
        if v.is_none() {
            *v = Some(r);
        }
    };
    for (job, body) in jobs.entries() {
        let mut job_void = wf_void.clone();
        if let Some((k, _)) = body.entries().iter().find(|(k, _)| !JOB_KEYS.contains(&k.as_str())) {
            set_void(&mut job_void, format!("job key `{k}:` is not on the allowlist"));
        }
        if let Some(r) = env_void("job", body.get("env")) {
            set_void(&mut job_void, r);
        }
        let mut job_guard = None;
        if let Some(c) = body.get("if") {
            match guard_if(c) {
                Some(g) => job_guard = Some(g),
                None => set_void(&mut job_void, format!("job `if: {}` is not the path-guard form", c.str().unwrap_or("?"))),
            }
        }
        let needs = body.get("needs").map(scalar_list).unwrap_or_default();
        let job_needs_guard = needs.iter().any(|n| n == "guard");
        if let Some(other) = needs.iter().find(|n| *n != "guard") {
            set_void(&mut job_void, format!("job needs `{other}` -- the gate cannot affirm that job runs"));
        }
        let job_wd = match defaults_of("job", body.get("defaults")) {
            // The most specific `working-directory` REPLACES the others; a
            // relative one resolves from the workspace (GitHub's documented
            // rule; round 11, F4: it used to be joined onto the outer default).
            Ok(Some(w)) => norm_dir(&w),
            Ok(None) => wf_wd.clone(),
            Err(e) => {
                set_void(&mut job_void, e);
                wf_wd.clone()
            }
        };
        let matrix = body.get("strategy").and_then(|s| s.get("matrix"));
        let matrix_exclude = matrix.is_some_and(|m| m.get("exclude").is_some());
        for step in body.get("steps").map(Node::items).unwrap_or(&[]) {
            let Some(run) = step.get("run") else { continue };
            let script = run.str().unwrap_or("");
            if !mentions_kani(script) {
                continue;
            }
            let mut void = job_void.clone();
            // Every guard on the path is checked: the job's AND the step's (a
            // step guard used to overwrite the job's, which then went unchecked: K2).
            let mut guards_used: Vec<String> = job_guard.iter().cloned().collect();
            if let Some((k, _)) = step.entries().iter().find(|(k, _)| !STEP_KEYS.contains(&k.as_str())) {
                set_void(&mut void, format!("step key `{k}:` is not on the allowlist (`env:`, `shell:`, `continue-on-error:` ... can change whether a failure fails the job)"));
            }
            if let Some(c) = step.get("if") {
                match guard_if(c) {
                    Some(g) => guards_used.push(g),
                    None => set_void(&mut void, format!("step `if: {}` is not the path-guard form", c.str().unwrap_or("?"))),
                }
            }
            if matches!(run, Node::Str(_, Style::Folded)) {
                set_void(&mut void, "a folded `run: >` scalar joins lines -- the gate does not read it".into());
            }
            let (mdir, tests) = match affirm_script(script) {
                Ok(x) => x,
                Err(e) => {
                    set_void(&mut void, e);
                    (None, false)
                }
            };
            for g in &guards_used {
                if !job_needs_guard || !guards.contains(g) {
                    set_void(
                        &mut void,
                        format!("`if: needs.guard.outputs.{g} == 'true'` without `needs: guard` and a `guard` job (no `if:`) whose output `{g}` maps to a step that writes `{g}=true`"),
                    );
                }
            }
            // The guard writes K=true unconditionally only on non-PR events; on a
            // pull_request it may write K=false every time (round 5, P1). A
            // guarded invocation needs a push-to-main or a firing schedule.
            if !guards_used.is_empty() && !(wf_triggers.contains("push") || wf_triggers.contains("schedule")) {
                set_void(&mut void, "a path-guarded invocation in a workflow with no push-to-main or schedule trigger: on pull_request the guard may skip it every time".into());
            }
            let base = match step.get("working-directory").and_then(Node::str) {
                Some(w) => norm_dir(w),
                None => job_wd.clone(),
            };
            let d = match mdir {
                Some(d) if d.starts_with('/') => d,
                Some(d) => join_dir(&base, &d),
                None => base,
            };
            // Expand ${{ matrix.K }}.
            let expanded: Vec<String> = if let Some(st) = d.find("${{ matrix.") {
                if matrix_exclude {
                    set_void(&mut void, "the matrix has `exclude:` -- the gate does not evaluate it".into());
                }
                let en = d[st..].find("}}").map(|x| st + x + 2).unwrap_or(d.len());
                let key = d[st + "${{ matrix.".len()..en - 2].trim().to_string();
                match matrix.and_then(|m| m.get(&key)) {
                    Some(Node::Seq(vals)) => vals.iter().filter_map(Node::str).map(|v| norm_dir(&format!("{}{v}{}", &d[..st], &d[en..]))).collect(),
                    _ => vec![d.clone()],
                }
            } else {
                vec![d.clone()]
            };
            for dir in expanded {
                let mut void = void.clone();
                if dir.contains('$') || !dir.split('/').all(|p| p.is_empty() || literal_value(p)) {
                    set_void(&mut void, format!("directory `{dir}` is not resolved"));
                }
                out.push(Invocation { workflow: name.into(), job: job.clone(), dir, tests, void, guard: (!guards_used.is_empty()).then(|| guards_used.join(", ")) });
            }
        }
    }
    out
}

/// A package directory, its harness figures, and whether any of its harnesses
/// live under `tests/`.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct HarnessCrate {
    /// `#[kani::proof]` in the package's files.
    pub harnesses: usize,
    /// ... in files inside the package's compiled roots.
    pub compiled: usize,
    pub needs_tests: bool,
    pub files: Vec<String>,
}

/// Is `inner` (a path relative to its package) inside a root cargo compiles,
/// given the package's lib directory?
pub fn in_compiled_root(inner: &str, lib_dir: &str) -> bool {
    let lib = format!("{}/", lib_dir.trim_end_matches('/'));
    inner.starts_with(&lib) || ["tests/", "benches/", "examples/", "src/bin/"].iter().any(|r| inner.starts_with(r)) || inner == "build.rs"
}

/// Crates with harnesses, from (path, text, owning package dir, package lib dir).
pub fn harness_crates<'a, I: IntoIterator<Item = (&'a str, &'a str, String, String)>>(files: I) -> BTreeMap<String, HarnessCrate> {
    let mut m: BTreeMap<String, HarnessCrate> = BTreeMap::new();
    for (path, text, pkg, lib_dir) in files {
        // `#[kani::proof_for_contract]` is a harness too (round 11, F6).
        let n = kani_proofs(text) + kani_contract_proofs(text);
        if n == 0 {
            continue;
        }
        let e = m.entry(pkg.clone()).or_default();
        e.harnesses += n;
        let inner = if pkg.is_empty() { path } else { path.strip_prefix(&format!("{pkg}/")).unwrap_or("") };
        if in_compiled_root(inner, &lib_dir) {
            e.compiled += n;
        }
        if inner.starts_with("tests/") {
            e.needs_tests = true;
        }
        e.files.push(path.to_string());
    }
    m
}

pub fn check(crates: &BTreeMap<String, HarnessCrate>, invs: &[Invocation]) -> Result<Vec<String>, Vec<String>> {
    let mut fails = Vec::new();
    let mut notes = Vec::new();
    for (dir, hc) in crates {
        let live: Vec<&Invocation> = invs.iter().filter(|i| i.dir == *dir && i.void.is_none()).collect();
        let shown = if dir.is_empty() { "<repo root>" } else { dir.as_str() };
        if live.is_empty() {
            let voids: Vec<String> = invs
                .iter()
                .filter(|i| i.dir == *dir)
                .map(|i| format!("{}:{} ({})", i.workflow, i.job, i.void.clone().unwrap_or_default()))
                .collect();
            fails.push(format!(
                "{shown}: {} #[kani::proof] harness(es) and no CI `cargo kani` that can fail runs there{}",
                hc.harnesses,
                if voids.is_empty() { String::new() } else { format!(" -- void: {}", voids.join(", ")) }
            ));
            continue;
        }
        // K9: a run that compiles none of the crate's harnesses checks nothing.
        if hc.compiled == 0 {
            fails.push(format!(
                "{shown}: {} #[kani::proof] harness(es), none in a compiled root (the `[lib]` directory, tests/, benches/, examples/, src/bin/) -- `cargo kani` builds none of them",
                hc.harnesses
            ));
            continue;
        }
        if hc.needs_tests && !live.iter().any(|i| i.tests) {
            fails.push(format!("{shown}: harnesses under tests/ and no `cargo kani --tests` there"));
            continue;
        }
        let by: BTreeSet<String> = live
            .iter()
            .map(|i| match &i.guard {
                Some(g) => format!("{}:{} (path-guarded: {g})", i.workflow, i.job),
                None => format!("{}:{}", i.workflow, i.job),
            })
            .collect();
        let src = if hc.compiled == hc.harnesses {
            format!("{} harness(es)", hc.harnesses)
        } else {
            format!(
                "{} harness(es) run ({} in source; {} in files outside the package's compiled roots are never built)",
                hc.compiled,
                hc.harnesses,
                hc.harnesses - hc.compiled
            )
        };
        notes.push(format!("{shown}: {src}, run by {}", by.into_iter().collect::<Vec<_>>().join(", ")));
    }
    for i in invs.iter().filter(|i| i.void.is_some()) {
        notes.push(format!(
            "not counted: {}:{} in {} -- {}",
            i.workflow,
            i.job,
            if i.dir.is_empty() { "<repo root>" } else { &i.dir },
            i.void.clone().unwrap_or_default()
        ));
    }
    if crates.is_empty() {
        return Err(vec!["found no #[kani::proof] harness -- an empty scan is not coverage".into()]);
    }
    if fails.is_empty() {
        Ok(notes)
    } else {
        Err(fails)
    }
}

/// The nearest package directory and its lib directory (`[lib] path`'s parent, or `src`).
fn owning_package(repo: &Path, path: &str, cache: &mut BTreeMap<String, Option<String>>) -> (String, String) {
    let mut parts: Vec<&str> = path.split('/').collect();
    parts.pop();
    loop {
        let d = parts.join("/");
        let lib = cache
            .entry(d.clone())
            .or_insert_with(|| {
                let t = std::fs::read_to_string(repo.join(&d).join("Cargo.toml")).ok()?;
                if !t.lines().any(|l| l.trim() == "[package]") {
                    return None;
                }
                let mut in_lib = false;
                let mut lib = "src".to_string();
                for l in t.lines() {
                    let l = l.trim();
                    if l.starts_with('[') {
                        in_lib = l == "[lib]";
                        continue;
                    }
                    if in_lib {
                        if let Some(v) = l.strip_prefix("path").map(str::trim).and_then(|v| v.strip_prefix('=')) {
                            let p = v.trim().trim_matches('"');
                            lib = p.rsplit_once('/').map_or("", |(d, _)| d).to_string();
                        }
                    }
                }
                Some(lib)
            })
            .clone();
        if let Some(lib) = lib {
            return (d, lib);
        }
        if parts.is_empty() {
            return (d, "src".into());
        }
        parts.pop();
    }
}

fn scan(repo: &Path) -> Result<(BTreeMap<String, HarnessCrate>, Vec<Invocation>), String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-files", "-z", "--", "*.rs"])
        .output()
        .map_err(|e| format!("git ls-files: {e}"))?;
    if !out.status.success() {
        return Err("git ls-files failed".into());
    }
    let mut cache = BTreeMap::new();
    let mut files = Vec::new();
    for p in String::from_utf8_lossy(&out.stdout).split('\0').filter(|p| !p.is_empty() && !p.starts_with("tools/xtask/")) {
        let Ok(t) = std::fs::read_to_string(repo.join(p)) else { continue };
        if !t.contains("kani") {
            continue;
        }
        let (pkg, lib) = owning_package(repo, p, &mut cache);
        files.push((p.to_string(), t, pkg, lib));
    }
    let crates = harness_crates(files.iter().map(|(p, t, k, l)| (p.as_str(), t.as_str(), k.clone(), l.clone())));
    let wfdir = repo.join(".github/workflows");
    let mut invs = Vec::new();
    let mut names: Vec<_> = std::fs::read_dir(&wfdir)
        .map_err(|e| format!("cannot read .github/workflows: {e}"))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| matches!(p.extension().and_then(|x| x.to_str()), Some("yml") | Some("yaml")))
        .collect();
    names.sort();
    for p in names {
        let t = std::fs::read_to_string(&p).map_err(|e| format!("cannot read {}: {e}", p.display()))?;
        let n = p.file_name().unwrap_or_default().to_string_lossy().to_string();
        invs.extend(workflow_invocations(&n, &t));
    }
    Ok((crates, invs))
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let (crates, invs) = match scan(repo) {
        Ok(x) => x,
        Err(e) => return Verdict::Refused(e),
    };
    let total: usize = crates.values().map(|c| c.harnesses).sum();
    let compiled: usize = crates.values().map(|c| c.compiled).sum();
    let head = format!(
        "{} crate(s): {total} #[kani::proof] in source, {compiled} in compiled roots (an upper bound on what CI runs: reachability and pass/fail are not checked here); \
         {} CI `cargo kani` invocation(s) found ({} counted)",
        crates.len(),
        invs.len(),
        invs.iter().filter(|i| i.void.is_none()).count()
    );
    let mut tail = vec!["cannot see:".to_string()];
    tail.extend(CANNOT_SEE.iter().map(|s| format!("  - {s}")));
    match check(&crates, &invs) {
        Ok(n) => Verdict::Pass([vec![head], n, tail].concat()),
        Err(f) => Verdict::Fail([vec![head], f, tail].concat()),
    }
}


// rivet: verifies VER-HONESTY-KANI-COVERAGE-001
pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };
    for (what, got) in yaml::self_check() {
        ck(&what, got);
    }
    let h = "#[cfg(kani)]\nmod v {\n  #[kani::proof]\n  fn a() {}\n  #[ kani::proof ]\n  fn b() {}\n}\n// #[kani::proof] in a comment\n";
    let files = [
        ("tests/kani_harnesses.rs", h, String::new(), "plain/src".to_string()),
        ("src/sem.rs", h, String::new(), "plain/src".to_string()),
        ("ffi/src/lib.rs", h, "ffi".to_string(), "src".to_string()),
        ("drivers/dma-own/src/lib.rs", h, "drivers/dma-own".to_string(), "src".to_string()),
        ("drivers/wdg-thin/src/lib.rs", h, "drivers/wdg-thin".to_string(), "src".to_string()),
        ("drivers/cfg/src/lib.rs", "#[cfg_attr(kani, kani::proof)]\nfn c() {}\n", "drivers/cfg".to_string(), "src".to_string()),
        ("drivers/none/src/lib.rs", "// #[kani::proof]\nfn x() {}\n", "drivers/none".to_string(), "src".to_string()),
    ];
    let crates = harness_crates(files.iter().map(|(p, t, k, l)| (*p, *t, k.clone(), l.clone())));
    ck("5 crates with harnesses; a comment mention is not a harness", crates.len() == 5 && !crates.contains_key("drivers/none"));
    ck("a `#[cfg_attr(kani, kani::proof)]` harness is a harness", crates.get("drivers/cfg").is_some_and(|c| c.harnesses == 1));
    // Round 11, F6 and F1: a contract harness, and a harness after a raw C string.
    let hidden = harness_crates([
        ("drivers/ctr/src/lib.rs", "pub fn g(x: u8) -> u8 { x }\n#[cfg(kani)]\n#[kani::proof_for_contract(g)]\nfn zz() { g(kani::any()); }\n", "drivers/ctr".to_string(), "src".to_string()),
        ("drivers/cfc/src/lib.rs", "#[cfg_attr(kani, kani::proof_for_contract(g))]\nfn zz() {}\n", "drivers/cfc".to_string(), "src".to_string()),
        ("drivers/crs/src/lib.rs", "const _P: &core::ffi::CStr = cr\"\\\"; #[cfg(kani)] #[kani::proof] fn zz() { assert!(false); } // \"\n", "drivers/crs".to_string(), "src".to_string()),
    ]);
    ck("PLANTED F6 a crate whose only harness is `#[kani::proof_for_contract(g)]` is a harness crate (and, with no CI step, fails)",
        hidden.get("drivers/ctr").is_some_and(|c| c.harnesses == 1) && check(&hidden, &[]).is_err());
    ck("F6 the `cfg_attr(kani, kani::proof_for_contract(g))` form is a harness", hidden.get("drivers/cfc").is_some_and(|c| c.harnesses == 1));
    ck("PLANTED F1 a `#[kani::proof]` after a raw C string `cr\"\\\"` on its line is a harness", hidden.get("drivers/crs").is_some_and(|c| c.harnesses == 1));
    let crates: BTreeMap<String, HarnessCrate> = crates.into_iter().filter(|(k, _)| k != "drivers/cfg").collect();
    ck("2 harnesses per driver crate (whitespace-tolerant)", ["ffi", "drivers/dma-own", "drivers/wdg-thin"].iter().all(|c| crates[*c].harnesses == 2));
    ck("root: 4 in source, 2 compiled (src/ is outside the lib root plain/src)",
        crates.get("").is_some_and(|c| c.harnesses == 4 && c.compiled == 2));
    ck("root harnesses under tests/ need --tests", crates.get("").is_some_and(|c| c.needs_tests));

    let wf = "name: fv\non: [push]\njobs:\n  kani:\n    runs-on: ubuntu-24.04\n    steps:\n      - name: install\n        run: |\n          cargo install --locked kani-verifier\n          cargo kani setup\n      - name: root\n        run: cargo kani --tests\n      - name: ffi\n        run: cargo kani --tests --all-features\n        working-directory: ffi\n  drivers:\n    runs-on: ubuntu-24.04\n    strategy:\n      matrix:\n        crate: [dma-own, wdg-thin]\n    steps:\n      - run: cargo kani\n        working-directory: drivers/${{ matrix.crate }}\n";
    let invs = workflow_invocations("fv.yml", wf);
    ck("`cargo kani setup` is not an invocation; root, ffi and 2 matrix dirs are", invs.len() == 4);
    ck("matrix expands to both driver dirs",
        invs.iter().any(|i| i.dir == "drivers/dma-own") && invs.iter().any(|i| i.dir == "drivers/wdg-thin"));
    let full = check(&crates, &invs);
    ck("the full workflow covers every crate", full.is_ok());
    ck("the root line says 2 run of 4 in source",
        full.as_ref().is_ok_and(|n| n.iter().any(|l| l.starts_with("<repo root>: 2 harness(es) run (4 in source"))));

    let red = |what: &str, text: &str| -> (String, bool) {
        let v = workflow_invocations("fv.yml", text);
        (what.to_string(), text != wf && check(&crates, &v).is_err())
    };
    let green = |what: &str, text: &str| -> (String, bool) {
        let v = workflow_invocations("fv.yml", text);
        (what.to_string(), text != wf && check(&crates, &v).is_ok())
    };
    let step = "      - run: cargo kani\n        working-directory: drivers/${{ matrix.crate }}\n";
    let job_hdr = "  drivers:\n    runs-on: ubuntu-24.04\n";
    let with_step = |s: &str| wf.replace(step, s);
    let with_job = |extra: &str| wf.replace(job_hdr, &format!("{job_hdr}{extra}"));
    let wd = "        working-directory: drivers/${{ matrix.crate }}\n";
    let script = |body: &str| with_step(&format!("      - run: |\n{}{wd}", body.lines().map(|l| format!("          {l}\n")).collect::<String>()));
    let cases: Vec<(String, bool)> = vec![
        red("PLANTED an uncovered crate (dropped from the matrix) fails", &wf.replace("[dma-own, wdg-thin]", "[dma-own]")),
        red("VOID matrix `exclude:` of a crate", &wf.replace("        crate: [dma-own, wdg-thin]\n", "        crate: [dma-own, wdg-thin]\n        exclude:\n          - crate: wdg-thin\n")),
        red("VOID job `if: \"false\"`", &with_job("    if: \"false\"\n")),
        red("VOID job `if: false && true`", &with_job("    if: false && true\n")),
        red("VOID job `if: false`", &with_job("    if: false\n")),
        red("VOID job `if: true` (only the guard form is allowlisted)", &with_job("    if: true\n")),
        red("VOID step `if: false`", &with_step(&format!("      - if: false\n        run: cargo kani\n{wd}"))),
        red("VOID step `if: ${{ false }}`", &with_step(&format!("      - if: ${{{{ false }}}}\n        run: cargo kani\n{wd}"))),
        red("VOID step `if: always()`", &with_step(&format!("      - if: always()\n        run: cargo kani\n{wd}"))),
        red("VOID step `if: true` (only the guard form is allowlisted)", &with_step(&format!("      - if: true\n        run: cargo kani\n{wd}"))),
        red("VOID step `continue-on-error: true`", &with_step(&format!("      - continue-on-error: true\n        run: cargo kani\n{wd}"))),
        red("VOID step `continue-on-error: false` (the key is off the allowlist)", &with_step(&format!("      - continue-on-error: false\n        run: cargo kani\n{wd}"))),
        red("VOID step `continue-on-error: ${{ true }}`", &with_step(&format!("      - continue-on-error: ${{{{ true }}}}\n        run: cargo kani\n{wd}"))),
        red("VOID step `continue-on-error: true # comment`", &with_step(&format!("      - continue-on-error: true # comment\n        run: cargo kani\n{wd}"))),
        red("VOID job `continue-on-error: True`", &with_job("    continue-on-error: True\n")),
        red("VOID job `container:` (off the job allowlist)", &with_job("    container: some/image\n")),
        red("VOID job `env: BASH_ENV`", &with_job("    env:\n      BASH_ENV: ci/relax.sh\n")),
        red("VOID workflow `env: BASH_ENV`", &wf.replace("jobs:\n", "env:\n  BASH_ENV: ci/relax.sh\njobs:\n")),
        red("VOID step `env: BASH_ENV`", &with_step(&format!("      - env:\n          BASH_ENV: ci/relax.sh\n        run: cargo kani\n{wd}"))),
        red("VOID `set +e`", &script("set +e\ncargo kani")),
        red("VOID `builtin set +e`", &script("builtin set +e\ncargo kani\necho done")),
        red("VOID `eval 'set +e'`", &script("eval 'set +e'\ncargo kani\necho done")),
        red("VOID `. ./ci/relax.sh` (source)", &script(". ./ci/relax.sh\ncargo kani\necho done")),
        red("VOID `cargo kani && echo ok; echo done` (one line)", &with_step(&format!("      - run: cargo kani && echo ok; echo done\n{wd}"))),
        red("VOID `cargo kani && echo ok; echo done` (block)", &script("cargo kani && echo ok; echo done")),
        red("VOID `set -- -o pipefail` then a tee pipe", &script("set -- -o pipefail\ncargo kani 2>&1 | tee kani.log")),
        red("VOID `cargo kani; true`", &script("cargo kani; true")),
        red("VOID `cargo kani \\` + `|| true` on the next line", &script("cargo kani \\\n  || true")),
        red("VOID `cargo kani |& tee log`", &script("set -o pipefail\ncargo kani |& tee log")),
        red("VOID `export PATH=...` before `cargo kani`", &script("export PATH=\"$PWD/fake:$PATH\"\ncargo kani")),
        red("VOID `enable -n exit` / `hash -p` before `cargo kani`", &script("enable -n exit\nhash -p /usr/bin/true cargo\ncargo kani")),
        red("VOID `set +o pipefail` after `set -o pipefail`", &script("set -o pipefail\nset +o pipefail\ncargo kani 2>&1 | tee log")),
        red("VOID `trap \"exit 0\" ERR`", &script("trap \"exit 0\" ERR\ncargo kani")),
        red("VOID `shell: bash {0}`", &with_step(&format!("      - shell: bash {{0}}\n        run: cargo kani\n{wd}"))),
        red("VOID `shell: bash` (the key is off the allowlist)", &with_step(&format!("      - shell: bash\n        run: cargo kani\n{wd}"))),
        red("VOID background `cargo kani &`", &with_step(&format!("      - run: cargo kani &\n{wd}"))),
        red("VOID folded `run: >-` with `|| true` on the next line", &with_step(&format!("      - run: >-\n          cargo kani\n          || true\n{wd}"))),
        red("VOID folded `run: >` alone", &with_step(&format!("      - run: >\n          cargo kani\n{wd}"))),
        red("VOID `echo cargo kani would run here`", &with_step(&format!("      - run: echo cargo kani would run here\n{wd}"))),
        red("VOID `env X=1 cargo kani`", &with_step(&format!("      - run: env X=1 cargo kani\n{wd}"))),
        red("VOID `RUST_BACKTRACE=1 cargo kani` (an assignment prefix is not the shape)", &with_step(&format!("      - run: RUST_BACKTRACE=1 cargo kani\n{wd}"))),
        red("VOID `if cargo kani; then ...`", &script("if cargo kani; then echo ok; fi")),
        red("VOID `! cargo kani`", &script("! cargo kani")),
        red("VOID `cargo kani --harness one_name`", &with_step(&format!("      - run: cargo kani --harness one_name\n{wd}"))),
        red("VOID `cargo kani -p something-else`", &with_step(&format!("      - run: cargo kani -p something-else\n{wd}"))),
        red("VOID `cargo kani --package=x`", &with_step(&format!("      - run: cargo kani --package=x\n{wd}"))),
        red("VOID `cd` in the script", &script("cd .\ncargo kani")),
        red("VOID `exit 0` before the invocation", &script("exit 0\ncargo kani")),
        red("VOID `cargo kani || true`", &wf.replace("run: cargo kani\n", "run: cargo kani || true\n")),
        red("VOID a commented-out `cargo kani`", &script("# cargo kani\necho skipped")),
        red("VOID an unknown matrix key leaves the directory unresolved", &wf.replace("drivers/${{ matrix.crate }}", "drivers/${{ matrix.nope }}")),
        red("VOID a working-directory typo", &wf.replace("drivers/${{ matrix.crate }}", "driver/${{ matrix.crate }}")),
        red("VOID `cargo kani --only-codegen`", &with_step(&format!("      - run: cargo kani --only-codegen\n{wd}"))),
        red("VOID `cargo kani --tests --help`", &with_step(&format!("      - run: cargo kani --tests --help\n{wd}"))),
        red("VOID `cargo kani --dry-run`", &with_step(&format!("      - run: cargo kani --dry-run\n{wd}"))),
        red("VOID `cargo kani --exclude x`", &with_step(&format!("      - run: cargo kani --exclude x\n{wd}"))),
        red("VOID a heredoc containing `cargo kani`", &script("cat <<EOF\ncargo kani\nEOF")),
        red("VOID `cargo kani --default-unwind $N`", &with_step(&format!("      - run: cargo kani --default-unwind $N\n{wd}"))),
        red("VOID `cargo kani --features --harness`", &with_step(&format!("      - run: cargo kani --features --harness\n{wd}"))),
        red("VOID `cargo kani \"--tests\"` (a quoted word is not a literal word)", &with_step(&format!("      - run: cargo kani \"--tests\"\n{wd}"))),
        red("VOID `echo start` then `cargo kani` (a second command)", &script("echo start\ncargo kani")),
        red("VOID `true && cargo kani`", &script("true && cargo kani")),
        red("VOID a tee pipe without the `set -o pipefail` line", &script("cargo kani 2>&1 | tee log")),
        red("VOID `set -euo pipefail` (only the exact `set -o pipefail` line)", &script("set -euo pipefail\ncargo kani 2>&1 | tee log")),
        red("VOID `| tee -a log` (an option to tee)", &script("set -o pipefail\ncargo kani 2>&1 | tee -a log")),
        red("VOID `| grep x` (only tee after the pipe)", &script("set -o pipefail\ncargo kani 2>&1 | grep x")),
        red("VOID D: `-   run:` (wide dash) with `continue-on-error: true`",
            &with_step("      -   run: cargo kani\n          working-directory: drivers/${{ matrix.crate }}\n          continue-on-error: true\n")),
        red("VOID D2: `-   run:` (wide dash) with `if: false`",
            &with_step("      -   run: cargo kani\n          working-directory: drivers/${{ matrix.crate }}\n          if: false\n")),
        red("VOID a step whose keys are mis-indented (the workflow is not read)",
            &with_step("      - run: cargo kani\n          working-directory: drivers/${{ matrix.crate }}\n")),
        green("`-   run:` (wide dash) with no other key counts (indentation, not a column rule)",
            &with_step("      -   run: cargo kani\n          working-directory: drivers/${{ matrix.crate }}\n")),
        green("`..` in working-directory resolves", &wf.replace("drivers/${{ matrix.crate }}", "drivers/x/../${{ matrix.crate }}")),
        green("job `defaults.working-directory` sets the base", &wf.replace(step, "      - run: cargo kani\n").replace("    strategy:\n", "    defaults:\n      run:\n        working-directory: drivers/${{ matrix.crate }}\n    strategy:\n")),
        green("literal allowlisted flags count (--features x --default-unwind 8 --jobs=2)", &with_step(&format!("      - run: cargo kani --features x --default-unwind 8 --jobs=2\n{wd}"))),
        green("`\"run\": cargo kani` (quoted key) is read", &with_step(&format!("      - \"run\": cargo kani\n{wd}"))),
        green("`2>&1` is allowed", &with_step(&format!("      - run: cargo kani 2>&1\n{wd}"))),
        green("`set -o pipefail` + `cargo kani 2>&1 | tee log` counts", &script("set -o pipefail\ncargo kani 2>&1 | tee ../kani-reports/x.log")),
        green("name, id, timeout-minutes are allowlisted step keys", &with_step(&format!("      - name: k\n        id: k\n        timeout-minutes: 30\n        run: cargo kani\n{wd}"))),
        green("workflow `env: GITHUB_TOKEN` is allowlisted", &wf.replace("jobs:\n", "env:\n  GITHUB_TOKEN: ${{ github.token }}\njobs:\n")),
        red("VOID `\"if\": false` (quoted key)", &with_step(&format!("      - \"if\": false\n        run: cargo kani\n{wd}"))),
        red("VOID `\"continue-on-error\": true`", &with_step(&format!("      - \"continue-on-error\": true\n        run: cargo kani\n{wd}"))),
        red("VOID job `'if': false`", &with_job("    'if': false\n")),
        red("VOID an anchor in the workflow (not read)", &with_job("    timeout-minutes: &t 30\n")),
        // Round 11, F4: the most specific working-directory REPLACES the others.
        green("F4 control: job default `drivers/${{ matrix.crate }}`, step with no working-directory",
            &with_job("    defaults:\n      run:\n        working-directory: drivers/${{ matrix.crate }}\n").replace(step, "      - run: cargo kani\n")),
        red("PLANTED F4 false green: job default `drivers/${{ matrix.crate }}`, step `working-directory: .` (the step runs at the root)",
            &with_job("    defaults:\n      run:\n        working-directory: drivers/${{ matrix.crate }}\n").replace(step, "      - run: cargo kani\n        working-directory: .\n")),
        red("PLANTED F4 false green: ... step `working-directory: ffi/..`",
            &with_job("    defaults:\n      run:\n        working-directory: drivers/${{ matrix.crate }}\n").replace(step, "      - run: cargo kani\n        working-directory: ffi/..\n")),
        green("PLANTED F4 false red: job default `ffi`, step `working-directory: drivers/${{ matrix.crate }}` (not ffi/drivers/..)",
            &with_job("    defaults:\n      run:\n        working-directory: ffi\n")),
        green("PLANTED F4 false red: a job default `.` replaces a workflow default `ffi`",
            &wf.replace("jobs:\n", "defaults:\n  run:\n    working-directory: ffi\njobs:\n").replace("  kani:\n    runs-on: ubuntu-24.04\n", "  kani:\n    runs-on: ubuntu-24.04\n    defaults:\n      run:\n        working-directory: .\n")),
    ];
    for (what, got) in cases {
        ck(&what, got);
    }
    let unresolved = workflow_invocations("fv.yml", &wf.replace("drivers/${{ matrix.crate }}", "drivers/${{ env.X }}"));
    ck("VOID an unresolved `${{ env.X }}` directory is voided as unresolved",
        !unresolved.is_empty() && unresolved.iter().filter(|i| i.dir.contains("env.X")).all(|i| i.void.as_deref().is_some_and(|v| v.contains("not resolved"))));
    let or_only = workflow_invocations("fv.yml", &wf.replace("run: cargo kani\n", "run: cargo kani || true\n"));
    ck("the `||` case names the word it rejects",
        or_only.iter().any(|i| i.void.as_deref().is_some_and(|v| v.contains("`||`"))));
    let env_step = workflow_invocations("fv.yml", &with_step(&format!("      - env:\n          BASH_ENV: x\n        run: cargo kani\n{wd}")));
    ck("the step `env:` case is voided by the step-key allowlist",
        env_step.iter().any(|i| i.void.as_deref().is_some_and(|v| v.contains("step key `env:`"))));
    let unread = workflow_invocations("fv.yml", &with_step("      - run: cargo kani\n          working-directory: drivers/${{ matrix.crate }}\n"));
    ck("an unread workflow that mentions `cargo kani` is one VOID invocation saying why",
        unread.len() == 1 && unread[0].void.as_deref().is_some_and(|v| v.contains("YAML subset")));

    // The repo's guard template (formal-verification.yml's `guard` step, reduced).
    let tmpl_line = "            echo \"drivers=true\" >> \"$GITHUB_OUTPUT\"; echo \"not a PR\"; exit 0\n";
    let guard_script = |third: &str| {
        format!(
            "          set -euo pipefail\n          {GUARD_IF}\n{third}          fi\n          echo \"drivers=false\" >> \"$GITHUB_OUTPUT\"\n"
        )
    };
    let guarded_with = |script: &str| {
        format!(
            "name: g\non: [push]\njobs:\n  guard:\n    runs-on: x\n    outputs:\n      drivers: ${{{{ steps.q.outputs.drivers }}}}\n    steps:\n      - id: q\n        run: |\n{script}  drivers:\n    needs: guard\n    runs-on: x\n    steps:\n      - if: needs.guard.outputs.drivers == 'true'\n        run: cargo kani\n        working-directory: drivers/dma-own\n"
        )
    };
    let guarded_s = guarded_with(&guard_script(tmpl_line));
    let guarded = guarded_s.as_str();
    ck("the guard template sets exactly its K=true outputs",
        guard_template_outputs(&guard_script(tmpl_line)) == ["drivers".to_string()].into_iter().collect());
    let gi = workflow_invocations("g.yml", guarded);
    ck("guard form with `needs: guard` and a declared output counts (noted as path-guarded)",
        gi.len() == 1 && gi[0].void.is_none() && gi[0].guard.as_deref() == Some("drivers"));
    let with_checkout = workflow_invocations("g.yml", &guarded.replace("      - id: q\n", "      - uses: actions/checkout@v4\n        with:\n          fetch-depth: 0\n      - id: q\n"));
    ck("K5c control: an `actions/checkout` step before the template step still counts", with_checkout.len() == 1 && with_checkout[0].void.is_none());
    let co_keys = workflow_invocations("g.yml", &guarded.replace("      - id: q\n", "      - id: co\n        uses: actions/checkout@v4\n        timeout-minutes: 5\n      - id: q\n"));
    ck("checkout control: a checkout step with `id:` and `timeout-minutes:` still counts", co_keys.len() == 1 && co_keys[0].void.is_none());
    let co_branch = workflow_invocations("g.yml", &guarded.replace("      - id: q\n", "      - uses: actions/checkout@releases/v1\n      - id: q\n"));
    ck("checkout control: `actions/checkout@releases/v1` (a branch ref with `/`) still counts", co_branch.len() == 1 && co_branch[0].void.is_none());
    for bad in ["actions/checkout@v4/../x", "actions/checkout@v4@evil", "actions/checkout@", "actions/checkout@/v4", "actions/checkout@a//b"] {
        let v = workflow_invocations("g.yml", &guarded.replace("      - id: q\n", &format!("      - uses: {bad}\n      - id: q\n")));
        ck(&format!("VOID an earlier `{bad}`"), v.len() == 1 && v[0].void.is_some());
    }
    let no_needs = workflow_invocations("g.yml", &guarded.replace("    needs: guard\n", ""));
    ck("VOID guard form without `needs: guard`", no_needs.len() == 1 && no_needs[0].void.is_some());
    let undeclared = workflow_invocations("g.yml", &guarded.replace("outputs.drivers == 'true'", "outputs.other == 'true'"));
    ck("VOID guard form naming an output the guard does not declare", undeclared.len() == 1 && undeclared[0].void.is_some());
    let not_true = workflow_invocations("g.yml", &guarded.replace("== 'true'", "!= 'true'"));
    ck("VOID guard output compared with anything but == 'true'", not_true.len() == 1 && not_true[0].void.is_some());

    let g_void = |what: &str, text: &str| -> (String, bool) {
        let v = workflow_invocations("g.yml", text);
        (what.to_string(), text != guarded && !v.is_empty() && v.iter().all(|i| i.void.is_some()))
    };
    let forgeries = vec![
        g_void("VOID guard output set to the literal 'false'", &guarded.replace("drivers: ${{ steps.q.outputs.drivers }}", "drivers: 'false'")),
        g_void("VOID guard output mapped to a step that does not exist", &guarded.replace("steps.q.outputs.drivers", "steps.nope.outputs.drivers")),
        g_void("VOID guard script that never writes drivers=true", &guarded.replace("drivers=true", "drivers=false")),
        g_void("VOID `if: false` on the guard job", &guarded.replace("  guard:\n    runs-on: x\n", "  guard:\n    if: false\n    runs-on: x\n")),
        g_void("VOID `needs: [guard, never]`", &guarded.replace("    needs: guard\n", "    needs: [guard, never]\n")),
        g_void("VOID a workflow reduced to `on: workflow_dispatch`", &guarded.replace("on: [push]", "on: workflow_dispatch")),
        g_void("VOID a block `on:` with only workflow_dispatch", &guarded.replace("on: [push]", "on:\n  workflow_dispatch:")),
        g_void("VOID a guard step with `env: BASH_ENV`", &guarded.replace("      - id: q\n", "      - id: q\n        env:\n          BASH_ENV: x\n")),
        g_void("K1 VOID a lone CR in a comment hiding a job `if: false`", &guarded.replace("  drivers:\n    needs: guard\n", "  drivers:\n    needs: guard # c\r    if: false\n")),
        g_void("K2 VOID a job guard on an undeclared output under a valid step guard", &guarded.replace("  drivers:\n    needs: guard\n", "  drivers:\n    needs: guard\n    if: needs.guard.outputs.nope == 'true'\n")),
        g_void("K3 VOID `needs:` on the guard job", &guarded.replace("  guard:\n    runs-on: x\n", "  guard:\n    needs: never\n    runs-on: x\n")),
        g_void("K4 VOID guard job `env: BASH_ENV`", &guarded.replace("  guard:\n    runs-on: x\n", "  guard:\n    env:\n      BASH_ENV: x\n    runs-on: x\n")),
        g_void("K5 VOID guard job `defaults: run: shell: \"true {0}\"`", &guarded.replace("  guard:\n    runs-on: x\n", "  guard:\n    defaults:\n      run:\n        shell: \"true {0}\"\n    runs-on: x\n")),
        g_void("K5c VOID an earlier guard-job step writing BASH_ENV to $GITHUB_ENV",
            &guarded.replace("      - id: q\n", "      - run: echo \"BASH_ENV=$PWD/x.sh\" >> \"$GITHUB_ENV\"\n      - id: q\n")),
        g_void("K5i VOID an earlier `uses: actions/checkout@v4/../../../evil/act@v1`",
            &guarded.replace("      - id: q\n", "      - uses: actions/checkout@v4/../../../evil/act@v1\n      - id: q\n")),
        g_void("K5e VOID an earlier `uses: actions/checkout-evil@v1`",
            &guarded.replace("      - id: q\n", "      - uses: actions/checkout-evil@v1\n      - id: q\n")),
        g_void("G VOID `concurrency:` (cancel-in-progress) on the guard job",
            &guarded.replace("  guard:\n    runs-on: x\n", "  guard:\n    concurrency:\n      group: x\n      cancel-in-progress: true\n    runs-on: x\n")),
        g_void("K5 VOID guard job `container:`", &guarded.replace("  guard:\n    runs-on: x\n", "  guard:\n    container: alpine\n    runs-on: x\n")),
    ];
    for (what, got) in forgeries {
        ck(&what, got);
    }
    let sec2 = vec![
        ("SEC-2 VOID `drivers=true` only in a comment", guarded_with("          # echo \"drivers=true\" >> \"$GITHUB_OUTPUT\"\n          echo x\n")),
        ("SEC-2 VOID `drivers=true` under `if false`", guarded_with("          if false; then echo \"drivers=true\" >> \"$GITHUB_OUTPUT\"; fi\n")),
        ("SEC-2 VOID `drivers=true` inside a function", guarded_with("          f() { echo \"drivers=true\" >> \"$GITHUB_OUTPUT\"; }\n")),
        ("SEC-2 VOID `drivers=true` inside a string", guarded_with("          echo \"the string drivers=true\"\n")),
        ("SEC-2 VOID the template with a second write of drivers=", guarded_with(&guard_script("            echo \"drivers=true\" >> \"$GITHUB_OUTPUT\"; echo \"drivers=false\" >> \"$GITHUB_OUTPUT\"; exit 0\n"))),
        ("SEC-2 VOID the template without `exit 0`", guarded_with(&guard_script("            echo \"drivers=true\" >> \"$GITHUB_OUTPUT\"; echo \"not a PR\"\n"))),
        ("SEC-2 VOID the template with an expansion in the message", guarded_with(&guard_script("            echo \"drivers=true\" >> \"$GITHUB_OUTPUT\"; echo \"$(false)\"; exit 0\n"))),
        ("SEC-2 VOID the template with `set -uo pipefail` (no -e)", guarded_with(&guard_script(tmpl_line).replacen("set -euo pipefail", "set -uo pipefail", 1))),
        ("SEC-2 VOID the template writing drivers=true twice", guarded_with(&guard_script("            echo \"drivers=true\" >> \"$GITHUB_OUTPUT\"; echo \"drivers=true\" >> \"$GITHUB_OUTPUT\"; exit 0\n"))),
        ("SEC-2 VOID a guard step with `if:`", guarded_with(&guard_script(tmpl_line)).replace("      - id: q\n", "      - id: q\n        if: false\n")),
        ("SEC-2 VOID a different condition", guarded_with(&guard_script(tmpl_line).replace("!= \"merge_group\"", "== \"merge_group\""))),
    ];
    for (what, text) in sec2 {
        ck(what, g_void(what, &text).1);
    }
    let sec3 = vec![
        ("SEC-3 VOID `push:` nested under workflow_dispatch inputs", "on:\n  workflow_dispatch:\n    inputs:\n      push:\n        type: string"),
        ("SEC-3 VOID a `- push` list item nested under workflow_dispatch", "on:\n  workflow_dispatch:\n    inputs:\n      x:\n        options:\n          - push"),
        ("SEC-3 VOID push with a `paths:` filter", "on:\n  push:\n    paths: ['x/**']"),
        ("SEC-3 VOID push limited to `branches: [dev]`", "on:\n  push:\n    branches: [dev]"),
        ("SEC-3 VOID pull_request with `branches-ignore: [main]`", "on:\n  pull_request:\n    branches-ignore: [main]"),
        ("SEC-3 VOID push with `tags:` only", "on:\n  push:\n    tags: ['v*']"),
        ("SEC-3 VOID a flow-mapping push", "on:\n  push: {branches: [dev]}"),
        ("K6 VOID push `branches: [main, '!main']`", "on:\n  push:\n    branches: [main, '!main']"),
        ("K7 VOID `schedule: []`", "on:\n  schedule: []"),
        ("SEC VOID a cron that never fires (Feb 31: `0 0 31 2 *`)", "on:\n  schedule:\n    - cron: '0 0 31 2 *'"),
        ("SEC VOID a cron with an out-of-range field (`0 24 * * *`)", "on:\n  schedule:\n    - cron: '0 24 * * *'"),
    ];
    for (what, on) in sec3 {
        ck(what, g_void(what, &guarded.replace("on: [push]", on)).1);
    }
    let sched = workflow_invocations("g.yml", &guarded.replace("on: [push]", "on:\n  push:\n    paths: ['x/**']\n  schedule:\n    - cron: '0 2 * * 0'"));
    let feb29 = workflow_invocations("g.yml", &guarded.replace("on: [push]", "on:\n  schedule:\n    - cron: '0 0 29 2 *'"));
    ck("VOID Feb 29 (`0 0 29 2 *`): fires only in leap years, not provable", feb29.len() == 1 && feb29[0].void.is_some());
    ck("SEC: cron parsing -- `*/15 0-6,22 1-7 */2 1-5` fires; `0 0 30 2 *` and `0 0 31 4,6 *` never do; `0 0 31 2 1` fires (weekday OR day)",
        cron_can_fire("*/15 0-6,22 1-7 */2 1-5").is_ok() && cron_can_fire("0 0 30 2 *").is_err() && cron_can_fire("0 0 31 4,6 *").is_err() && cron_can_fire("0 0 31 2 1").is_ok());
    ck("C1: names -- `0 3 * * MON-FRI`, `0 3 * * SUN`, `0 0 1 JAN *`, `0 0 1 jan-mar *` fire",
        ["0 3 * * MON-FRI", "0 3 * * SUN", "0 0 1 JAN *", "0 0 1 jan-mar *"].iter().all(|c| cron_can_fire(c).is_ok()));
    ck("cron: `0 0 31 2 */2` (a `*`-led weekday is unrestricted: day 31 of Feb, never) is not read as firing; `@daily`, `60 * * * *`, `0 3 * * FUNDAY` are refused",
        ["0 0 31 2 */2", "@daily", "60 * * * *", "0 3 * * FUNDAY"].iter().all(|c| cron_can_fire(c).is_err()));
    ck("cron: `0 0 * * 7` (Sunday, as cron reads 7) fires", cron_can_fire("0 0 * * 7").is_ok());
    ck("a schedule trigger counts even when push is path-filtered", sched.len() == 1 && sched[0].void.is_none());
    let pr_main = workflow_invocations("g.yml", &guarded.replace("on: [push]", "on:\n  pull_request:\n    branches:\n      - main"));
    ck("PLANTED P1: a path-guarded invocation in a pull_request-only workflow is VOID (on a PR the guard may write K=false every time)",
        pr_main.len() == 1 && pr_main[0].void.as_deref().is_some_and(|v| v.contains("push-to-main or schedule")));
    let pr_unguarded = workflow_invocations("fv.yml", &wf.replace("on: [push]", "on:\n  pull_request:\n    branches: [main]"));
    ck("P1 control: an UNguarded invocation in a pull_request-on-main workflow counts", !pr_unguarded.is_empty() && pr_unguarded.iter().all(|i| i.void.is_none()));
    let block_on = workflow_invocations("g.yml", &guarded.replace("on: [push]", "on:\n  push:\n    branches: [main]\n  workflow_dispatch:"));
    ck("a block `on:` with push counts", block_on.len() == 1 && block_on[0].void.is_none());

    let mut planted = crates.clone();
    planted.insert("drivers/new-thin".into(), HarnessCrate { harnesses: 1, compiled: 1, needs_tests: false, files: vec![] });
    ck("PLANTED a new crate with a harness and no job fails", check(&planted, &invs).is_err());
    let mut outside = crates.clone();
    outside.insert("drivers/dma-own".into(), HarnessCrate { harnesses: 1, compiled: 0, needs_tests: false, files: vec![] });
    ck("K9 PLANTED a crate whose only harness is outside its compiled roots fails, even with a step there",
        invs.iter().any(|i| i.dir == "drivers/dma-own" && i.void.is_none()) && check(&outside, &invs).is_err());
    let crlf = workflow_invocations("fv.yml", &wf.replace('\n', "\r\n"));
    ck("control: the same workflow with CRLF line ends is read and counts", crlf.len() == 4 && crlf.iter().all(|i| i.void.is_none()));
    let no_tests = wf.replace("run: cargo kani --tests\n", "run: cargo kani\n");
    ck("root tests/ harnesses without --tests fail", check(&crates, &workflow_invocations("fv.yml", &no_tests)).is_err());

    // nightly.yml's kani-extended shape: the reports directory is made by its
    // own step; the Kani step is `set -o pipefail` + one tee'd line.
    let nightly = "name: n\non:\n  schedule:\n    - cron: '0 3 * * *'\njobs:\n  kani-extended:\n    steps:\n      - name: reports dir\n        run: mkdir -p kani-reports\n      - name: ffi\n        working-directory: ffi\n        run: |\n          set -o pipefail\n          cargo kani --tests --all-features --default-unwind 16 --object-bits 12 2>&1 | tee ../kani-reports/ffi.log\n";
    let ni = workflow_invocations("nightly.yml", nightly);
    ck("a tee pipe after `set -o pipefail` counts (nightly shape, in ffi)",
        ni.len() == 1 && ni[0].dir == "ffi" && ni[0].tests && ni[0].void.is_none());
    let ni_old = workflow_invocations("nightly.yml", &nightly.replace("          set -o pipefail\n", ""));
    ck("the same nightly step without pipefail does not count", ni_old.len() == 1 && ni_old[0].void.is_some());
    let ni_mk = workflow_invocations("nightly.yml", &nightly.replace("          set -o pipefail\n", "          set -o pipefail\n          mkdir -p ../kani-reports\n"));
    ck("VOID the old nightly shape (a `mkdir` line in the Kani step)", ni_mk.len() == 1 && ni_mk[0].void.is_some());
    let ni_var = workflow_invocations("nightly.yml", &nightly.replace("--default-unwind 16 --object-bits 12", "$KANI_FLAGS"));
    ck("VOID arguments from a variable (`$KANI_FLAGS` could carry `--harness`)", ni_var.len() == 1 && ni_var[0].void.is_some());
    let mp = "name: m\non: [push]\njobs:\n  k:\n    steps:\n      - run: cargo kani --manifest-path drivers/dma-own/Cargo.toml\n";
    ck("--manifest-path sets the directory", workflow_invocations("m.yml", mp).first().is_some_and(|i| i.dir == "drivers/dma-own" && i.void.is_none()));
    ck("an empty harness scan fails", check(&BTreeMap::new(), &invs).is_err());

    match scan(repo) {
        Ok((c, i)) => ck("the real tree: harness crates and workflow invocations found", !c.is_empty() && !i.is_empty()),
        Err(e) => ck(&format!("the real tree scans: {e}"), false),
    }
    // Positive control on the reader: every real workflow is in its subset.
    let mut wfs: Vec<_> = std::fs::read_dir(repo.join(".github/workflows")).map(|d| d.flatten().map(|e| e.path()).collect()).unwrap_or_default();
    wfs.retain(|p: &std::path::PathBuf| matches!(p.extension().and_then(|x| x.to_str()), Some("yml") | Some("yaml")));
    wfs.sort();
    let unread: Vec<String> = wfs
        .iter()
        .filter_map(|p| {
            let t = std::fs::read_to_string(p).unwrap_or_default();
            yaml::parse(&t).err().map(|e| format!("{}: {e}", p.file_name().unwrap_or_default().to_string_lossy()))
        })
        .collect();
    ck(&format!("the real tree: the YAML reader reads {} of {} workflows{}", wfs.len() - unread.len(), wfs.len(), if unread.is_empty() { String::new() } else { format!(" (unread: {})", unread.join("; ")) }),
        !wfs.is_empty() && unread.is_empty());
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

//! Every sieve gale's CI runs is the one pinned sieve, and the pin agrees with itself.
//!
//! sieve (pulseengine's shared CI path classifier) decides, per pull request,
//! which heavy jobs run (sieve.toml). It is not in the varve layer gale pins,
//! so CI installs it from its GitHub release through
//! .github/actions/setup-sieve, by sha256. A classifier that is not the pinned
//! one classifies with rules nobody reviewed, and a skipped job reports green.
//!
//! The action must agree with itself:
//!   - SIEVE_VERSION is an exact X.Y.Z, and the install step's name says the same;
//!   - SIEVE_SHA256_X86_64 and SIEVE_SHA256_AARCH64 are 64 lowercase hex digits and
//!     differ (one value copied into both slots pins one asset and breaks the other);
//!   - the lines that build the asset url from the version, map each architecture
//!     to its own sha256, check it, ask the binary its version, and put it on PATH
//!     are all present (outside comments);
//!   - no step carries `if:` or `continue-on-error:`.
//! Every workflow job that runs sieve (a `sieve <subcommand>` command, or temper's
//! gate-probe action, which runs it through tools/ci/sieve-gate-probe.sh) must run
//! the action and then, as its very next step, "Pinned sieve is the one on PATH",
//! before the first use. The action cannot prove what PATH resolves: its
//! GITHUB_PATH write only reaches later steps. No other workflow or action may
//! download a sieve release, so the pin exists once.
//!
//! Every classifier job records its verdict (sieve v0.4.0 `--record`): its one
//! classify call is exactly RECORD_RUN, and its very next step uploads that
//! record (SHA-pinned upload-artifact, named sieve-verdict-<workflow>-<job>-
//! <run_attempt>, the record's path, if-no-files-found: error, retention-days:
//! 90, `if:` only always()). The record on a pull request is the verdict that
//! skipped jobs, the input the observe follow-up compares with the push run.
//!
//! The action must also equal the canonical text held below (a present-but-
//! neutralised line fails), and the required job "every required context can
//! actually report" must keep its four sieve steps, exactly and unguarded.
//!
//! What it cannot see: that the sha256 values are the release's real ones (that
//! was checked with cosign when they were copied; docs/ci/sieve.md), a job that
//! reaches sieve through a name other than `sieve`, or PATH changes made inside
//! a script. It reads YAML by indentation, not with a YAML parser.

use crate::verdict::Verdict;
use std::path::Path;

pub const NAME: &str = "sieve-pin";

const ACTION: &str = ".github/actions/setup-sieve/action.yml";
const ACTION_REF: &str = "./.github/actions/setup-sieve";
const GATE_PROBE_ACTION: &str = "pulseengine/temper/.github/actions/gate-probe";

/// Lines the action must carry, outside comments.
const ACTION_LINES: &[&str] = &[
    r#"Linux/x86_64) target=x86_64-unknown-linux-musl; sha="$SIEVE_SHA256_X86_64" ;;"#,
    r#"Linux/aarch64|Linux/arm64) target=aarch64-unknown-linux-musl; sha="$SIEVE_SHA256_AARCH64" ;;"#,
    r#"url="https://github.com/pulseengine/sieve/releases/download/v$SIEVE_VERSION/sieve-v$SIEVE_VERSION-$target.tar.gz""#,
    r#"echo "$sha  $tgz" | sha256sum -c -"#,
    r#"got="$("$dir/sieve" --version)""#,
    r#"if [ "$first" != "sieve $SIEVE_VERSION" ]; then"#,
    r#"echo "$dir" >> "$GITHUB_PATH""#,
    r#"echo "SIEVE_PINNED_DIR=$dir" >> "$GITHUB_ENV""#,
];

const VERIFY_STEP: &str = "Pinned sieve is the one on PATH";
const VERIFY_LINES: &[&str] = &[
    r#"set -euo pipefail"#,
    r#"got="$(command -v sieve || true)""#,
    r#"if [ "$got" != "$SIEVE_PINNED_DIR/sieve" ]; then"#,
    r#"exit 1"#,
];

/// sieve's subcommands (sieve 0.3.0 --help). `command -v sieve` is not a use.
const SUBCOMMANDS: &[&str] =
    &["classify", "graph", "check-graph", "check-workflows", "equivalence", "observe", "--version", "--help"];

/// The line without indentation and a leading list dash; None for a comment.
fn code_of(line: &str) -> Option<&str> {
    let code = line.trim_start();
    let code = code.strip_prefix("- ").unwrap_or(code).trim_start();
    if code.starts_with('#') { None } else { Some(code) }
}

fn has_code_line(text: &str, needle: &str) -> bool {
    text.lines().filter_map(code_of).any(|c| c.contains(needle))
}

fn has_key(text: &str, key: &str) -> bool {
    text.lines().filter_map(code_of).any(|c| c.starts_with(key))
}

/// The first `"..."` after `key`, on a non-comment line.
fn quoted_after(text: &str, key: &str) -> Option<String> {
    for c in text.lines().filter_map(code_of) {
        if let Some(rest) = c.strip_prefix(key) {
            let rest = rest.trim_start().strip_prefix('"')?;
            return Some(rest[..rest.find('"')?].to_string());
        }
    }
    None
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn is_exact_version(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    parts.len() == 3 && parts.iter().all(|p| !p.is_empty() && p.bytes().all(|b| b.is_ascii_digit()))
}

/// Whether a line runs sieve: a `sieve` (or `.../sieve`) command token followed
/// by one of its subcommands, or temper's gate-probe action.
fn invokes_sieve(line: &str) -> bool {
    let Some(code) = code_of(line) else { return false };
    if code.starts_with("uses:") {
        return code.contains(GATE_PROBE_ACTION);
    }
    let toks: Vec<&str> = code
        .split(|c: char| c.is_whitespace() || ";|&()`\"'".contains(c))
        .filter(|t| !t.is_empty())
        .collect();
    toks.windows(2).any(|w| (w[0] == "sieve" || w[0].ends_with("/sieve")) && SUBCOMMANDS.contains(&w[1]))
}

/// (job id, job text) for each job under the top-level `jobs:` key.
fn jobs(wf: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut in_jobs = false;
    for line in wf.lines() {
        if !line.starts_with(' ') && !line.trim().is_empty() && !line.starts_with('#') {
            in_jobs = line.trim_end() == "jobs:";
            continue;
        }
        if !in_jobs {
            continue;
        }
        let is_key = line.starts_with("  ")
            && !line.starts_with("   ")
            && line.trim_end().ends_with(':')
            && !line.trim_start().starts_with('#');
        if is_key {
            out.push((line.trim().trim_end_matches(':').to_string(), String::new()));
        } else if let Some(last) = out.last_mut() {
            last.1.push_str(line);
            last.1.push('\n');
        }
    }
    out
}

/// A job's text split into the part before its first step, then one entry per step.
fn steps(job: &str) -> Vec<String> {
    let mut out = vec![String::new()];
    let mut in_steps = false;
    let mut indent: Option<usize> = None;
    for line in job.lines() {
        let t = line.trim_start();
        let ind = line.len() - t.len();
        if !in_steps && t.trim_end() == "steps:" {
            in_steps = true;
        } else if in_steps && t.starts_with("- ") {
            let item = *indent.get_or_insert(ind);
            if ind == item {
                out.push(String::new());
            }
        } else if in_steps && !t.is_empty() && !t.starts_with('#') && indent.is_some_and(|i| ind < i) {
            in_steps = false;
        }
        let last = out.last_mut().expect("never empty");
        last.push_str(line);
        last.push('\n');
    }
    out
}

fn is_install(step: &str) -> bool {
    step.lines()
        .filter_map(code_of)
        .any(|c| c.strip_prefix("uses:").is_some_and(|r| r.trim() == ACTION_REF))
}

fn is_verify(step: &str) -> bool {
    has_code_line(step, VERIFY_STEP) && VERIFY_LINES.iter().all(|l| has_code_line(step, l))
}

fn guarded(step: &str) -> bool {
    has_key(step, "if:") || has_key(step, "continue-on-error:")
}

/// Every way a workflow's jobs reach a sieve that is not the verified pin.
fn unpinned_jobs(file: &str, wf: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (name, body) in jobs(wf) {
        let st = steps(&body);
        // A shallow checkout cannot resolve the PR base: sieve then runs everything
        // with a "git diff failed" reason, silently, and the saving disappears.
        if body.lines().any(|l| invokes_sieve(l) && l.contains(" classify") && l.contains("--base")) && !fetch_depth_ok(&body) {
            out.push(format!("{file}: job `{name}` runs `sieve classify --base` without a checkout of fetch-depth 0 or >= 2"));
        }
        let mut installed = false;
        for (i, step) in st.iter().enumerate() {
            if i > 0 && is_install(step) {
                if guarded(step) {
                    out.push(format!("{file}: job `{name}`: the {ACTION_REF} step carries `if:` or `continue-on-error:`"));
                }
                if !st.get(i + 1).is_some_and(|n| is_verify(n)) {
                    out.push(format!("{file}: job `{name}`: {ACTION_REF} is not immediately followed by the `{VERIFY_STEP}` step with all its checks"));
                }
                continue;
            }
            if i > 1 && is_install(&st[i - 1]) && is_verify(step) {
                if guarded(step) {
                    out.push(format!("{file}: job `{name}`: the `{VERIFY_STEP}` step carries `if:` or `continue-on-error:`"));
                } else if !guarded(&st[i - 1]) {
                    installed = true;
                }
                continue;
            }
            if let Some(l) = step.lines().find(|l| invokes_sieve(l)) {
                if !installed {
                    out.push(format!("{file}: job `{name}` runs `{}` before (or without) a verified {ACTION_REF}", l.trim()));
                    break;
                }
            }
        }
    }
    out
}

/// A sieve release fetched anywhere but the action is a second pin.
fn second_downloads(file: &str, text: &str) -> Vec<String> {
    text.lines()
        .filter_map(code_of)
        .filter(|c| c.contains("pulseengine/sieve/releases") || (c.contains("release download") && c.contains("pulseengine/sieve")))
        .map(|c| format!("{file}: downloads sieve outside {ACTION}: `{c}`"))
        .collect()
}

fn check(action: &str, files: &[(String, String)]) -> Verdict {
    let (Some(ver), Some(x86), Some(arm)) = (
        quoted_after(action, "SIEVE_VERSION:"),
        quoted_after(action, "SIEVE_SHA256_X86_64:"),
        quoted_after(action, "SIEVE_SHA256_AARCH64:"),
    ) else {
        return Verdict::Refused(format!("{ACTION}: no SIEVE_VERSION/SIEVE_SHA256_X86_64/SIEVE_SHA256_AARCH64 -- the gate cannot answer"));
    };
    let mut lines = vec![format!("{ACTION}: sieve {ver}, x86_64 sha256={x86}, aarch64 sha256={arm}")];
    let mut failed = false;
    let mut fail = |lines: &mut Vec<String>, m: String| {
        lines.push(format!("FAIL: {m}"));
        failed = true;
    };
    if !is_exact_version(&ver) {
        fail(&mut lines, format!("{ACTION}: SIEVE_VERSION `{ver}` is not an exact X.Y.Z"));
    }
    if !has_code_line(action, &format!("name: Install sieve {ver} (pinned by sha256)")) {
        fail(&mut lines, format!("{ACTION}: no step named `Install sieve {ver} (pinned by sha256)`: the name and SIEVE_VERSION disagree"));
    }
    for (slot, v) in [("SIEVE_SHA256_X86_64", &x86), ("SIEVE_SHA256_AARCH64", &arm)] {
        if !is_sha256(v) {
            fail(&mut lines, format!("{ACTION}: {slot} `{v}` is not 64 lowercase hex digits"));
        }
    }
    if x86 == arm {
        fail(&mut lines, format!("{ACTION}: both architectures pin the same sha256, so at most one asset can match"));
    }
    let missing: Vec<&&str> = ACTION_LINES.iter().filter(|l| !has_code_line(action, l)).collect();
    for l in &missing {
        fail(&mut lines, format!("{ACTION}: missing the line `{l}`"));
    }
    lines.push(format!("{ACTION}: {}/{} required lines present", ACTION_LINES.len() - missing.len(), ACTION_LINES.len()));
    if guarded(action) {
        fail(&mut lines, format!("{ACTION}: a step carries `if:` or `continue-on-error:`"));
    }
    let mut running = 0;
    for (file, text) in files {
        if file != ACTION {
            for d in second_downloads(file, text) {
                fail(&mut lines, d);
            }
        }
        if !file.starts_with(".github/workflows/") {
            continue;
        }
        if jobs(text).iter().any(|(_, b)| b.lines().any(invokes_sieve)) {
            running += 1;
        }
        for b in unpinned_jobs(file, text) {
            fail(&mut lines, b);
        }
    }
    lines.push(format!("{} file(s) scanned, {running} workflow(s) run sieve", files.len()));
    if running == 0 {
        return Verdict::Refused("no workflow runs sieve -- the job sweep would be vacuous".into());
    }
    if failed { Verdict::Fail(lines) } else { Verdict::Pass(lines) }
}

/// Every workflow and every action file, as (repo-relative path, text).
fn files(repo: &Path) -> Option<Vec<(String, String)>> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(repo.join(".github/workflows")).ok()? {
        let p = e.ok()?.path();
        if matches!(p.extension().and_then(|x| x.to_str()), Some("yml" | "yaml")) {
            let name = format!(".github/workflows/{}", p.file_name()?.to_string_lossy());
            v.push((name, std::fs::read_to_string(&p).ok()?));
        }
    }
    if let Ok(rd) = std::fs::read_dir(repo.join(".github/actions")) {
        for e in rd.flatten() {
            for f in ["action.yml", "action.yaml"] {
                let p = e.path().join(f);
                if let Ok(t) = std::fs::read_to_string(&p) {
                    v.push((format!(".github/actions/{}/{f}", e.file_name().to_string_lossy()), t));
                }
            }
        }
    }
    v.sort();
    Some(v)
}

// ------------------------------------------- the verdict of record, per classifier job

/// The one classify call a classifier job runs, exactly: github-output for the
/// gated jobs plus the JSON verdict of record (sieve v0.4.0 `--record`, written
/// before the outputs; sieve exits 2 and writes no output if it cannot).
const RECORD_RUN: &str = r#"run: sieve classify --config sieve.toml --base "$BASE_SHA" --event "$EVENT_NAME" --format github-output --record "$RUNNER_TEMP/sieve-verdict.json""#;

/// The checkout's fetch-depth: 0 (all history) or at least 2.
fn fetch_depth_ok(job: &str) -> bool {
    job.lines()
        .filter_map(code_of)
        .find_map(|c| c.strip_prefix("fetch-depth:").map(|v| v.split_whitespace().next().unwrap_or("").to_string()))
        .and_then(|v| v.parse::<u32>().ok())
        .is_some_and(|d| d == 0 || d >= 2)
}

/// Keys the upload step must carry, exactly (outside comments); the artifact
/// name is checked apart, because it names the workflow and the job.
const UPLOAD_LINES: &[&str] = &[
    "path: ${{ runner.temp }}/sieve-verdict.json",
    "if-no-files-found: error",
    "retention-days: 90",
];

fn is_upload(step: &str) -> bool {
    step.lines().filter_map(code_of).any(|c| {
        c.strip_prefix("uses: actions/upload-artifact@").is_some_and(|r| {
            let sha = r.split_whitespace().next().unwrap_or("");
            sha.len() == 40 && sha.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    })
}

/// The step's `if:` value, if any.
fn step_if(step: &str) -> Option<String> {
    step.lines().filter_map(code_of).find_map(|c| c.strip_prefix("if:").map(|v| v.trim().to_string()))
}

/// The workflow file's stem: `.github/workflows/coverage.yml` -> `coverage`.
fn stem(file: &str) -> &str {
    let b = file.rsplit('/').next().unwrap_or(file);
    b.strip_suffix(".yml").or_else(|| b.strip_suffix(".yaml")).unwrap_or(b)
}

/// Every classifier job (a step running `sieve classify ... --format
/// github-output`) must run exactly RECORD_RUN, then, as its very next step, a
/// SHA-pinned upload of that record named sieve-verdict-<workflow>-<job>-<attempt>
/// with the path, if-no-files-found: error and 90-day retention, unguarded but
/// for `if: always()`.
fn record_problems(file: &str, wf: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (name, body) in jobs(wf) {
        let st = steps(&body);
        let Some(c) = st.iter().position(|s| s.lines().any(|l| invokes_sieve(l) && l.contains(" classify") && l.contains("--format github-output"))) else {
            continue;
        };
        if !st[c].lines().filter_map(code_of).any(|l| l == RECORD_RUN) {
            out.push(format!("{file}: job `{name}`: the classify step is not exactly `{RECORD_RUN}`"));
        }
        let Some(up) = st.get(c + 1).filter(|s| is_upload(s)) else {
            out.push(format!("{file}: job `{name}`: the classify step is not immediately followed by a SHA-pinned actions/upload-artifact step uploading the record"));
            continue;
        };
        let want_name = format!("name: sieve-verdict-{}-{name}-${{{{ github.run_attempt }}}}", stem(file));
        for l in std::iter::once(want_name.as_str()).chain(UPLOAD_LINES.iter().copied()) {
            if !up.lines().filter_map(code_of).any(|c| c == l) {
                out.push(format!("{file}: job `{name}`: the record upload lacks `{l}`"));
            }
        }
        if has_key(up, "continue-on-error:") {
            out.push(format!("{file}: job `{name}`: the record upload carries `continue-on-error:`"));
        }
        match step_if(up).as_deref() {
            None | Some("always()") | Some("${{ always() }}") => {}
            Some(other) => out.push(format!("{file}: job `{name}`: the record upload's `if: {other}` can skip it (only always())")),
        }
    }
    out
}
// ------------------------------------------------------- canonical texts
//
// A required line can be present and still neutralised (`|| true` after the
// sha256 check, the sha reassigned before it, `set +e`, a dropped `exit 1`),
// so the install is compared with the canonical text below from `runs:` on,
// line for line, after dropping blank lines, comment-only lines and trailing
// spaces. Only the version and the two sha256 values may differ ({VER}, {X86},
// {ARM}). Changing the install script means changing this gate in the same
// commit, on purpose. (The classify call itself is held exactly by RECORD_RUN
// and by check-workflows gate b.)

const ACTION_CANON: &str = r##"runs:
  using: composite
  steps:
    - name: Install sieve {VER} (pinned by sha256)
      shell: bash
      env:
        SIEVE_VERSION: "{VER}"
        SIEVE_SHA256_X86_64: "{X86}"
        SIEVE_SHA256_AARCH64: "{ARM}"
        PREFIX: ${{ runner.temp }}
      run: |
        set -euo pipefail
        os="$(uname -s)"
        arch="$(uname -m)"
        case "$os/$arch" in
          Linux/x86_64) target=x86_64-unknown-linux-musl; sha="$SIEVE_SHA256_X86_64" ;;
          Linux/aarch64|Linux/arm64) target=aarch64-unknown-linux-musl; sha="$SIEVE_SHA256_AARCH64" ;;
          *) echo "::error::no pinned sieve $SIEVE_VERSION for $os/$arch"; exit 1 ;;
        esac
        dir="$PREFIX/sieve-$SIEVE_VERSION"
        tgz="$PREFIX/sieve-$SIEVE_VERSION-$target.tar.gz"
        url="https://github.com/pulseengine/sieve/releases/download/v$SIEVE_VERSION/sieve-v$SIEVE_VERSION-$target.tar.gz"
        mkdir -p "$dir"
        curl -sSfL --retry 3 --retry-delay 5 -o "$tgz" "$url"
        echo "$sha  $tgz" | sha256sum -c -
        tar xzf "$tgz" -C "$dir"
        rm -f "$tgz"
        got="$("$dir/sieve" --version)"
        first="${got%%$'\n'*}"
        echo "$first"
        if [ "$first" != "sieve $SIEVE_VERSION" ]; then
          echo "::error::installed sieve reports '$first', not 'sieve $SIEVE_VERSION'"
          exit 1
        fi
        echo "$dir" >> "$GITHUB_PATH"
        echo "SIEVE_PINNED_DIR=$dir" >> "$GITHUB_ENV"
        echo "SIEVE_PINNED_VERSION=$SIEVE_VERSION" >> "$GITHUB_ENV"
"##;

/// The text without blank lines, comment-only lines and trailing whitespace.
fn significant(text: &str) -> Vec<String> {
    text.lines()
        .map(str::trim_end)
        .filter(|l| !l.trim().is_empty() && !l.trim_start().starts_with('#'))
        .map(str::to_string)
        .collect()
}

/// The text from the first line equal to `start` on, or None.
fn from_line<'a>(text: &'a str, start: &str) -> Option<&'a str> {
    let mut off = 0;
    for l in text.split_inclusive('\n') {
        if l.trim_end() == start {
            return Some(&text[off..]);
        }
        off += l.len();
    }
    None
}

/// None when `got` equals `want` (both significant); else the first difference.
fn diff_canon(what: &str, got: &str, want: &str) -> Option<String> {
    let (g, w) = (significant(got), significant(want));
    for i in 0..g.len().max(w.len()) {
        let (a, b) = (g.get(i), w.get(i));
        if a != b {
            return Some(format!(
                "{what} differs from the canonical text held in sieve-pin at significant line {}: got `{}`, want `{}`",
                i + 1,
                a.map(String::as_str).unwrap_or("<end>").trim(),
                b.map(String::as_str).unwrap_or("<end>").trim()
            ));
        }
    }
    None
}

/// Every way the action differs from its canonical form.
fn canonical_problems(action: &str) -> Vec<String> {
    let mut out = Vec::new();
    match (
        quoted_after(action, "SIEVE_VERSION:"),
        quoted_after(action, "SIEVE_SHA256_X86_64:"),
        quoted_after(action, "SIEVE_SHA256_AARCH64:"),
        from_line(action, "runs:"),
    ) {
        (Some(v), Some(x), Some(a), Some(runs)) => {
            let want = ACTION_CANON.replace("{VER}", &v).replace("{X86}", &x).replace("{ARM}", &a);
            out.extend(diff_canon(ACTION, runs, &want));
        }
        _ => out.push(format!("{ACTION}: no `runs:` block or no version/sha256 values to compare")),
    }
    out
}

// ------------------------------------------ the sieve steps of the required job

const KILL_CRITERIA: &str = ".github/workflows/kill-criteria.yml";
const REQUIRED_JOB: &str = "every required context can actually report";
/// The steps the required job must keep, each exactly this `run:` and unguarded.
const REQUIRED_RUNS: &[&str] = &[
    "run: sieve check-workflows --config sieve.toml",
    "run: sieve check-graph --config sieve.toml",
    "run: cargo xtask check sieve-pin --self-test",
    "run: cargo xtask check sieve-pin",
];

/// Why the required job no longer runs sieve's gates as it must.
fn required_job_problems(kc: Option<&str>) -> Vec<String> {
    let Some(kc) = kc else { return vec![format!("{KILL_CRITERIA} is missing")] };
    let want_name = format!("name: \"{REQUIRED_JOB}\"");
    let Some((id, body)) = jobs(kc).into_iter().find(|(_, b)| steps(b)[0].lines().filter_map(code_of).any(|c| c == want_name)) else {
        return vec![format!("{KILL_CRITERIA}: no job named \"{REQUIRED_JOB}\"")];
    };
    let st = steps(&body);
    let mut out = Vec::new();
    if guarded(&st[0]) {
        out.push(format!("{KILL_CRITERIA}: job `{id}` carries a job-level `if:` or `continue-on-error:`"));
    }
    for r in REQUIRED_RUNS {
        let hits: Vec<&String> = st[1..].iter().filter(|s| s.lines().filter_map(code_of).any(|c| c == *r)).collect();
        match hits.as_slice() {
            [] => out.push(format!("{KILL_CRITERIA}: job `{id}` has no step `{r}` (exactly, nothing appended)")),
            [s] if guarded(s) => out.push(format!("{KILL_CRITERIA}: job `{id}`: the step `{r}` carries `if:` or `continue-on-error:`")),
            _ => {}
        }
    }
    out
}
// ------------------------------------------ leaf-workflow scoping stays on

const SIEVE_TOML: &str = "sieve.toml";
const LEAF_LINE: &str = "scope_leaf_edits = true";

/// Why sieve.toml no longer turns leaf-workflow scoping on (sieve REQ-030).
/// Dropping the line is safe (every .github/ edit runs everything again) but
/// silently gives the saving back, so it must be a visible decision: exactly one
/// code line `scope_leaf_edits = true` (a trailing comment allowed), inside
/// `[workflows]`, and no other `scope_leaf_edits` key anywhere.
fn leaf_scope_problems(toml: Option<&str>) -> Vec<String> {
    let Some(toml) = toml else { return vec![format!("{SIEVE_TOML} is missing")] };
    let mut section = String::new();
    let (mut good, mut other) = (0, Vec::new());
    for line in toml.lines() {
        let code = line.split('#').next().unwrap_or("").trim();
        if code.starts_with('[') {
            section = code.to_string();
            continue;
        }
        let key = code.split('=').next().unwrap_or("").trim();
        if key == "scope_leaf_edits" {
            let norm: String = code.split_whitespace().collect::<Vec<_>>().join(" ");
            if section == "[workflows]" && norm == LEAF_LINE {
                good += 1;
            } else {
                other.push(format!("`{code}` under {}", if section.is_empty() { "the top level" } else { &section }));
            }
        }
    }
    let mut out: Vec<String> = other.into_iter().map(|o| format!("{SIEVE_TOML}: {o}: only `{LEAF_LINE}` under [workflows] is allowed")).collect();
    if good != 1 {
        out.push(format!("{SIEVE_TOML}: [workflows] has {good} `{LEAF_LINE}` line(s), want exactly 1 (turning leaf scoping off is an owner decision: change this gate in the same commit)"));
    }
    out
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let Ok(action) = std::fs::read_to_string(repo.join(ACTION)) else {
        return Verdict::Refused(format!("cannot read {ACTION} -- no pinned install exists"));
    };
    let Some(fs) = files(repo) else {
        return Verdict::Refused("cannot read .github/workflows".into());
    };
    let kc = std::fs::read_to_string(repo.join(KILL_CRITERIA)).ok();
    let mut vp = canonical_problems(&action);
    let mut recorders = 0;
    for (file, text) in fs.iter().filter(|(f, _)| f.starts_with(".github/workflows/")) {
        recorders += jobs(text).iter().filter(|(_, b)| b.lines().any(|l| invokes_sieve(l) && l.contains(" classify") && l.contains("--format github-output"))).count();
        vp.extend(record_problems(file, text));
    }
    if recorders == 0 {
        vp.push("no classifier job runs `sieve classify --format github-output`: nothing records a verdict".into());
    }
    vp.extend(required_job_problems(kc.as_deref()));
    vp.extend(leaf_scope_problems(std::fs::read_to_string(repo.join(SIEVE_TOML)).ok().as_deref()));
    match check(&action, &fs) {
        Verdict::Pass(mut l) if vp.is_empty() => {
            l.push(format!("{ACTION} equals its canonical text; {recorders} classifier job(s) record and upload their verdict"));
            l.push(format!("{KILL_CRITERIA}: \"{REQUIRED_JOB}\" runs sieve check-workflows, check-graph and sieve-pin (self-test + real), unguarded"));
            l.push(format!("{SIEVE_TOML}: [workflows] keeps `{LEAF_LINE}`"));
            Verdict::Pass(l)
        }
        Verdict::Pass(mut l) | Verdict::Fail(mut l) => {
            l.extend(vp.into_iter().map(|p| format!("FAIL: {p}")));
            Verdict::Fail(l)
        }
        r => r,
    }
}

const SHA_X: &str = "aae674edf2c454ae5047409f571160f4615ef67f901b2e55628e8b756fb69b40";
const SHA_A: &str = "b8e8cd3167ae3adcbecdcb7081fedf66430ff671d97537597890c31a01fc3e0d";

fn action_text(ver: &str, step_ver: &str, x86: &str, arm: &str) -> String {
    let mut s = format!(
        "runs:\n  using: composite\n  steps:\n    - name: Install sieve {step_ver} (pinned by sha256)\n      env:\n        SIEVE_VERSION: \"{ver}\"\n        SIEVE_SHA256_X86_64: \"{x86}\"\n        SIEVE_SHA256_AARCH64: \"{arm}\"\n      run: |\n"
    );
    for l in ACTION_LINES {
        s.push_str("        ");
        s.push_str(l);
        s.push('\n');
    }
    s
}

fn install_and_verify() -> String {
    let mut s = format!("      - uses: {ACTION_REF}\n      - name: {VERIFY_STEP}\n        run: |\n");
    for l in VERIFY_LINES {
        s.push_str("          ");
        s.push_str(l);
        s.push('\n');
    }
    s
}

const CHECKOUT: &str = "      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1\n        with:\n          fetch-depth: 0\n";
const CLASSIFY: &str = "      - id: sieve\n        run: sieve classify --config sieve.toml --base \"$BASE_SHA\" --event \"$EVENT_NAME\" --format github-output\n";

fn wf(steps: &str) -> Vec<(String, String)> {
    vec![(".github/workflows/w.yml".into(), format!("name: w\non: pull_request\njobs:\n  classify:\n    steps:\n{steps}"))]
}

pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };
    let pass = |v: &Verdict| matches!(v, Verdict::Pass(_));
    let fail = |v: &Verdict| matches!(v, Verdict::Fail(_));
    let good = action_text("0.3.0", "0.3.0", SHA_X, SHA_A);
    let iv = install_and_verify();
    let pinned = format!("{CHECKOUT}{iv}{CLASSIFY}");

    ck("a self-consistent action + a pinned classify job pass", pass(&check(&good, &wf(&pinned))));
    ck("a version that is not exact X.Y.Z fails", fail(&check(&action_text("0.3", "0.3", SHA_X, SHA_A), &wf(&pinned))));
    ck("a step name naming another version fails", fail(&check(&action_text("0.3.0", "0.2.1", SHA_X, SHA_A), &wf(&pinned))));
    ck("an uppercase sha256 fails", fail(&check(&action_text("0.3.0", "0.3.0", &SHA_X.to_uppercase(), SHA_A), &wf(&pinned))));
    ck("a short sha256 fails", fail(&check(&action_text("0.3.0", "0.3.0", SHA_X, &SHA_A[..63]), &wf(&pinned))));
    ck("the same sha256 in both slots fails", fail(&check(&action_text("0.3.0", "0.3.0", SHA_X, SHA_X), &wf(&pinned))));
    for l in ACTION_LINES {
        let gutted = good.replacen(&format!("        {l}\n"), "", 1);
        ck(&format!("an action without `{l}` fails"), fail(&check(&gutted, &wf(&pinned))));
    }
    ck("an action line that is only a comment does not count",
       fail(&check(&good.replacen("        echo \"$sha", "        # echo \"$sha", 1), &wf(&pinned))));
    ck("`if:` in the action fails",
       fail(&check(&good.replacen("      run: |\n", "      if: false\n      run: |\n", 1), &wf(&pinned))));
    ck("a missing sha256 slot is refused, not passed",
       matches!(check(&good.replacen("SIEVE_SHA256_AARCH64:", "X:", 1), &wf(&pinned)), Verdict::Refused(_)));

    ck("a classify --base job on a depth-1 checkout fails", fail(&check(&good, &wf(&pinned.replacen("fetch-depth: 0", "fetch-depth: 1", 1)))));
    ck("fetch-depth: 0 with a trailing comment passes",
       pass(&check(&good, &wf(&pinned.replacen("fetch-depth: 0", "fetch-depth: 0        # sieve diffs against the PR base", 1)))));
    ck("a classify --base job with no fetch-depth fails",
       fail(&check(&good, &wf(&pinned.replacen("        with:\n          fetch-depth: 0\n", "", 1)))));
    ck("a job running sieve classify with no install fails", fail(&check(&good, &wf(CLASSIFY))));
    ck("an install AFTER sieve classify does not count", fail(&check(&good, &wf(&format!("{CLASSIFY}{iv}")))));
    ck("an install not followed by the verify step fails",
       fail(&check(&good, &wf(&format!("      - uses: {ACTION_REF}\n{CLASSIFY}")))));
    for (i, l) in VERIFY_LINES.iter().enumerate() {
        let gutted = pinned.replacen(&format!("          {l}\n"), "", 1);
        ck(&format!("a verify step missing check {i} (`{l}`) fails"), fail(&check(&good, &wf(&gutted))));
    }
    ck("`continue-on-error:` on the install step fails",
       fail(&check(&good, &wf(&pinned.replacen("setup-sieve\n", "setup-sieve\n        continue-on-error: true\n", 1)))));
    ck("`if:` on the verify step fails",
       fail(&check(&good, &wf(&pinned.replacen("        run: |\n", "        if: false\n        run: |\n", 1)))));
    ck("a path-qualified sieve check-workflows without an install fails",
       fail(&check(&good, &wf("      - run: /tmp/x/sieve check-workflows --config sieve.toml\n"))));
    ck("temper's gate-probe action without an install fails",
       fail(&check(&good, &wf(&format!("      - uses: {GATE_PROBE_ACTION}@0000000000000000000000000000000000000000\n")))));
    ck("a second sieve download in a workflow fails",
       fail(&check(&good, &wf(&format!("{pinned}      - run: curl -sSfLO https://github.com/pulseengine/sieve/releases/download/v0.2.1/x.tar.gz\n")))));
    let two = format!("jobs:\n  a:\n    steps:\n{pinned}  b:\n    steps:\n{CHECKOUT}{CLASSIFY}");
    let bad = unpinned_jobs("w.yml", &two);
    ck("an install in job a does not cover job b", bad.len() == 1 && bad[0].contains("job `b`"));
    ck("`command -v sieve` is not a use", !invokes_sieve("          got=\"$(command -v sieve || true)\""));
    ck("a comment naming sieve classify is not a use", !invokes_sieve("    # sieve classify decides"));
    ck("no workflow running sieve is refused, not passed",
       matches!(check(&good, &wf("      - run: echo hi\n")), Verdict::Refused(_)));

    // The verdict of record, per classifier job (sieve v0.4.0 --record).
    let rj = format!(
        "jobs:\n  changes:\n    steps:\n{CHECKOUT}{}      - id: sieve\n        {RECORD_RUN}\n      - name: Upload the verdict of record\n        if: always()\n        uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2\n        with:\n          name: sieve-verdict-coverage-changes-${{{{ github.run_attempt }}}}\n          path: ${{{{ runner.temp }}}}/sieve-verdict.json\n          if-no-files-found: error\n          retention-days: 90\n",
        install_and_verify()
    );
    let rf = ".github/workflows/coverage.yml";
    let rok = |w: &str| record_problems(rf, w).is_empty();
    ck("a classifier job that records and uploads its verdict passes", rok(&rj));
    ck("a classify call without --record fails", !rok(&rj.replacen(" --record \"$RUNNER_TEMP/sieve-verdict.json\"", "", 1)));
    ck("`|| true` on the classify call fails", !rok(&rj.replacen("sieve-verdict.json\"\n      - name: Upload", "sieve-verdict.json\" || true\n      - name: Upload", 1)));
    ck("`--event push` hardcoded fails", !rok(&rj.replacen("--event \"$EVENT_NAME\"", "--event push", 1)));
    let up_at = rj.find("      - name: Upload the verdict of record").unwrap();
    ck("PLANT: the upload step removed fails", !rok(&rj[..up_at]));
    ck("PLANT: a step between classify and upload (could overwrite the record) fails",
       !rok(&rj.replacen("      - name: Upload the verdict of record\n", "      - run: echo '{}' > \"$RUNNER_TEMP/sieve-verdict.json\"\n      - name: Upload the verdict of record\n", 1)));
    ck("PLANT: the upload path pointing elsewhere fails",
       !rok(&rj.replacen("path: ${{ runner.temp }}/sieve-verdict.json", "path: ${{ runner.temp }}/nope.json", 1)));
    ck("if-no-files-found: warn fails", !rok(&rj.replacen("if-no-files-found: error", "if-no-files-found: warn", 1)));
    ck("a retention other than 90 days fails", !rok(&rj.replacen("retention-days: 90", "retention-days: 30", 1)));
    ck("an artifact name not naming the workflow and job fails (names collide within a run)",
       !rok(&rj.replacen("sieve-verdict-coverage-changes-", "sieve-verdict-", 1)));
    ck("the same job in another workflow file needs that file's name",
       !record_problems(".github/workflows/llvm-lto.yml", &rj).is_empty());
    ck("an upload-artifact pinned to a tag fails", !rok(&rj.replacen("upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9", "upload-artifact@v7", 1)));
    ck("`continue-on-error:` on the upload fails", !rok(&rj.replacen("        if: always()\n", "        if: always()\n        continue-on-error: true\n", 1)));
    ck("an upload `if:` other than always() fails", !rok(&rj.replacen("        if: always()\n", "        if: success()\n", 1)));
    ck("a job that runs no classify call is not a recorder", record_problems(rf, "jobs:\n  x:\n    steps:\n      - run: echo hi\n").is_empty());

    // Canonical action: a required line present but neutralised must fail.
    let act = format!("name: setup-sieve\ndescription: x\n\n{}", ACTION_CANON.replace("{VER}", "0.4.0").replace("{X86}", SHA_X).replace("{ARM}", SHA_A));
    let canon_ok = |a: &str| canonical_problems(a).is_empty();
    ck("the canonical action passes", canon_ok(&act));
    ck("comments and blank lines may change", canon_ok(&act.replacen("runs:\n", "runs:\n  # a note\n\n", 1)));
    ck("another version and other sha256 values pass the canonical comparison",
       canon_ok(&act.replace("0.4.0", "0.5.0").replace(SHA_X, &"1".repeat(64))));
    let act_bad = |w: &str| !canon_ok(w);
    ck("A1: `|| true` on the sha256 check fails",
       act_bad(&act.replacen("| sha256sum -c -", "| sha256sum -c - || true", 1)));
    ck("A3: the sha reassigned before the check fails",
       act_bad(&act.replacen("        echo \"$sha  $tgz\"", "        sha=\"$(sha256sum \"$tgz\" | cut -d\" \" -f1)\"\n        echo \"$sha  $tgz\"", 1)));
    ck("A7: `set +e` instead of `set -euo pipefail` fails", act_bad(&act.replacen("set -euo pipefail", "set +e", 1)));
    ck("A2: the `exit 1` after the version mismatch dropped fails",
       act_bad(&{ let i = act.find("not 'sieve $SIEVE_VERSION'").unwrap(); let j = i + act[i..].find("exit 1\n").unwrap(); format!("{}{}", &act[..j], &act[j + 7..]) }));
    ck("A5: curl fetching another url fails", act_bad(&act.replacen("-o \"$tgz\" \"$url\"", "-o \"$tgz\" \"https://example.com/x\"", 1)));
    ck("A6: a second GITHUB_PATH entry fails", act_bad(&act.replacen("        echo \"$dir\" >> \"$GITHUB_PATH\"\n", "        echo \"$dir\" >> \"$GITHUB_PATH\"\n        echo /tmp >> \"$GITHUB_PATH\"\n", 1)));

    // The required job's four sieve steps.
    let kc = "jobs:\n  required-contexts:\n    name: \"every required context can actually report\"\n    runs-on: x\n    steps:\n      - uses: actions/checkout@x\n      - name: a\n        run: sieve check-workflows --config sieve.toml\n      - name: b\n        run: sieve check-graph --config sieve.toml\n      - name: c\n        run: cargo xtask check sieve-pin --self-test\n      - name: d\n        run: cargo xtask check sieve-pin\n";
    let kc_ok = |t: &str| required_job_problems(Some(t)).is_empty();
    ck("the required job with its four sieve steps passes", kc_ok(kc));
    ck("K1: `|| true` on check-workflows fails", !kc_ok(&kc.replacen("--config sieve.toml\n      - name: b", "--config sieve.toml || true\n      - name: b", 1)));
    ck("K2: `|| true` on sieve-pin fails", !kc_ok(&kc.replacen("check sieve-pin\n", "check sieve-pin || true\n", 1)));
    ck("K3: the check-graph step removed fails", !kc_ok(&kc.replacen("      - name: b\n        run: sieve check-graph --config sieve.toml\n", "", 1)));
    ck("K4: the check-workflows step removed fails", !kc_ok(&kc.replacen("      - name: a\n        run: sieve check-workflows --config sieve.toml\n", "", 1)));
    ck("`continue-on-error:` on a sieve step fails", !kc_ok(&kc.replacen("      - name: c\n", "      - name: c\n        continue-on-error: true\n", 1)));
    ck("the steps in another job do not count", !kc_ok(&kc.replacen("every required context can actually report", "something else", 1)));

    // Leaf-workflow scoping stays on (sieve REQ-030).
    let toml = "schema = \"sieve.config.v1\"\n\n[workflows]\nrequired_contexts = \"x\"\n# comment\nscope_leaf_edits = true\n\n[workflows.coe_exempt]\n\"a\" = \"b\"\n";
    let leaf_ok = |t: &str| leaf_scope_problems(Some(t)).is_empty();
    ck("[workflows] with scope_leaf_edits = true passes", leaf_ok(toml));
    ck("a trailing comment on the line passes", leaf_ok(&toml.replacen("scope_leaf_edits = true\n", "scope_leaf_edits = true  # REQ-030\n", 1)));
    ck("PLANT L1: the line dropped fails", !leaf_ok(&toml.replacen("scope_leaf_edits = true\n", "", 1)));
    ck("PLANT L2: scope_leaf_edits = false fails", !leaf_ok(&toml.replacen("= true\n", "= false\n", 1)));
    ck("PLANT L3: the line commented out fails", !leaf_ok(&toml.replacen("scope_leaf_edits = true\n", "# scope_leaf_edits = true\n", 1)));
    ck("PLANT L4: the line moved under [workflows.coe_exempt] fails",
       !leaf_ok(&format!("{}scope_leaf_edits = true\n", toml.replacen("scope_leaf_edits = true\n", "", 1))));
    ck("PLANT L5: a second key overriding it later fails", !leaf_ok(&toml.replacen("scope_leaf_edits = true\n", "scope_leaf_edits = true\nscope_leaf_edits = false\n", 1)));
    ck("a missing sieve.toml fails", !leaf_scope_problems(None).is_empty());
    if broken { Verdict::Fail(lines) } else { Verdict::Pass(lines) }
}

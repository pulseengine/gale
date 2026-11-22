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
//! The repo-wide verdict record (.github/workflows/sieve-verdict.yml) must keep,
//! in order and unguarded, the `sieve classify --format json` step writing the
//! record, the `jq -e -f tools/ci/sieve-verdict.jq` validation and a SHA-pinned
//! upload-artifact step (name sieve-verdict-<run_attempt>, the same path,
//! if-no-files-found: error, retention-days: 90, `if:` only always()), with no
//! job-level `if:`. TODO(sieve#25): drops out when one classify call records.
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

// ------------------------------------------------ the repo-wide verdict record

const VERDICT_WF: &str = ".github/workflows/sieve-verdict.yml";
const VERDICT_JQ: &str = "tools/ci/sieve-verdict.jq";
const VERDICT_FILE: &str = "\"$RUNNER_TEMP/sieve-verdict.json\"";

/// Lines the validation filter must carry: the schema, the per-class
/// completeness test against `sieve graph`, and the boolean test.
const JQ_LINES: &[&str] = &[
    r#"and (.schema == "sieve.classify.v1")"#,
    r#"and (($g[0].classes | keys) - (.classes | keys) | length == 0)"#,
    r#"and ([.classes[] | type == "boolean"] | all)"#,
];

/// The classify step's push branch: sieve's observe form (the pushed commit's
/// own diff as a pull request against HEAD~1), failing loudly without HEAD~1.
const PUSH_LINES: &[&str] = &[
    r#"if [ "$EVENT_NAME" = push ]; then"#,
    r#"ev=pull_request"#,
    r#"base="$(git rev-parse --verify --quiet 'HEAD~1^{commit}')" || {"#,
    r#"exit 1"#,
];

/// The validation step's tool check: no jq is a loud failure, not a skip.
const JQ_PRESENT: &str = "command -v jq >/dev/null || {";

/// The checkout's fetch-depth: 0 (all history) or at least 2, so HEAD~1 exists.
fn fetch_depth_ok(job: &str) -> bool {
    job.lines()
        .filter_map(code_of)
        .find_map(|c| c.strip_prefix("fetch-depth:").map(|v| v.trim().to_string()))
        .and_then(|v| v.parse::<u32>().ok())
        .is_some_and(|d| d == 0 || d >= 2)
}
/// Keys the upload step must carry, exactly (outside comments).
const UPLOAD_LINES: &[&str] = &[
    "name: sieve-verdict-${{ github.run_attempt }}",
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

/// Why the verdict record is missing or can go missing silently.
/// `wf`/`jq`: the texts of VERDICT_WF and VERDICT_JQ (None when absent).
fn verdict_problems(wf: Option<&str>, jq: Option<&str>) -> Vec<String> {
    let mut out = Vec::new();
    let Some(wf) = wf else {
        return vec![format!("{VERDICT_WF} is missing: no run records sieve's verdict")];
    };
    match jq {
        None => out.push(format!("{VERDICT_JQ} is missing: the verdict is not validated")),
        Some(t) => {
            for l in JQ_LINES {
                if !has_code_line(t, l) {
                    out.push(format!("{VERDICT_JQ}: missing the line `{l}`"));
                }
            }
        }
    }
    let records: Vec<(String, String)> = jobs(wf)
        .into_iter()
        .filter(|(_, b)| b.lines().any(|l| invokes_sieve(l) && l.contains("--format json")))
        .collect();
    if records.len() != 1 {
        out.push(format!("{VERDICT_WF}: {} jobs run `sieve classify --format json`; exactly one must", records.len()));
        return out;
    }
    let (name, body) = &records[0];
    if !fetch_depth_ok(body) {
        out.push(format!("{VERDICT_WF}: job `{name}` must check out with fetch-depth 0 or >= 2 (the push record needs HEAD~1)"));
    }
    if has_key(&steps(body)[0], "if:") {
        out.push(format!("{VERDICT_WF}: job `{name}` has a job-level `if:`; the record must be made on every run"));
    }
    let st = steps(body);
    let pos = |f: &dyn Fn(&str) -> bool| st.iter().position(|s| f(s));
    let classify = pos(&|s: &str| s.lines().any(|l| invokes_sieve(l) && l.contains("--format json") && l.contains(&format!("> {VERDICT_FILE}"))));
    let validate = pos(&|s: &str| has_code_line(s, "jq -e --slurpfile g") && has_code_line(s, &format!("-f {VERDICT_JQ} {VERDICT_FILE}")));
    let upload = pos(&|s: &str| is_upload(s));
    match (classify, validate, upload) {
        (None, _, _) => out.push(format!("{VERDICT_WF}: no step writes `sieve classify ... --format json` to {VERDICT_FILE}")),
        (_, None, _) => out.push(format!("{VERDICT_WF}: no step validates {VERDICT_FILE} with `jq -e ... -f {VERDICT_JQ}`")),
        (_, _, None) => out.push(format!("{VERDICT_WF}: no SHA-pinned actions/upload-artifact step uploads the verdict")),
        (Some(c), Some(v), Some(u)) => {
            if !(c < v && v < u) {
                out.push(format!("{VERDICT_WF}: the steps must run classify, then validate, then upload"));
            }
            let up = &st[u];
            for l in UPLOAD_LINES {
                if !up.lines().filter_map(code_of).any(|c| c == *l) {
                    out.push(format!("{VERDICT_WF}: the upload step lacks `{l}`"));
                }
            }
            if has_key(up, "continue-on-error:") {
                out.push(format!("{VERDICT_WF}: the upload step carries `continue-on-error:`"));
            }
            match step_if(up).as_deref() {
                None | Some("always()") | Some("${{ always() }}") => {}
                Some(other) => out.push(format!("{VERDICT_WF}: the upload step's `if: {other}` can skip it (only always() is allowed)")),
            }
            for l in PUSH_LINES {
                if !has_code_line(&st[c], l) {
                    out.push(format!("{VERDICT_WF}: the classify step lacks the push observe-form line `{l}`"));
                }
            }
            if !has_code_line(&st[c], "--head HEAD") {
                out.push(format!("{VERDICT_WF}: the classify call lacks `--head HEAD`"));
            }
            if !has_code_line(&st[v], JQ_PRESENT) {
                out.push(format!("{VERDICT_WF}: the validation step does not fail loudly when jq is missing (`{JQ_PRESENT}`)"));
            }
            for (what, i) in [("classify", c), ("validation", v)] {
                if guarded(&st[i]) {
                    out.push(format!("{VERDICT_WF}: the {what} step carries `if:` or `continue-on-error:`"));
                }
            }
        }
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
    let wf = std::fs::read_to_string(repo.join(VERDICT_WF)).ok();
    let jq = std::fs::read_to_string(repo.join(VERDICT_JQ)).ok();
    let vp = verdict_problems(wf.as_deref(), jq.as_deref());
    match check(&action, &fs) {
        Verdict::Pass(mut l) if vp.is_empty() => {
            l.push(format!("{VERDICT_WF}: the verdict is recorded, validated and uploaded on every run"));
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

const CLASSIFY: &str = "      - id: sieve\n        run: sieve classify --config sieve.toml --base \"$BASE_SHA\" --event \"$EVENT_NAME\" --format github-output\n";

fn wf(steps: &str) -> Vec<(String, String)> {
    vec![(".github/workflows/w.yml".into(), format!("name: w\non: pull_request\njobs:\n  classify:\n    steps:\n{steps}"))]
}

/// The verdict job's steps as the workflow carries them (self-test fixture).
const VERDICT_GOOD: &str = r#"name: sieve verdict
on:
  pull_request:
jobs:
  verdict:
    name: "record sieve's verdict"
    runs-on: x
    timeout-minutes: 10
    steps:
      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7
        with:
          fetch-depth: 0
      - uses: ./.github/actions/setup-sieve
      - name: Pinned sieve is the one on PATH
        run: |
          set -euo pipefail
          got="$(command -v sieve || true)"
          if [ "$got" != "$SIEVE_PINNED_DIR/sieve" ]; then
            exit 1
          fi
      - name: Classify this change (JSON record)
        run: |
          set -euo pipefail
          ev="$EVENT_NAME"
          base="$BASE_SHA"
          if [ "$EVENT_NAME" = push ]; then
            ev=pull_request
            base="$(git rev-parse --verify --quiet 'HEAD~1^{commit}')" || {
              echo "::error::no HEAD~1"
              exit 1
            }
          fi
          sieve classify --config sieve.toml --base "$base" --head HEAD --event "$ev" --format json --explain > "$RUNNER_TEMP/sieve-verdict.json"
      - name: The verdict is a complete sieve.classify.v1 record
        run: |
          set -euo pipefail
          command -v jq >/dev/null || { echo "::error::no jq"; exit 1; }
          sieve graph --config sieve.toml --format json > "$RUNNER_TEMP/sieve-graph.json"
          jq -e --slurpfile g "$RUNNER_TEMP/sieve-graph.json" -f tools/ci/sieve-verdict.jq "$RUNNER_TEMP/sieve-verdict.json"
      - name: Upload the verdict
        if: always()
        uses: actions/upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9 # v7.0.2
        with:
          name: sieve-verdict-${{ github.run_attempt }}
          path: ${{ runner.temp }}/sieve-verdict.json
          if-no-files-found: error
          retention-days: 90
"#;
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
    let pinned = format!("{iv}{CLASSIFY}");

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
    let two = format!("jobs:\n  a:\n    steps:\n{pinned}  b:\n    steps:\n{CLASSIFY}");
    let bad = unpinned_jobs("w.yml", &two);
    ck("an install in job a does not cover job b", bad.len() == 1 && bad[0].contains("job `b`"));
    ck("`command -v sieve` is not a use", !invokes_sieve("          got=\"$(command -v sieve || true)\""));
    ck("a comment naming sieve classify is not a use", !invokes_sieve("    # sieve classify decides"));
    ck("no workflow running sieve is refused, not passed",
       matches!(check(&good, &wf("      - run: echo hi\n")), Verdict::Refused(_)));

    // The repo-wide verdict record (sieve-verdict.yml + tools/ci/sieve-verdict.jq).
    let jq_good: String = std::iter::once("(type == \"object\")").chain(JQ_LINES.iter().copied()).collect::<Vec<_>>().join("\n");
    let vwf = VERDICT_GOOD.to_string();
    let ok = |w: &str| verdict_problems(Some(w), Some(&jq_good)).is_empty();
    let bad = |w: &str| !verdict_problems(Some(w), Some(&jq_good)).is_empty();
    ck("the verdict workflow as committed passes", ok(&vwf));
    let no_upload = vwf[..vwf.find("      - name: Upload the verdict").unwrap()].to_string();
    ck("PLANT 1: the upload step removed fails", bad(&no_upload));
    ck("PLANT 2: the upload path pointing elsewhere fails",
       bad(&vwf.replacen("path: ${{ runner.temp }}/sieve-verdict.json", "path: ${{ runner.temp }}/nope.json", 1)));
    ck("if-no-files-found: warn fails", bad(&vwf.replacen("if-no-files-found: error", "if-no-files-found: warn", 1)));
    ck("a retention other than 90 days fails", bad(&vwf.replacen("retention-days: 90", "retention-days: 30", 1)));
    ck("another artifact name fails", bad(&vwf.replacen("name: sieve-verdict-${{ github.run_attempt }}", "name: verdict", 1)));
    ck("an upload-artifact pinned to a tag fails", bad(&vwf.replacen("upload-artifact@cf430e030ddbb5b0abf93d22962f4752f3646cd9", "upload-artifact@v7", 1)));
    ck("`continue-on-error:` on the upload fails", bad(&vwf.replacen("        if: always()\n", "        if: always()\n        continue-on-error: true\n", 1)));
    ck("an upload `if:` other than always() fails", bad(&vwf.replacen("        if: always()\n", "        if: success()\n", 1)));
    ck("PLANT 3 (static): the jq validation step removed fails",
       bad(&vwf.replacen("-f tools/ci/sieve-verdict.jq", "-f /dev/null", 1)));
    ck("the classify record written to another file fails",
       bad(&vwf.replacen("--explain > \"$RUNNER_TEMP/sieve-verdict.json\"", "--explain > out.json", 1)));
    ck("PLANT 4: a job-level `if:` on the verdict job fails",
       bad(&vwf.replacen("    timeout-minutes: 10\n", "    timeout-minutes: 10\n    if: github.event_name == 'push'\n", 1)));
    ck("a push record without the observe form fails", bad(&vwf.replacen("            ev=pull_request\n", "", 1)));
    ck("a push branch that does not fail without HEAD~1 fails",
       bad(&vwf.replacen("base=\"$(git rev-parse --verify --quiet 'HEAD~1^{commit}')\" || {", "base=\"$(git rev-parse HEAD~1 || true)\"; {", 1)));
    ck("a classify call without --head HEAD fails", bad(&vwf.replacen(" --head HEAD", "", 1)));
    ck("a shallow (depth 1) checkout fails", bad(&vwf.replacen("fetch-depth: 0", "fetch-depth: 1", 1)));
    ck("a checkout with no fetch-depth fails", bad(&vwf.replacen("        with:\n          fetch-depth: 0\n", "", 1)));
    ck("fetch-depth: 2 passes", ok(&vwf.replacen("fetch-depth: 0", "fetch-depth: 2", 1)));
    ck("a validation without the jq presence check fails",
       bad(&vwf.replacen("          command -v jq >/dev/null || { echo \"::error::no jq\"; exit 1; }\n", "", 1)));
    ck("a missing workflow fails", !verdict_problems(None, Some(&jq_good)).is_empty());
    ck("a missing jq filter fails", !verdict_problems(Some(&vwf), None).is_empty());
    for l in JQ_LINES {
        let gutted = jq_good.replacen(l, "", 1);
        ck(&format!("a jq filter without `{l}` fails"), !verdict_problems(Some(&vwf), Some(&gutted)).is_empty());
    }
    if broken { Verdict::Fail(lines) } else { Verdict::Pass(lines) }
}

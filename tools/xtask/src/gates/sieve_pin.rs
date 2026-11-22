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

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let Ok(action) = std::fs::read_to_string(repo.join(ACTION)) else {
        return Verdict::Refused(format!("cannot read {ACTION} -- no pinned install exists"));
    };
    let Some(fs) = files(repo) else {
        return Verdict::Refused("cannot read .github/workflows".into());
    };
    check(&action, &fs)
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
    if broken { Verdict::Fail(lines) } else { Verdict::Pass(lines) }
}

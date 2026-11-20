//! Every Renode gale runs is the one pinned Renode.
//!
//! Before this gate, gale ran three different Renodes and nothing said so: the
//! Bazel module downloaded 1.15.3, every container workflow ran whatever the
//! ci-base image bundled (1.16.0), and local runs used 1.16.1. Each was green
//! about a different simulator. The move to 1.17.0 showed the cost: 1.17 changed
//! how a SemihostingUart registers and started honouring semihosting SYS_EXIT,
//! which turned a gate's "200,000,000 instructions" into what it always was, the
//! 2 s budget. A version difference is a behaviour difference.
//!
//! So the pin lives in two places that must agree byte for byte, and every
//! workflow job that runs Renode must install it:
//!   1. os/gust/renode-test/MODULE.bazel: rules_renode's download_portable url + sha256;
//!   2. .github/actions/setup-renode/action.yml: RENODE_URL + RENODE_SHA256 + RENODE_VERSION,
//!      which the container workflows use instead of the image's bundled Renode.
//!
//! Each consuming job must run the action and then, as its very next step, the
//! "Pinned Renode is the one on PATH" step, which fails unless `renode` and
//! `renode-test` resolve inside the pinned directory and `renode --version`
//! reports the pinned version. The action cannot check that itself: its
//! GITHUB_PATH write only reaches later steps. The gate also fails a
//! path-qualified renode outside the pinned directory (bar the allowlist below),
//! a PATH assignment naming a renode directory, an `if:`/`continue-on-error:` on
//! either step, and an action that lost its sha256, version or PATH lines.
//!
//! What it cannot see: a job that reaches Renode by a name other than `renode`
//! or `renode-test` (a wrapper script, a variable holding the path), PATH changes
//! made inside a script the workflow calls, and the Renode a developer has
//! locally. It reads YAML by indentation, not with a YAML parser.

use crate::verdict::Verdict;
use std::path::Path;

pub const NAME: &str = "renode-pin";

const MODULE: &str = "os/gust/renode-test/MODULE.bazel";
const ACTION: &str = ".github/actions/setup-renode/action.yml";
const ACTION_REF: &str = ".github/actions/setup-renode";

/// Lines the action must carry, outside comments. Without the first, a tarball
/// that is not the pinned one installs silently; without the second, the binary's
/// own version is never asked; without the GITHUB_PATH line, later steps resolve
/// the container's bundled Renode; without the two GITHUB_ENV lines, the verify
/// step below has nothing to compare against.
const ACTION_LINES: &[&str] = &[
    r#"echo "$RENODE_SHA256  $tgz" | sha256sum -c -"#,
    r#"*"v$RENODE_VERSION"*) ;;"#,
    r#"echo "$dir" >> "$GITHUB_PATH""#,
    r#"echo "RENODE_PINNED_DIR=$dir" >> "$GITHUB_ENV""#,
    r#"echo "RENODE_PINNED_VERSION=$RENODE_VERSION" >> "$GITHUB_ENV""#,
];

/// The step every job runs immediately after the action. The action's GITHUB_PATH
/// write only takes effect in later steps, so the action itself cannot prove what
/// `renode` resolves to; this step does, in the job, after the PATH change.
const VERIFY_STEP: &str = "Pinned Renode is the one on PATH";
const VERIFY_LINES: &[&str] = &[
    r#"set -euo pipefail"#,
    r#"for t in renode renode-test; do"#,
    r#"got="$(command -v "$t" || true)""#,
    r#"[ "$got" = "$RENODE_PINNED_DIR/$t" ] ||"#,
    r#"v="$(renode --version 2>&1 || true)""#,
    r#"*"v$RENODE_PINNED_VERSION"*) ;;"#,
];

/// A Renode that is deliberately NOT the pin: (workflow file, install directory,
/// why). A path-qualified `renode`/`renode-test`, or a PATH assignment, naming any
/// other directory fails the gate.
const UNPINNED_ALLOW: &[(&str, &str, &str)] = &[(
    "engine-bench-renode-synth.yml",
    "/opt/renode-nightly",
    "the cycle-model A/B arm runs Renode nightly on purpose, beside the pinned headline arms",
)];

/// How a step may name the pinned install's directory.
const PINNED_DIR_VARS: &[&str] = &["${RENODE_PINNED_DIR}", "$RENODE_PINNED_DIR"];

#[derive(Debug, PartialEq, Eq)]
struct Pin {
    url: String,
    sha256: String,
}

/// The `renode.download_portable(...)` block's url and sha256.
fn module_pin(text: &str) -> Option<Pin> {
    let start = text.find("renode.download_portable(")?;
    let block = &text[start..];
    let block = &block[..block.find("\n)").map(|e| e + 2).unwrap_or(block.len())];
    Some(Pin { url: quoted_after(block, "url =")?, sha256: quoted_after(block, "sha256 =")? })
}

/// The action's RENODE_URL / RENODE_SHA256 / RENODE_VERSION env values.
fn action_pin(text: &str) -> Option<(Pin, String)> {
    let pin = Pin { url: quoted_after(text, "RENODE_URL:")?, sha256: quoted_after(text, "RENODE_SHA256:")? };
    Some((pin, quoted_after(text, "RENODE_VERSION:")?))
}

/// The first `"..."` after `key`, on the same line.
fn quoted_after(text: &str, key: &str) -> Option<String> {
    for line in text.lines() {
        let t = line.trim_start();
        if t.starts_with('#') {
            continue;
        }
        if let Some(rest) = t.strip_prefix(key) {
            let rest = rest.trim_start();
            let rest = rest.strip_prefix('"')?;
            return Some(rest[..rest.find('"')?].to_string());
        }
    }
    None
}

fn is_sha256(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

/// The line without indentation and a leading list dash; None for a comment.
fn code_of(line: &str) -> Option<&str> {
    let code = line.trim_start();
    let code = code.strip_prefix("- ").unwrap_or(code).trim_start();
    if code.starts_with('#') { None } else { Some(code) }
}

/// Whether `text` has a non-comment line containing `needle`.
fn has_code_line(text: &str, needle: &str) -> bool {
    text.lines().filter_map(code_of).any(|c| c.contains(needle))
}

/// Whether `text` has a YAML key `key` (e.g. `if:`) on some non-comment line.
fn has_key(text: &str, key: &str) -> bool {
    text.lines().filter_map(code_of).any(|c| c.starts_with(key))
}

/// Byte ranges of each `renode` / `renode-test` used as a command token in a run
/// line, outside a comment. `renode-baseline` (a directory name) and
/// `renode_test(` (the Bazel rule) are not invocations.
fn renode_tokens(line: &str) -> Vec<(usize, usize)> {
    let Some(code) = code_of(line) else { return Vec::new() };
    // Keys whose value is a path or a label, never a command.
    const NOT_COMMANDS: &[&str] =
        &["working-directory:", "path:", "key:", "restore-keys:", "name:", "uses:"];
    if NOT_COMMANDS.iter().any(|k| code.starts_with(k)) {
        return Vec::new();
    }
    let bytes = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while let Some(p) = code[i..].find("renode") {
        let s = i + p;
        let before_ok =
            s == 0 || matches!(bytes[s - 1], b' ' | b'/' | b'(' | b'"' | b'\'' | b'\t' | b'|' | b';' | b'&' | b'`');
        let mut e = s + "renode".len();
        if code[e..].starts_with("-test") {
            e += "-test".len();
        }
        let after_ok = e == code.len() || matches!(bytes[e], b' ' | b'"' | b'\'' | b')' | b'\t' | b';' | b'`');
        if before_ok && after_ok {
            out.push((s, e));
        }
        i = s + 1;
    }
    out
}

fn invokes_renode(line: &str) -> bool {
    !renode_tokens(line).is_empty()
}

fn allowed_dirs(file: &str) -> Vec<&'static str> {
    UNPINNED_ALLOW.iter().filter(|(f, _, _)| *f == file).map(|(_, d, _)| *d).collect()
}

/// A path-qualified `<dir>/renode[-test]` whose `<dir>` is neither the pinned
/// install nor allowlisted for this file: the path, else None.
fn foreign_renode_path(file: &str, line: &str) -> Option<String> {
    let code = code_of(line)?;
    let bytes = code.as_bytes();
    for (s, e) in renode_tokens(line) {
        if s == 0 || bytes[s - 1] != b'/' {
            continue;
        }
        let mut b = s;
        while b > 0 && !matches!(bytes[b - 1], b' ' | b'\t' | b'"' | b'\'' | b'(' | b'`' | b'=' | b';' | b'|' | b'&') {
            b -= 1;
        }
        let dir = &code[b..s - 1];
        let ok = PINNED_DIR_VARS.contains(&dir) || allowed_dirs(file).contains(&dir);
        if !ok {
            return Some(code[b..e].to_string());
        }
    }
    None
}

/// A PATH assignment (shell `PATH=`, an `env:` `PATH:` key, or a GITHUB_PATH
/// write) that mentions renode other than the pinned or an allowlisted directory.
fn renode_path_assignment(file: &str, line: &str) -> bool {
    let Some(code) = code_of(line) else { return false };
    if !(code.contains("PATH=") || code.contains("GITHUB_PATH") || code.starts_with("PATH:")) {
        return false;
    }
    let mut rest = code.to_string();
    for d in allowed_dirs(file).into_iter().chain(PINNED_DIR_VARS.iter().copied()) {
        rest = rest.replace(d, "");
    }
    rest.to_ascii_lowercase().contains("renode")
}

/// (job name, job text) for each job under the top-level `jobs:` key.
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

/// A job's text split into the part before its first step, then one entry per
/// step (a list item at the indentation of the first `- ` under `steps:`).
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
            in_steps = false; // left the steps list (a job key after it)
        }
        let last = out.last_mut().expect("never empty");
        last.push_str(line);
        last.push('\n');
    }
    out
}

fn is_install(step: &str) -> bool {
    step.lines().filter_map(code_of).any(|c| c.starts_with("uses:") && c.contains(ACTION_REF))
}

fn is_verify(step: &str) -> bool {
    has_code_line(step, VERIFY_STEP) && VERIFY_LINES.iter().all(|l| has_code_line(step, l))
}

fn guarded(step: &str) -> bool {
    has_key(step, "if:") || has_key(step, "continue-on-error:")
}

/// Every way a workflow's jobs reach a Renode that is not the verified pin.
fn unpinned_jobs(file: &str, wf: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (name, body) in jobs(wf) {
        let st = steps(&body);
        let mut installed = false;
        let mut reported_unpinned = false;
        for (i, step) in st.iter().enumerate() {
            if i > 0 && is_install(step) {
                if guarded(step) {
                    out.push(format!("{file}: job `{name}`: the {ACTION_REF} step carries `if:` or `continue-on-error:`, so it can be skipped or fail open"));
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
            }
            for line in step.lines() {
                if let Some(p) = foreign_renode_path(file, line) {
                    out.push(format!("{file}: job `{name}` runs `{p}`, a Renode outside the pinned install and the allowlist"));
                }
                if renode_path_assignment(file, line) {
                    out.push(format!("{file}: job `{name}` puts a Renode on PATH: `{}`", line.trim()));
                }
                if invokes_renode(line) && !installed && !reported_unpinned {
                    out.push(format!("{file}: job `{name}` runs `{}` before (or without) a verified {ACTION_REF}", line.trim()));
                    reported_unpinned = true;
                }
            }
        }
    }
    out
}

fn check(module: &str, action: &str, workflows: &[(String, String)]) -> Verdict {
    let Some(m) = module_pin(module) else {
        return Verdict::Refused(format!("{MODULE}: no renode.download_portable url/sha256 -- the gate cannot answer"));
    };
    let Some((a, version)) = action_pin(action) else {
        return Verdict::Refused(format!("{ACTION}: no RENODE_URL/RENODE_SHA256/RENODE_VERSION -- the gate cannot answer"));
    };
    let mut lines = vec![
        format!("{MODULE}: {} sha256={}", m.url, m.sha256),
        format!("{ACTION}: {} sha256={} version={version}", a.url, a.sha256),
    ];
    let mut failed = false;
    if m.url != a.url {
        lines.push("FAIL: the module and the action download different Renode assets".into());
        failed = true;
    }
    if m.sha256 != a.sha256 {
        lines.push("FAIL: the module and the action pin different sha256 values".into());
        failed = true;
    }
    for (who, p) in [(MODULE, &m), (ACTION, &a)] {
        if !is_sha256(&p.sha256) {
            lines.push(format!("FAIL: {who}: sha256 `{}` is not 64 lowercase hex digits", p.sha256));
            failed = true;
        }
    }
    let want = format!("/v{version}/renode-{version}.linux-portable.tar.gz");
    if !a.url.ends_with(&want) {
        lines.push(format!("FAIL: {ACTION}: RENODE_VERSION {version} does not name the asset (expected the url to end in {want})"));
        failed = true;
    }
    let missing: Vec<&&str> = ACTION_LINES.iter().filter(|l| !has_code_line(action, l)).collect();
    for l in &missing {
        lines.push(format!("FAIL: {ACTION}: missing the line `{l}`"));
        failed = true;
    }
    lines.push(format!("{ACTION}: {}/{} required lines present", ACTION_LINES.len() - missing.len(), ACTION_LINES.len()));
    if guarded(action) {
        lines.push(format!("FAIL: {ACTION}: a step carries `if:` or `continue-on-error:`"));
        failed = true;
    }
    let mut scanned = 0;
    let mut running = 0;
    for (file, text) in workflows {
        scanned += 1;
        if jobs(text).iter().any(|(_, b)| b.lines().any(invokes_renode)) {
            running += 1;
        }
        let bad = unpinned_jobs(file, text);
        if !bad.is_empty() {
            failed = true;
            lines.push("FAIL: a job reaches a Renode that is not the verified pin:".into());
            lines.extend(bad.into_iter().map(|b| format!("  {b}")));
        }
    }
    lines.push(format!("{scanned} workflow(s) scanned, {running} run Renode directly"));
    for (f, d, why) in UNPINNED_ALLOW {
        lines.push(format!("allowlisted unpinned Renode: {d} in {f} ({why})"));
    }
    if running == 0 {
        return Verdict::Refused("no workflow runs Renode -- the job sweep would be vacuous".into());
    }
    if failed { Verdict::Fail(lines) } else { Verdict::Pass(lines) }
}

fn workflows(repo: &Path) -> Option<Vec<(String, String)>> {
    let mut v = Vec::new();
    for e in std::fs::read_dir(repo.join(".github/workflows")).ok()? {
        let p = e.ok()?.path();
        if matches!(p.extension().and_then(|x| x.to_str()), Some("yml" | "yaml")) {
            let name = p.file_name()?.to_string_lossy().to_string();
            v.push((name, std::fs::read_to_string(&p).ok()?));
        }
    }
    v.sort();
    Some(v)
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let Ok(module) = std::fs::read_to_string(repo.join(MODULE)) else {
        return Verdict::Refused(format!("cannot read {MODULE}"));
    };
    let Ok(action) = std::fs::read_to_string(repo.join(ACTION)) else {
        return Verdict::Refused(format!("cannot read {ACTION} -- no pinned install exists"));
    };
    let Some(wfs) = workflows(repo) else {
        return Verdict::Refused("cannot read .github/workflows".into());
    };
    check(&module, &action, &wfs)
}

const SHA_A: &str = "4ba7c68b59e2447f188ef4b4b112fcccd2802582c460beedceb63b84e5605a5f";
const SHA_B: &str = "73fe07a8fd0849a9f30c02bd7ce7c7dd67d11070365f41ddd6145a477ebdea65";
const URL_A: &str = "https://github.com/renode/renode/releases/download/v1.17.0/renode-1.17.0.linux-portable.tar.gz";

fn module_text(url: &str, sha: &str) -> String {
    format!("renode.download_portable(\n    name = \"renode_toolchain\",\n    sha256 = \"{sha}\",\n    url = \"{url}\",\n)\n")
}

fn action_text(url: &str, sha: &str, ver: &str) -> String {
    let mut s = format!(
        "runs:\n  steps:\n    - env:\n        RENODE_VERSION: \"{ver}\"\n        RENODE_URL: \"{url}\"\n        RENODE_SHA256: \"{sha}\"\n      run: |\n"
    );
    for l in ACTION_LINES {
        s.push_str("        ");
        s.push_str(l);
        s.push('\n');
    }
    s
}

fn wf_named(file: &str, body: &str) -> Vec<(String, String)> {
    vec![(file.to_string(), format!("name: w\non: push\njobs:\n  bench:\n    steps:\n{body}"))]
}

fn wf(body: &str) -> Vec<(String, String)> {
    wf_named("w.yml", body)
}

/// The install step plus the verify step, as the workflows carry them.
fn install_and_verify() -> String {
    let mut s = format!("      - uses: ./gale/.github/actions/setup-renode\n      - name: {VERIFY_STEP}\n        run: |\n");
    for l in VERIFY_LINES {
        s.push_str("          ");
        s.push_str(l);
        s.push('\n');
    }
    s
}

fn pinned_job() -> String {
    format!("{}      - run: |\n          renode-test x.robot\n", install_and_verify())
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
    let good_m = module_text(URL_A, SHA_A);
    let good_a = action_text(URL_A, SHA_A, "1.17.0");
    let pass = |v: &Verdict| matches!(v, Verdict::Pass(_));
    let fail = |v: &Verdict| matches!(v, Verdict::Fail(_));
    let pj = pinned_job();
    let iv = install_and_verify();
    let after_pin = |run: &str| format!("{iv}      - run: |\n          {run}\n");

    ck("agreeing pins + pinned job pass", pass(&check(&good_m, &good_a, &wf(&pj))));
    // The base tree's shape: module on 1.15.3, container renode, no action step.
    let old_url = "https://builds.renode.io/renode-1.15.3+20250128git85f0aac72.linux-portable-dotnet.tar.gz";
    ck("module on a different asset fails", fail(&check(&module_text(old_url, SHA_A), &good_a, &wf(&pj))));
    ck("module sha256 drift fails", fail(&check(&module_text(URL_A, SHA_B), &good_a, &wf(&pj))));
    ck("action version not naming the asset fails",
       fail(&check(&good_m, &action_text(URL_A, SHA_A, "1.16.0"), &wf(&pj))));
    ck("uppercase / short sha256 fails",
       fail(&check(&module_text(URL_A, "ABC"), &action_text(URL_A, "ABC", "1.17.0"), &wf(&pj))));
    ck("a job running renode-test with no pinned install fails",
       fail(&check(&good_m, &good_a, &wf("      - run: |\n          renode-test x.robot\n"))));
    ck("a job whose only uses: step is actions/checkout does not count as installed",
       fail(&check(&good_m, &good_a, &wf("      - uses: actions/checkout@v4\n      - run: |\n          renode-test x.robot\n"))));
    ck("a pinned install AFTER the renode call does not count",
       fail(&check(&good_m, &good_a, &wf(&format!("      - run: renode --version\n{iv}")))));

    // D1: the install proves nothing about PATH; the verify step after it does.
    ck("an install not followed by the verify step fails",
       fail(&check(&good_m, &good_a, &wf("      - uses: ./gale/.github/actions/setup-renode\n      - run: |\n          renode-test x.robot\n"))));
    for (i, l) in VERIFY_LINES.iter().enumerate() {
        let gutted = pj.replacen(&format!("          {l}\n"), "", 1);
        ck(&format!("a verify step missing check {i} (`{l}`) fails"), fail(&check(&good_m, &good_a, &wf(&gutted))));
    }
    let late_verify = format!("      - uses: ./gale/.github/actions/setup-renode\n      - run: echo between\n{}",
        iv.replacen("      - uses: ./gale/.github/actions/setup-renode\n", "", 1));
    ck("a verify step that is not IMMEDIATELY after the install fails",
       fail(&check(&good_m, &good_a, &wf(&format!("{late_verify}      - run: renode-test x.robot\n")))));

    // D2: ways to run a Renode other than the pin.
    ck("a path-qualified /opt/renode/renode-test after the install fails",
       fail(&check(&good_m, &good_a, &wf(&after_pin("/opt/renode/renode-test x.robot")))));
    ck("a path-qualified $RENODE_PINNED_DIR/renode passes",
       pass(&check(&good_m, &good_a, &wf(&after_pin("\"$RENODE_PINNED_DIR/renode\" --version")))));
    ck("the allowlisted nightly dir passes in its own workflow",
       pass(&check(&good_m, &good_a, &wf_named("engine-bench-renode-synth.yml", &after_pin("/opt/renode-nightly/renode --version")))));
    ck("the allowlisted nightly dir fails in another workflow",
       fail(&check(&good_m, &good_a, &wf(&after_pin("/opt/renode-nightly/renode --version")))));
    ck("export PATH=/opt/renode:$PATH after the install fails",
       fail(&check(&good_m, &good_a, &wf(&after_pin("export PATH=\"/opt/renode:$PATH\"")))));
    ck("a GITHUB_PATH write naming a renode dir fails",
       fail(&check(&good_m, &good_a, &wf(&after_pin("echo /opt/renode >> \"$GITHUB_PATH\"")))));
    ck("an env PATH key naming a renode dir fails",
       fail(&check(&good_m, &good_a, &wf(&format!("{iv}      - env:\n          PATH: /opt/renode:/usr/bin\n        run: echo hi\n")))));
    ck("the allowlisted nightly PATH export passes in its own workflow",
       pass(&check(&good_m, &good_a, &wf_named("engine-bench-renode-synth.yml",
           &after_pin("export PATH=\"/opt/renode-nightly:$PATH\"")))));
    ck("`if:` on the install step fails",
       fail(&check(&good_m, &good_a, &wf(&pj.replacen("setup-renode\n", "setup-renode\n        if: false\n", 1)))));
    ck("`continue-on-error:` on the install step fails",
       fail(&check(&good_m, &good_a, &wf(&pj.replacen("setup-renode\n", "setup-renode\n        continue-on-error: true\n", 1)))));
    ck("`continue-on-error:` on the verify step fails",
       fail(&check(&good_m, &good_a, &wf(&pj.replacen("        run: |\n", "        continue-on-error: true\n        run: |\n", 1)))));
    ck("a backtick invocation counts", invokes_renode("          V=`renode-test --version`"));
    ck("a job running renode-test only in backticks, with no install, fails",
       fail(&check(&good_m, &good_a, &wf("      - run: |\n          V=`renode-test --version`\n"))));

    // D4: the action body itself.
    for l in ACTION_LINES {
        let gutted = good_a.replacen(&format!("        {l}\n"), "", 1);
        ck(&format!("an action without `{l}` fails"), fail(&check(&good_m, &gutted, &wf(&pj))));
    }
    ck("an action line that is only a comment does not count",
       fail(&check(&good_m, &good_a.replacen("        echo \"$RENODE_SHA256", "        # echo \"$RENODE_SHA256", 1), &wf(&pj))));

    ck("`$(renode --version ...)` counts as an invocation",
       invokes_renode("            echo \"x: $(renode --version 2>&1 | head -1)\""));
    ck("a comment mentioning renode-test is not an invocation", !invokes_renode("          # renode-test runs here"));
    ck("a renode-baseline directory is not an invocation",
       !invokes_renode("          mkdir -p \"$GITHUB_WORKSPACE/renode-baseline\""));
    ck("the bazel rule name is not an invocation", !invokes_renode("    renode_test(name = \"x\")"));
    ck("a working-directory ending in renode-test is not an invocation",
       !invokes_renode("        working-directory: os/gust/renode-test"));
    ck("a path-qualified renode binary is an invocation",
       invokes_renode("          /opt/renode-nightly/renode --version"));
    // Job scoping: the install in one job does not cover another.
    let two = format!("jobs:\n  a:\n    steps:\n{pj}  b:\n    steps:\n      - run: renode-test y.robot\n");
    let bad = unpinned_jobs("w.yml", &two);
    ck("an install in job a does not cover job b", bad.len() == 1 && bad[0].contains("job `b`"));
    ck("no workflow running renode is refused, not passed",
       matches!(check(&good_m, &good_a, &wf("      - run: echo hi\n")), Verdict::Refused(_)));
    ck("an unparsable module is refused", matches!(check("", &good_a, &wf(&pj)), Verdict::Refused(_)));
    if broken { Verdict::Fail(lines) } else { Verdict::Pass(lines) }
}

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
//! What it cannot see: a job that reaches Renode by a name other than `renode`
//! or `renode-test` (a wrapper script), and the Renode a developer has locally.

use crate::verdict::Verdict;
use std::path::Path;

pub const NAME: &str = "renode-pin";

const MODULE: &str = "os/gust/renode-test/MODULE.bazel";
const ACTION: &str = ".github/actions/setup-renode/action.yml";
const ACTION_REF: &str = ".github/actions/setup-renode";

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

/// A run line that invokes Renode: the word `renode` or `renode-test` as a
/// command token, outside a comment. `renode-baseline` (a directory name) and
/// `renode_test(` (the Bazel rule) are not invocations.
fn invokes_renode(line: &str) -> bool {
    let code = line.trim_start().trim_start_matches("- ");
    if code.starts_with('#') {
        return false;
    }
    // Keys whose value is a path or a label, never a command.
    const NOT_COMMANDS: &[&str] =
        &["working-directory:", "path:", "key:", "restore-keys:", "name:", "uses:"];
    if NOT_COMMANDS.iter().any(|k| code.starts_with(k)) {
        return false;
    }
    let bytes = code.as_bytes();
    let mut i = 0;
    while let Some(p) = code[i..].find("renode") {
        let s = i + p;
        let before_ok = s == 0 || matches!(bytes[s - 1], b' ' | b'/' | b'(' | b'"' | b'\'' | b'\t' | b'|' | b';' | b'&');
        let mut e = s + "renode".len();
        if code[e..].starts_with("-test") {
            e += "-test".len();
        }
        let after_ok = e == code.len() || matches!(bytes[e], b' ' | b'"' | b'\'' | b')' | b'\t' | b';');
        if before_ok && after_ok {
            return true;
        }
        i = s + 1;
    }
    false
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

/// Jobs that run Renode without installing the pinned one first.
fn unpinned_jobs(file: &str, wf: &str) -> Vec<String> {
    let mut out = Vec::new();
    for (name, body) in jobs(wf) {
        let mut installed = false;
        for line in body.lines() {
            let t = line.trim_start();
            if t.starts_with("uses:") || t.starts_with("- uses:") {
                if t.contains(ACTION_REF) {
                    installed = true;
                }
                continue;
            }
            if invokes_renode(line) && !installed {
                out.push(format!("{file}: job `{name}` runs `{}` before (or without) {ACTION_REF}", t.trim()));
                break;
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
            lines.push("FAIL: a job runs Renode without the pinned install:".into());
            lines.extend(bad.into_iter().map(|b| format!("  {b}")));
        }
    }
    lines.push(format!("{scanned} workflow(s) scanned, {running} run Renode directly"));
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
    format!("runs:\n  steps:\n    - env:\n        RENODE_VERSION: \"{ver}\"\n        RENODE_URL: \"{url}\"\n        RENODE_SHA256: \"{sha}\"\n")
}

fn wf(body: &str) -> Vec<(String, String)> {
    vec![("w.yml".to_string(), format!("name: w\non: push\njobs:\n  bench:\n    steps:\n{body}"))]
}

const PINNED_JOB: &str = "      - uses: ./gale/.github/actions/setup-renode\n      - run: |\n          renode-test x.robot\n";

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

    ck("agreeing pins + pinned job pass", pass(&check(&good_m, &good_a, &wf(PINNED_JOB))));
    // The base tree's shape: module on 1.15.3, container renode, no action step.
    let old_url = "https://builds.renode.io/renode-1.15.3+20250128git85f0aac72.linux-portable-dotnet.tar.gz";
    ck("module on a different asset fails", fail(&check(&module_text(old_url, SHA_A), &good_a, &wf(PINNED_JOB))));
    ck("module sha256 drift fails", fail(&check(&module_text(URL_A, SHA_B), &good_a, &wf(PINNED_JOB))));
    ck("action version not naming the asset fails",
       fail(&check(&good_m, &action_text(URL_A, SHA_A, "1.16.0"), &wf(PINNED_JOB))));
    ck("uppercase / short sha256 fails",
       fail(&check(&module_text(URL_A, "ABC"), &action_text(URL_A, "ABC", "1.17.0"), &wf(PINNED_JOB))));
    ck("a job running renode-test with no pinned install fails",
       fail(&check(&good_m, &good_a, &wf("      - run: |\n          renode-test x.robot\n"))));
    ck("a pinned install AFTER the renode call does not count",
       fail(&check(&good_m, &good_a, &wf("      - run: renode --version\n      - uses: ./.github/actions/setup-renode\n"))));
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
    let two = format!(
        "jobs:\n  a:\n    steps:\n{PINNED_JOB}  b:\n    steps:\n      - run: renode-test y.robot\n"
    );
    ck("an install in job a does not cover job b",
       unpinned_jobs("w.yml", &two).len() == 1 && unpinned_jobs("w.yml", &two)[0].contains("job `b`"));
    ck("no workflow running renode is refused, not passed",
       matches!(check(&good_m, &good_a, &wf("      - run: echo hi\n")), Verdict::Refused(_)));
    ck("an unparsable module is refused", matches!(check("", &good_a, &wf(PINNED_JOB)), Verdict::Refused(_)));
    if broken { Verdict::Fail(lines) } else { Verdict::Pass(lines) }
}

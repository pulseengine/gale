//! No CI retry loop may swallow its own failure.
//!
//! `llvm-lto-test (msgq)` failed on a PR that touched only rivet YAML. The log
//! showed the Zephyr SDK's GNU toolchain install failing three times -- and the
//! step that ran it PASSED. The step AFTER it failed, looking for a toolchain
//! that had never been installed. The install reported success while installing
//! nothing, and the error surfaced one step later wearing a different name.
//!
//! The idiom:
//!
//! ```text
//! for attempt in 1 2 3; do CMD && break || sleep 5; done
//! ```
//!
//! When every attempt fails the last command executed is `sleep`, which
//! succeeds, so the loop's exit status is 0. `bash -e` -- what Actions runs
//! `run:` under -- does not help: a failing command in an `&&`/`||` list is
//! exempt, and the list's status is the final `sleep`. Verified directly:
//! `bash -e -c 'for a in 1 2 3; do false && break || sleep 0.1; done'` exits 0.
//!
//! There were 28 of these, in 10 workflows, guarding every `west update` and
//! `west sdk install` in the repo (#370).
//!
//! Ported from tools/check-retry-loops.py (gale#368). The rule is unchanged: it
//! matches `&& break ||` GENERALLY, not the `sleep` spelling -- `|| true` is the
//! same defect with a shorter name.

use crate::verdict::Verdict;
use std::path::Path;

pub const NAME: &str = "retry-loops";

/// `CMD && break || <anything>`: the trailing branch decides the loop's status,
/// and it is chosen to succeed. Hand-scanned rather than via a regex crate --
/// the shape is three fixed tokens in order on one line.
fn swallows(line: &str) -> bool {
    let Some(amp) = line.find("&&") else { return false };
    let rest = &line[amp + 2..];
    let t = rest.trim_start();
    if !t.starts_with("break") {
        return false;
    }
    let after = &t["break".len()..];
    // Only a `break` as its own word, then an `||` before anything else.
    if !after.starts_with(|c: char| c.is_whitespace() || c == ';') {
        return false;
    }
    after.contains("||")
}

fn scan(text: &str) -> Vec<(usize, String)> {
    text.lines()
        .enumerate()
        .filter(|(_, l)| swallows(l))
        .map(|(i, l)| (i + 1, l.trim().to_string()))
        .collect()
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let wf = repo.join(".github/workflows");
    if !wf.is_dir() {
        return Verdict::Refused(format!("no workflows directory at {}", wf.display()));
    }
    let mut files: Vec<_> = match std::fs::read_dir(&wf) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "yml"))
            .collect(),
        Err(e) => return Verdict::Refused(format!("{}: {e}", wf.display())),
    };
    files.sort();
    if files.is_empty() {
        return Verdict::Refused("no workflow files -- the sweep would be vacuous".into());
    }

    let mut hits = Vec::new();
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap_or_default();
        for (ln, line) in scan(&text) {
            hits.push(format!(
                "{}:{ln}\n    {}",
                f.file_name().unwrap_or_default().to_string_lossy(),
                &line[..line.len().min(110)]
            ));
        }
    }
    if !hits.is_empty() {
        let mut lines = vec![format!("FAIL: {} retry loop(s) that cannot report failure:", hits.len())];
        lines.extend(hits);
        lines.push("`CMD && break || sleep N` exits 0 when every attempt fails: the last".into());
        lines.push("command run is the sleep. Use instead:".into());
        lines.push("  n=0; until CMD; do n=$((n+1));".into());
        lines.push("    if [ $n -ge 3 ]; then echo \"::error::CMD failed after 3 attempts\" >&2; exit 1; fi;".into());
        lines.push("    sleep $((N*n)); done".into());
        return Verdict::Fail(lines);
    }
    Verdict::Pass(vec![format!(
        "{} workflow(s) swept, no unfailable retry loops",
        files.len()
    )])
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
    ck(
        "unfailable form caught",
        !scan("for attempt in 1 2 3; do west update && break || sleep 15; done").is_empty(),
    );
    ck(
        "`|| true` variant caught",
        !scan("do CMD && break || true; done").is_empty(),
    );
    ck(
        "corrected form NOT flagged",
        scan("n=0; until west update; do n=$((n+1)); if [ $n -ge 3 ]; then exit 1; fi; sleep $((15*n)); done")
            .is_empty(),
    );
    // A plain `&&` with no break is ordinary shell, not this defect.
    ck("plain && not flagged", scan("mkdir -p x && cd x").is_empty());
    // `break` inside a word must not count.
    ck(
        "breakfast not flagged",
        scan("echo a && breakfast || true").is_empty(),
    );
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

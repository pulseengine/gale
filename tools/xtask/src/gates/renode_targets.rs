//! Every `renode_test` target defined must actually be RUN by CI.
//!
//! gust-renode.yml names its bazel targets in an ENUMERATED list. That list
//! silently narrows: `gust-iso-renode` was defined in BUILD.bazel by the commit
//! that added it (5561fde, "EXECUTE the dissolved isolation core — and it finds
//! a miscompile in our pinned synth") and was NEVER added to the workflow. Its
//! own comment calls it "the FIRST harness to EXECUTE iso-core-fused-cm3.o" and
//! notes that "two defects reached main through that gap" -- while not running.
//!
//! It could not have run either: the target references `:gust_iso.elf`, which
//! was never committed and which nothing generates.
//!
//! This is the same shape as the enumerated Lean target list (gale#292) and the
//! hand-written provider list this repo has already been bitten by twice. A gate
//! that exists but is not invoked is worth less than no gate, because it reads
//! as coverage.

use crate::verdict::Verdict;
use std::path::Path;

pub const NAME: &str = "renode-targets";

/// Every committed BUILD file that declares `renode_test`, mapped to the
/// workflow that runs its targets. This was a single hardcoded BUILD path, and
/// the gate was blind to `renode/BUILD.bazel` -- three targets pointing at ELFs
/// that never existed, in a package whose rule set the root MODULE.bazel had
/// already removed (gale#387). A gate scoped to one file, auditing a defect
/// class that exists in two. The census below makes an unmapped BUILD file a
/// failure instead of a blind spot.
const SUITES: &[(&str, &str)] = &[(
    "benches/gust/renode-test/BUILD.bazel",
    ".github/workflows/gust-renode.yml",
)];

/// Committed BUILD files that declare a `renode_test` rule.
fn declaring_builds(repo: &Path) -> Option<Vec<String>> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-files", "--", "BUILD", "BUILD.bazel", "**/BUILD", "**/BUILD.bazel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let mut v: Vec<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .filter(|l| {
            std::fs::read_to_string(repo.join(l))
                .map(|t| !defined_targets(&t).is_empty())
                .unwrap_or(false)
        })
        .map(String::from)
        .collect();
    v.sort();
    Some(v)
}

/// `renode_test(\n    name = "X"` -- the rule's own declaration, not any mention.
fn defined_targets(build: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = build;
    while let Some(p) = rest.find("renode_test(") {
        let after = &rest[p + "renode_test(".len()..];
        if let Some(np) = after.find("name = \"") {
            let n = &after[np + "name = \"".len()..];
            if let Some(q) = n.find('"') {
                out.push(n[..q].to_string());
            }
        }
        rest = after;
    }
    out.sort();
    out.dedup();
    out
}

/// `//:target` as the workflow invokes them.
fn gated_targets(wf: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = wf;
    while let Some(p) = rest.find("//:") {
        let after = &rest[p + 3..];
        let end = after
            .find(|c: char| !(c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-'))
            .unwrap_or(after.len());
        if end > 0 {
            out.push(after[..end].to_string());
        }
        rest = &after[end.max(1)..];
    }
    out.sort();
    out.dedup();
    out
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let Some(builds) = declaring_builds(repo) else {
        return Verdict::Refused("git ls-files failed -- the gate cannot answer".into());
    };
    if builds.is_empty() {
        return Verdict::Refused("no renode_test targets found -- the sweep would be vacuous".into());
    }
    let mut lines = Vec::new();
    let mut failed = false;

    // Census: a BUILD file declaring renode_test that no workflow is mapped to.
    let unmapped: Vec<&String> =
        builds.iter().filter(|b| !SUITES.iter().any(|(s, _)| s == b)).collect();
    if !unmapped.is_empty() {
        lines.push(format!(
            "FAIL: {} BUILD file(s) declare renode_test with no workflow mapped to run them:",
            unmapped.len()
        ));
        lines.extend(unmapped.iter().map(|u| format!("  {u}")));
        lines.push("Map it in SUITES with the workflow that runs it, or delete the targets.".into());
        failed = true;
    }
    for (b, _) in SUITES {
        if !builds.iter().any(|x| x == b) {
            lines.push(format!("FAIL: SUITES lists {b}, which declares no renode_test (remove it)"));
            failed = true;
        }
    }

    for b in &builds {
        let bp = repo.join(b);
        let Ok(build) = std::fs::read_to_string(&bp) else {
            return Verdict::Refused(format!("cannot read {}", bp.display()));
        };
        let defined = defined_targets(&build);
        let wf_path = SUITES.iter().find(|(s, _)| s == b).map(|(_, w)| *w);
        let gated = match wf_path {
            Some(w) => match std::fs::read_to_string(repo.join(w)) {
                Ok(t) => gated_targets(&t),
                Err(_) => return Verdict::Refused(format!("cannot read {w}")),
            },
            None => Vec::new(),
        };
        let ungated: Vec<&String> = defined.iter().filter(|d| !gated.contains(d)).collect();
        lines.push(format!(
            "{b}: {} renode_test target(s) defined, {} gated",
            defined.len(),
            defined.len() - ungated.len()
        ));
        if wf_path.is_some() && !ungated.is_empty() {
            lines.push(format!("FAIL: {} defined but NOT run by CI:", ungated.len()));
            lines.extend(ungated.iter().map(|u| format!("  {u}")));
            lines.push("Add it to the workflow's target list, or delete the target.".into());
            failed = true;
        }
        // A target whose ELF is absent cannot run even when listed -- that is how
        // gust-iso-renode stayed invisible. Checked separately from gating.
        let missing = missing_elfs(&build, bp.parent().unwrap_or(repo));
        if !missing.is_empty() {
            lines.push(format!(
                "FAIL: {} target(s) in {b} reference an ELF that does not exist:",
                missing.len()
            ));
            lines.extend(missing.iter().map(|m| format!("  {m}")));
            failed = true;
        }
    }
    if failed {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

/// `"ELF": ":label"` or `"ELF": "path"` -- both spellings resolve relative to
/// the BUILD file's package. The first version matched only `":`, so a plain
/// path (renode/BUILD.bazel's spelling) was never checked at all.
fn missing_elfs(build: &str, dir: &Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = build;
    while let Some(p) = rest.find("renode_test(") {
        let after = &rest[p..];
        let block_end = after.find("\n)").map(|e| e + 2).unwrap_or(after.len());
        let block = &after[..block_end];
        if let (Some(np), Some(ep)) = (block.find("name = \""), block.find("\"ELF\": \"")) {
            let name = block[np + 8..].split('"').next().unwrap_or("").to_string();
            let elf = block[ep + 8..].split('"').next().unwrap_or("");
            let elf = elf.trim_start_matches(':');
            if !elf.is_empty() && !dir.join(elf).is_file() {
                out.push(format!("{name} -> {elf}"));
            }
        }
        rest = &after[block_end.max(1)..];
    }
    out
}

pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got { broken = true; }
    };
    let b = "renode_test(\n    name = \"a-renode\",\n)\nrenode_test(\n    name = \"b-renode\",\n)\n";
    ck("both targets parsed", defined_targets(b) == vec!["a-renode", "b-renode"]);
    ck("gated list parsed", gated_targets("bazel test \\\n //:a-renode \\\n //:b-renode\n") == vec!["a-renode", "b-renode"]);
    let d = defined_targets(b);
    let g = gated_targets("bazel test //:a-renode\n");
    ck("ungated target detected", d.iter().filter(|x| !g.contains(x)).count() == 1);
    // A mention that is not a rule declaration must not count as defined.
    ck("comment mentioning renode_test not counted",
       defined_targets("# see renode_test( above\n").is_empty());
    // Both ELF spellings are checked (the first version saw only `":label`).
    let tmp = std::env::temp_dir();
    let lab = "renode_test(\n    name = \"x\",\n    variables_with_label = {\n        \"ELF\": \":nope-label.elf\",\n    },\n)\n";
    let pth = "renode_test(\n    name = \"y\",\n    variables_with_label = {\n        \"ELF\": \"elfs/nope-path.elf\",\n    },\n)\n";
    ck("a missing :label ELF is reported", missing_elfs(lab, &tmp).len() == 1);
    ck("a missing plain-path ELF is reported", missing_elfs(pth, &tmp).len() == 1);
    // The census must see more than the one file it used to hardcode.
    match declaring_builds(_repo) {
        Some(b) => ck("the census finds the hardcoded suite", b.iter().any(|x| x == SUITES[0].0)),
        None => ck("declaring_builds could not run", false),
    }
    if broken { Verdict::Fail(lines) } else { Verdict::Pass(lines) }
}

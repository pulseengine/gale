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

const BUILD: &str = "benches/gust/renode-test/BUILD.bazel";
const WORKFLOW: &str = ".github/workflows/gust-renode.yml";

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
    let (bp, wp) = (repo.join(BUILD), repo.join(WORKFLOW));
    let Ok(build) = std::fs::read_to_string(&bp) else {
        return Verdict::Refused(format!("cannot read {}", bp.display()));
    };
    let Ok(wf) = std::fs::read_to_string(&wp) else {
        return Verdict::Refused(format!("cannot read {}", wp.display()));
    };
    let defined = defined_targets(&build);
    if defined.is_empty() {
        return Verdict::Refused("no renode_test targets found -- the sweep would be vacuous".into());
    }
    let gated = gated_targets(&wf);

    let mut lines = Vec::new();
    let ungated: Vec<&String> = defined.iter().filter(|d| !gated.contains(d)).collect();

    // A target whose ELF is absent cannot run even when listed -- that is how
    // gust-iso-renode stayed invisible. Checked separately from gating.
    let dir = bp.parent().unwrap_or(repo);
    let mut missing_elf = Vec::new();
    let mut rest = build.as_str();
    while let Some(p) = rest.find("renode_test(") {
        let after = &rest[p..];
        let block_end = after.find("\n)").map(|e| e + 2).unwrap_or(after.len());
        let block = &after[..block_end];
        if let (Some(np), Some(ep)) = (block.find("name = \""), block.find("\"ELF\": \":")) {
            let name = block[np + 8..].split('"').next().unwrap_or("").to_string();
            let elf = block[ep + 9..].split('"').next().unwrap_or("").to_string();
            if !elf.is_empty() && !dir.join(&elf).is_file() {
                missing_elf.push(format!("{name} -> {elf}"));
            }
        }
        rest = &after[block_end.max(1)..];
    }

    lines.push(format!("{} renode_test target(s) defined, {} gated", defined.len(), defined.len() - ungated.len()));
    let mut failed = false;
    if !ungated.is_empty() {
        lines.push(format!("FAIL: {} defined but NOT run by CI:", ungated.len()));
        lines.extend(ungated.iter().map(|u| format!("  {u}")));
        lines.push("Add it to the target list in gust-renode.yml, or delete the target.".into());
        failed = true;
    }
    if !missing_elf.is_empty() {
        lines.push(format!("FAIL: {} target(s) reference an ELF that does not exist:", missing_elf.len()));
        lines.extend(missing_elf.iter().map(|m| format!("  {m}")));
        failed = true;
    }
    if failed {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
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
    if broken { Verdict::Fail(lines) } else { Verdict::Pass(lines) }
}

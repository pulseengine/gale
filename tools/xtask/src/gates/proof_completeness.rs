//! A Rocq proof that is merely STATED must not pass as proven.
//!
//! `coqc` SUCCEEDS on a file whose every theorem ends in `Admitted.` -- an admit
//! is a deferral, not an error. So `bazel test //proofs:..._test` is green
//! whether a theorem is proven or merely stated, and the job named
//! "Rocq Proofs (14 files)" cannot tell the two apart. Measured:
//!
//! ```text
//! 10 files fully discharged
//!  3 files ENTIRELY admitted -- 74 theorems, zero Qed
//!  1 file EMPTY (heap_proofs.v, 0 bytes) but still a named CI target
//! ```
//!
//! Ported from tools/check-proof-completeness.py (gale#368). It does NOT demand
//! the admits be discharged -- two carry honest in-file reasons about Coq 9.0
//! tactics. It makes them VISIBLE and pins them, and fails if a file gains
//! admits, if a new file is admitted, or if a ledgered file is fixed WITHOUT
//! shrinking the ledger. A ledger nobody prunes becomes a permanent excuse.

use crate::verdict::Verdict;
use std::path::Path;

pub const NAME: &str = "proof-completeness";

/// Exact admitted count per incomplete file. These three state their theorems
/// and prove none of them.
const ADMITTED_LEDGER: &[(&str, usize)] = &[
    ("poll_proofs.v", 22),
    ("sched_proofs.v", 23),
    ("thread_lifecycle_proofs.v", 29),
];

/// Empty proof files that are nonetheless named CI targets. An empty file passes
/// `coqc` trivially, so a target pointing at one is a green check over nothing.
const EMPTY_LEDGER: &[&str] = &["heap_proofs.v"];

/// A line-initial `Admitted.` / `admit.` tactic.
///
/// Anchored on the line, NOT a substring search. executor_proofs.v mentions the
/// Rust method `Tasks::admit` in a COMMENT, and a naive grep reads that as two
/// admitted proofs -- it fooled me first time, and that file is in fact fully
/// discharged.
fn count_admits(text: &str) -> usize {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("Admitted.") || t.starts_with("admit.")
        })
        .count()
}

fn count_qed(text: &str) -> usize {
    text.match_indices("Qed.").count()
}

fn count_theorems(text: &str) -> usize {
    text.lines()
        .filter(|l| {
            let t = l.trim_start();
            t.starts_with("Theorem ") || t.starts_with("Lemma ") || t.starts_with("Corollary ")
        })
        .count()
}

fn count_sorry(text: &str) -> usize {
    // Whole word, so `sorryState` or a comment word boundary is not a hit.
    let b = text.as_bytes();
    let mut n = 0;
    let mut i = 0;
    while let Some(p) = text[i..].find("sorry") {
        let s = i + p;
        let e = s + 5;
        let before_ok = s == 0 || !(b[s - 1].is_ascii_alphanumeric() || b[s - 1] == b'_');
        let after_ok = e >= b.len() || !(b[e].is_ascii_alphanumeric() || b[e] == b'_');
        if before_ok && after_ok {
            n += 1;
        }
        i = e;
    }
    n
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let rocq = repo.join("proofs");
    if !rocq.is_dir() {
        return Verdict::Refused(format!("no proofs directory at {}", rocq.display()));
    }
    let mut files: Vec<_> = match std::fs::read_dir(&rocq) {
        Ok(rd) => rd
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| p.extension().is_some_and(|x| x == "v"))
            .collect(),
        Err(e) => return Verdict::Refused(format!("{}: {e}", rocq.display())),
    };
    files.sort();
    if files.is_empty() {
        return Verdict::Refused("no .v files -- the sweep would be vacuous".into());
    }

    let ledger = |n: &str| ADMITTED_LEDGER.iter().find(|(k, _)| *k == n).map(|(_, v)| *v);
    let mut lines = vec!["file                                admits   Qed  theorems  verdict".into()];
    let mut problems: Vec<String> = Vec::new();

    for f in &files {
        let name = f.file_name().unwrap_or_default().to_string_lossy().to_string();
        let text = std::fs::read_to_string(f).unwrap_or_default();
        if text.trim().is_empty() {
            let listed = EMPTY_LEDGER.contains(&name.as_str());
            if !listed {
                problems.push(format!("{name}: empty proof file, not on the ledger"));
            }
            lines.push(format!(
                "{name:<34} {:>6} {:>5} {:>9}  {}",
                0, 0, 0,
                if listed { "EMPTY (ledgered)" } else { "EMPTY — NOT LEDGERED" }
            ));
            continue;
        }
        let (a, q, t) = (count_admits(&text), count_qed(&text), count_theorems(&text));
        let expected = ledger(&name).unwrap_or(0);
        let verdict = if a == expected && expected == 0 {
            "proven".to_string()
        } else if a == expected {
            format!("admitted (ledgered {expected})")
        } else if a > expected {
            problems.push(format!("{name}: admits grew {expected} -> {a}"));
            format!("ADMITS GREW {expected} -> {a}")
        } else {
            problems.push(format!("{name}: admits shrank {expected} -> {a}; update ADMITTED_LEDGER"));
            format!("admits SHRANK {expected} -> {a} — prune the ledger")
        };
        lines.push(format!("{name:<34} {a:>6} {q:>5} {t:>9}  {verdict}"));
    }

    let present: Vec<String> = files
        .iter()
        .map(|f| f.file_name().unwrap_or_default().to_string_lossy().to_string())
        .collect();
    for (n, _) in ADMITTED_LEDGER {
        if !present.iter().any(|p| p == n) {
            problems.push(format!("{n}: on the ledger but no longer exists"));
        }
    }
    for n in EMPTY_LEDGER {
        if !present.iter().any(|p| p == n) {
            problems.push(format!("{n}: on the empty-ledger but no longer exists"));
        }
    }

    // Lean is currently clean; watch it so it stays that way.
    let lean = repo.join("proofs/lean");
    let mut lean_n = 0usize;
    if lean.is_dir() {
        if let Ok(rd) = std::fs::read_dir(&lean) {
            for p in rd.filter_map(|e| e.ok().map(|e| e.path())) {
                if p.extension().is_some_and(|x| x == "lean") {
                    lean_n += 1;
                    let s = count_sorry(&std::fs::read_to_string(&p).unwrap_or_default());
                    if s > 0 {
                        problems.push(format!(
                            "lean/{}: {s} sorry — Lean has been clean; do not start now",
                            p.file_name().unwrap_or_default().to_string_lossy()
                        ));
                    }
                }
            }
        }
    }

    if !problems.is_empty() {
        lines.push("FAIL: proof completeness changed:".into());
        lines.extend(problems.iter().map(|p| format!("  {p}")));
        lines.push("If a proof was discharged, SHRINK the ledger. If a new admit is".into());
        lines.push("intended, add it with a reason in-file.".into());
        return Verdict::Fail(lines);
    }
    let total: usize = ADMITTED_LEDGER.iter().map(|(_, v)| v).sum();
    lines.push(format!("ok: {} Rocq file(s), {lean_n} Lean file(s)", files.len()));
    lines.push(format!(
        "    {total} theorem(s) admitted across {} ledgered file(s); {} empty ledgered.",
        ADMITTED_LEDGER.len(),
        EMPTY_LEDGER.len()
    ));
    Verdict::Pass(lines)
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
    ck("bare `Admitted.` caught", count_admits("Admitted.\n") == 1);
    ck("indented `admit.` caught", count_admits("    admit.\n") == 1);
    // The false positive that fooled me: prose naming a Rust method `admit`.
    ck(
        "prose `Tasks::admit` NOT counted",
        count_admits("        * [Tasks::admit] clears the ready bit\n") == 0,
    );
    ck("inline `Qed.` counted", count_qed("Proof. trivial. Qed.\n") == 1);
    ck("theorem counted", count_theorems("Theorem foo : True.\n") == 1);
    ck("`sorry` whole word", count_sorry("exact sorry\n") == 1);
    ck("`sorryState` not a sorry", count_sorry("let sorryState = 1\n") == 0);
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

//! A moved directory leaves no live reference behind.
//!
//! The repository rework (docs/architecture/three-modes-two-axes.md, step 2)
//! moves directories out of `benches/gust/` one at a time. Each move rewrites
//! every reference to the old path. This gate is the check that the rewrite was
//! complete, and stays complete: for every move in `MOVES`, no tracked text file
//! may still name the old path, the new path must exist, and the old one must not.
//!
//! Scoped to the ledger on purpose. A whole-repository "every path exists" check
//! was measured on 2026-11-08 at 829 dangling references over 427 paths, most of
//! them not rot (paths into upstream Zephyr's tree, paths relative to a
//! subdirectory, paths in other repositories). A gate that noisy cannot fail on
//! the one thing a move breaks. This one can.
//!
//! Exempt: dated historical records (`HISTORICAL`), which keep the paths they had
//! when written, and this file, which has to name the old paths.
//!
//! Blind spot, stated: a reference written RELATIVE to a moved directory's old
//! parent (`../../targets/...` from inside the old tree) is not a string this gate
//! can match. Relative paths in code are caught by the build; relative paths in
//! prose are not caught at all.

use crate::verdict::Verdict;
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "moved-paths";

/// (old path, new path, why). Append one row per completed move; never remove a
/// row, so a later edit cannot quietly reintroduce an old path.
pub const MOVES: &[(&str, &str, &str)] = &[(
    "benches/gust/targets",
    "targets",
    "the AADL target model is hardware data, not a benchmark (rework step 2a)",
), (
    "benches/gust/drivers",
    "drivers",
    "drivers, providers and apps are OS components, not benchmarks (rework step 2b)",
)];

/// Dated records. They describe what was true when written, paths included.
const HISTORICAL: &[&str] = &[
    "docs/releases/",
    "docs/superpowers/",
    ".superpowers/",
    "docs/plans/",
    "tools/xtask/src/gates/moved_paths.rs",
];

/// Characters that may appear inside a path. An old path preceded or followed by
/// one of these is part of a different, longer name.
fn path_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.' | '/')
}

/// Every 1-based line in `text` that names `old` as a whole path (alone, or
/// followed by `/`), not as part of a longer name.
fn hits(text: &str, old: &str) -> Vec<usize> {
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        let mut from = 0;
        while let Some(i) = line[from..].find(old) {
            let at = from + i;
            let rest = &line[at + old.len()..];
            // Starts a path: nothing before it, or a non-path character.
            let starts = line[..at].chars().next_back().is_none_or(|c| !path_char(c));
            // Ends the name: end of line, `/`, a non-path character, or a
            // sentence-ending `.` ("... under benches/gust/targets.").
            let mut it = rest.chars();
            let ends = match it.next() {
                None | Some('/') => true,
                Some('.') => it.next().is_none_or(|d| !path_char(d)),
                Some(c) => !path_char(c),
            };
            if starts && ends {
                out.push(n + 1);
                break;
            }
            from = at + old.len();
        }
    }
    out
}

fn exempt(path: &str) -> bool {
    HISTORICAL.iter().any(|h| path == *h || path.starts_with(h))
}

fn tracked(repo: &Path) -> Option<Vec<String>> {
    let out = Command::new("git").arg("-C").arg(repo).args(["ls-files", "-z"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .split('\0')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect(),
    )
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    if MOVES.is_empty() {
        return Verdict::Refused("no moves recorded -- the check would be vacuous".into());
    }
    for (old, new, _) in MOVES {
        if !repo.join(new).is_dir() {
            return Verdict::Refused(format!(
                "ledger says {old} moved to {new}, but {new}/ does not exist -- the ledger is wrong"
            ));
        }
    }
    let Some(files) = tracked(repo) else {
        return Verdict::Refused("git ls-files failed -- the gate cannot answer".into());
    };
    let mut lines = Vec::new();
    let mut bad = Vec::new();
    let (mut scanned, mut binary) = (0usize, 0usize);
    for (old, new, _) in MOVES {
        if repo.join(old).exists() {
            bad.push(format!("{old}/ exists again, but the ledger says it moved to {new}/"));
        }
    }
    for f in &files {
        if exempt(f) {
            continue;
        }
        let Ok(bytes) = std::fs::read(repo.join(f)) else { continue };
        let Ok(text) = String::from_utf8(bytes) else {
            binary += 1;
            continue;
        };
        scanned += 1;
        for (old, new, _) in MOVES {
            for n in hits(&text, old) {
                bad.push(format!("{f}:{n} still names {old} (moved to {new})"));
            }
        }
    }
    if !bad.is_empty() {
        lines.push(format!("FAIL: {} live reference(s) to a moved path:", bad.len()));
        lines.extend(bad.into_iter().map(|b| format!("  {b}")));
        return Verdict::Fail(lines);
    }
    for (old, new, why) in MOVES {
        lines.push(format!("{old} -> {new}: no live reference ({why})"));
    }
    lines.push(format!(
        "{scanned} tracked text file(s) scanned, {binary} binary skipped, {} historical prefix(es) exempt.",
        HISTORICAL.len()
    ));
    Verdict::Pass(lines)
}

/// Each case is a way the scanner could be WRONG.
pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };
    let old = "benches/gust/targets";
    ck("a plain reference is a hit", hits("see benches/gust/targets/regen.sh", old) == vec![1]);
    ck("the bare directory is a hit", hits("under benches/gust/targets DO parse", old) == vec![1]);
    ck("a sentence-ending period is a hit", hits("lives in benches/gust/targets.", old) == vec![1]);
    ck("a quoted path is a hit", hits("repo.join(\"benches/gust/targets/generated\")", old) == vec![1]);
    ck("the line number is the right one", hits("a\nb\nx benches/gust/targets/y\n", old) == vec![3]);
    ck("a longer name is not a hit", hits("benches/gust/targets2/x and benches/gust/targets_old", old).is_empty());
    ck("a file with that stem is not a hit", hits("benches/gust/targets.md", old).is_empty());
    ck("a deeper path ending the same is not a hit", hits("vendor/benches/gust/targets/x", old).is_empty());
    ck("the new path is not a hit", hits("bash targets/regen.sh", old).is_empty());
    ck("a release note is exempt", exempt("docs/releases/v0.6.0.md"));
    ck("a dated plan is exempt", exempt("docs/superpowers/plans/2026-07-22-x.md"));
    ck("a live doc is not exempt", !exempt("docs/architecture/three-modes-two-axes.md"));
    ck("a workflow is not exempt", !exempt(".github/workflows/gust-targets.yml"));
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

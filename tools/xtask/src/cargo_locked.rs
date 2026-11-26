//! Every `cargo` that xtask runs is `--locked`. Without it a build may quietly
//! re-resolve and rewrite a committed Cargo.lock, which then fails gale's lock
//! census only afterwards, or builds against versions no lock records.
//! A test reads xtask's own sources: every cargo `Command` must name
//! `"--locked"` (or be a `--version` probe) within the next WINDOW lines.

#[cfg(test)]
const WINDOW: usize = 12;

/// The `file:line` of every cargo `Command` with no `--locked` or
/// `"--version"` within WINDOW lines.
#[cfg(test)]
fn unlocked(file: &str, src: &str) -> Vec<String> {
    let lines: Vec<&str> = src.lines().collect();
    let mut out = Vec::new();
    for (i, l) in lines.iter().enumerate() {
        if !l.contains("Command::new(\"cargo\")") {
            continue;
        }
        let end = (i + WINDOW).min(lines.len());
        let ok = lines[i..end].iter().any(|w| w.contains("\"--locked\"") || w.contains("\"--version\""));
        if !ok {
            out.push(format!("{file}:{}", i + 1));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::unlocked;
    use std::path::Path;

    fn sources(dir: &Path, acc: &mut Vec<(String, String)>) {
        for e in std::fs::read_dir(dir).expect("read src").flatten() {
            let p = e.path();
            if p.is_dir() {
                sources(&p, acc);
            } else if p.extension().is_some_and(|x| x == "rs") {
                acc.push((p.display().to_string(), std::fs::read_to_string(&p).expect("read file")));
            }
        }
    }

    #[test]
    fn every_cargo_xtask_runs_is_locked() {
        let mut files = Vec::new();
        sources(&Path::new(env!("CARGO_MANIFEST_DIR")).join("src"), &mut files);
        let calls: usize = files.iter().map(|(_, s)| s.matches("Command::new(\"cargo\")").count()).sum();
        // Positive control on the reader: the six known call sites (five builds
        // and the providers `--version` probe) are seen. This file's own
        // spellings sit inside string literals with escaped quotes, so they do
        // not count.
        assert!(calls >= 6, "found {calls} cargo calls; the scan is not reading xtask's sources");
        let bad: Vec<String> = files.iter().flat_map(|(f, s)| unlocked(f, s)).collect();
        assert!(bad.is_empty(), "cargo without --locked: {bad:?}");
    }

    #[test]
    fn planted_unlocked_call_is_caught() {
        let planted = "let o = Command::new(\"cargo\")\n    .args([\"build\", \"--release\"])\n    .output();\n";
        assert_eq!(unlocked("planted.rs", planted), vec!["planted.rs:1".to_string()]);
        let fixed = planted.replace("\"build\",", "\"build\", \"--locked\",");
        assert!(unlocked("fixed.rs", &fixed).is_empty());
        let probe = "Command::new(\"cargo\").arg(\"--version\")";
        assert!(unlocked("probe.rs", probe).is_empty());
        // `--locked` past the window does not count.
        let far = format!("Command::new(\"cargo\")\n{}.arg(\"--locked\")\n", "//\n".repeat(super::WINDOW));
        assert_eq!(unlocked("far.rs", &far).len(), 1);
    }
}

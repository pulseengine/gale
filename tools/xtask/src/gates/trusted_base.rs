//! The trusted-base figures in the honesty doc are computed, never typed.
//!
//! gale#467: `docs/safety/verification-honesty.md` said 133 `external_body`
//! instances, 2 `assume_specification` calls in `src/poll.rs`, and a per-module
//! table. The source said 142, in 12 files, and both `assume_specification`
//! items are in `src/sched.rs` (#388). The figure that lists what is trusted
//! had fallen behind the source it describes, and other repos copied it.
//!
//! So the inventory has one source, this module, and two consumers:
//!
//! - `cargo xtask trusted-base` prints it as JSON (schema 1, with the commit
//!   and the UTC time it was taken). The Pages workflow publishes that at a
//!   stable anonymous URL on every push to main that can change it.
//! - The honesty doc carries a block between `trusted-base:begin` and
//!   `trusted-base:end` markers that `cargo xtask trusted-base --write-doc`
//!   renders. This gate re-renders it from the source and fails if the doc's
//!   block differs by a single character, so the prose cannot drift again.
//!
//! What is counted, over comment-stripped text (a mention in a comment or a
//! doc comment is not a trust unit):
//! - `#[verifier::external_body]` (and the `#[verifier(external_body)]`
//!   spelling) in `src/**/*.rs` -- Verus trusts the contract, not the body;
//! - `assume_specification` items in `src/**/*.rs` -- a contract assumed for a
//!   function Verus never sees;
//! - line-initial `Admitted.` / `admit.` in `proofs/*.v` (the same anchor as
//!   proof-completeness, which owns the ratchet on those);
//! - `sorry` tokens in `proofs/lean/**/*.lean`.
//!
//! This gate does not judge whether the trusted base is the right size; the
//! vacuous-lemmas and proof-completeness ratchets do that. It only makes the
//! stated size the measured one.

use crate::gates::proof_completeness::count_admits;
use crate::gates::vacuous_lemmas::strip_comments;
use crate::verdict::Verdict;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

pub const NAME: &str = "trusted-base";

pub const DOC: &str = "docs/safety/verification-honesty.md";
pub const BEGIN: &str = "<!-- trusted-base:begin";
pub const END: &str = "<!-- trusted-base:end -->";

/// Where the Pages workflow publishes the JSON. Named here so the doc block,
/// the workflow and readers agree on one URL.
pub const PUBLISHED_URL: &str = "https://pulseengine.github.io/gale/trusted-base.json";

/// Per-file counts, keyed by repo-relative path. Files with a zero count are
/// left out so a reader's `by_file` is the list of files that carry trust.
#[derive(Debug, Default, PartialEq)]
pub struct Inventory {
    pub external_body: BTreeMap<String, usize>,
    pub assume_specification: BTreeMap<String, usize>,
    pub admitted_rocq: BTreeMap<String, usize>,
    pub sorry_lean: BTreeMap<String, usize>,
    /// How many files each scan read -- a scan that read nothing is refused,
    /// not reported as an empty trusted base.
    pub rust_files: usize,
    pub rocq_files: usize,
}

fn total(m: &BTreeMap<String, usize>) -> usize {
    m.values().sum()
}

/// `#[verifier::external_body]` or `#[verifier(external_body)]`, whitespace
/// inside the brackets tolerated.
fn count_external_body(stripped: &str) -> usize {
    let squashed: String = stripped.chars().filter(|c| !c.is_whitespace()).collect();
    squashed.matches("#[verifier::external_body]").count()
        + squashed.matches("#[verifier(external_body)]").count()
}

/// `assume_specification` as a whole token, outside string literals.
fn count_assume_spec(stripped: &str) -> usize {
    count_token(&blank_strings(stripped), "assume_specification")
}

/// `sorry` as a whole token in Lean source with `--` and `/- -/` comments
/// removed.
fn count_sorry(lean: &str) -> usize {
    count_token(&strip_lean_comments(lean), "sorry")
}

fn count_token(text: &str, tok: &str) -> usize {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_';
    let mut n = 0;
    let mut from = 0;
    while let Some(i) = text[from..].find(tok) {
        let at = from + i;
        let before = text[..at].chars().next_back();
        let after = text[at + tok.len()..].chars().next();
        if !before.is_some_and(is_ident) && !after.is_some_and(is_ident) {
            n += 1;
        }
        from = at + tok.len();
    }
    n
}

/// Replace the contents of `"..."` literals with spaces (strip_comments keeps
/// strings intact, which is right for it and wrong for a token count).
fn blank_strings(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_str = false;
    let mut esc = false;
    for c in s.chars() {
        if in_str {
            if esc {
                esc = false;
                out.push(' ');
            } else if c == '\\' {
                esc = true;
                out.push(' ');
            } else if c == '"' {
                in_str = false;
                out.push('"');
            } else {
                out.push(if c == '\n' { '\n' } else { ' ' });
            }
        } else {
            if c == '"' {
                in_str = true;
            }
            out.push(c);
        }
    }
    out
}

fn strip_lean_comments(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut depth = 0usize;
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if depth > 0 {
            if c == '/' && it.peek() == Some(&'-') {
                it.next();
                depth += 1;
            } else if c == '-' && it.peek() == Some(&'/') {
                it.next();
                depth -= 1;
            } else if c == '\n' {
                out.push('\n');
            }
        } else if c == '/' && it.peek() == Some(&'-') {
            it.next();
            depth = 1;
        } else if c == '-' && it.peek() == Some(&'-') {
            for d in it.by_ref() {
                if d == '\n' {
                    out.push('\n');
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for e in std::fs::read_dir(dir)? {
        let p = e?.path();
        if p.is_dir() {
            walk(&p, ext, out)?;
        } else if p.extension().and_then(|e| e.to_str()) == Some(ext) {
            out.push(p);
        }
    }
    Ok(())
}

fn rel(repo: &Path, p: &Path) -> String {
    p.strip_prefix(repo).unwrap_or(p).to_string_lossy().replace('\\', "/")
}

/// Measure the trusted base from the tree at `repo`.
pub fn inventory(repo: &Path) -> Result<Inventory, String> {
    let mut inv = Inventory::default();

    let mut rs = Vec::new();
    walk(&repo.join("src"), "rs", &mut rs).map_err(|e| format!("cannot walk src/: {e}"))?;
    rs.sort();
    for f in &rs {
        let text = std::fs::read_to_string(f).map_err(|e| format!("cannot read {}: {e}", f.display()))?;
        let s = strip_comments(&text);
        let (eb, asp) = (count_external_body(&s), count_assume_spec(&s));
        if eb > 0 {
            inv.external_body.insert(rel(repo, f), eb);
        }
        if asp > 0 {
            inv.assume_specification.insert(rel(repo, f), asp);
        }
    }
    inv.rust_files = rs.len();

    // Top-level proofs/*.v only: the same set proof-completeness and the Bazel
    // Rocq targets cover.
    let mut v: Vec<PathBuf> = std::fs::read_dir(repo.join("proofs"))
        .map_err(|e| format!("cannot read proofs/: {e}"))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("v"))
        .collect();
    v.sort();
    for f in &v {
        let text = std::fs::read_to_string(f).map_err(|e| format!("cannot read {}: {e}", f.display()))?;
        let n = count_admits(&text);
        if n > 0 {
            inv.admitted_rocq.insert(rel(repo, f), n);
        }
    }
    inv.rocq_files = v.len();

    let lean_dir = repo.join("proofs/lean");
    if lean_dir.is_dir() {
        let mut lean = Vec::new();
        walk(&lean_dir, "lean", &mut lean).map_err(|e| format!("cannot walk proofs/lean: {e}"))?;
        lean.sort();
        for f in lean.iter().filter(|p| !rel(repo, p).contains("/.lake/")) {
            let text = std::fs::read_to_string(f).map_err(|e| format!("cannot read {}: {e}", f.display()))?;
            let n = count_sorry(&text);
            if n > 0 {
                inv.sorry_lean.insert(rel(repo, f), n);
            }
        }
    }

    if inv.rust_files == 0 || inv.rocq_files == 0 {
        return Err(format!(
            "scanned {} Rust and {} Rocq files -- an empty scan is not an empty trusted base",
            inv.rust_files, inv.rocq_files
        ));
    }
    Ok(inv)
}

fn json_map(m: &BTreeMap<String, usize>) -> String {
    let items: Vec<String> = m.iter().map(|(k, v)| format!("\"{k}\": {v}")).collect();
    format!("{{ \"total\": {}, \"by_file\": {{ {} }} }}", total(m), items.join(", "))
}

/// The published document. `commit` and `generated_utc` are passed in so the
/// rendering is a pure function of the inventory (the self-test relies on it).
pub fn to_json(inv: &Inventory, commit: &str, generated_utc: &str) -> String {
    format!(
        "{{\n  \"schema\": 1,\n  \"repo\": \"pulseengine/gale\",\n  \"commit\": \"{commit}\",\n  \
         \"generated_utc\": \"{generated_utc}\",\n  \"scope\": {{ \"rust\": \"src/**/*.rs\", \
         \"rocq\": \"proofs/*.v\", \"lean\": \"proofs/lean/**/*.lean\" }},\n  \
         \"external_body\": {},\n  \"assume_specification\": {},\n  \
         \"admitted_rocq\": {},\n  \"sorry_lean\": {}\n}}\n",
        json_map(&inv.external_body),
        json_map(&inv.assume_specification),
        json_map(&inv.admitted_rocq),
        json_map(&inv.sorry_lean),
    )
}

/// Largest first, then by path, so the table reads as "where the trust is".
fn ranked(m: &BTreeMap<String, usize>) -> Vec<(&String, &usize)> {
    let mut v: Vec<_> = m.iter().collect();
    v.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    v
}

/// The doc block, markers included.
pub fn render_doc_block(inv: &Inventory) -> String {
    let (eb, asp, adm, sorry) = (
        total(&inv.external_body),
        total(&inv.assume_specification),
        total(&inv.admitted_rocq),
        total(&inv.sorry_lean),
    );
    let mut s = String::new();
    s.push_str(&format!(
        "{BEGIN} -- generated by `cargo xtask trusted-base --write-doc`; \
         `cargo xtask check trusted-base` fails if this block differs from the source -->\n"
    ));
    s.push_str(&format!(
        "**{eb}** `#[verifier::external_body]` functions and **{asp}** `assume_specification` \
         items across `src/`; **{adm}** `Admitted.` Rocq theorems across `proofs/*.v`; \
         **{sorry}** `sorry` in `proofs/lean`. Machine-readable, with the commit and time it \
         was measured: <{PUBLISHED_URL}>.\n\n"
    ));
    s.push_str("| File | `external_body` | `assume_specification` |\n|---|---:|---:|\n");
    let mut files: BTreeMap<&String, (usize, usize)> = BTreeMap::new();
    for (f, n) in &inv.external_body {
        files.entry(f).or_default().0 = *n;
    }
    for (f, n) in &inv.assume_specification {
        files.entry(f).or_default().1 = *n;
    }
    let mut rows: Vec<_> = files.into_iter().collect();
    rows.sort_by(|a, b| (b.1 .0 + b.1 .1).cmp(&(a.1 .0 + a.1 .1)).then(a.0.cmp(b.0)));
    for (f, (e, a)) in rows {
        s.push_str(&format!("| `{f}` | {e} | {a} |\n"));
    }
    s.push_str(&format!("| **total** | **{eb}** | **{asp}** |\n"));
    if !inv.admitted_rocq.is_empty() {
        s.push_str("\n| Rocq file | `Admitted.` |\n|---|---:|\n");
        for (f, n) in ranked(&inv.admitted_rocq) {
            s.push_str(&format!("| `{f}` | {n} |\n"));
        }
        s.push_str(&format!("| **total** | **{adm}** |\n"));
    }
    if !inv.sorry_lean.is_empty() {
        s.push_str("\n| Lean file | `sorry` |\n|---|---:|\n");
        for (f, n) in ranked(&inv.sorry_lean) {
            s.push_str(&format!("| `{f}` | {n} |\n"));
        }
    }
    s.push_str(END);
    s
}

/// The (start, end) byte range of the block in `doc`, end exclusive and
/// covering the END marker. Exactly one block is accepted.
fn find_block(doc: &str) -> Result<(usize, usize), String> {
    let starts: Vec<_> = doc.match_indices(BEGIN).map(|(i, _)| i).collect();
    let ends: Vec<_> = doc.match_indices(END).map(|(i, _)| i).collect();
    match (starts.as_slice(), ends.as_slice()) {
        ([s], [e]) if s < e => Ok((*s, *e + END.len())),
        ([], []) => Err(format!("{DOC} has no trusted-base block -- the figure would be unchecked")),
        _ => Err(format!(
            "{DOC} has {} begin and {} end marker(s); exactly one ordered pair is required",
            starts.len(),
            ends.len()
        )),
    }
}

/// Compare the doc's block with the rendering; the first differing line is the
/// diagnostic.
fn check_doc(doc: &str, inv: &Inventory) -> Result<(), Vec<String>> {
    let (s, e) = find_block(doc).map_err(|m| vec![m])?;
    let want = render_doc_block(inv);
    let got = &doc[s..e];
    if got == want {
        return Ok(());
    }
    let mut out = vec![format!(
        "{DOC}: the trusted-base block does not match the source -- run \
         `cargo xtask trusted-base --write-doc`"
    )];
    for (i, (g, w)) in got.lines().zip(want.lines()).enumerate() {
        if g != w {
            out.push(format!("  first difference, block line {}:", i + 1));
            out.push(format!("    doc:    {g}"));
            out.push(format!("    source: {w}"));
            return Err(out);
        }
    }
    out.push(format!(
        "  the block has {} line(s), the source renders {}",
        got.lines().count(),
        want.lines().count()
    ));
    Err(out)
}

/// Rewrite the doc's block in place from the source.
pub fn write_doc(repo: &Path) -> Result<String, String> {
    let inv = inventory(repo)?;
    let path = repo.join(DOC);
    let doc = std::fs::read_to_string(&path).map_err(|e| format!("cannot read {DOC}: {e}"))?;
    let (s, e) = find_block(&doc)?;
    let new = format!("{}{}{}", &doc[..s], render_doc_block(&inv), &doc[e..]);
    if new != doc {
        std::fs::write(&path, new).map_err(|e| format!("cannot write {DOC}: {e}"))?;
        Ok(format!("{DOC}: block rewritten"))
    } else {
        Ok(format!("{DOC}: block already current"))
    }
}

/// The commit the counts describe: `GITHUB_SHA` in CI, else `git rev-parse HEAD`
/// with `-dirty` when the tree has local changes (a dirty tree's counts are not
/// any commit's).
pub fn commit(repo: &Path) -> Result<String, String> {
    if let Ok(sha) = std::env::var("GITHUB_SHA") {
        if !sha.is_empty() {
            return Ok(sha);
        }
    }
    let git = |args: &[&str]| {
        std::process::Command::new("git")
            .arg("-C")
            .arg(repo)
            .args(args)
            .output()
            .map_err(|e| format!("git: {e}"))
    };
    let head = git(&["rev-parse", "HEAD"])?;
    if !head.status.success() {
        return Err("git rev-parse HEAD failed".into());
    }
    let sha = String::from_utf8_lossy(&head.stdout).trim().to_string();
    let dirty = git(&["status", "--porcelain", "--", "src", "proofs"])?;
    Ok(if dirty.stdout.is_empty() { sha } else { format!("{sha}-dirty") })
}

/// Seconds since the epoch as `YYYY-MM-DDTHH:MM:SSZ` (civil-from-days; no
/// date crate for one timestamp).
pub fn utc(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let inv = match inventory(repo) {
        Ok(i) => i,
        Err(e) => return Verdict::Refused(e),
    };
    let doc = match std::fs::read_to_string(repo.join(DOC)) {
        Ok(d) => d,
        Err(e) => return Verdict::Refused(format!("cannot read {DOC}: {e}")),
    };
    match check_doc(&doc, &inv) {
        Ok(()) => Verdict::Pass(vec![format!(
            "{DOC} states the measured trusted base: {} external_body, {} assume_specification, \
             {} Admitted (Rocq), {} sorry (Lean), over {} Rust and {} Rocq files.",
            total(&inv.external_body),
            total(&inv.assume_specification),
            total(&inv.admitted_rocq),
            total(&inv.sorry_lean),
            inv.rust_files,
            inv.rocq_files
        )]),
        Err(lines) => Verdict::Fail(lines),
    }
}

// rivet: verifies REQ-HONESTY-TCB-001
/// Each case is a way the gate could be WRONG: count a mention as a trust
/// unit, miss a spelling, accept a stale figure, or pass a doc with no block.
pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };

    let rs = r#"
/// Wraps the array with #[verifier::external_body] -- a doc comment, not a unit.
// #[verifier::external_body]
/* #[verifier::external_body] */
#[verifier::external_body]
fn a() {}
#[ verifier :: external_body ]
fn b() {}
#[verifier(external_body)]
fn c() {}
pub assume_specification [<T as PartialEq>::eq](a: &T, b: &T) -> bool;
// assume_specification in a comment
const S: &str = "assume_specification";
fn my_assume_specification_helper() {}
"#;
    let s = strip_comments(rs);
    ck("three external_body spellings counted, comment mentions not", count_external_body(&s) == 3);
    ck(
        "one assume_specification item; comment, string and identifier mentions not",
        count_assume_spec(&s) == 1,
    );

    let v = "Theorem t : True.\nProof. Admitted.\n(* Admitted. in a comment *)\nAdmitted.\n  admit.\n";
    ck("line-initial Admitted./admit. counted, comment not", count_admits(v) == 2);

    let lean = "theorem a : True := by sorry\n-- sorry\n/- sorry /- nested sorry -/ -/\ndef sorryish := 1\n";
    ck("Lean sorry counted once; comments and identifiers not", count_sorry(lean) == 1);

    let mut inv = Inventory::default();
    inv.external_body.insert("src/a.rs".into(), 3);
    inv.external_body.insert("src/b.rs".into(), 5);
    inv.assume_specification.insert("src/a.rs".into(), 1);
    inv.admitted_rocq.insert("proofs/x.v".into(), 2);
    inv.rust_files = 2;
    inv.rocq_files = 1;
    let block = render_doc_block(&inv);
    let good = format!("# Doc\n\nprose\n\n{block}\n\nmore prose\n");
    ck("a doc carrying the rendered block passes", check_doc(&good, &inv).is_ok());

    let stale = good.replace("**8** `#[verifier::external_body]`", "**133** `#[verifier::external_body]`");
    ck("the negative control is applied (total edited)", stale != good);
    ck("a hand-edited total fails", check_doc(&stale, &inv).is_err());

    let row = good.replace("| `src/b.rs` | 5 | 0 |", "| `src/b.rs` | 4 | 0 |");
    ck("a hand-edited row fails", row != good && check_doc(&row, &inv).is_err());

    let mut grown = Inventory { rust_files: 2, rocq_files: 1, ..Default::default() };
    grown.external_body = inv.external_body.clone();
    grown.external_body.insert("src/c.rs".into(), 1);
    grown.assume_specification = inv.assume_specification.clone();
    grown.admitted_rocq = inv.admitted_rocq.clone();
    ck("a new trust unit in the source fails an unchanged doc", check_doc(&good, &grown).is_err());

    ck("a doc with no block fails", check_doc("# Doc\n133 external_body\n", &inv).is_err());
    let twice = format!("{good}\n{block}\n");
    ck("a doc with two blocks fails", check_doc(&twice, &inv).is_err());

    let j = to_json(&inv, "abc", "2026-11-07T00:00:00Z");
    ck(
        "JSON carries totals, by_file, commit and time",
        j.contains("\"external_body\": { \"total\": 8, \"by_file\": { \"src/a.rs\": 3, \"src/b.rs\": 5 } }")
            && j.contains("\"commit\": \"abc\"")
            && j.contains("\"generated_utc\": \"2026-11-07T00:00:00Z\"")
            && j.contains("\"admitted_rocq\": { \"total\": 2,"),
    );
    ck("utc(0) is the epoch", utc(0) == "1970-01-01T00:00:00Z");
    ck("utc on a leap day", utc(1_709_210_096) == "2024-02-29T12:34:56Z");

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

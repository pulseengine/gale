//! The trust ledger: a count of every input-narrowing assumption SPELLING the
//! gate can see, per file, held equal to (later: no larger than) a committed
//! ledger (v0.11.0 bet 3, gale#467 follow-on).
//!
//! `trusted-base` keeps the honesty doc's figures equal to the source for the
//! four classes it publishes (Verus `external_body` and `assume_specification`
//! in `src/`, Rocq `Admitted`, Lean `sorry`). It does not judge growth, and it
//! cannot see the assumptions a Kani harness narrows its inputs with. This gate
//! sits beside it: it reuses the same spelling rules and adds the Kani
//! narrowing spellings, over every tracked `.rs`/`.v`/`.lean` file (and the
//! Bazel files, for `-impredicative-set`) rather than `src/` alone, and it
//! holds the result against `tools/xtask/trust-ledger.ledger`.
//!
//! Policy (the plan's ratchet, one explicit field in the ledger):
//!
//! - `mode = equality` (until the baseline is set): any change in any
//!   (class, file) row fails -- growth, AND an unrecorded shrink. A change is
//!   admitted only by the same commit editing the row, naming the artifact that
//!   justifies it.
//! - `mode = shrink-only` (set once, in the first commit AFTER the release
//!   tag, with `baseline = <the tagged sha>`): the ledger AT the baseline
//!   (`git show <sha>:tools/xtask/trust-ledger.ledger`) plus the matching
//!   `admit` rows is the ceiling -- every row of the current ledger must be no
//!   larger than its baseline row, and a row absent from the baseline is
//!   growth, unless an `admit` row names it. The tree must be no larger than
//!   the current ledger; a shrink passes and is listed so the ledger can be
//!   tightened.
//!
//! The baseline must be a `v*`-tagged ancestor of HEAD and the NEWEST such
//! release by version (`git tag --merged HEAD`, not the nearest tag), not
//! counting a tag on HEAD itself (a ledger cannot name the commit that
//! contains it): a tagged release commit is green when its baseline names the
//! release before it. Once any ledger in HEAD's history (`git log
//! --full-history -m -G`, uncapped, so neither a merge nor a long history can
//! hide it) was shrink-only, switching back to equality keeps that newest
//! release's ledger as the ceiling. Baseline paths follow exact renames
//! (`git diff -M100%`) to HEAD.
//!
//! Growth above the ceiling (owner decision, L11 round 4): an
//! `admit <class> <path> +<N> @<baseline> <#NNN | owner/repo#NNN> <reason>` row
//! raises the ceiling for that one item by exactly N, against the baseline it
//! names (its sha or v* tag): an admit left in place when the baseline moves
//! fails (round 5, A1). Growth with no matching admit fails;
//! an admit whose item does not now stand at exactly baseline + N (a stale or
//! oversized admit) fails; a second admit for the same item fails; a malformed
//! issue reference or a missing reason fails. The gate does NOT fetch the
//! issue. It shows an admit names the current baseline and is NOT already in
//! the ledger at that baseline bound to it (round 6, A1': a row pre-written
//! in the release commit, naming that release's future tag, would otherwise
//! grant growth after the fold untouched): the row is absent from the ledger
//! at the baseline commit. It cannot show when or by whom the row was written,
//! that anyone reviewed it, or that the owner approved it.
//!
//! At a release: the tagged commit is still held to the previous release plus
//! its admits (green). After the tag, the next commit is red until it moves
//! `baseline` to the new release (the ceiling only moves forward; a tag alone
//! reddens the next commit, round 4's T4), and that same commit folds the
//! admits in: the new baseline's ledger already carries the admitted growth,
//! so each admit then no longer matches (baseline + N exceeds the tree) and
//! must be deleted.
//!
//! An unwind bound is recorded as the multiset of bounds per file, so lowering
//! one is a change even though the attribute count is unchanged. In
//! shrink-only mode the bounds may only lose attributes (a sub-multiset) or
//! only be raised (same count, each sorted bound >= its partner); anything
//! else -- a removal together with a change, a `?` gained -- is ambiguous and
//! fails. The rule is applied to the ledger against the baseline AND to the
//! tree against the baseline directly, not only through the ledger: the
//! relation does not compose (round 11, F3: a removal recorded in the ledger
//! plus a "raise" in the tree hid a lowered bound). With an admit, every
//! baseline bound must still be in the tree unchanged.
//!
//! What it counts is listed by `CLASSES`; what it cannot see is `CANNOT_SEE`,
//! printed on every run. It counts spellings, not how much each one narrows.

use crate::gates::lex::{cfg_attr_items, count_token, lean_code, lean_unmodelled, rocq_code, rocq_sentences, rust_code, token_positions};
use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

pub const NAME: &str = "trust-ledger";
pub const LEDGER: &str = "tools/xtask/trust-ledger.ledger";

/// The rule for growth above the ceiling (owner decision on T1, L11 round 4;
/// round 3's finding 12 had found the old text promising an "owner-approved
/// entry" that did not exist).
const GROWTH_RULE: &str = "growth above the ceiling (the ledger at the newest v* release before HEAD) needs an \
     `admit <class> <path> +<N> @<baseline> <#issue> <reason>` row in the ledger matching it exactly and naming the current \
     baseline; the gate checks the row is recorded against that baseline, not that anyone reviewed or approved it";

/// The gate's own sources carry every spelling inside self-test literals. They
/// are excluded by name, and the exclusion is printed.
const EXCLUDED_PREFIXES: &[&str] = &["tools/xtask/"];

/// (class, language, what is counted)
pub const CLASSES: &[(&str, &str, &str)] = &[
    ("kani::assume", "rs", "the path `kani::assume`: calls, `kani::assume::<>(..)`, `(kani::assume)(..)`, and a `use`/`let` naming it"),
    ("kani::any_where", "rs", "`kani::any_where` (a generator with a built-in assumption)"),
    ("kani::unwind", "rs", "`#[kani::unwind(N)]` attributes (also inside `cfg_attr`), with the bound N"),
    ("verus:external_body", "rs", "`#[verifier::external_body]` / `#[verifier(external_body)]` / `#![...]` / inside `cfg_attr`"),
    ("verus:assume_specification", "rs", "`assume_specification` items"),
    ("verus:axiom_fn", "rs", "`axiom fn` / `broadcast axiom fn`"),
    ("verus:assume", "rs", "bare `assume(` (not `kani::assume`, not a method or `fn assume`)"),
    ("verus:admit", "rs", "bare `admit(` (not a method or `fn admit`)"),
    ("rocq:Admitted", "v", "`Admitted`"),
    ("rocq:admit", "v", "the `admit` / `give_up` tactics"),
    ("rocq:Admit-Obligations", "v", "`Admit Obligations`"),
    ("rocq:Axiom", "v", "`Axiom` / `Axioms` / `Conjecture`"),
    ("rocq:Parameter", "v", "`Parameter` / `Parameters`"),
    ("rocq:Variable", "v", "`Variable(s)` / `Hypothesis` / `Hypotheses` / `Context` outside a Section (an axiom there)"),
    ("rocq:impredicative-set", "bazel", "Rocq targets built with `-impredicative-set` (a changed logic): per literal, times a `for x in [...]` list, plus each call of a .bzl macro that sets it"),
    ("lean:sorry", "lean", "`sorry`"),
    ("lean:sorryAx", "lean", "`sorryAx` written out"),
    ("lean:admit", "lean", "the `admit` tactic"),
    ("lean:axiom", "lean", "`axiom` declarations"),
];

pub const CANNOT_SEE: &[&str] = &[
    "nothing about a `v<digit>` tag that is not vMAJOR.MINOR.PATCH or a -pre-release (`v1.2`, `v1.2.0rc1`, `v1.2.0+b1`; `vendor-x` is ignored): the newest release cannot be ordered, so every run FAILS until the tag is renamed or deleted (fails closed, never green)",
    "whether the owner approved an `admit` row: the gate checks its format (an issue reference, a reason) that it matches the tree exactly, that it names the current baseline, and that the row is ABSENT from the ledger at that baseline commit -- not when or by whom it was written, nor whether anyone reviewed it; it does not fetch the issue",
    "a rename that also edits the file (only exact `-M100%` renames carry a baseline row to its new path; anything else is growth in the new path)",
    "assumptions written as narrowed types or bounded generators (e.g. `kani::any::<u8>()` where u32 is the domain)",
    "Verus `requires` clauses and spec-level preconditions; `#[verifier::external]`, `external_fn_specification`, `external_type_specification`",
    "aliases: the calls made through `use kani::assume as a;` or `let f = kani::assume;` (the naming itself is counted once), `use kani as k;` + `k::assume(`, `use kani::assume;` + bare `assume(` (counted as verus:assume), `vstd::prelude::assume(` (a path call, not counted)",
    "Lean string interpolation (`s!\"{..}\"`, `m!`, `f!`, `throwError`, any `interpolatedStr` syntax): its braces hold code the lexer does not model, so a .lean file with an unescaped `{` inside a string literal is REFUSED (fails closed), never counted around",
    "Kani contracts and stubs: `#[kani::stub]`, `#[kani::requires]`, `#[kani::proof_for_contract]`",
    "harnesses or assumptions produced by macros, and `include!` of a non-.rs file",
    "unwind bounds given on the command line (`--default-unwind`, `--unwind`) or a non-literal `#[kani::unwind(CONST)]` bound (recorded as `?`)",
    "Rocq `Hypothesis`/`Variable` inside a Section, `Opaque`, `Unset Guard Checking`, `#[bypass_check]`, and axioms of external libraries (RocqOfRust, Stdlib) -- no gate in this repo reads them yet (an axiom audit from outside the file is designed in docs/design/axioms-trusted-audit.md)",
    "Lean `@[implemented_by]`, `unsafe`, `native_decide`, `stop` -- no gate in this repo reads their axioms yet (docs/design/axioms-trusted-audit.md)",
    "`-impredicative-set` reaching a target through a variable, an aliased macro name, or a macro called from another .bzl",
    "files that are not tracked by git, and the excluded prefixes",
];

fn lang_of(path: &str) -> Option<&'static str> {
    let base = path.rsplit('/').next().unwrap_or(path);
    if path.ends_with(".rs") {
        Some("rs")
    } else if path.ends_with(".v") {
        Some("v")
    } else if path.ends_with(".lean") {
        Some("lean")
    } else if base == "BUILD" || base.ends_with(".bazel") || base.ends_with(".bzl") {
        Some("bazel")
    } else {
        None
    }
}

/// Remove whitespace next to punctuation, keep one space between identifier
/// characters: `#[ kani :: unwind ( 5 ) ]` -> `#[kani::unwind(5)]`, while
/// `return kani::assume` keeps its token boundary. A reformat cannot change it.
fn tighten(s: &str) -> String {
    let is_id = |c: char| c.is_alphanumeric() || c == '_';
    let mut out = String::with_capacity(s.len());
    let mut pending_ws = false;
    for ch in s.chars() {
        if ch.is_whitespace() {
            pending_ws = true;
            continue;
        }
        if pending_ws && out.chars().next_back().is_some_and(is_id) && is_id(ch) {
            out.push(' ');
        }
        pending_ws = false;
        out.push(ch);
    }
    out
}

/// One file's counts. `unwind` holds the bounds (`None` = not a literal).
#[derive(Debug, Default, Clone, PartialEq)]
pub struct FileCounts {
    pub counts: BTreeMap<&'static str, usize>,
    pub unwind: Vec<Option<u64>>,
}

fn unwind_bound(lit: &str) -> Option<u64> {
    lit.chars().filter(|c| *c != '_').collect::<String>().parse::<u64>().ok()
}

/// `#[kani::proof]` attributes in one file: the attribute form plus `cfg_attr` items.
pub fn kani_proofs(text: &str) -> usize {
    let t = tighten(&rust_code(text));
    t.matches("#[kani::proof]").count() + cfg_attr_items(&t).iter().filter(|i| *i == "kani::proof").count()
}

/// `#[kani::proof_for_contract(f)]` harnesses in one file (attribute and
/// `cfg_attr` forms). `cargo kani` runs them like `#[kani::proof]`; they are
/// kept apart so trust-ledger's `#[kani::proof]` figure keeps its meaning.
pub fn kani_contract_proofs(text: &str) -> usize {
    let t = tighten(&rust_code(text));
    t.matches("#[kani::proof_for_contract(").count() + cfg_attr_items(&t).iter().filter(|i| i.starts_with("kani::proof_for_contract(")).count()
}

/// Rocq declarations outside a Section that a Section would scope:
/// `Variable(s)`, `Hypothesis`, `Hypotheses`, `Context`.
fn rocq_top_level_variables(code: &str) -> usize {
    let mut stack: Vec<(bool, String)> = Vec::new(); // (is_section, name)
    let mut n = 0;
    for s in rocq_sentences(code) {
        let mut s = s.trim_start_matches(|c: char| c.is_whitespace() || "-+*{}".contains(c)).trim_end_matches('.').trim();
        while s.starts_with("#[") {
            s = s.find(']').map_or("", |e| s[e + 1..].trim_start());
        }
        for m in ["Local ", "Global ", "Polymorphic ", "Monomorphic "] {
            s = s.strip_prefix(m).unwrap_or(s).trim_start();
        }
        let mut w = s.split_whitespace();
        let (kw, arg) = (w.next().unwrap_or(""), w.next().unwrap_or(""));
        match kw {
            "Section" => stack.push((true, arg.to_string())),
            "Module" if arg != "Type" && !s.contains(":=") => stack.push((false, arg.to_string())),
            "End" => {
                if stack.last().is_some_and(|(_, n)| n == arg) {
                    stack.pop();
                }
            }
            "Variable" | "Variables" | "Hypothesis" | "Hypotheses" | "Context" if !stack.iter().any(|(sec, _)| *sec) => n += 1,
            _ => {}
        }
    }
    n
}

/// Count every class over one file's text.
pub fn count_file(path: &str, text: &str) -> FileCounts {
    let mut fc = FileCounts::default();
    let put = |k: &'static str, n: usize, fc: &mut FileCounts| {
        if n > 0 {
            fc.counts.insert(k, n);
        }
    };
    match lang_of(path) {
        Some("rs") => {
            let t = tighten(&rust_code(text));
            let cfg = cfg_attr_items(&t);
            // Every `kani::assume` path, not only `kani::assume(`: rustc also runs
            // `(kani::assume)(c)` and `kani::assume::<>(c)` (round 11, F6).
            put("kani::assume", token_positions(&t, "assume").into_iter().filter(|&i| t[..i].ends_with("kani::")).count(), &mut fc);
            put("kani::any_where", token_positions(&t, "any_where").into_iter().filter(|&i| t[..i].ends_with("kani::")).count(), &mut fc);
            let mut from = 0;
            while let Some(i) = t[from..].find("#[kani::unwind(") {
                let at = from + i + "#[kani::unwind(".len();
                let end = t[at..].find(')').map(|e| at + e).unwrap_or(at);
                fc.unwind.push(unwind_bound(&t[at..end]));
                from = end;
            }
            for item in &cfg {
                if let Some(lit) = item.strip_prefix("kani::unwind(").and_then(|r| r.strip_suffix(')')) {
                    fc.unwind.push(unwind_bound(lit));
                }
            }
            fc.unwind.sort();
            put("kani::unwind", fc.unwind.len(), &mut fc);
            put(
                "verus:external_body",
                t.matches("#[verifier::external_body]").count()
                    + t.matches("#[verifier(external_body)]").count()
                    + t.matches("#![verifier::external_body]").count()
                    + t.matches("#![verifier(external_body)]").count()
                    + cfg.iter().filter(|i| *i == "verifier::external_body" || *i == "verifier(external_body)").count(),
                &mut fc,
            );
            put("verus:assume_specification", count_token(&t, "assume_specification"), &mut fc);
            put("verus:axiom_fn", token_positions(&t, "axiom").into_iter().filter(|&i| t[i + "axiom".len()..].starts_with(" fn ")).count(), &mut fc);
            put("verus:assume", bare_calls(&t, "assume"), &mut fc);
            put("verus:admit", bare_calls(&t, "admit"), &mut fc);
        }
        Some("v") => {
            let t = rocq_code(text);
            put("rocq:Admitted", count_token(&t, "Admitted"), &mut fc);
            put("rocq:admit", count_token(&t, "admit") + count_token(&t, "give_up"), &mut fc);
            put(
                "rocq:Admit-Obligations",
                token_positions(&t, "Admit").into_iter().filter(|&i| t[i + "Admit".len()..].trim_start().starts_with("Obligations")).count(),
                &mut fc,
            );
            put(
                "rocq:Axiom",
                count_token(&t, "Axiom") + count_token(&t, "Axioms") + count_token(&t, "Conjecture"),
                &mut fc,
            );
            put("rocq:Parameter", count_token(&t, "Parameter") + count_token(&t, "Parameters"), &mut fc);
            put("rocq:Variable", rocq_top_level_variables(&t), &mut fc);
        }
        Some("lean") => {
            let t = lean_code(text);
            put("lean:sorry", count_token(&t, "sorry"), &mut fc);
            put("lean:sorryAx", count_token(&t, "sorryAx"), &mut fc);
            put("lean:admit", count_token(&t, "admit"), &mut fc);
            put("lean:axiom", count_token(&t, "axiom"), &mut fc);
        }
        Some("bazel") => {
            let n: usize = bazel_statements(text).iter().map(|s| flag_targets(s)).sum();
            put("rocq:impredicative-set", n, &mut fc);
        }
        _ => {}
    }
    fc
}

const IMPRED: &str = "-impredicative-set";

/// Starlark with comments removed and adjacent string literals joined
/// (`"-impredicative-" + "set"` -> `"-impredicative-set"`), split into
/// top-level statements (each starts at a non-blank column-0 line).
fn bazel_statements(text: &str) -> Vec<String> {
    let mut code = String::new();
    for l in text.lines() {
        let mut in_str: Option<char> = None;
        for ch in l.chars() {
            match (in_str, ch) {
                (None, '#') => break,
                (None, '"' | '\'') => in_str = Some(ch),
                (Some(q), c) if c == q => in_str = None,
                _ => {}
            }
            code.push(ch);
        }
        code.push('\n');
    }
    // Join `"a" + "b"` / `'a' + 'b'` (also across a line break).
    let c: Vec<char> = code.chars().collect();
    let mut joined = String::new();
    let mut i = 0;
    while i < c.len() {
        if c[i] == '"' || c[i] == '\'' {
            let mut j = i + 1;
            while j < c.len() && c[j].is_whitespace() {
                j += 1;
            }
            if c.get(j) == Some(&'+') {
                let mut k = j + 1;
                while k < c.len() && c[k].is_whitespace() {
                    k += 1;
                }
                if c.get(k) == Some(&c[i]) && joined.chars().filter(|x| *x == c[i]).count() % 2 == 1 {
                    i = k + 1;
                    continue;
                }
            }
        }
        joined.push(c[i]);
        i += 1;
    }
    let mut out: Vec<String> = Vec::new();
    for l in joined.lines() {
        if !l.trim().is_empty() && !l.starts_with(char::is_whitespace) && !l.starts_with(')') && !l.starts_with(']') {
            out.push(String::new());
        }
        if let Some(s) = out.last_mut() {
            s.push_str(l);
            s.push('\n');
        }
    }
    out
}

/// Targets one statement sets the flag on: each literal, times the length of
/// the list a `for X in [...]` comprehension in the statement iterates.
fn flag_targets(stmt: &str) -> usize {
    let n = stmt.matches(&format!("\"{IMPRED}\"")).count() + stmt.matches(&format!("'{IMPRED}'")).count();
    if n == 0 {
        return 0;
    }
    let mult = stmt
        .find(" in [")
        .filter(|&i| stmt[..i].contains("for "))
        .and_then(|i| stmt[i + 5..].split_once(']').map(|(list, _)| list.matches('"').count() / 2 + list.matches('\'').count() / 2))
        .filter(|m| *m > 0)
        .unwrap_or(1);
    n * mult
}

/// Names of `.bzl` macros (`def NAME(`) that set the flag themselves.
fn flag_macros(text: &str) -> Vec<String> {
    bazel_statements(text)
        .iter()
        .filter(|s| flag_targets(s) > 0)
        .filter_map(|s| s.strip_prefix("def ").map(|r| r.chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect()))
        .collect()
}

/// `name(` as a free call: not `x.name(`, not `path::name(`, not `fn name(`.
fn bare_calls(t: &str, name: &str) -> usize {
    token_positions(t, name)
        .into_iter()
        .filter(|&at| {
            let after = &t[at + name.len()..];
            if !after.starts_with('(') {
                return false;
            }
            let before = &t[..at];
            !(before.ends_with('.') || before.ends_with("::") || before.ends_with("fn ") || before.ends_with("fn"))
        })
        .count()
}

/// The whole census: per file, files with no spelling left out.
#[derive(Debug, Default, PartialEq)]
pub struct Census {
    pub files: BTreeMap<String, FileCounts>,
    pub scanned: BTreeMap<&'static str, usize>,
    /// `#[kani::proof]` attributes in tracked source (attribute and `cfg_attr`
    /// forms) -- a spelling count, not the number CI compiles or runs.
    pub kani_proofs: usize,
    /// Files the lexer cannot model (a Lean string with an unescaped `{`: a
    /// possible interpolation), as `path:line`. Any entry REFUSES the run.
    pub unmodelled: Vec<String>,
}

pub fn census_of<'a, I: IntoIterator<Item = (&'a str, &'a str)>>(files: I) -> Census {
    let mut c = Census::default();
    let mut macros: Vec<String> = Vec::new();
    let mut bazel: Vec<(&str, &str)> = Vec::new();
    for (path, text) in files {
        if EXCLUDED_PREFIXES.iter().any(|p| path.starts_with(p)) {
            continue;
        }
        let Some(lang) = lang_of(path) else { continue };
        *c.scanned.entry(lang).or_default() += 1;
        if lang == "rs" {
            c.kani_proofs += kani_proofs(text);
        }
        if lang == "lean" {
            if let Some(line) = lean_unmodelled(text) {
                c.unmodelled.push(format!("{path}:{line}"));
            }
        }
        if lang == "bazel" {
            if path.ends_with(".bzl") {
                macros.extend(flag_macros(text));
            }
            bazel.push((path, text));
        }
        let fc = count_file(path, text);
        if !fc.counts.is_empty() {
            c.files.insert(path.to_string(), fc);
        }
    }
    // Each call of a flag-setting macro is one more target, on the calling file.
    for (path, text) in bazel {
        let calls: usize = bazel_statements(text)
            .iter()
            .filter(|s| !s.starts_with("def ") && !s.starts_with("load("))
            .map(|s| macros.iter().map(|m| token_positions(s, m).into_iter().filter(|&i| s[i + m.len()..].starts_with('(')).count()).sum::<usize>())
            .sum();
        if calls > 0 {
            *c.files.entry(path.to_string()).or_default().counts.entry("rocq:impredicative-set").or_default() += calls;
        }
    }
    c
}

/// An empty scan of any language is a broken scan, not an empty ledger.
pub fn check_scanned(c: &Census) -> Result<(), String> {
    if !c.unmodelled.is_empty() {
        return Err(format!(
            "a Lean string literal holds an unescaped `{{` at {} -- possibly an interpolation (`s!\"{{..}}\"`, `m!`, `f!`, `throwError`), whose braces hold code the lexer does not model; refusing rather than count around it (escape it as `\\{{` if it is a literal brace)",
            c.unmodelled.join(", ")
        ));
    }
    for lang in ["rs", "v", "lean", "bazel"] {
        if c.scanned.get(lang).copied().unwrap_or(0) == 0 {
            return Err(format!("scanned 0 {lang} files -- an empty scan is not an empty ledger"));
        }
    }
    Ok(())
}

/// Read every tracked `.rs`/`.v`/`.lean`/Bazel file.
pub fn census(repo: &Path) -> Result<Census, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["ls-files", "-z", "--", "*.rs", "*.v", "*.lean", "*.bazel", "*.bzl", "BUILD", "*/BUILD"])
        .output()
        .map_err(|e| format!("git ls-files: {e}"))?;
    if !out.status.success() {
        return Err("git ls-files failed".into());
    }
    let list = String::from_utf8_lossy(&out.stdout).to_string();
    let mut texts = Vec::new();
    for p in list.split('\0').filter(|p| !p.is_empty()) {
        // A tracked file deleted in the working tree is not a file to count.
        match std::fs::read(repo.join(p)) {
            Ok(b) => texts.push((p.to_string(), String::from_utf8_lossy(&b).to_string())),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("cannot read {p}: {e}")),
        }
    }
    let c = census_of(texts.iter().map(|(p, t)| (p.as_str(), t.as_str())));
    check_scanned(&c)?;
    Ok(c)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Mode {
    Equality,
    ShrinkOnly,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub class: String,
    pub path: String,
    pub count: usize,
    pub bounds: Vec<Option<u64>>,
    pub id: String,
}

/// An `admit` row: growth above the ceiling for one (class, file), by exactly
/// `n`, with the GitHub issue that tracks it and a reason (owner decision on
/// T1, L11 round 4). The gate checks the row's FORMAT and that it matches the
/// tree and the current baseline; it does not fetch the issue, so it shows the
/// admit is recorded, not that anyone reviewed or approved it.
#[derive(Debug, Clone, PartialEq)]
pub struct Admit {
    pub class: String,
    pub path: String,
    pub n: usize,
    /// The baseline (40-hex sha or v* tag) the admit was written against.
    pub base: String,
    pub issue: String,
    pub reason: String,
}

#[derive(Debug, PartialEq)]
pub struct Ledger {
    pub mode: Mode,
    pub baseline: Option<String>,
    pub rows: Vec<Row>,
    pub admits: Vec<Admit>,
    /// For a ledger READ AT a baseline: that commit and the v* tags on it
    /// (set by `ceiling_with`; None for a ledger parsed from text).
    pub at: Option<(String, Vec<String>)>,
}

/// `#123` or `owner/repo#123` (GitHub's issue reference forms; not fetched).
fn is_issue_ref(s: &str) -> bool {
    let Some((repo, num)) = s.rsplit_once('#') else { return false };
    let num_ok = !num.is_empty() && num.chars().all(|c| c.is_ascii_digit()) && !num.starts_with('0');
    let repo_ok = repo.is_empty()
        || repo.split_once('/').is_some_and(|(o, r)| {
            !o.is_empty()
                && !r.is_empty()
                && o.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
                && r.chars().all(|c| c.is_ascii_alphanumeric() || "-_.".contains(c))
        });
    num_ok && repo_ok
}

/// `admit <class> <path> +<N> <issue> <reason ...>` (read before comments are
/// stripped: the issue reference contains `#`).
fn parse_admit(ln: usize, line: &str, known: &BTreeSet<&str>) -> Result<Admit, String> {
    let f: Vec<&str> = line.split_whitespace().collect();
    if f.len() < 7 {
        return Err(format!("line {ln}: `admit <class> <path> +<N> @<baseline sha or v* tag> <#NNN | owner/repo#NNN> <reason>` -- every field and the reason are required"));
    }
    if !known.contains(f[1]) {
        return Err(format!("line {ln}: admit names unknown class '{}'", f[1]));
    }
    // `+N`, digits only (`++2` and `+-1` are not amounts; Rust's parse accepts a sign).
    let n: usize = f[3]
        .strip_prefix('+')
        .filter(|x| !x.is_empty() && x.chars().all(|c| c.is_ascii_digit()) && !x.starts_with('0'))
        .and_then(|x| x.parse().ok())
        .ok_or(format!("line {ln}: admit amount '{}' is not +N with N >= 1", f[3]))?;
    // The baseline the admit was written against (round 5, A1: an admit left in
    // place across a release fold re-admitted growth against the NEW baseline).
    let base = f[4].strip_prefix('@').filter(|b| {
        (b.len() == 40 && b.chars().all(|c| c.is_ascii_hexdigit())) || release_version(b).is_some()
    });
    let base = base.ok_or(format!("line {ln}: '{}' is not @<40-hex baseline sha> or @<vMAJOR.MINOR.PATCH tag>", f[4]))?;
    if !is_issue_ref(f[5]) {
        return Err(format!("line {ln}: '{}' is not a GitHub issue reference (#NNN or owner/repo#NNN)", f[5]));
    }
    Ok(Admit { class: f[1].into(), path: f[2].into(), n, base: base.into(), issue: f[5].into(), reason: f[6..].join(" ") })
}

fn fmt_bounds(b: &[Option<u64>]) -> String {
    b.iter().map(|x| x.map_or("?".to_string(), |n| n.to_string())).collect::<Vec<_>>().join(",")
}

fn is_artifact_id(s: &str) -> bool {
    let parts: Vec<&str> = s.split('-').collect();
    parts.len() >= 2
        && parts.iter().all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit()))
        && parts.last().is_some_and(|p| p.chars().all(|c| c.is_ascii_digit()))
}

pub fn parse_ledger(text: &str) -> Result<Ledger, String> {
    let mut mode = None;
    let mut baseline: Option<Option<String>> = None;
    let mut rows = Vec::new();
    let mut admits: Vec<Admit> = Vec::new();
    let known: BTreeSet<&str> = CLASSES.iter().map(|c| c.0).collect();
    for (n, raw) in text.lines().enumerate() {
        if raw.split_whitespace().next() == Some("admit") {
            let a = parse_admit(n + 1, raw.trim(), &known)?;
            if admits.iter().any(|x| x.class == a.class && x.path == a.path) {
                return Err(format!("line {}: a second admit row for {} {} -- one row per item, with its whole amount", n + 1, a.class, a.path));
            }
            admits.push(a);
            continue;
        }
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let ln = n + 1;
        let f: Vec<&str> = line.split_whitespace().collect();
        if f[0] == "mode" || f[0].starts_with("mode=") {
            let v = line["mode".len()..].trim().strip_prefix('=').ok_or(format!("line {ln}: `mode = equality|shrink-only`"))?.trim();
            if mode.is_some() {
                return Err(format!("line {ln}: a second `mode =` line -- exactly one is allowed"));
            }
            mode = Some(match v {
                "equality" => Mode::Equality,
                "shrink-only" => Mode::ShrinkOnly,
                o => return Err(format!("line {ln}: unknown mode '{o}' (equality | shrink-only)")),
            });
            continue;
        }
        if f[0] == "baseline" || f[0].starts_with("baseline=") {
            let v = line["baseline".len()..].trim().strip_prefix('=').ok_or(format!("line {ln}: `baseline = none|<sha>`"))?.trim();
            if baseline.is_some() {
                return Err(format!("line {ln}: a second `baseline =` line -- exactly one is allowed"));
            }
            baseline = Some(if v == "none" { None } else { Some(v.to_string()) });
            continue;
        }
        let class = f[0];
        if !known.contains(class) {
            return Err(format!("line {ln}: unknown class '{class}'"));
        }
        let want = if class == "kani::unwind" { 5 } else { 4 };
        if f.len() != want {
            return Err(format!("line {ln}: {class} rows have {want} fields, found {}", f.len()));
        }
        let count: usize = f[2].parse().map_err(|_| format!("line {ln}: count '{}' is not a number", f[2]))?;
        let bounds = if want == 5 {
            f[3].split(',')
                .map(|b| if b == "?" { Ok(None) } else { b.parse::<u64>().map(Some).map_err(|_| format!("line {ln}: bound '{b}'")) })
                .collect::<Result<Vec<_>, _>>()?
        } else {
            Vec::new()
        };
        if want == 5 && bounds.len() != count {
            return Err(format!("line {ln}: {count} unwind attributes but {} bounds", bounds.len()));
        }
        let id = f[want - 1];
        if !is_artifact_id(id) {
            return Err(format!("line {ln}: '{id}' is not an artifact ID -- every row names what justifies it"));
        }
        if count == 0 {
            return Err(format!("line {ln}: a zero row is a removed row -- delete it"));
        }
        rows.push(Row { class: class.into(), path: f[1].into(), count, bounds, id: id.into() });
    }
    let mode = mode.ok_or("the ledger has no `mode =` line")?;
    let baseline = baseline.ok_or("the ledger has no `baseline =` line")?;
    match (mode, &baseline) {
        (Mode::ShrinkOnly, None) => return Err("mode = shrink-only needs `baseline = <commit sha>`".into()),
        (Mode::ShrinkOnly, Some(s)) if !(s.len() == 40 && s.chars().all(|c| c.is_ascii_hexdigit())) => {
            return Err(format!("baseline '{s}' is not a 40-hex commit sha"))
        }
        (Mode::Equality, Some(_)) => return Err("mode = equality with a baseline set: switch the mode or clear it".into()),
        _ => {}
    }
    let mut seen = BTreeSet::new();
    for r in &rows {
        if !seen.insert((r.class.clone(), r.path.clone())) {
            return Err(format!("duplicate row {} {}", r.class, r.path));
        }
    }
    Ok(Ledger { mode, baseline, rows, admits, at: None })
}

/// `then` is a sub-multiset of `now`: every baseline bound is still there,
/// unchanged; the extra bounds of `now` are the admitted new ones. Strict on
/// purpose: bounds are a multiset, so "8 lowered to 3 plus a new 9" and "8
/// raised to 9 plus a new 3" look the same -- a raise goes in its own commit.
fn bounds_kept(then: &[Option<u64>], now: &[Option<u64>]) -> bool {
    let mut pool: Vec<Option<u64>> = now.to_vec();
    then.iter().all(|b| match pool.iter().position(|x| x == b) {
        Some(i) => {
            pool.remove(i);
            true
        }
        None => false,
    })
}

/// Shrink-only for unwind bounds, strictly: `now` either only LOST attributes
/// (a sub-multiset of `then`) or only RAISED bounds (same count, each sorted
/// literal >= its partner, `?` exactly where it was). Anything else --
/// a removal together with a change, a `?` gained or swapped for a literal --
/// cannot be told apart from a narrowing, so it is not admitted.
fn unwind_not_narrower(now: &[Option<u64>], then: &[Option<u64>]) -> bool {
    let mut pool: Vec<Option<u64>> = then.to_vec();
    let sub = now.iter().all(|b| match pool.iter().position(|x| x == b) {
        Some(k) => {
            pool.remove(k);
            true
        }
        None => false,
    });
    if sub {
        return true;
    }
    if now.len() != then.len() {
        return false;
    }
    let (qn, qt) = (now.iter().filter(|b| b.is_none()).count(), then.iter().filter(|b| b.is_none()).count());
    if qn != qt {
        return false;
    }
    let mut n: Vec<u64> = now.iter().flatten().copied().collect();
    let mut t: Vec<u64> = then.iter().flatten().copied().collect();
    n.sort();
    t.sort();
    n.iter().zip(t.iter()).all(|(a, b)| a >= b)
}

/// A (class, file) figure: the count, and the unwind bounds (empty otherwise).
type Figure = (usize, Vec<Option<u64>>);

fn figure_grew(class: &str, now: &Figure, then: &Figure) -> bool {
    now.0 > then.0 || (class == "kani::unwind" && !unwind_not_narrower(&now.1, &then.1))
}

/// Compare. `ids` is the set of artifact IDs that exist; a row naming any other
/// fails. In shrink-only mode `baseline` is the ledger at the baseline commit
/// (required). Ok carries the notes (shrinks in shrink-only mode).
pub fn compare(ledger: &Ledger, c: &Census, ids: &BTreeSet<String>, baseline: Option<&Ledger>) -> Result<Vec<String>, Vec<String>> {
    let mut fails = Vec::new();
    let mut notes = Vec::new();
    let mut rec: BTreeMap<(String, String), &Row> = BTreeMap::new();
    for r in &ledger.rows {
        if !ids.contains(&r.id) {
            fails.push(format!("{LEDGER}: {} {} names {} -- no such artifact in artifacts/", r.class, r.path, r.id));
        }
        rec.insert((r.class.clone(), r.path.clone()), r);
    }
    let mut cur: BTreeMap<(String, String), Figure> = BTreeMap::new();
    for (path, fc) in &c.files {
        for (class, n) in &fc.counts {
            let b = if *class == "kani::unwind" { fc.unwind.clone() } else { Vec::new() };
            cur.insert((class.to_string(), path.clone()), (*n, b));
        }
    }
    // The current ledger may not exceed the ledger at the baseline plus the
    // matching `admit` rows: in shrink-only mode (required), and in equality
    // mode too once the ledger has ever been shrink-only (`baseline` then
    // comes from history).
    let admits: BTreeMap<(String, String), &Admit> = ledger.admits.iter().map(|a| ((a.class.clone(), a.path.clone()), a)).collect();
    match baseline {
        None if ledger.mode == Mode::ShrinkOnly => {
            fails.push("mode = shrink-only and the baseline ledger was not read -- refusing to judge growth".into())
        }
        None => {
            for a in &ledger.admits {
                fails.push(format!(
                    "admit {} {} +{} ({}): there is no ceiling to admit against (no shrink-only ledger in history) -- delete the row",
                    a.class, a.path, a.n, a.issue
                ));
            }
        }
        Some(b) => {
            let base: BTreeMap<(String, String), &Row> = b.rows.iter().map(|r| ((r.class.clone(), r.path.clone()), r)).collect();
            for (k, r) in &rec {
                let now: Figure = (r.count, r.bounds.clone());
                let then: Figure = base.get(k).map_or((0, Vec::new()), |br| (br.count, br.bounds.clone()));
                if let Some(a) = admits.get(k) {
                    // Bound to the baseline it was written against (round 5, A1).
                    let bound = b.at.as_ref().is_some_and(|(sha, tags)| a.base == *sha || tags.contains(&a.base));
                    if !bound {
                        fails.push(format!(
                            "admit {} {} +{} ({}): written against @{}, but the ceiling is the ledger at {} -- an admit does not survive a baseline move; fold it into the baseline and delete it",
                            k.0, k.1, a.n, a.issue, a.base,
                            b.at.as_ref().map_or("an unnamed baseline".to_string(), |(s, t)| if t.is_empty() { s.clone() } else { format!("{s} ({})", t.join(", ")) })
                        ));
                        continue;
                    }
                    // Not pre-written: the ledger AT the baseline must not already
                    // carry an admit for this item bound to that same baseline
                    // (round 6, A1').
                    if b.admits.iter().any(|x| x.class == k.0 && x.path == k.1 && b.at.as_ref().is_some_and(|(sha, tags)| x.base == *sha || tags.contains(&x.base))) {
                        fails.push(format!(
                            "admit {} {} +{} ({}): the ledger AT the baseline already carries an admit for this item naming that baseline -- written before the baseline existed, it admits nothing; delete it (a new admit belongs in a commit after the baseline)",
                            k.0, k.1, a.n, a.issue
                        ));
                        continue;
                    }
                    // An admit names EXACTLY the growth it admits: no headroom.
                    let want = then.0 + a.n;
                    let tree = cur.get(k).map_or(0, |f| f.0);
                    if now.0 != want || tree != want {
                        fails.push(format!(
                            "admit {} {} +{} ({}): baseline {} + {} = {want}, but the ledger row is {} and the tree {tree} -- an admit must match the growth exactly (a stale or oversized admit is headroom)",
                            k.0, k.1, a.n, a.issue, then.0, a.n, now.0
                        ));
                    }
                    // An admit adds N attributes; it does not license narrowing
                    // the ones already there (security review: the `continue`
                    // skipped the bound check, so an admitted file could lower an
                    // existing unwind bound).
                    if k.0 == "kani::unwind" && !bounds_kept(&then.1, &now.1) {
                        fails.push(format!(
                            "admit {} {} +{} ({}): baseline bounds [{}] are not all still present, unchanged, in [{}] -- an admit adds bounds; it does not change existing ones (raise them in a separate commit)",
                            k.0, k.1, a.n, a.issue, fmt_bounds(&then.1), fmt_bounds(&now.1)
                        ));
                    }
                    continue;
                }
                if !base.contains_key(k) {
                    fails.push(format!("{} {}: a row the baseline ledger does not have -- {GROWTH_RULE}", k.0, k.1));
                } else if figure_grew(&k.0, &now, &then) {
                    fails.push(format!(
                        "{} {}: ledger row {} [{}] exceeds the baseline's {} [{}] -- {GROWTH_RULE}",
                        k.0, k.1, now.0, fmt_bounds(&now.1), then.0, fmt_bounds(&then.1)
                    ));
                }
            }
            // The TREE against the baseline directly (round 11, F3): for unwind
            // bounds the two comparisons -- ledger vs baseline above, tree vs
            // ledger below -- do not compose. Baseline [6,28,30], ledger [6,28]
            // (a removal) and tree [7,28] (a "raise" over the ledger) passed both
            // while the 30 was lowered to 7.
            for (k, tree) in &cur {
                if k.0 != "kani::unwind" {
                    continue;
                }
                let Some(br) = base.get(k) else { continue };
                let ok = if admits.contains_key(k) { bounds_kept(&br.bounds, &tree.1) } else { unwind_not_narrower(&tree.1, &br.bounds) };
                if !ok {
                    fails.push(format!(
                        "{} {}: tree [{}] against the baseline's [{}] -- neither only removed nor only raised bounds{} (a lowered bound can hide behind a removal recorded in the ledger) -- {GROWTH_RULE}",
                        k.0,
                        k.1,
                        fmt_bounds(&tree.1),
                        fmt_bounds(&br.bounds),
                        if admits.contains_key(k) { ", and an admit keeps every baseline bound unchanged" } else { "" }
                    ));
                }
            }
            for (k, a) in &admits {
                if !rec.contains_key(k) {
                    fails.push(format!("admit {} {} +{} ({}): no ledger row for that item -- a stale admit", k.0, k.1, a.n, a.issue));
                }
            }
        }
    }
    let keys: BTreeSet<_> = rec.keys().cloned().chain(cur.keys().cloned()).collect();
    for k in keys {
        let (class, path) = (&k.0, &k.1);
        let now = cur.get(&k).cloned().unwrap_or((0, Vec::new()));
        let then: Figure = rec.get(&k).map_or((0, Vec::new()), |r| (r.count, r.bounds.clone()));
        if now == then {
            continue;
        }
        let desc = if class == "kani::unwind" {
            format!("{class} {path}: ledger {} [{}], tree {} [{}]", then.0, fmt_bounds(&then.1), now.0, fmt_bounds(&now.1))
        } else {
            format!("{class} {path}: ledger {}, tree {}", then.0, now.0)
        };
        match ledger.mode {
            Mode::Equality => {
                let why = if now.0 > then.0 {
                    "growth"
                } else if now.0 < then.0 {
                    "unrecorded shrink"
                } else {
                    "bounds changed"
                };
                fails.push(format!("{desc} -- {why}; update the row with the justifying artifact ID"));
            }
            Mode::ShrinkOnly => {
                if figure_grew(class, &now, &then) {
                    fails.push(format!("{desc} -- growth (or an ambiguous bound change) above the ledger; the row may be raised only as far as the baseline release's row (above that: {GROWTH_RULE})"));
                } else {
                    notes.push(format!("{desc} -- shrink; the row may be tightened"));
                }
            }
        }
    }
    if fails.is_empty() {
        Ok(notes)
    } else {
        Err(fails)
    }
}

/// Render the current census as ledger rows. Rows whose figure is unchanged
/// keep their ID; changed or new rows take `id`.
pub fn render_rows(c: &Census, old: Option<&Ledger>, id: &str) -> String {
    let mut s = String::new();
    for (class, _, _) in CLASSES {
        for (path, fc) in &c.files {
            let Some(n) = fc.counts.get(class) else { continue };
            let bounds = if *class == "kani::unwind" { format!(" {}", fmt_bounds(&fc.unwind)) } else { String::new() };
            let keep = old.and_then(|l| {
                l.rows.iter().find(|r| {
                    r.class == *class && r.path == *path && r.count == *n && (*class != "kani::unwind" || r.bounds == fc.unwind)
                })
            });
            let rid = keep.map_or(id, |r| r.id.as_str());
            s.push_str(&format!("{class} {path} {n}{bounds} {rid}\n"));
        }
    }
    s
}

/// Every `id:` declared in artifacts/**/*.yaml.
pub fn artifact_ids(repo: &Path) -> Result<BTreeSet<String>, String> {
    let mut files = Vec::new();
    fn walk(d: &Path, out: &mut Vec<std::path::PathBuf>) -> std::io::Result<()> {
        for e in std::fs::read_dir(d)? {
            let p = e?.path();
            if p.is_dir() {
                walk(&p, out)?;
            } else if p.extension().and_then(|x| x.to_str()) == Some("yaml") {
                out.push(p);
            }
        }
        Ok(())
    }
    walk(&repo.join("artifacts"), &mut files).map_err(|e| format!("cannot walk artifacts/: {e}"))?;
    let mut ids = BTreeSet::new();
    for f in files {
        let t = std::fs::read_to_string(&f).map_err(|e| format!("cannot read {}: {e}", f.display()))?;
        for l in t.lines() {
            let l = l.trim_start().trim_start_matches("- ").trim_start();
            if let Some(v) = l.strip_prefix("id:") {
                ids.insert(v.trim().trim_matches('"').to_string());
            }
        }
    }
    if ids.is_empty() {
        return Err("no artifact IDs found under artifacts/".into());
    }
    Ok(ids)
}

fn git(repo: &Path, args: &[&str]) -> Result<std::process::Output, String> {
    std::process::Command::new("git").arg("-C").arg(repo).args(args).output().map_err(|e| format!("git {}: {e}", args.join(" ")))
}

/// One git query: (exit 0, stdout). Injected so the self-test can drive every
/// branch of the baseline rules without a repository of its own.
type GitQuery<'a> = &'a dyn Fn(&[&str]) -> Result<(bool, String), String>;

fn real_git(repo: &Path) -> impl Fn(&[&str]) -> Result<(bool, String), String> + '_ {
    move |args: &[&str]| git(repo, args).map(|o| (o.status.success(), String::from_utf8_lossy(&o.stdout).into_owned()))
}

/// The ledger at the baseline commit: the sha must name a commit in this
/// repository, that commit must be one a `v*` tag points at (a release the
/// owner cut -- not any commit a later change could name), and it must be an
/// ancestor of (or be) HEAD.
pub fn baseline_ledger_with(g: GitQuery, sha: &str) -> Result<Ledger, String> {
    if !(sha.len() == 40 && sha.chars().all(|c| c.is_ascii_hexdigit())) {
        return Err(format!("baseline '{sha}' is not a 40-hex commit sha"));
    }
    if !g(&["cat-file", "-e", &format!("{sha}^{{commit}}")])?.0 {
        return Err(format!("baseline {sha} is not a commit in this repository (a shallow clone needs fetch-depth: 0 and the tags)"));
    }
    let (ok, tags) = g(&["tag", "--points-at", sha, "--list", "v*"])?;
    if !ok || tags.trim().is_empty() {
        return Err(format!("baseline {sha} is not a commit a `v*` tag points at -- the baseline is a release, not any commit"));
    }
    if !g(&["merge-base", "--is-ancestor", sha, "HEAD"])?.0 {
        return Err(format!("baseline {sha} is not an ancestor of HEAD"));
    }
    // Monotonic: the ceiling is the NEWEST release in HEAD's history. A change
    // cannot point the editable `baseline` field back at an older tag.
    let newest = newest_release_with(g)?;
    if newest != sha {
        return Err(format!(
            "baseline {sha} is not the newest v* release in HEAD's history ({newest}) -- the ceiling only moves forward"
        ));
    }
    let (ok, text) = g(&["show", &format!("{sha}:{LEDGER}")])?;
    if !ok {
        return Err(format!("{LEDGER} does not exist at the baseline {sha}"));
    }
    parse_ledger(&text).map_err(|e| format!("the ledger at the baseline {sha}: {e}"))
}

/// A release tag's version: exactly `vMAJOR.MINOR.PATCH`. Anything else is None.
fn release_version(tag: &str) -> Option<(u64, u64, u64)> {
    // A pre-release (`v1.0.0-rc.10`) is not a release and is never a
    // baseline: it is skipped by `newest_release_with` (a string comparison of
    // the suffix ordered rc.10 below rc.9; refusing pre-releases outright is
    // simpler than semver precedence).
    let v = tag.strip_prefix('v')?;
    let n: Vec<u64> = v.split('.').map(|x| if x.is_empty() || !x.chars().all(|c| c.is_ascii_digit()) { None } else { x.parse().ok() }).collect::<Option<Vec<_>>>()?;
    if n.len() != 3 {
        return None;
    }
    Some((n[0], n[1], n[2]))
}

/// `vMAJOR.MINOR.PATCH-...`: a pre-release tag.
fn is_prerelease(tag: &str) -> bool {
    tag.split_once('-').is_some_and(|(core, pre)| !pre.is_empty() && release_version(core).is_some())
}

/// The commit of the NEWEST `v*` release reachable from HEAD -- the highest
/// version among `git tag --merged HEAD`, not the nearest tag (`git describe`
/// picked a merged maintenance tag v1.0.1 over v2.0.0: L11 round 4, T2) --
/// NOT counting a tag on HEAD itself: a ledger cannot name the commit that
/// contains it, so on a tagged release commit the newest release is the one
/// before it (round 3, finding 11). A pre-release tag (`v1.0.0-rc.1`) is
/// skipped; any other `v*` tag that is not `vMAJOR.MINOR.PATCH` cannot be
/// ordered and is an error.
pub fn newest_release_with(g: GitQuery) -> Result<String, String> {
    let (_, own) = g(&["tag", "--points-at", "HEAD", "--list", "v*"])?;
    let own: BTreeSet<&str> = own.lines().map(str::trim).filter(|t| !t.is_empty()).collect();
    let (ok, merged) = g(&["tag", "--merged", "HEAD", "--list", "v*"])?;
    if !ok {
        return Err("git tag --merged HEAD failed".into());
    }
    let mut best: Option<((u64, u64, u64), String)> = None;
    for t in merged.lines().map(str::trim).filter(|t| !t.is_empty() && !own.contains(t)) {
        // `v*` also matches `vendor-x`: only `v<digit>...` tags are release-shaped.
        if !t[1..].starts_with(|c: char| c.is_ascii_digit()) {
            continue;
        }
        if is_prerelease(t) {
            continue;
        }
        let v = release_version(t).ok_or(format!("the tag {t} is not vMAJOR.MINOR.PATCH (or a -pre-release, which is skipped): the newest release cannot be ordered"))?;
        if best.as_ref().is_none_or(|(b, _)| v > *b) {
            best = Some((v, t.to_string()));
        }
    }
    let Some((_, tag)) = best else {
        return Err("no v* tag in HEAD's history (a shallow clone needs fetch-depth: 0 and the tags)".into());
    };
    let (ok, sha) = g(&["rev-list", "-n", "1", &tag])?;
    let sha = sha.trim();
    if !ok || sha.len() != 40 {
        return Err(format!("cannot resolve the tag {tag}"));
    }
    Ok(sha.to_string())
}

/// The ceiling is read from history (the baseline commit, its tags, every
/// past shrink-only ledger). In a shallow clone, "never shrink-only" and
/// "history missing" look the same and the ceiling would be silently
/// forgotten (round 11, F5), so a shallow repository -- or one that cannot
/// say -- is refused.
fn full_history_with(g: GitQuery) -> Result<(), String> {
    match g(&["rev-parse", "--is-shallow-repository"])? {
        (true, s) if s.trim() == "false" => Ok(()),
        (true, s) if s.trim() == "true" => Err("a shallow clone: the ceiling is read from history (fetch-depth: 0 and the tags), refusing rather than forget it".into()),
        (_, s) => Err(format!("git rev-parse --is-shallow-repository answered `{}` -- cannot tell whether history is complete", s.trim())),
    }
}

pub fn baseline_ledger(repo: &Path, sha: &str) -> Result<Ledger, String> {
    baseline_ledger_with(&real_git(repo), sha)
}

/// The commits whose diff adds or removes a `mode = shrink-only` line of the
/// ledger, over ALL of HEAD's history: `--full-history` so an `-s ours` merge
/// cannot hide the side that set it (round 3, finding 10), `-m` so a merge's
/// own diff counts, and no `-n` cap (500 ledger-touching commits used to
/// forget it: round 4, T3).
const SHRINK_LINE_RE: &str = "^[[:space:]]*mode[[:space:]]*=[[:space:]]*shrink-only";

/// The newest ledger in HEAD's history that was shrink-only, with the commit
/// it was read at.
pub fn last_shrink_only_with(g: GitQuery) -> Result<Option<(String, Ledger)>, String> {
    let (ok, revs) = g(&["log", "--full-history", "-m", "--format=%H", "-G", SHRINK_LINE_RE, "HEAD", "--", LEDGER])?;
    if !ok {
        return Err("git log of the ledger failed".into());
    }
    let mut seen = BTreeSet::new();
    for rev in revs.lines().map(str::trim).filter(|r| !r.is_empty()) {
        if !seen.insert(rev.to_string()) {
            continue;
        }
        let (ok, text) = g(&["show", &format!("{rev}:{LEDGER}")])?;
        if !ok {
            continue;
        }
        if let Ok(l) = parse_ledger(&text) {
            if l.mode == Mode::ShrinkOnly {
                return Ok(Some((rev.to_string(), l)));
            }
        }
    }
    Ok(None)
}

/// The baseline ledger with its paths carried through EXACT renames
/// (`git diff -M100%` from the baseline to HEAD): a pure `git mv` of a file is
/// not growth in the new path and a stale row in the old (round 4, T8). A
/// rename that also edits the file is not followed (cannot-see list).
fn follow_renames(g: GitQuery, sha: &str, mut l: Ledger) -> Result<Ledger, String> {
    let (ok, out) = g(&["diff", "--name-status", "-M100%", "--diff-filter=R", sha, "HEAD"])?;
    if !ok {
        return Err(format!("git diff --name-status {sha} HEAD failed"));
    }
    let mut map: BTreeMap<String, String> = BTreeMap::new();
    for line in out.lines() {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() == 3 && f[0] == "R100" {
            map.insert(f[1].to_string(), f[2].to_string());
        }
    }
    if map.is_empty() {
        return Ok(l);
    }
    let mut merged: BTreeMap<(String, String), Row> = BTreeMap::new();
    for mut r in l.rows.drain(..) {
        if let Some(n) = map.get(&r.path) {
            r.path = n.clone();
        }
        match merged.get_mut(&(r.class.clone(), r.path.clone())) {
            Some(m) => {
                m.count += r.count;
                m.bounds.extend(r.bounds);
                m.bounds.sort();
            }
            None => {
                merged.insert((r.class.clone(), r.path.clone()), r);
            }
        }
    }
    l.rows = merged.into_values().collect();
    Ok(l)
}

/// The ceiling the current ledger is held to: the baseline's ledger in
/// shrink-only mode; in equality mode, the newest release's ledger if a
/// shrink-only ledger was ever in history (switching back does not lift it).
/// Either way with paths carried through exact renames since the baseline.
fn ceiling_with(g: GitQuery, ledger: &Ledger) -> Result<Option<(String, Ledger)>, String> {
    let raw = match (&ledger.mode, &ledger.baseline) {
        (Mode::ShrinkOnly, Some(sha)) => (sha.clone(), baseline_ledger_with(g, sha)?),
        (Mode::ShrinkOnly, None) => return Err("mode = shrink-only with no baseline".into()),
        (Mode::Equality, _) => match last_shrink_only_with(g)? {
            None => return Ok(None),
            Some((rev, _old)) => {
                // The newest release, not the old ledger's baseline field: monotonic.
                let sha = newest_release_with(g).map_err(|e| format!("(the ledger was shrink-only at {rev}) {e}"))?;
                let l = baseline_ledger_with(g, &sha).map_err(|e| format!("(the ledger was shrink-only at {rev}) {e}"))?;
                (sha, l)
            }
        },
    };
    let mut followed = follow_renames(g, &raw.0, raw.1)?;
    let (_, tags) = g(&["tag", "--points-at", &raw.0, "--list", "v*"])?;
    followed.at = Some((raw.0.clone(), tags.lines().map(str::trim).filter(|t| !t.is_empty()).map(str::to_string).collect()));
    Ok(Some((raw.0, followed)))
}

fn summary(c: &Census) -> Vec<String> {
    let mut v = Vec::new();
    v.push(format!(
        "scanned {} .rs, {} .v, {} .lean, {} Bazel tracked files (excluded: {}); {} #[kani::proof] attributes in source \
         (a spelling count: `cargo xtask check kani-coverage` prints how many sit in compiled roots -- an upper bound on what CI runs)",
        c.scanned.get("rs").unwrap_or(&0),
        c.scanned.get("v").unwrap_or(&0),
        c.scanned.get("lean").unwrap_or(&0),
        c.scanned.get("bazel").unwrap_or(&0),
        EXCLUDED_PREFIXES.join(", "),
        c.kani_proofs
    ));
    for (class, _, what) in CLASSES {
        let files: Vec<usize> = c.files.values().filter_map(|f| f.counts.get(class).copied()).collect();
        v.push(format!("  {class:<28} {:>4} in {:>2} file(s)   {what}", files.iter().sum::<usize>(), files.len()));
    }
    v.push("cannot see (a count of spellings, not of assumptions):".into());
    for s in CANNOT_SEE {
        v.push(format!("  - {s}"));
    }
    v
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    run_with(repo, census(repo))
}

/// `run` with the census injected, so the self-test can prove a failed census
/// is REFUSED and never a pass.
fn run_with(repo: &Path, census: Result<Census, String>) -> Verdict {
    let c = match census {
        Ok(c) => c,
        Err(e) => return Verdict::Refused(e),
    };
    let text = match std::fs::read_to_string(repo.join(LEDGER)) {
        Ok(t) => t,
        Err(e) => return Verdict::Refused(format!("cannot read {LEDGER}: {e}")),
    };
    let ledger = match parse_ledger(&text) {
        Ok(l) => l,
        Err(e) => return Verdict::Fail(vec![format!("{LEDGER}: {e}")]),
    };
    let ids = match artifact_ids(repo) {
        Ok(i) => i,
        Err(e) => return Verdict::Refused(e),
    };
    let g = real_git(repo);
    if let Err(e) = full_history_with(&g) {
        return Verdict::Refused(e);
    }
    let ceiling = match ceiling_with(&g, &ledger) {
        Ok(c) => c,
        Err(e) => return Verdict::Fail(vec![format!("{LEDGER}: {e}")]),
    };
    let base = ceiling.as_ref().map(|(_, l)| l);
    let mut lines = vec![format!(
        "mode = {}{}",
        if ledger.mode == Mode::Equality { "equality (any change fails until the baseline is set)" } else { "shrink-only" },
        ceiling.as_ref().map_or(" (no shrink-only ledger in history: no ceiling yet)".to_string(), |(b, _)| format!(
            ", ceiling = the ledger at the tagged baseline {b}"
        ))
    )];
    lines.extend(summary(&c));
    match compare(&ledger, &c, &ids, base) {
        Ok(notes) => {
            lines.extend(notes);
            lines.push(format!("the tree matches {LEDGER} ({} rows)", ledger.rows.len()));
            Verdict::Pass(lines)
        }
        Err(f) => {
            lines.extend(f);
            lines.push("regenerate rows with `cargo xtask trust-ledger --write --id <ARTIFACT-ID>`".into());
            Verdict::Fail(lines)
        }
    }
}

/// `xtask trust-ledger [--write --id ID]`: print the current rows, or rewrite
/// the ledger's rows in place (mode and baseline are kept).
pub fn cmd(repo: &Path, args: &[String]) -> i32 {
    use crate::verdict::{EXIT_PASS, EXIT_REFUSED, EXIT_USAGE};
    let mut write = false;
    let mut id: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--write" => write = true,
            "--id" => id = it.next().cloned(),
            o => {
                eprintln!("xtask: unknown argument '{o}' to trust-ledger (--write --id ID)");
                return EXIT_USAGE;
            }
        }
    }
    let c = match census(repo) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("xtask: {e}");
            return EXIT_REFUSED;
        }
    };
    let path = repo.join(LEDGER);
    let old_text = std::fs::read_to_string(&path).ok();
    let old = old_text.as_deref().and_then(|t| parse_ledger(t).ok());
    if !write {
        print!("{}", render_rows(&c, old.as_ref(), id.as_deref().unwrap_or("UNJUSTIFIED-0")));
        return EXIT_PASS;
    }
    let Some(id) = id.filter(|i| is_artifact_id(i)) else {
        eprintln!("xtask: --write needs --id <ARTIFACT-ID> naming what justifies the change");
        return EXIT_USAGE;
    };
    let Some(old_text) = old_text else {
        eprintln!("xtask: {LEDGER} is missing; it carries the mode, which is not invented here");
        return EXIT_REFUSED;
    };
    let header: String = old_text
        .lines()
        .take_while(|l| {
            let t = l.trim();
            t.is_empty() || t.starts_with('#') || t.starts_with("mode") || t.starts_with("baseline")
        })
        .map(|l| format!("{l}\n"))
        .collect();
    // `admit` rows are kept verbatim: --write regenerates counts, never admits.
    let admits: String = old_text
        .lines()
        .skip(header.lines().count())
        .filter(|l| l.split_whitespace().next() == Some("admit"))
        .map(|l| format!("{l}\n"))
        .collect();
    let new = format!("{header}{admits}{}", render_rows(&c, old.as_ref(), &id));
    if let Err(e) = std::fs::write(&path, new) {
        eprintln!("xtask: cannot write {LEDGER}: {e}");
        return EXIT_REFUSED;
    }
    println!("wrote {LEDGER}");
    EXIT_PASS
}

// rivet: verifies VER-HONESTY-LEDGER-GATE-001
/// The planted inputs of the plan, plus the ways the gate itself could be wrong:
/// a miscount, a missing spelling, the mode switch ignored, an unknown ID.
pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };

    // -- spelling rules --------------------------------------------------
    let rs = "/// kani::assume(x) in a doc comment\n// kani::assume(y)\n/* kani::assume(z) /* nested */ */\n\
              /* outer /* inner */ kani::assume(hidden_by_depth) */\n\
              fn h() { kani::assume(a < 3); kani :: assume ( b ); let s = \"kani::assume(c)\"; let q = '\"'; kani::assume(q != 'x'); }\n\
              #[kani::unwind(5)]\n#[ kani :: unwind ( 17 ) ]\n#[kani::unwind(N)]\n\
              #[verifier::external_body]\n#[verifier(external_body)]\npub assume_specification [f] (x: u8);\n\
              fn assume(x: bool) {} fn admit() {} t.admit(); m::admit(); proof { assume(x); admit(); }\n\
              let r = r#\"kani::assume(raw) \"quoted\" \"#;\n";
    let fc = count_file("x.rs", rs);
    let g = |k: &str| fc.counts.get(k).copied().unwrap_or(0);
    ck("kani::assume: 3 calls; comment, nested comment (both depths), string, raw string and char-literal mentions not", g("kani::assume") == 3);
    ck("kani::unwind: 3 attributes, bounds [5,17,?]", fc.unwind == vec![None, Some(5), Some(17)] && g("kani::unwind") == 3);
    ck("verus:external_body: both spellings", g("verus:external_body") == 2);
    ck("verus:assume_specification: 1", g("verus:assume_specification") == 1);
    ck("verus:assume: the bare call only (not kani::, not fn)", g("verus:assume") == 1);
    ck("verus:admit: the bare call only (not .admit(), ::admit(), fn admit)", g("verus:admit") == 1);
    let more = "#[cfg_attr(kani, kani::proof, kani::unwind(2))]\nfn h() { let x: u8 = kani::any_where(|v| *v < 4); }\n\
                #[cfg_attr(verus_keep_ghost, verifier::external_body)]\nfn e() {}\n#![verifier::external_body]\n\
                axiom fn ax1(); broadcast axiom fn ax2(); fn axiom_fn_like() {}\n";
    let fm = count_file("m.rs", more);
    let gm = |k: &str| fm.counts.get(k).copied().unwrap_or(0);
    ck("cfg_attr: `kani::unwind(2)` inside cfg_attr is an unwind row", fm.unwind == vec![Some(2)]);
    ck("cfg_attr: `kani::proof` inside cfg_attr is a harness", kani_proofs(more) == 1);
    ck("kani::any_where: 1", gm("kani::any_where") == 1);
    ck("verus:external_body: the cfg_attr and `#![...]` forms", gm("verus:external_body") == 2);
    ck("verus:axiom_fn: `axiom fn` and `broadcast axiom fn`, not an identifier", gm("verus:axiom_fn") == 2);

    let v = "(* Admitted. in a comment (* nested Axiom *) *)\nTheorem t : True.\nProof. admit. Admitted.\n\
             Axiom ax : False.\nParameter p : nat.\nParameters q r : nat.\nDefinition s := \"Admitted\".\nLemma Admitted_like : True. Proof. exact I. Qed.\n";
    let fv = count_file("p.v", v);
    let gv = |k: &str| fv.counts.get(k).copied().unwrap_or(0);
    ck("rocq: 1 Admitted, 1 admit, 1 Axiom, 2 Parameter(s); comment/string/identifier mentions not",
        gv("rocq:Admitted") == 1 && gv("rocq:admit") == 1 && gv("rocq:Axiom") == 1 && gv("rocq:Parameter") == 2);
    let v2 = "Lemma g : True. Proof. give_up. Admitted.\nAxioms a b : False.\nConjecture c : False.\n\
              (* \"*)\" Admitted. *) Axiom real : False.\nVariable top : nat.\nHypothesis h : top = 0.\n\
              Section S.\nVariable in_section : nat.\nContext (x : nat).\nEnd S.\nModule M.\nHypotheses hm : True.\nEnd M.\n\
              Obligation Tactic := idtac.\nAdmit Obligations.\n";
    let f2 = count_file("q.v", v2);
    let g2 = |k: &str| f2.counts.get(k).copied().unwrap_or(0);
    ck("rocq: `give_up` is rocq:admit", g2("rocq:admit") == 1);
    ck("rocq: `Axioms` + `Conjecture` + the Axiom after a string-in-comment are 3 rocq:Axiom", g2("rocq:Axiom") == 3);
    ck("rocq: the `Admitted.` inside `(* \"*)\" ... *)` is in the comment", g2("rocq:Admitted") == 1);
    ck("rocq: top-level Variable, Hypothesis and a Module's Hypotheses count (3); a Section's do not", g2("rocq:Variable") == 3);
    ck("rocq: `Admit Obligations` counts", g2("rocq:Admit-Obligations") == 1);

    let lean = "-- sorry\n/- axiom /- nested sorry -/ -/\ntheorem a : True := by sorry\naxiom foo : False\ntheorem b : True := by admit\ndef sorry' := 1\n\
                def q := '\"'\ntheorem f : False := sorry\ndef h' := 'x'\nexample : False := sorryAx False\n";
    let fl = count_file("p.lean", lean);
    let gl = |k: &str| fl.counts.get(k).copied().unwrap_or(0);
    ck("lean: 2 sorry (one after a '\"' char literal), 1 axiom, 1 admit, 1 sorryAx; comments and identifiers not",
        gl("lean:sorry") == 2 && gl("lean:axiom") == 1 && gl("lean:admit") == 1 && gl("lean:sorryAx") == 1);
    let bz = "# extra_flags = [\"-impredicative-set\"] in a comment\nrocq_library(\n    extra_flags = [\"-impredicative-set\"],\n)\nx = \"a#b\" + '-impredicative-set'\n";
    ck("bazel: 2 -impredicative-set flags, not the commented one",
        count_file("proofs/BUILD.bazel", bz).counts.get("rocq:impredicative-set") == Some(&2));
    let imp = |p: &str, t: &str| count_file(p, t).counts.get("rocq:impredicative-set").copied().unwrap_or(0);
    ck("N8 bazel: a split literal `\"-impredicative-\" + \"set\"` counts", imp("p/BUILD.bazel", "r(\n    extra_flags = [\"-impredicative-\" + \"set\"],\n)\n") == 1);
    ck("N8 bazel: one literal in a `for m in [a, b, c]` comprehension is 3 targets",
        imp("p/BUILD.bazel", "[\n    r(name = m, extra_flags = [\"-impredicative-set\"])\n    for m in [\n        \"a\",\n        \"b\",\n        \"c\",\n    ]\n]\n") == 3);
    let mac = census_of([
        ("bzl/m.bzl", "def mk(name):\n    r(name = name, extra_flags = [\"-impredicative-set\"])\n"),
        ("p/BUILD.bazel", "load(\"//bzl:m.bzl\", \"mk\")\nmk(name = \"x\")\nmk(name = \"y\")\n"),
    ]);
    ck("N8 bazel: each call of a flag-setting macro is a target on the calling file (2), the def is 1",
        mac.files.get("p/BUILD.bazel").and_then(|f| f.counts.get("rocq:impredicative-set")) == Some(&2)
            && mac.files.get("bzl/m.bzl").and_then(|f| f.counts.get("rocq:impredicative-set")) == Some(&1));

    // -- a small tree and its ledger --------------------------------------
    let base_rs = "#[kani::proof]\n#[kani::unwind(8)]\nfn h1() { let x: u8 = kani::any(); kani::assume(x < 4); }\n\
                   #[kani::proof]\n#[kani::unwind(5)]\nfn h2() { kani::assume(true); }\n";
    let base_v = "Theorem t : True.\nProof. Admitted.\n";
    let tree = |rs: &str, v: &str| {
        census_of([
            ("a/src/lib.rs", rs),
            ("proofs/p.v", v),
            ("proofs/L.lean", "theorem x : True := trivial\n"),
            ("proofs/BUILD.bazel", "x = 1\n"),
            ("tools/xtask/src/x.rs", "fn f(){kani::assume(x);}"),
        ])
    };
    let c0 = tree(base_rs, base_v);
    ck("excluded prefix is not counted", !c0.files.contains_key("tools/xtask/src/x.rs"));
    ck("kani::proof attributes counted", c0.kani_proofs == 2);
    ck("a census of all four languages passes the empty-scan check", check_scanned(&c0).is_ok());
    let no_lean = census_of([("a/src/lib.rs", base_rs), ("proofs/p.v", base_v), ("proofs/BUILD.bazel", "")]);
    ck("REFUSE an empty scan of one language (no .lean)", check_scanned(&no_lean).is_err());
    let mut ids = BTreeSet::new();
    ids.insert("VER-X-001".to_string());
    let eq_text = format!("mode = equality\nbaseline = none\n{}", render_rows(&c0, None, "VER-X-001"));
    let eq = parse_ledger(&eq_text).expect("rendered ledger parses");
    ck("rendered rows: assume 2, unwind 2 [5,8], Admitted 1",
        eq_text.contains("kani::assume a/src/lib.rs 2 VER-X-001")
            && eq_text.contains("kani::unwind a/src/lib.rs 2 5,8 VER-X-001")
            && eq_text.contains("rocq:Admitted proofs/p.v 1 VER-X-001"));
    ck("the unchanged tree passes the equality ledger", compare(&eq, &c0, &ids, None).is_ok());

    let added = base_rs.replace("kani::assume(true);", "kani::assume(true); kani::assume(false);");
    ck("PLANTED an added kani::assume fails (equality)", added != base_rs && compare(&eq, &tree(&added, base_v), &ids, None).is_err());
    let lowered = base_rs.replace("unwind(8)", "unwind(3)");
    ck("PLANTED a lowered unwind bound fails (equality)", lowered != base_rs && compare(&eq, &tree(&lowered, base_v), &ids, None).is_err());
    let adm = format!("{base_v}Lemma u : True.\nProof. Admitted.\n");
    ck("PLANTED an added `Admitted.` fails (equality)", compare(&eq, &tree(base_rs, &adm), &ids, None).is_err());
    let shrunk = base_rs.replace("kani::assume(true);", "");
    ck("PLANTED an unrecorded shrink fails (equality)", shrunk != base_rs && compare(&eq, &tree(&shrunk, base_v), &ids, None).is_err());
    let gone_v = "Theorem t : True.\nProof. exact I. Qed.\n";
    ck("PLANTED a removed row (count to 0) fails (equality)", compare(&eq, &tree(base_rs, gone_v), &ids, None).is_err());
    let qbound = base_rs.replace("unwind(8)", "unwind(N)");
    ck("PLANTED a literal bound turned non-literal `?` fails (equality)", compare(&eq, &tree(&qbound, base_v), &ids, None).is_err());

    let reformatted = "\n\n#[kani::proof]\n#[kani::unwind(5)]\nfn h2() {\n    kani::assume(\n        true\n    );\n}\n\n\
                       #[kani::proof]\n#[ kani::unwind( 8 ) ]\nfn h1() {\n  let x: u8 = kani::any();\n  kani::assume(x < 4); // was: kani::assume(x < 5)\n}\n";
    ck("STABLE a reformat (whitespace, line moves, harness order) passes", compare(&eq, &tree(reformatted, base_v), &ids, None).is_ok());
    let commented = format!("// kani::assume(note)\n/* #[kani::unwind(1)] */\n{base_rs}");
    let v_commented = format!("(* Admitted. Axiom x : False. *)\n{base_v}");
    ck("STABLE a comment-only edit passes", compare(&eq, &tree(&commented, &v_commented), &ids, None).is_ok());

    // -- the mode switch --------------------------------------------------
    let sha = "0123456789abcdef0123456789abcdef01234567";
    let sh_text = eq_text.replace("mode = equality\nbaseline = none", &format!("mode = shrink-only\nbaseline = {sha}"));
    let sh = parse_ledger(&sh_text).expect("shrink-only ledger parses");
    ck("SWITCH shrink-only: an unrecorded shrink passes (noted)",
        matches!(compare(&sh, &tree(&shrunk, base_v), &ids, Some(&eq)), Ok(n) if !n.is_empty()));
    ck("SWITCH shrink-only: growth still fails", compare(&sh, &tree(&added, base_v), &ids, Some(&eq)).is_err());
    ck("SWITCH shrink-only: a lowered bound still fails", compare(&sh, &tree(&lowered, base_v), &ids, Some(&eq)).is_err());
    let raised = base_rs.replace("unwind(8)", "unwind(30)");
    ck("SWITCH shrink-only: a raised bound passes", compare(&sh, &tree(&raised, base_v), &ids, Some(&eq)).is_ok());
    ck("SWITCH shrink-only: without the baseline ledger it fails (never judged against itself)",
        compare(&sh, &c0, &ids, None).is_err());
    // Growth recorded in the ledger itself: the ledger at the baseline is the ceiling.
    let grown_ledger = parse_ledger(&format!(
        "mode = shrink-only\nbaseline = {sha}\n{}",
        render_rows(&tree(&added, base_v), None, "VER-X-001")
    ))
    .expect("parses");
    ck("PLANTED shrink-only: bumping a row (assume 2 -> 3) together with the tree fails against the baseline ledger",
        compare(&grown_ledger, &tree(&added, base_v), &ids, Some(&eq)).is_err());
    let new_file_ledger = parse_ledger(&format!("{sh_text}kani::assume b/src/lib.rs 1 VER-X-001\n")).expect("parses");
    let new_file_tree = census_of([
        ("a/src/lib.rs", base_rs),
        ("b/src/lib.rs", "fn f() { kani::assume(x); }"),
        ("proofs/p.v", base_v),
        ("proofs/L.lean", "theorem x : True := trivial\n"),
        ("proofs/BUILD.bazel", "x = 1\n"),
    ]);
    ck("PLANTED shrink-only: a new file with a new row fails (absent from the baseline)",
        compare(&new_file_ledger, &new_file_tree, &ids, Some(&eq)).is_err());
    // -- admit rows (owner decision on T1, L11 round 4) ------------------------
    let eq_at = {
        let mut e = parse_ledger(&eq_text).expect("parses");
        e.at = Some((sha.to_string(), vec!["v0.11.0".to_string()]));
        e
    };
    let grown_rows = render_rows(&tree(&added, base_v), None, "VER-X-001");
    let with_admit = |admit: &str| format!("mode = shrink-only\nbaseline = {sha}\n{admit}{grown_rows}");
    let adm_ok = with_admit("admit kani::assume a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #467 the seam's new precondition\n");
    let adm_ok_l = parse_ledger(&adm_ok).expect("an admit row parses");
    ck("ADMIT growth (assume 2 -> 3) with a matching `admit ... +1 #467 reason` passes",
        adm_ok_l.admits.len() == 1 && compare(&adm_ok_l, &tree(&added, base_v), &ids, Some(&eq_at)).is_ok());
    ck("ADMIT the baseline may be named by its tag (`@v0.11.0`)",
        compare(&parse_ledger(&with_admit("admit kani::assume a/src/lib.rs +1 @v0.11.0 #467 r\n")).expect("parses"), &tree(&added, base_v), &ids, Some(&eq_at)).is_ok());
    let moved = parse_ledger(&with_admit(&format!("admit kani::assume a/src/lib.rs +1 @{} #467 written against the old baseline\n", "f".repeat(40)))).expect("parses");
    ck("PLANTED A1: an admit written against another baseline (left in place across a fold) fails",
        compare(&moved, &tree(&added, base_v), &ids, Some(&eq_at)).is_err());
    ck("A1: an admit judged against a baseline of unknown identity fails (never assumed to match)",
        compare(&adm_ok_l, &tree(&added, base_v), &ids, Some(&eq)).is_err());
    let base_with = |row: &str| {
        let mut e = parse_ledger(&format!("{eq_text}{row}")).expect("parses");
        e.at = Some((sha.to_string(), vec!["v0.11.0".to_string()]));
        e
    };
    ck("PLANTED A1': the same admit already in the ledger AT the baseline (pre-written, naming the baseline's tag) fails",
        compare(&adm_ok_l, &tree(&added, base_v), &ids, Some(&base_with("admit kani::assume a/src/lib.rs +1 @v0.11.0 #467 pre-written\n"))).is_err());
    ck("A1' control: the baseline ledger carries an OLDER admit for the item (@v0.10.0, the previous release's) -- a new admit passes",
        compare(&adm_ok_l, &tree(&added, base_v), &ids, Some(&base_with("admit kani::assume a/src/lib.rs +2 @v0.10.0 #400 last release\n"))).is_ok());
    for amt in ["++1", "+-1", "+01", "+"] {
        ck(&format!("PLANTED ADMIT amount `{amt}` is rejected"),
            parse_ledger(&with_admit(&format!("admit kani::assume a/src/lib.rs {amt} @v0.11.0 #467 r\n"))).is_err());
    }
    ck("ADMIT a missing or malformed `@baseline` is rejected",
        parse_ledger(&with_admit("admit kani::assume a/src/lib.rs +1 #467 r and more\n")).is_err()
            && parse_ledger(&with_admit("admit kani::assume a/src/lib.rs +1 @main #467 r\n")).is_err());
    ck("ADMIT an `owner/repo#NNN` reference is accepted",
        parse_ledger(&with_admit("admit kani::assume a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 pulseengine/gale#467 reason\n")).is_ok());
    ck("PLANTED ADMIT growth with no admit row fails (the same tree, the admit deleted)",
        compare(&parse_ledger(&with_admit("")).expect("parses"), &tree(&added, base_v), &ids, Some(&eq_at)).is_err());
    let stale = parse_ledger(&format!("{sh_text}admit kani::assume a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #467 no longer needed\n")).expect("parses");
    ck("PLANTED ADMIT a stale admit (no growth in the tree) fails", compare(&stale, &c0, &ids, Some(&eq_at)).is_err());
    let oversized = parse_ledger(&with_admit("admit kani::assume a/src/lib.rs +2 @0123456789abcdef0123456789abcdef01234567 #467 headroom\n")).expect("parses");
    ck("PLANTED ADMIT an oversized admit (+2 for growth of 1) fails", compare(&oversized, &tree(&added, base_v), &ids, Some(&eq_at)).is_err());
    let headroom = parse_ledger(&with_admit("admit kani::assume a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #467 headroom kept in the ledger\n")).expect("parses");
    ck("PLANTED ADMIT the ledger row grown with an admit while the tree did not grow (headroom) fails",
        compare(&headroom, &c0, &ids, Some(&eq_at)).is_err());
    let orphan = parse_ledger(&with_admit("admit kani::assume z/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #467 nothing there\n")).expect("parses");
    ck("PLANTED ADMIT an admit naming an item with no row fails", compare(&orphan, &tree(&added, base_v), &ids, Some(&eq_at)).is_err());
    for (what, bad) in [
        ("a malformed issue reference (`467`)", "admit kani::assume a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 467 reason\n"),
        ("a malformed issue reference (`#04`)", "admit kani::assume a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #04 reason\n"),
        ("a malformed issue reference (`gale#467`, no owner)", "admit kani::assume a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 gale#467 reason\n"),
        ("no reason", "admit kani::assume a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #467\n"),
        ("an amount that is not +N", "admit kani::assume a/src/lib.rs 1 @0123456789abcdef0123456789abcdef01234567 #467 reason\n"),
        ("an unknown class", "admit kani::nope a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #467 reason\n"),
        ("a duplicate admit for one item", "admit kani::assume a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #467 one\nadmit kani::assume a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #468 two\n"),
    ] {
        ck(&format!("PLANTED ADMIT {what} is rejected"), parse_ledger(&with_admit(bad)).is_err());
    }
    ck("PLANTED ADMIT an admit with no ceiling (equality, no shrink-only history) fails",
        compare(&parse_ledger(&format!("{eq_text}admit kani::assume a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #467 r\n")).expect("parses"), &c0, &ids, None).is_err());
    // Security review: an admit on kani::unwind must not license narrowing.
    let unwind_admit = |rs: &str| {
        let t = tree(rs, base_v);
        let l = parse_ledger(&format!(
            "mode = shrink-only\nbaseline = {sha}\nadmit kani::unwind a/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #467 a new harness\n{}",
            render_rows(&t, None, "VER-X-001")
        ))
        .expect("parses");
        compare(&l, &t, &ids, Some(&eq_at))
    };
    let new_h = "#[kani::proof]\n#[kani::unwind(9)]\nfn h3() {}\n";
    ck("ADMIT control: a new harness with unwind(9) and the old bounds unchanged ([5,8] -> [5,8,9]) passes",
        unwind_admit(&format!("{base_rs}{new_h}")).is_ok());
    ck("PLANTED SEC: admit +1 on a file that also lowers an existing bound (8 -> 3) fails",
        unwind_admit(&format!("{}{}", base_rs.replace("unwind(8)", "unwind(3)"), new_h.replace("unwind(9)", "unwind(2)"))).is_err());
    ck("PLANTED SEC: ... also when the new bound could cover the lowered one (8 -> 3 plus a new 9)",
        unwind_admit(&format!("{}{new_h}", base_rs.replace("unwind(8)", "unwind(3)"))).is_err());
    let new_file_admit = parse_ledger(&format!("{sh_text}admit kani::assume b/src/lib.rs +1 @0123456789abcdef0123456789abcdef01234567 #467 a new driver\nkani::assume b/src/lib.rs 1 VER-X-001\n")).expect("parses");
    ck("ADMIT a new file's row (absent from the baseline) with a matching admit passes",
        compare(&new_file_admit, &new_file_tree, &ids, Some(&eq_at)).is_ok());
    // sen66's fake shrink: [6,28,30] -> [7,28] (one attribute deleted, 30 lowered to 7).
    ck("PLANTED shrink-only: a removal together with a lowered bound is ambiguous and fails",
        !unwind_not_narrower(&[Some(7), Some(28)], &[Some(6), Some(28), Some(30)]));
    ck("shrink-only: removing an attribute only (sub-multiset) passes", unwind_not_narrower(&[Some(6), Some(30)], &[Some(6), Some(28), Some(30)]));
    ck("shrink-only: raising only passes", unwind_not_narrower(&[Some(9), Some(28)], &[Some(6), Some(28)]));
    ck("PLANTED shrink-only: gaining a `?` fails", !unwind_not_narrower(&[None, None], &[Some(5), None]));
    ck("PLANTED shrink-only: a `?` replaced by a literal fails", !unwind_not_narrower(&[Some(5)], &[None]));
    // Round 11, F3: the same fake shrink split across the two comparisons --
    // the ledger records the removal ([6,28,30] -> [6,28], a sub-multiset) and
    // the tree "raises" over the ledger ([6,28] -> [7,28]).
    let u3 = |bs: &[u64]| bs.iter().enumerate().map(|(n, b)| format!("#[kani::proof]\n#[kani::unwind({b})]\nfn u{n}() {{}}\n")).collect::<String>();
    let base3 = parse_ledger(&format!("mode = equality\nbaseline = none\n{}", render_rows(&tree(&u3(&[6, 28, 30]), base_v), None, "VER-X-001"))).expect("parses");
    let led3 = |bs: &[u64]| parse_ledger(&format!("mode = shrink-only\nbaseline = {sha}\n{}", render_rows(&tree(&u3(bs), base_v), None, "VER-X-001"))).expect("parses");
    ck("F3 control: baseline [6,28,30], ledger and tree [6,28] (an honest removal) passes",
        compare(&led3(&[6, 28]), &tree(&u3(&[6, 28]), base_v), &ids, Some(&base3)).is_ok());
    ck("F3 control: baseline [6,28,30], ledger and tree [6,28,31] (a raise) passes",
        compare(&led3(&[6, 28, 31]), &tree(&u3(&[6, 28, 31]), base_v), &ids, Some(&base3)).is_ok());
    ck("PLANTED F3 baseline [6,28,30], ledger [6,28], tree [7,28] (30 lowered to 7 behind a recorded removal) fails",
        compare(&led3(&[6, 28]), &tree(&u3(&[7, 28]), base_v), &ids, Some(&base3)).is_err());
    ck("PLANTED F3b baseline [6,28,30], ledger [6], tree [7] fails",
        compare(&led3(&[6]), &tree(&u3(&[7]), base_v), &ids, Some(&base3)).is_err());
    // Round 11, F1 and F6: spellings that hid a kani::assume.
    let assumes = |src: &str| count_file("x.rs", src).counts.get("kani::assume").copied().unwrap_or(0);
    ck("PLANTED F1 a kani::assume after a raw C string `cr\"\\\"` (a `\"` in the trailing comment) counts",
        assumes("fn h(c: bool) {\n  kani::assume(c);\n  let _p = cr\"\\\"; kani::assume(c); // \"\n}\n") == 2);
    ck("PLANTED F1 ... and after `cr#\"\\\"#`", assumes("fn h(c: bool) { let _p = cr#\"\\\"#; kani::assume(c); } // \"\n") == 1);
    ck("PLANTED F6 `(kani::assume)(c)` counts", assumes("fn h(c: bool) { (kani::assume)(c); }\n") == 1);
    ck("PLANTED F6 `kani::assume::<>(c)` counts", assumes("fn h(c: bool) { kani::assume::<>(c); }\n") == 1);
    ck("F6 `kani::assume_x(c)` and `kani::assumed` are other names", assumes("fn h(c: bool) { kani::assume_x(c); let _ = kani::assumed; }\n") == 0);
    // Round 11, F2: a Lean interpolated string holding a quote hid a `sorry`.
    let interp = census_of([
        ("a/src/lib.rs", base_rs),
        ("proofs/p.v", base_v),
        ("proofs/L.lean", "def zz_s : String := s!\"{\"\\\"\"}\"\ntheorem zz_bad : False := sorry\n-- \"\n"),
        ("proofs/BUILD.bazel", "x = 1\n"),
    ]);
    ck("PLANTED F2 a Lean `s!\"{\"\\\"\"}\"` before a `sorry` is REFUSED (not counted around)",
        check_scanned(&interp).is_err_and(|e| e.contains("proofs/L.lean:1")));
    ck("PLANTED F2 ... and the run is REFUSED, not a pass", matches!(run_with(repo, check_scanned(&interp).map(|_| Census::default())), Verdict::Refused(_)));
    // Round 11, F5: a shallow clone would forget the ceiling.
    let shallow = |ok: bool, ans: &'static str| move |args: &[&str]| -> Result<(bool, String), String> {
        Ok(if args == ["rev-parse", "--is-shallow-repository"] { (ok, format!("{ans}\n")) } else { (false, String::new()) })
    };
    ck("F5 control: a full clone (`false`) is accepted", full_history_with(&shallow(true, "false")).is_ok());
    ck("PLANTED F5 a shallow clone (`true`) is refused", full_history_with(&shallow(true, "true")).is_err());
    ck("PLANTED F5 a git that cannot answer is refused", full_history_with(&shallow(false, "")).is_err() && full_history_with(&shallow(true, "--is-shallow-repository")).is_err());
    ck("SWITCH shrink-only without a baseline sha is rejected",
        parse_ledger(&eq_text.replace("mode = equality", "mode = shrink-only")).is_err());
    ck("equality with a baseline set is rejected",
        parse_ledger(&eq_text.replace("baseline = none", &format!("baseline = {sha}"))).is_err());
    ck("a ledger with no mode line is rejected", parse_ledger(&eq_text.replace("mode = equality\n", "")).is_err());
    ck("PLANTED a second `mode =` line is rejected", parse_ledger(&format!("{eq_text}mode = equality\n")).is_err());
    ck("PLANTED a second `baseline =` line is rejected", parse_ledger(&format!("{sh_text}baseline = {sha}\n")).is_err());
    ck("PLANTED a baseline sha that is not a commit is refused (git cat-file)",
        baseline_ledger(repo, "0000000000000000000000000000000000000000").is_err());
    let head = git(repo, &["rev-parse", "HEAD"]).ok().filter(|o| o.status.success()).map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
    match head {
        Some(h) => ck("N4 an untagged real commit (HEAD) as the baseline is refused", baseline_ledger(repo, &h).is_err()),
        None => ck("git rev-parse HEAD", false),
    }
    ck("PLANTED a shrink-only baseline that is not 40-hex is rejected by the parser",
        parse_ledger(&sh_text.replace(sha, "abc123")).is_err());
    // A fake repository: commit B (tagged v0.11.0, equality ledger `eq_text`),
    // its descendant A (untagged), and HEAD.
    let b = "b".repeat(40);
    let a = "a".repeat(40);
    let fake = |tagged: bool, ancestor: bool, history: Vec<(String, String)>| {
        let (b, eq_text) = (b.clone(), eq_text.clone());
        move |args: &[&str]| -> Result<(bool, String), String> {
            Ok(match args {
                ["cat-file", "-e", r] => (r.starts_with(&b) || r.starts_with(&"a".repeat(40)), String::new()),
                ["tag", "--points-at", s, ..] => (true, if tagged && *s == b { "v0.11.0\n".into() } else { String::new() }),
                ["tag", "--merged", ..] => (true, if tagged { "v0.11.0\n".into() } else { String::new() }),
                ["diff", ..] => (true, String::new()),
                ["rev-list", "-n", "1", "v0.11.0"] => (true, format!("{b}\n")),
                ["merge-base", "--is-ancestor", _, "HEAD"] => (ancestor, String::new()),
                ["show", spec] if spec.starts_with(&b) => (true, eq_text.clone()),
                ["show", spec] => match history.iter().find(|(r, _)| spec.starts_with(r.as_str())) {
                    Some((_, t)) => (true, t.clone()),
                    None => (false, String::new()),
                },
                ["log", ..] => (true, history.iter().map(|(r, _)| format!("{r}\n")).collect()),
                _ => (false, String::new()),
            })
        }
    };
    let ok_git = fake(true, true, vec![]);
    ck("N4 a tagged ancestor baseline resolves to its ledger", baseline_ledger_with(&ok_git, &b).is_ok_and(|l| l == eq));
    ck("N4 PLANTED an untagged baseline (commit A, set after its own growth) is refused", baseline_ledger_with(&ok_git, &a).is_err());
    ck("N4 PLANTED a tagged baseline that is not an ancestor of HEAD is refused", baseline_ledger_with(&fake(true, false, vec![]), &b).is_err());
    ck("N4 PLANTED a baseline that is not a commit is refused", baseline_ledger_with(&ok_git, &"c".repeat(40)).is_err());
    ck("N4 PLANTED a non-hex baseline is refused before any git call", baseline_ledger_with(&ok_git, "HEAD").is_err());
    // A repository answering yes to everything: only the 40-hex rule can refuse "HEAD".
    let eq_for_yes = eq_text.clone();
    let yes = move |args: &[&str]| -> Result<(bool, String), String> {
        Ok(match args {
            ["rev-list", ..] => (true, format!("{}\n", "z".repeat(40))),
            ["show", _] => (true, eq_for_yes.clone()),
            _ => (true, "v9\n".into()),
        })
    };
    ck("ONLY-CAUSE a 40-char non-hex baseline is refused by the 40-hex rule", baseline_ledger_with(&yes, &"z".repeat(40)).is_err());
    // Two releases: B = v0.11.0 (older), C = v0.12.0 (newest).
    let c = "c".repeat(40);
    let (b2, c2, eq2) = (b.clone(), c.clone(), eq_text.clone());
    let two = move |args: &[&str]| -> Result<(bool, String), String> {
        Ok(match args {
            ["cat-file", "-e", r] => (r.starts_with(&b2) || r.starts_with(&c2), String::new()),
            ["tag", "--points-at", s, ..] => {
                (true, if *s == b2 { "v0.11.0\n".into() } else if *s == c2 { "v0.12.0\n".into() } else { String::new() })
            }
            ["merge-base", "--is-ancestor", _, "HEAD"] => (true, String::new()),
            ["tag", "--merged", ..] => (true, "v0.11.0\nv0.12.0\n".into()),
            ["diff", ..] => (true, String::new()),
            ["rev-list", "-n", "1", "v0.12.0"] => (true, format!("{c2}\n")),
            ["show", _] => (true, eq2.clone()),
            _ => (false, String::new()),
        })
    };
    ck("SEC-1 PLANTED a baseline moved back to an older v* tag (B while C is newer) is refused", baseline_ledger_with(&two, &b).is_err());
    ck("SEC-1 the newest v* release (C) is accepted", baseline_ledger_with(&two, &c).is_ok());
    // Back to equality with a grown row, after the ledger was shrink-only at A.
    // The old shrink-only ledger names a STALE baseline: the ceiling must come
    // from the newest release (b), not from that field.
    let sh_b = eq_text.replace("mode = equality\nbaseline = none", &format!("mode = shrink-only\nbaseline = {}", "d".repeat(40)));
    let hist = fake(true, true, vec![(a.clone(), sh_b.clone())]);
    let back = parse_ledger(&format!("mode = equality\nbaseline = none\n{}", render_rows(&tree(&added, base_v), None, "VER-X-001"))).expect("parses");
    let ceiling = ceiling_with(&hist, &back);
    ck("N4 equality after a shrink-only ledger in history: the tagged baseline is still the ceiling",
        ceiling.as_ref().is_ok_and(|c| c.as_ref().is_some_and(|(s, _)| *s == b)));
    ck("N4 PLANTED shrink-only -> equality with a grown row (assume 2 -> 3) FAILS",
        ceiling.ok().flatten().is_some_and(|(_, l)| compare(&back, &tree(&added, base_v), &ids, Some(&l)).is_err()));
    let back_same = parse_ledger(&eq_text).expect("parses");
    ck("shrink-only -> equality with no growth passes",
        ceiling_with(&hist, &back_same).ok().flatten().is_some_and(|(_, l)| compare(&back_same, &c0, &ids, Some(&l)).is_ok()));
    ck("equality with no shrink-only ledger in history has no ceiling", ceiling_with(&ok_git, &back_same).is_ok_and(|c| c.is_none()));
    // Finding 10: a `-s ours` merge kept the equality ledger and the shrink-only
    // commit sits on the merged side; plain `git log -- LEDGER` simplifies the
    // merge to the kept side and never lists it.
    let merged = {
        let inner = fake(true, true, vec![(a.clone(), sh_b.clone())]);
        move |args: &[&str]| -> Result<(bool, String), String> {
            if args.first() == Some(&"log") && !args.contains(&"--full-history") {
                return Ok((true, String::new()));
            }
            inner(args)
        }
    };
    ck("PLANTED finding 10: a shrink-only commit behind a `-s ours` merge still sets the ceiling (growth FAILS)",
        ceiling_with(&merged, &back).ok().flatten().is_some_and(|(_, l)| compare(&back, &tree(&added, base_v), &ids, Some(&l)).is_err()));
    // Finding 11: HEAD is the tagged release commit C (v0.12.0); its ledger names
    // the release before it, B (v0.11.0). A ledger cannot name its own commit.
    let (b3, c3, eq3) = (b.clone(), c.clone(), eq_text.clone());
    let rel = move |args: &[&str]| -> Result<(bool, String), String> {
        Ok(match args {
            ["cat-file", "-e", r] => (r.starts_with(&b3) || r.starts_with(&c3), String::new()),
            ["tag", "--points-at", "HEAD", ..] => (true, "v0.12.0\n".into()),
            ["tag", "--points-at", s, ..] => {
                (true, if *s == b3 { "v0.11.0\n".into() } else if *s == c3 { "v0.12.0\n".into() } else { String::new() })
            }
            ["merge-base", "--is-ancestor", _, "HEAD"] => (true, String::new()),
            ["tag", "--merged", ..] => (true, "v0.11.0\nv0.12.0\n".into()),
            ["diff", ..] => (true, String::new()),
            ["rev-list", "-n", "1", "v0.11.0"] => (true, format!("{b3}\n")),
            ["rev-list", "-n", "1", "v0.12.0"] => (true, format!("{c3}\n")),
            ["show", _] => (true, eq3.clone()),
            _ => (false, String::new()),
        })
    };
    ck("PLANTED finding 11: on the tagged release commit, the baseline naming the release before it is accepted",
        baseline_ledger_with(&rel, &b).is_ok());
    ck("finding 11: on the tagged release commit, a baseline naming HEAD's own tag is refused",
        baseline_ledger_with(&rel, &c).is_err());
    ck("finding 12: the growth message states the rule, not an owner-approval mechanism",
        !GROWTH_RULE.contains("owner-approved") && GROWTH_RULE.contains("admit") && GROWTH_RULE.contains("not that anyone reviewed or approved"));

    // T2 (round 4): a merged maintenance tag v1.0.1 is NEARER than v2.0.0 (git
    // describe picked it); the newest release is the highest version.
    let tags3 = |merged: &'static str| {
        move |args: &[&str]| -> Result<(bool, String), String> {
            Ok(match args {
                ["tag", "--points-at", ..] => (true, String::new()),
                ["tag", "--merged", ..] => (true, merged.into()),
                ["rev-list", "-n", "1", t] => (true, format!("{}\n", &t.chars().filter(|c| c.is_ascii_digit()).collect::<String>().repeat(40)[..40])),
                _ => (false, String::new()),
            })
        }
    };
    let want_v2 = "200".repeat(40)[..40].to_string();
    ck("PLANTED T2: with v1.0.0, a merged v1.0.1 and v2.0.0 reachable, the newest release is v2.0.0 (not the nearest)",
        newest_release_with(&tags3("v1.0.1\nv1.0.0\nv2.0.0\n")).is_ok_and(|s| s == want_v2));
    ck("T2: 0.10.0 orders after 0.9.0 (numbers, not strings)", release_version("v0.10.0") > release_version("v0.9.0"));
    // Security review: the suffix was compared as a string, so rc.10 < rc.9.
    // Pre-releases are not releases: skipped, never a baseline.
    let want_v09 = "090".repeat(40)[..40].to_string();
    ck("PLANTED SEC: with v0.9.0, v1.0.0-rc.9 and v1.0.0-rc.10 reachable, the newest release is v0.9.0 (pre-releases skipped)",
        newest_release_with(&tags3("v0.9.0\nv1.0.0-rc.9\nv1.0.0-rc.10\n")).is_ok_and(|s| s == want_v09));
    ck("SEC: only pre-release tags reachable -> no release (an error, never a pre-release baseline)",
        newest_release_with(&tags3("v1.0.0-rc.9\nv1.0.0-rc.10\n")).is_err());
    ck("T2: a `v<digit>` tag that is not vMAJOR.MINOR.PATCH (`v1.2`) is an error, not skipped (fails closed)", newest_release_with(&tags3("v1.0.0\nv1.2\n")).is_err());
    ck("T2: a `v*` tag that is not version-shaped (`vendor-x`) is ignored", newest_release_with(&tags3("v1.0.0\nvendor-x\n")).is_ok());
    // T3 (round 4): the shrink-only search has no cap.
    let capped = {
        let inner = fake(true, true, vec![(a.clone(), sh_b.clone())]);
        move |args: &[&str]| -> Result<(bool, String), String> {
            if args.first() == Some(&"log") && args.contains(&"-n") {
                return Ok((true, String::new()));
            }
            inner(args)
        }
    };
    ck("PLANTED T3: the shrink-only commit beyond any -n cap still sets the ceiling",
        last_shrink_only_with(&capped).is_ok_and(|r| r.is_some()));
    // T8 (round 4): a pure rename carries the baseline row to the new path.
    let renamed_tree = census_of([
        ("b/src/lib.rs", base_rs),
        ("proofs/p.v", base_v),
        ("proofs/L.lean", "theorem x : True := trivial\n"),
        ("proofs/BUILD.bazel", "x = 1\n"),
    ]);
    let renamed_ledger = parse_ledger(&sh_text.replace("a/src/lib.rs", "b/src/lib.rs")).expect("parses");
    let mv = |status: &'static str| {
        move |args: &[&str]| -> Result<(bool, String), String> {
            Ok(match args {
                ["diff", ..] => (true, format!("{status}\ta/src/lib.rs\tb/src/lib.rs\n")),
                _ => (false, String::new()),
            })
        }
    };
    let followed = follow_renames(&mv("R100"), sha, parse_ledger(&eq_text).expect("parses")).expect("follows");
    ck("PLANTED T8: `git mv a b` with the row path updated passes against the renamed baseline",
        compare(&renamed_ledger, &renamed_tree, &ids, Some(&followed)).is_ok());
    let not_followed = follow_renames(&mv("R090"), sha, parse_ledger(&eq_text).expect("parses")).expect("follows");
    ck("T8 control: a rename with edits (R090) is not followed (growth in the new path fails)",
        compare(&renamed_ledger, &renamed_tree, &ids, Some(&not_followed)).is_err());

    // -- lexers (round 3 findings 6 and 7) ------------------------------------
    for (what, got) in crate::gates::lex::self_check() {
        ck(&what, got);
    }

    // -- ledger rows --------------------------------------------------------
    let unknown = parse_ledger(&eq_text.replace("VER-X-001", "VER-NOPE-001")).expect("parses");
    ck("a row naming an artifact that does not exist fails", compare(&unknown, &c0, &ids, None).is_err());
    ck("a row with no artifact ID is rejected",
        parse_ledger(&eq_text.replace("rocq:Admitted proofs/p.v 1 VER-X-001", "rocq:Admitted proofs/p.v 1")).is_err());
    ck("a zero row is rejected", parse_ledger(&eq_text.replace("rocq:Admitted proofs/p.v 1 VER-X-001", "rocq:Admitted proofs/p.v 0 VER-X-001")).is_err());
    ck("an unwind row whose bounds disagree with its count is rejected",
        parse_ledger(&eq_text.replace("kani::unwind a/src/lib.rs 2 5,8 VER-X-001", "kani::unwind a/src/lib.rs 2 5 VER-X-001")).is_err());
    ck("a census failure is REFUSED, never a pass", matches!(run_with(repo, Err("planted".into())), Verdict::Refused(_)));

    // -- the real tree --------------------------------------------------------
    match census(repo) {
        Ok(c) => ck("census reads the tracked tree (rs, v, lean and Bazel all non-empty)", c.kani_proofs > 0),
        Err(e) => ck(&format!("census reads the tracked tree: {e}"), false),
    }

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

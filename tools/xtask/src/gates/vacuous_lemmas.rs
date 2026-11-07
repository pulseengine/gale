//! No proof fn may prove nothing.
//!
//! gale#462: `sem::lemma_invariant_inductive` claimed P1 ("the invariant is
//! inductive across all operations") with `ensures true` and an empty body.
//! Verus accepts that whatever the code does. Measured across src/ it was not one
//! lemma but 92 of 309 proof fns (30%) -- every module's P1 among them -- with the
//! property written as COMMENTS above `true`, and 18 of them cited as evidence by
//! approved requirements.
//!
//! A proof fn is VACUOUS here when, with comments removed, its `ensures` is
//! exactly `true` (or absent) AND its body is empty. A non-empty body is not
//! counted: `assert`s inside a body are checked by Verus even when the ensures is
//! `true`.
//!
//! This is a RATCHET, not a rewrite. The ledger (vacuous-lemmas.ledger) records
//! every vacuous lemma that existed when the gate landed and fails in BOTH
//! directions: a new vacuous lemma fails, and a ledgered one that has been
//! repaired or removed fails until its ledger line is deleted. The count can only
//! fall, and the ledger cannot outlive the gaps it names.

use crate::verdict::Verdict;
use std::collections::BTreeSet;
use std::path::Path;

pub const NAME: &str = "vacuous-lemmas";

const LEDGER: &str = "tools/xtask/vacuous-lemmas.ledger";

/// Remove `//` line comments and `/* */` block comments, keeping string
/// literals intact (Verus proof headers rarely contain them, but a `"//"`
/// inside one must not truncate the line).
fn strip_comments(src: &str) -> String {
    let b = src.as_bytes();
    let mut out = String::with_capacity(src.len());
    let mut i = 0;
    let mut in_str = false;
    while i < b.len() {
        let c = b[i];
        if in_str {
            out.push(c as char);
            if c == b'\\' && i + 1 < b.len() {
                out.push(b[i + 1] as char);
                i += 2;
                continue;
            }
            if c == b'"' {
                in_str = false;
            }
            i += 1;
            continue;
        }
        if c == b'"' {
            in_str = true;
            out.push('"');
            i += 1;
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'/' {
            while i < b.len() && b[i] != b'\n' {
                i += 1;
            }
        } else if c == b'/' && i + 1 < b.len() && b[i + 1] == b'*' {
            i += 2;
            while i + 1 < b.len() && !(b[i] == b'*' && b[i + 1] == b'/') {
                i += 1;
            }
            i += 2;
        } else {
            out.push(c as char);
            i += 1;
        }
    }
    out
}

/// One function header, as far as this gate needs it.
#[derive(Debug, PartialEq)]
struct Header {
    name: String,
    params: String,
    requires: String,
    is_proof: bool,
    external_body: bool,
    ensures: String,
    empty_body: bool,
}

/// Index of the function body's `{`, scanning from `from` (just past the
/// name). Inner clause blocks are skipped whole.
fn find_body(b: &[u8], from: usize) -> Option<usize> {
    let mut depth: i32 = 0;
    let mut seen_params = false;
    let mut pending: i32 = 0; // `if` / `match` whose block has not opened yet
    let mut j = from;
    let word_at = |j: usize, w: &[u8]| -> bool {
        j + w.len() <= b.len()
            && &b[j..j + w.len()] == w
            && (j == 0 || !(b[j - 1].is_ascii_alphanumeric() || b[j - 1] == b'_'))
            && (j + w.len() == b.len() || !(b[j + w.len()].is_ascii_alphanumeric() || b[j + w.len()] == b'_'))
    };
    while j < b.len() {
        match b[j] {
            b'(' => {
                depth += 1;
                seen_params = true;
            }
            b')' => depth -= 1,
            b';' if depth == 0 => return None,
            _ if depth == 0 && seen_params && (word_at(j, b"if") || word_at(j, b"match")) => {
                pending += 1;
            }
            b'{' if depth == 0 && seen_params => {
                // previous non-whitespace token
                let mut k = j;
                while k > 0 && (b[k - 1] as char).is_whitespace() {
                    k -= 1;
                }
                let prev2 = if k >= 2 { &b[k - 2..k] } else { &b[..k] };
                let after_op = prev2 == b"=>" || prev2 == b"&&" || prev2 == b"||";
                let after_else = k >= 4 && &b[k - 4..k] == b"else";
                // No struct-literal case: Verus rejects a bare struct literal in a
                // clause, so one can only appear inside parens (depth > 0). A
                // first version guessed "uppercase identifier then `{`" and took
                // `-> IpcEndpointState {` -- a return type -- for one, swallowing
                // the functions after it.
                if after_op || after_else || pending > 0 {
                    if pending > 0 && !after_else {
                        pending -= 1;
                    }
                    // skip the whole inner block
                    let mut bd = 0i32;
                    while j < b.len() {
                        if b[j] == b'{' {
                            bd += 1;
                        } else if b[j] == b'}' {
                            bd -= 1;
                            if bd == 0 {
                                break;
                            }
                        }
                        j += 1;
                    }
                } else {
                    return Some(j);
                }
            }
            _ => {}
        }
        j += 1;
    }
    None
}

/// Every `fn` header in one file's (comment-stripped) text.
fn headers(src: &str) -> Vec<Header> {
    let s = strip_comments(src);
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(off) = s[from..].find("fn ") {
        let start = from + off;
        // a whole token: preceded by start or whitespace
        if start > 0 && !(b[start - 1] as char).is_whitespace() {
            from = start + 3;
            continue;
        }
        let name_start = start + 3;
        let name: String = s[name_start..]
            .chars()
            .take_while(|c| c.is_alphanumeric() || *c == '_')
            .collect();
        if name.is_empty() {
            from = name_start;
            continue;
        }
        // The text between the previous item end and `fn` holds the qualifiers
        // and attributes: look back to the last `}` / `;` (item boundary).
        let lead_from = s[..start].rfind(|c| c == '}' || c == ';').map(|i| i + 1).unwrap_or(0);
        let lead = &s[lead_from..start];
        let is_proof = lead.split_whitespace().last() == Some("proof");
        let external_body = lead.contains("verifier::external_body");
        // The body's opening brace: the first `{` at paren depth 0 after the
        // parameter list that is not an INNER block of a clause. Clauses contain
        // blocks: `ensures ({ .. })` (inside parens), `x ==> { .. }`,
        // `if c { .. } else { .. }`, `match r { .. }`. A first version took the
        // first depth-0 `{` and so read `merge_left ==> {` in heap's requires as
        // the body, losing the ensures (gale#462).
        let body = find_body(b, name_start + name.len());
        let Some(bo) = body else {
            from = name_start;
            continue;
        };
        let header = &s[name_start..bo];
        let params = header.find('(').and_then(|o| header.find(')').map(|c| header[o + 1..c].to_string())).unwrap_or_default();
        let requires = match (header.find("requires"), header.find("ensures")) {
            (Some(r), Some(e)) if r < e => header[r..e].to_string(),
            (Some(r), _) => header[r..].to_string(),
            _ => String::new(),
        };
        let ensures = match header.find("ensures") {
            Some(k) => header[k + "ensures".len()..].split_whitespace().collect::<Vec<_>>().join(" "),
            None => String::new(),
        };
        let ensures = ensures.trim().trim_end_matches(',').trim().to_string();
        let empty_body = s[bo + 1..].trim_start().starts_with('}');
        out.push(Header { name, params, requires, is_proof, external_body, ensures, empty_body });
        from = bo + 1;
    }
    out
}

/// A proof fn that states nothing: `ensures` is `true` or absent, body empty.
fn is_vacuous(h: &Header) -> bool {
    h.is_proof && h.empty_body && (h.ensures == "true" || h.ensures.is_empty())
}

/// A function whose contract Verus ASSUMES: `external_body` with a real
/// `ensures`. Worse than vacuous when it is a proof fn -- it is an axiom other
/// proofs can rest on.
fn is_assumed(h: &Header) -> bool {
    h.external_body && !h.ensures.is_empty() && h.ensures != "true"
}

/// Ledger keys for one file: `vacuous:m::f` and `assumed:m::f`.
///
/// A name can occur twice in one file (`new` in two `impl` blocks). Repeat
/// occurrences get `#2`, `#3`..., so repairing one of a pair is visible to the
/// ratchet instead of hidden behind its twin's identical key.
fn classify(stem: &str, src: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen: std::collections::BTreeMap<String, usize> = Default::default();
    for h in headers(src) {
        let class = if is_vacuous(&h) {
            "vacuous"
        } else if is_assumed(&h) {
            "assumed"
        } else {
            continue;
        };
        let base = format!("{class}:{stem}::{}", h.name);
        let n = seen.entry(base.clone()).or_insert(0);
        *n += 1;
        out.push(if *n == 1 { base } else { format!("{base}#{n}") });
    }
    out
}

/// Names of vacuous proof fns only (self-test convenience).
fn vacuous_in(src: &str) -> Vec<String> {
    headers(src).into_iter().filter(is_vacuous).map(|h| h.name).collect()
}

/// P1, checked structurally instead of by a lemma (gale#462). In a module whose
/// state has an invariant, every operation that takes `&mut self` must
/// `require old(self).inv()` and `ensure self.inv()` -- Verus then proves P1
/// operation by operation. The 23 `lemma_invariant_inductive` stubs only
/// restated that in comments; this catches the real failure, an operation
/// added (or edited) without the clauses. External-body ops are excluded:
/// their contracts are assumed, and the `assumed:` ledger tracks them.
fn p1_gaps(stem: &str, src: &str) -> Vec<String> {
    if !strip_comments(src).contains("spec fn inv(&self)") {
        return Vec::new();
    }
    headers(src)
        .into_iter()
        .filter(|h| !h.is_proof && !h.external_body && h.params.contains("&mut self"))
        // `inv(..)` with arguments counts: zms's invariant is parameterised.
        .filter(|h| !(h.requires.contains("old(self).inv(") && h.ensures.contains("self.inv(")))
        .map(|h| format!("p1:{stem}::{}", h.name))
        .collect()
}

fn census(repo: &Path) -> Result<BTreeSet<String>, String> {
    let dir = repo.join("src");
    let rd = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut files: Vec<_> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().and_then(|e| e.to_str()) == Some("rs"))
        .collect();
    files.sort();
    let mut set = BTreeSet::new();
    for f in files {
        let stem = f.file_stem().unwrap().to_string_lossy().to_string();
        let t = std::fs::read_to_string(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        for k in classify(&stem, &t) {
            set.insert(k);
        }
        for k in p1_gaps(&stem, &t) {
            set.insert(k);
        }
    }
    Ok(set)
}

fn ledger(repo: &Path) -> Result<BTreeSet<String>, String> {
    let t = std::fs::read_to_string(repo.join(LEDGER)).map_err(|e| format!("{LEDGER}: {e}"))?;
    Ok(t.lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(String::from)
        .collect())
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let found = match census(repo) {
        Ok(f) => f,
        Err(e) => return Verdict::Refused(e),
    };
    let led = match ledger(repo) {
        Ok(l) => l,
        Err(e) => return Verdict::Refused(e),
    };
    let new: Vec<&String> = found.difference(&led).collect();
    let gone: Vec<&String> = led.difference(&found).collect();
    let nv = found.iter().filter(|k| k.starts_with("vacuous:")).count();
    let na = found.iter().filter(|k| k.starts_with("assumed:")).count();
    let np = found.iter().filter(|k| k.starts_with("p1:")).count();
    let mut lines = vec![format!(
        "{nv} vacuous proof fn(s) (ensures `true`/absent, empty body) and {na} assumed \
         contract(s) (external_body with an ensures), {np} &mut-self op(s) not carrying inv \
         in src/; ledger holds {}",
        led.len()
    )];
    if new.is_empty() && gone.is_empty() {
        lines.push("ledger exact -- the count can only fall from here".into());
        return Verdict::Pass(lines);
    }
    for n in &new {
        lines.push(format!(
            "FAIL: {n} is not on the ledger -- {}",
            if n.starts_with("assumed:") {
                "prove its contract instead of assuming it"
            } else if n.starts_with("p1:") {
                "require old(self).inv() and ensure self.inv()"
            } else {
                "state its property"
            }
        ));
    }
    for g in &gone {
        lines.push(format!("FAIL: {g} is on the ledger but no longer applies -- delete its ledger line"));
    }
    Verdict::Fail(lines)
}

pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };
    let v = |s: &str| vacuous_in(s);

    // The exact shape of the reported defect: prose in comments above `true`.
    let reported = "pub proof fn lemma_p1()\n    ensures\n        // init establishes inv\n        // give preserves inv\n        true,\n{\n}\n";
    ck("ensures `true` under comment prose is vacuous", v(reported) == ["lemma_p1"]);
    ck("one-line `ensures true, {}` is vacuous", v("proof fn a() ensures true, {}") == ["a"]);
    ck("no ensures and an empty body is vacuous", v("proof fn b() {}") == ["b"]);
    ck(
        "a requires does not rescue an ensures-true lemma",
        v("proof fn c(x: u32) requires x > 0, ensures true, {}") == ["c"],
    );
    // Ways it could over-report.
    ck("a real ensures is not vacuous", v("proof fn d(x: u32) ensures x + 0 == x, {}").is_empty());
    ck(
        "an `ensures ({ ... })` block is not mistaken for the body",
        v("proof fn e(a: u32) requires a < 5, ensures ({ let b = a + 1; b > a }) {}").is_empty(),
    );
    ck(
        "ensures true with a non-empty body is not counted (asserts are checked)",
        v("proof fn f() ensures true, { assert(1 + 1 == 2); }").is_empty(),
    );
    ck("`true` inside a comment alone does not make a real ensures vacuous",
        v("proof fn g(x: u32) ensures // true\n x == x, {}").is_empty());
    ck("a spec fn is not a proof fn", v("spec fn h() -> bool { true }").is_empty());
    // The assumed class.
    let ax = "#[verifier::external_body]\npub proof fn ax(x: u32) ensures x + 1 > x, {}\n";
    ck("an external_body proof fn with an ensures is ASSUMED", classify("m", ax) == ["assumed:m::ax"]);
    let ex = "#[verifier::external_body]\npub fn op(&mut self) -> (r: u32) ensures r == 0, { 0 }\n";
    ck("an external_body exec fn with an ensures is ASSUMED", classify("m", ex) == ["assumed:m::op"]);
    let stub = "#[verifier::external_body]\npub proof fn st() { }\n";
    ck("an external_body stub with no ensures is vacuous, not assumed", classify("m", stub) == ["vacuous:m::st"]);
    let ok = "pub fn op2(x: u32) -> (r: u32) ensures r == x, { x }\nfn later() {}\n";
    ck("a checked exec fn is neither", classify("m", ok).is_empty());
    let twin = "#[verifier::external_body]\npub fn new() -> (r: u32) ensures r == 0, { 0 }\n}\n#[verifier::external_body]\npub fn new() -> (r: u32) ensures r == 1, { 1 }\n";
    let inv_mod = "spec fn inv(&self) -> bool { self.x < 5 }\npub fn good(&mut self) requires old(self).inv(), ensures self.inv(), { }\npub fn bad(&mut self) requires old(self).inv(), ensures self.x == 0, { }\npub fn reader(&self) -> (r: u32) ensures r == 0, { 0 }\n";
    ck("P1: an &mut op without `ensures self.inv()` is a gap", p1_gaps("m", inv_mod) == ["p1:m::bad"]);
    ck("P1: no inv in the module, no obligation", p1_gaps("m", "pub fn f(&mut self) { }\n").is_empty());
    let inner = "pub fn c(&mut self, m: bool) requires old(self).inv(), m ==> { if m { true } else { false } }, ensures self.inv(), match r { Ok(x) => { x > 0 }, Err(e) => { e < 0 } }, { }\nspec fn inv(&self) -> bool { true }\n";
    ck("inner `==> {`, `if {} else {}` and `match {}` blocks are not the body", p1_gaps("m", inner).is_empty());
    ck("a parenthesised struct literal in ensures is not the body",
        vacuous_in("proof fn s() ensures (S { a: 1 }).a == 1, {}").is_empty());
    ck("an uppercase return type is followed by the real body",
        classify("m", "pub open spec fn st(self) -> State { State::A }\n#[verifier::external_body]\npub fn d(v: bool) -> (r: i32) ensures v ==> r == 0, { 0 }\n") == ["assumed:m::d"]);
    ck("a duplicated name yields two distinct keys", classify("m", twin) == ["assumed:m::new", "assumed:m::new#2"]);
    let attr_other = "#[verifier::external_body]\npub fn a1() ensures true, {}\n}\npub fn a2(x: u32) -> (r: u32) ensures r == x, { x }\n";
    ck("external_body does not leak past an item boundary", classify("m", attr_other).is_empty());
    // The census must actually read the tree; run() compares it to the ledger.
    ck("census reads src/", census(repo).is_ok());
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

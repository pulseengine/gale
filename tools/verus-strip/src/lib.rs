//! Verus annotation stripper — converts Verus-annotated Rust to plain Rust.
//!
//! Uses `proc_macro2` for tokenization (matched delimiters for free) and
//! `syn` for parsing the preamble and validating output.
//!
//! Strips:
//! - `verus! { ... }` wrapper
//! - `use vstd::*` imports
//! - `pub open spec fn` / `pub closed spec fn` / `pub proof fn` (entire items)
//! - `requires` / `ensures` / `invariant` / `decreases` / `recommends` clauses
//! - Named return type bindings: `-> (name: Type)` → `-> Type`
//! - Verus `assert(...)` proof assertions (no `!`)
//! - `#[verifier::*]` and `#[trigger]` attributes
//! - Doc comments preceding stripped items

use proc_macro2::{TokenStream, TokenTree, Delimiter, Spacing};

/// Result of stripping a file.
pub struct StripResult {
    pub output: String,
    pub errors: Vec<String>,
}

/// Strip Verus annotations from a Rust source file.
pub fn strip_file(input: &str) -> StripResult {
    let (preamble, body, _after) = split_at_verus_macro(input);
    let clean_preamble = strip_vstd_imports(&preamble);
    let stripped_body = strip_body(&body);

    // Reassemble: preamble (comments + use statements) + stripped body
    let mut raw = String::new();
    let trimmed_pre = clean_preamble.trim_end();
    if !trimmed_pre.is_empty() {
        raw.push_str(trimmed_pre);
        raw.push('\n');
    }
    let trimmed_body = stripped_body.trim();
    if !trimmed_body.is_empty() {
        if !raw.is_empty() {
            raw.push('\n');
        }
        raw.push_str(trimmed_body);
        raw.push('\n');
    }

    // Parse and re-format with prettyplease for clean output
    match syn::parse_file(&raw) {
        Ok(file) => {
            let mut output = prettyplease::unparse(&file);
            // Preserve non-doc comments from preamble.
            // prettyplease preserves `//!` inner doc comments (they are
            // syntactic attrs in the AST), so we only re-prepend regular
            // `//` comments that prettyplease would drop.
            let doc_lines: Vec<&str> = trimmed_pre
                .lines()
                .take_while(|l| {
                    let t = l.trim();
                    t.is_empty() || t.starts_with("//")
                })
                .filter(|l| {
                    let t = l.trim();
                    // Keep blank lines and regular comments, but NOT //! doc comments
                    // (prettyplease already emits those from the AST)
                    t.is_empty() || (t.starts_with("//") && !t.starts_with("//!"))
                })
                .collect();
            if !doc_lines.is_empty() {
                let doc_block = doc_lines.join("\n");
                let trimmed_doc = doc_block.trim_end();
                if !trimmed_doc.is_empty() {
                    output = format!("{}\n\n{}", trimmed_doc, output);
                }
            }
            StripResult { output, errors: vec![] }
        }
        Err(e) => {
            // If parsing fails, return raw output with errors
            StripResult {
                output: raw,
                errors: vec![format!("{e}")],
            }
        }
    }
}

// ─── Phase 1: Find verus! macro ─────────────────────────────────────────

/// Split input at the `verus! { ... }` macro boundary.
/// Returns (text_before, body_inside_braces, text_after).
fn split_at_verus_macro(input: &str) -> (String, String, String) {
    // Find "verus!" followed by a brace-delimited block.
    // We scan for the token sequence rather than parsing, since the
    // body contains non-standard Rust that confuses full parsers.
    if let Some(macro_start) = input.find("verus!") {
        let after_bang = macro_start + "verus!".len();
        // Find the opening {
        let rest = &input[after_bang..];
        if let Some(brace_offset) = rest.find('{') {
            let body_start = after_bang + brace_offset + 1;
            // Find matching closing } by counting braces
            let mut depth = 1i32;
            let mut pos = body_start;
            let bytes = input.as_bytes();
            while pos < bytes.len() && depth > 0 {
                match bytes[pos] {
                    b'{' => depth += 1,
                    b'}' => depth -= 1,
                    b'/' if pos + 1 < bytes.len() && bytes[pos + 1] == b'/' => {
                        // Skip line comment
                        while pos < bytes.len() && bytes[pos] != b'\n' {
                            pos += 1;
                        }
                        continue;
                    }
                    b'"' => {
                        // Skip string literal
                        pos += 1;
                        while pos < bytes.len() && bytes[pos] != b'"' {
                            if bytes[pos] == b'\\' { pos += 1; }
                            pos += 1;
                        }
                    }
                    _ => {}
                }
                pos += 1;
            }
            if depth == 0 {
                let body_end = pos - 1; // before the closing }
                let before = input[..macro_start].to_string();
                let body = input[body_start..body_end].to_string();
                let after = input[pos..].to_string();
                return (before, body, after);
            }
        }
    }
    (input.to_string(), String::new(), String::new())
}

/// Remove `use vstd::...` lines.
fn strip_vstd_imports(text: &str) -> String {
    text.lines()
        .filter(|line| !line.trim().starts_with("use vstd"))
        .collect::<Vec<_>>()
        .join("\n")
}

// ─── Phase 2: Strip verus body using token trees ────────────────────────

/// Strip Verus annotations from the body of a `verus! { }` block.
fn strip_body(body: &str) -> String {
    // Strip leading module doc comments (//!) from the body — the preamble
    // already has them, so they'd be duplicated.
    let body = strip_leading_doc_comments(body);

    // Parse as token stream. If parsing fails (unlikely for valid Verus code),
    // fall back to returning the body unchanged.
    let tokens: TokenStream = match body.parse() {
        Ok(ts) => ts,
        Err(_) => return body.to_string(),
    };

    let trees: Vec<TokenTree> = tokens.into_iter().collect();
    let mut out = String::new();
    let mut i = 0;

    while i < trees.len() {
        // Check for doc comment attributes preceding a verus item.
        // In proc_macro2, `/// text` becomes `#` + `[doc = "text"]`.
        // If doc attrs are followed by a verus item, skip both.
        if is_doc_attr_at(&trees, i) {
            let past_docs = skip_doc_attrs(&trees, i);
            if try_skip_verus_item(&trees, past_docs).is_some() {
                // Doc attrs + verus item — skip everything
                let skip_to = try_skip_verus_item(&trees, past_docs).unwrap();
                i = skip_to;
                trim_trailing_blank_lines(&mut out);
                continue;
            }
            // Doc attrs before a normal item — emit them
        }

        // Check for items to skip: spec fn, proof fn
        if let Some(skip_to) = try_skip_verus_item(&trees, i) {
            strip_trailing_doc_comments(&mut out);
            trim_trailing_blank_lines(&mut out);
            i = skip_to;
            continue;
        }

        // Check for verus clause keywords: requires, ensures, invariant, decreases
        if is_verus_clause_at(&trees, i) {
            let keyword = if let TokenTree::Ident(id) = &trees[i] {
                id.to_string()
            } else {
                String::new()
            };
            let skip_to = skip_clause(&trees, i);
            trim_trailing_whitespace(&mut out);
            i = skip_to;
            continue;
        }

        // Check for Verus assert(...) — no `!`
        if is_verus_assert_at(&trees, i) {
            let skip_to = skip_verus_assert(&trees, i);
            trim_trailing_blank_lines(&mut out);
            i = skip_to;
            continue;
        }

        // Check for #[verifier::*] or #[trigger] attributes
        if is_verifier_attr_at(&trees, i) {
            i += 2; // skip # + [group]
            continue;
        }

        // Check for named return type: -> (name: Type)
        if is_arrow_at(&trees, i) {
            if let Some((replacement, skip_to)) = try_strip_named_return(&trees, i) {
                out.push_str(&replacement);
                i = skip_to;
                continue;
            }
        }

        // For brace groups, recurse to strip verus constructs inside
        // (impl bodies have spec/proof fns; function bodies have
        // loop invariants, decreases, and proof assertions).
        if let TokenTree::Group(g) = &trees[i] {
            if g.delimiter() == Delimiter::Brace {
                let inner = strip_body(&g.stream().to_string());
                out.push('{');
                out.push_str(&inner);
                out.push('}');
                i += 1;
                continue;
            }
        }

        // Emit the token
        emit_token(&trees[i], &mut out);
        i += 1;
    }

    out
}

// ─── Item detection ─────────────────────────────────────────────────────

/// Check if tokens at `pos` start a Verus item to skip (spec fn, proof fn).
/// Returns the index past the item's closing brace if matched.
fn try_skip_verus_item(trees: &[TokenTree], pos: usize) -> Option<usize> {
    let idents = collect_idents(trees, pos);
    let id_strs: Vec<&str> = idents.iter().map(|s| s.as_str()).collect();

    let is_spec = matches!(
        id_strs.as_slice(),
        ["pub", "open", "spec", "fn", ..]
        | ["pub", "closed", "spec", "fn", ..]
    );
    let is_proof = matches!(
        id_strs.as_slice(),
        ["pub", "proof", "fn", ..] | ["proof", "fn", ..]
    );

    if !is_spec && !is_proof {
        return None;
    }

    // Find the function body: it's the last Brace group in the item.
    // Scan forward to find all brace groups; the last one is the body.
    let mut last_brace_end = None;
    let mut j = pos;
    while j < trees.len() {
        match &trees[j] {
            TokenTree::Group(g) if g.delimiter() == Delimiter::Brace => {
                last_brace_end = Some(j + 1);
                // Check if next non-punct token starts a new item
                let next = next_meaningful(trees, j + 1);
                match next {
                    Some(k) if is_item_start(trees, k) => return last_brace_end,
                    None => return last_brace_end,
                    _ => {}
                }
            }
            // If we hit another item-starting keyword (and we've seen at least
            // one brace group), the current item ended before this.
            TokenTree::Ident(id) if last_brace_end.is_some() && is_item_keyword(&id.to_string()) => {
                return last_brace_end;
            }
            _ => {}
        }
        j += 1;
    }

    last_brace_end
}

/// Collect up to 5 consecutive ident strings starting at pos.
fn collect_idents(trees: &[TokenTree], pos: usize) -> Vec<String> {
    let mut result = Vec::new();
    let mut j = pos;
    while j < trees.len() && result.len() < 5 {
        match &trees[j] {
            TokenTree::Ident(id) => {
                result.push(id.to_string());
                j += 1;
            }
            _ => break,
        }
    }
    result
}

/// Check if the Brace group at `pos` is an impl or mod body
/// (as opposed to a function body, match arm, loop body, etc.).
/// We check by looking backwards for `impl` or `mod` keywords.
fn is_impl_or_mod_body(trees: &[TokenTree], pos: usize) -> bool {
    // Walk backwards to find the nearest ident before this brace group.
    // Skip over type parameters, where clauses, etc.
    let mut j = pos;
    while j > 0 {
        j -= 1;
        match &trees[j] {
            TokenTree::Ident(id) => {
                let s = id.to_string();
                if s == "impl" || s == "mod" {
                    return true;
                }
                // If we hit fn, struct, enum, etc. — not an impl body
                if is_item_keyword(&s) {
                    return false;
                }
                // Other idents (type names, where clause) — keep looking
            }
            TokenTree::Group(_) => {
                // Skip over generic params, where clauses
            }
            TokenTree::Punct(_) => {
                // Skip punctuation
            }
            _ => {}
        }
    }
    false
}

fn is_item_keyword(s: &str) -> bool {
    matches!(s, "pub" | "fn" | "struct" | "enum" | "impl" | "use" | "const"
        | "type" | "trait" | "mod" | "static" | "unsafe" | "extern")
}

fn is_item_start(trees: &[TokenTree], pos: usize) -> bool {
    if let Some(TokenTree::Ident(id)) = trees.get(pos) {
        is_item_keyword(&id.to_string())
    } else if let Some(TokenTree::Punct(p)) = trees.get(pos) {
        // Could be #[attr] starting an item
        p.as_char() == '#'
    } else {
        false
    }
}

/// Find next non-comment token index.
fn next_meaningful(trees: &[TokenTree], start: usize) -> Option<usize> {
    // proc_macro2 doesn't have whitespace/comment tokens — they're all meaningful
    if start < trees.len() { Some(start) } else { None }
}

// ─── Clause stripping ───────────────────────────────────────────────────

const CLAUSE_KEYWORDS: &[&str] = &["requires", "ensures", "recommends", "invariant", "decreases"];

fn is_verus_clause_at(trees: &[TokenTree], pos: usize) -> bool {
    if let Some(TokenTree::Ident(id)) = trees.get(pos) {
        CLAUSE_KEYWORDS.contains(&id.to_string().as_str())
    } else {
        false
    }
}

/// Skip a loop invariant/decreases clause. Returns the index of the
/// loop body brace group. Takes the FIRST Brace group after the keyword.
fn skip_clause_simple(trees: &[TokenTree], pos: usize) -> usize {
    let mut j = pos + 1;
    while j < trees.len() {
        if let TokenTree::Group(g) = &trees[j] {
            if g.delimiter() == Delimiter::Brace {
                return j;
            }
        }
        j += 1;
    }
    j
}

/// Skip a requires/ensures/invariant/decreases clause. Returns the index
/// of the body brace group (function body or loop body) which should be emitted.
fn skip_clause(trees: &[TokenTree], pos: usize) -> usize {
    // The body `{...}` is distinguished from clause-internal braces by
    // what PRECEDES it:
    //   - preceded by `,` → body (clause expression list ended with comma)
    //   - preceded by Group::Brace → body (clause had match/if block, then body)
    //   - preceded by `else` or ident → clause-internal (if/else block)
    let mut j = pos + 1; // skip the keyword
    while j < trees.len() {
        if let TokenTree::Group(g) = &trees[j] {
            if g.delimiter() == Delimiter::Brace {
                if j == 0 {
                    return j; // edge case
                }
                let is_body = match &trees[j - 1] {
                    // Preceded by `,` → clause ended, this is the body
                    TokenTree::Punct(p) => p.as_char() == ',',
                    // Preceded by another Group::Brace → match/if block ended,
                    // this is the body (e.g., `match x { ... } { body }`)
                    TokenTree::Group(prev_g) => prev_g.delimiter() == Delimiter::Brace,
                    _ => false,
                };
                if is_body {
                    return j;
                }
                // Otherwise clause-internal (if/else/match block). Continue.
            }
        }
        j += 1;
    }
    j
}

// ─── Assert stripping ───────────────────────────────────────────────────

/// Check for Verus assert(...) — no `!` before the paren group.
fn is_verus_assert_at(trees: &[TokenTree], pos: usize) -> bool {
    if let Some(TokenTree::Ident(id)) = trees.get(pos) {
        if id.to_string() == "assert" {
            // Next token should be a Paren group (not `!` then paren)
            if let Some(TokenTree::Group(g)) = trees.get(pos + 1) {
                return g.delimiter() == Delimiter::Parenthesis;
            }
        }
    }
    false
}

fn skip_verus_assert(trees: &[TokenTree], pos: usize) -> usize {
    // Skip assert + (...)
    let mut j = pos + 2; // skip ident + paren group
    // Skip trailing semicolon if present
    if let Some(TokenTree::Punct(p)) = trees.get(j) {
        if p.as_char() == ';' {
            j += 1;
        }
    }
    j
}

// ─── Attribute stripping ────────────────────────────────────────────────

/// Check if tokens at pos are `#` `[doc = "..."]` (a doc comment attribute).
fn is_doc_attr_at(trees: &[TokenTree], pos: usize) -> bool {
    if let Some(TokenTree::Punct(p)) = trees.get(pos) {
        if p.as_char() == '#' {
            if let Some(TokenTree::Group(g)) = trees.get(pos + 1) {
                if g.delimiter() == Delimiter::Bracket {
                    let text = g.stream().to_string();
                    return text.starts_with("doc");
                }
            }
        }
    }
    false
}

/// Skip consecutive doc comment attributes. Returns index of first non-doc token.
fn skip_doc_attrs(trees: &[TokenTree], pos: usize) -> usize {
    let mut j = pos;
    while is_doc_attr_at(trees, j) {
        j += 2; // skip # + [doc = "..."]
    }
    j
}

fn is_verifier_attr_at(trees: &[TokenTree], pos: usize) -> bool {
    if let Some(TokenTree::Punct(p)) = trees.get(pos) {
        if p.as_char() == '#' {
            if let Some(TokenTree::Group(g)) = trees.get(pos + 1) {
                if g.delimiter() == Delimiter::Bracket {
                    let text = g.stream().to_string();
                    return text.starts_with("verifier") || text.starts_with("trigger");
                }
            }
        }
    }
    false
}

// ─── Named return type ──────────────────────────────────────────────────

fn is_arrow_at(trees: &[TokenTree], pos: usize) -> bool {
    if let Some(TokenTree::Punct(p)) = trees.get(pos) {
        if p.as_char() == '-' && p.spacing() == Spacing::Joint {
            if let Some(TokenTree::Punct(p2)) = trees.get(pos + 1) {
                return p2.as_char() == '>';
            }
        }
    }
    false
}

/// Try to strip `-> (name: Type)` to `-> Type`.
fn try_strip_named_return(trees: &[TokenTree], pos: usize) -> Option<(String, usize)> {
    // pos is `-`, pos+1 is `>`, pos+2 should be Paren group
    let group_pos = pos + 2;
    if let Some(TokenTree::Group(g)) = trees.get(group_pos) {
        if g.delimiter() == Delimiter::Parenthesis {
            let inner: Vec<TokenTree> = g.stream().into_iter().collect();
            // Check pattern: Ident `:` Type...
            if inner.len() >= 3 {
                if let (Some(TokenTree::Ident(_)), Some(TokenTree::Punct(colon))) =
                    (inner.get(0), inner.get(1))
                {
                    if colon.as_char() == ':' {
                        // Collect everything after the `:` as the type
                        let type_tokens: TokenStream = inner[2..].iter().cloned().collect();
                        let type_text = type_tokens.to_string();
                        return Some((format!("-> {}", type_text), group_pos + 1));
                    }
                }
            }
        }
    }
    None
}

// ─── Token emission ─────────────────────────────────────────────────────

fn emit_token(tree: &TokenTree, out: &mut String) {
    match tree {
        TokenTree::Group(g) => {
            let (open, close) = match g.delimiter() {
                Delimiter::Brace => ("{", "}"),
                Delimiter::Parenthesis => ("(", ")"),
                Delimiter::Bracket => ("[", "]"),
                Delimiter::None => ("", ""),
            };
            out.push_str(open);
            // Emit the group's content preserving original formatting.
            // proc_macro2 loses whitespace, so we use the span's source text.
            // Fallback: re-emit tokens (loses formatting).
            let inner_text = g.stream().to_string();
            out.push_str(&inner_text);
            out.push_str(close);
        }
        TokenTree::Ident(id) => {
            // Add space before ident if output doesn't end with whitespace/open-delim
            if needs_space_before(out) {
                out.push(' ');
            }
            out.push_str(&id.to_string());
        }
        TokenTree::Punct(p) => {
            out.push(p.as_char());
        }
        TokenTree::Literal(lit) => {
            if needs_space_before(out) {
                out.push(' ');
            }
            out.push_str(&lit.to_string());
        }
    }
}

fn needs_space_before(out: &str) -> bool {
    match out.chars().last() {
        None => false,
        Some(c) => c.is_alphanumeric() || c == '_' || c == ')' || c == '}'
    }
}

// ─── Output cleanup helpers ─────────────────────────────────────────────

/// Strip leading `//!` module doc comment lines from body text.
fn strip_leading_doc_comments(text: &str) -> String {
    let mut lines = text.lines().peekable();
    // Skip leading blank lines and //! lines
    while let Some(line) = lines.peek() {
        let t = line.trim();
        if t.is_empty() || t.starts_with("//!") {
            lines.next();
        } else {
            break;
        }
    }
    lines.collect::<Vec<_>>().join("\n")
}

fn strip_trailing_doc_comments(out: &mut String) {
    loop {
        while out.ends_with(' ') || out.ends_with('\t') {
            out.pop();
        }
        if let Some(last_nl) = out.rfind('\n') {
            let last_line = out[last_nl + 1..].trim();
            if last_line.starts_with("///")
                || last_line.starts_with("// =")
                || last_line.starts_with("#[doc")
                || last_line.starts_with("#[doc =")
            {
                out.truncate(last_nl);
                continue;
            }
        }
        break;
    }
}

fn trim_trailing_blank_lines(out: &mut String) {
    while out.ends_with("\n\n") {
        out.pop();
    }
}

fn trim_trailing_whitespace(out: &mut String) {
    while out.ends_with(' ') || out.ends_with('\t') {
        out.pop();
    }
}

// ─── Standalone mode ─────────────────────────────────────────────────────

/// Stub definitions prepended in standalone mode.
///
/// These provide minimal type/constant definitions so that the stripped file
/// is self-contained and can be fed to rocq_of_rust without resolving
/// cross-module references.
const STANDALONE_STUBS: &str = r#"// === Standalone stubs (generated by verus-strip --standalone) ===

// Error codes (from crate::error)
pub const OK: i32 = 0;
pub const EINVAL: i32 = -22;
pub const EAGAIN: i32 = -11;
pub const EBUSY: i32 = -16;
pub const EPERM: i32 = -1;
pub const ENOMEM: i32 = -12;
pub const ENOMSG: i32 = -42;
pub const EPIPE: i32 = -32;
pub const ECANCELED: i32 = -125;
pub const EBADF: i32 = -9;
pub const EOVERFLOW: i32 = -75;
pub const ETIMEDOUT: i32 = -110;
pub const ENOSPC: i32 = -28;
pub const ENOENT: i32 = -2;
pub const EADDRINUSE: i32 = -98;

// Thread types (from crate::thread)
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct ThreadId {
    pub id: u32,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum ThreadState {
    Ready,
    Running,
    Blocked,
    Suspended,
}

#[derive(Debug, Copy, Clone)]
pub struct Thread {
    pub id: ThreadId,
    pub priority: Priority,
    pub state: ThreadState,
    pub return_value: i32,
}

// Priority (from crate::priority)
pub const MAX_PRIORITY: u32 = 32;

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Priority {
    pub value: u32,
}

impl Priority {
    pub fn new(value: u32) -> Option<Self> {
        if value < MAX_PRIORITY {
            Some(Priority { value })
        } else {
            None
        }
    }
    pub fn get(&self) -> u32 {
        self.value
    }
}

// WaitQueue (from crate::wait_queue)
pub const MAX_WAITERS: u32 = 64;

#[derive(Debug)]
pub struct WaitQueue {
    pub entries: [Option<Thread>; 64],
    pub len: u32,
}

impl WaitQueue {
    pub fn new() -> Self {
        WaitQueue {
            entries: [
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
                None, None, None, None, None, None, None, None,
            ],
            len: 0,
        }
    }
    pub fn len(&self) -> u32 {
        self.len
    }
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }
}

// === End stubs ===
"#;

/// Post-process stripped output for standalone mode:
/// 1. Remove `use crate::*` imports
/// 2. Remove `#[cfg(not(verus_keep_ghost))]` lines
/// 3. Prepend stub definitions
pub fn make_standalone(stripped: &str) -> String {
    let filtered: Vec<&str> = stripped
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("use crate::") && trimmed != "#[cfg(not(verus_keep_ghost))]"
        })
        .collect();
    let body = filtered.join("\n");

    let mut result = String::new();
    result.push_str(STANDALONE_STUBS);
    result.push('\n');
    result.push_str(&body);
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_strip_vstd_imports() {
        let input = "use vstd::prelude::*;\nuse crate::error::*;\n";
        let result = strip_vstd_imports(input);
        assert!(!result.contains("vstd"));
        assert!(result.contains("use crate::error::*;"));
    }

    #[test]
    fn test_strip_spec_fn() {
        let body = r#"
pub open spec fn inv(&self) -> bool {
    &&& self.limit > 0
    &&& self.count <= self.limit
}

pub fn count_get(&self) -> u32 {
    self.count
}
"#;
        let result = strip_body(body);
        assert!(!result.contains("spec fn"), "output: {result}");
        assert!(result.contains("count_get"), "output: {result}");
    }

    #[test]
    fn test_strip_proof_fn() {
        let body = r#"
pub proof fn lemma_invariant_inductive()
    ensures
        true,
{
}

pub fn real_fn() -> u32 {
    42
}
"#;
        let result = strip_body(body);
        assert!(!result.contains("proof fn"), "output: {result}");
        assert!(result.contains("real_fn"), "output: {result}");
    }

    #[test]
    fn test_strip_requires_ensures() {
        let body = r#"
pub fn init(count: u32, limit: u32) -> (result: Result<Self, i32>)
    ensures
        match result {
            Ok(sem) => {
                &&& sem.count == count
            },
            Err(e) => {
                &&& e == EINVAL
            },
        },
{
    if limit == 0 {
        return Err(EINVAL);
    }
    Ok(Self { count, limit })
}
"#;
        let result = strip_body(body);
        assert!(!result.contains("ensures"), "output: {result}");
        assert!(!result.contains("&&&"), "output: {result}");
        assert!(result.contains("limit") && result.contains("EINVAL"), "output: {result}");
    }

    #[test]
    fn test_strip_named_return_type() {
        let body = "pub fn foo() -> (result: i32) { 42 }";
        let result = strip_body(body);
        assert!(result.contains("-> i32"), "output: {result}");
        assert!(!result.contains("result :"), "output: {result}");
    }

    #[test]
    fn test_strip_named_return_complex_type() {
        let body = "pub fn foo() -> (result: Result<Self, i32>) { Ok(Self {}) }";
        let result = strip_body(body);
        assert!(result.contains("-> Result"), "output: {result}");
    }

    #[test]
    fn test_preserve_runtime_code() {
        let body = r#"
pub fn give(&mut self) -> GiveResult {
    let thread = self.wait_q.unpend_first(OK);
    match thread {
        Some(t) => GiveResult::WokeThread(t),
        None => {
            if self.count != self.limit {
                self.count = self.count + 1;
                GiveResult::Incremented
            } else {
                GiveResult::Saturated
            }
        }
    }
}
"#;
        let result = strip_body(body);
        assert!(result.contains("give"), "output: {result}");
        assert!(result.contains("WokeThread"), "output: {result}");
    }

    #[test]
    fn test_full_file_strip() {
        let input = r#"//! Module doc.

use vstd::prelude::*;
use crate::error::*;

verus! {

pub struct Foo {
    pub count: u32,
}

impl Foo {
    pub open spec fn inv(&self) -> bool {
        self.count <= 100
    }

    pub fn new() -> (result: Self)
        ensures
            result.inv(),
    {
        Foo { count: 0 }
    }

    pub fn inc(&mut self)
        requires
            old(self).inv(),
            old(self).count < 100,
        ensures
            self.inv(),
            self.count == old(self).count + 1,
    {
        self.count = self.count + 1;
    }
}

pub proof fn lemma_foo()
    ensures true,
{
}

} // verus!
"#;
        let result = strip_file(input);
        eprintln!("=== OUTPUT ===\n{}\n=== END ===", result.output);
        if !result.errors.is_empty() {
            eprintln!("ERRORS: {:?}", result.errors);
        }
        assert!(result.errors.is_empty(), "parse errors: {:?}", result.errors);
        assert!(!result.output.contains("vstd"), "contains vstd");
        assert!(!result.output.contains("verus!"), "contains verus!");
        assert!(!result.output.contains("spec fn"), "contains spec fn");
        assert!(!result.output.contains("proof fn"), "contains proof fn");
        assert!(!result.output.contains("requires"), "contains requires");
        assert!(!result.output.contains("ensures"), "contains ensures");

        assert!(result.output.contains("struct Foo"), "missing struct");
        assert!(result.output.contains("fn new"), "missing new fn");
        assert!(result.output.contains("fn inc"), "missing inc fn");
        assert!(result.output.contains("//! Module doc."), "missing doc");
    }

    #[test]
    fn test_proof_fn_with_block_ensures() {
        let body = r#"
pub proof fn lemma_give_take_roundtrip(count: u32, limit: u32)
    requires
        limit > 0,
        count < limit,
    ensures
        ({
            let after_give = (count + 1) as u32;
            let after_take = (after_give - 1) as u32;
            after_take == count
        }),
{
}

pub fn real() -> u32 { 1 }
"#;
        let result = strip_body(body);
        assert!(!result.contains("proof fn"), "contains proof fn: {result}");
        assert!(!result.contains("lemma_give_take_roundtrip"), "contains lemma: {result}");
        assert!(result.contains("real"), "missing real fn: {result}");
    }

    #[test]
    fn test_make_standalone() {
        let input = r#"//! Module doc.

use vstd::prelude::*;
use crate::error::*;
use crate::thread::{Thread, ThreadState};
use crate::wait_queue::WaitQueue;

verus! {

pub struct Foo {
    pub count: u32,
}

impl Foo {
    pub fn new() -> (result: Self) {
        Foo { count: 0 }
    }
}

} // verus!
"#;
        let result = strip_file(input);
        assert!(result.errors.is_empty(), "parse errors: {:?}", result.errors);
        let standalone = make_standalone(&result.output);

        // Should NOT contain any `use crate::` lines
        assert!(!standalone.contains("use crate::"), "contains use crate:: import: {standalone}");

        // Should contain stub definitions
        assert!(standalone.contains("pub const OK: i32 = 0;"), "missing OK constant");
        assert!(standalone.contains("pub const EINVAL: i32 = -22;"), "missing EINVAL constant");
        assert!(standalone.contains("pub struct Thread"), "missing Thread struct");
        assert!(standalone.contains("pub struct WaitQueue"), "missing WaitQueue struct");
        assert!(standalone.contains("pub struct Priority"), "missing Priority struct");
        assert!(standalone.contains("pub enum ThreadState"), "missing ThreadState enum");

        // Should still contain the actual module content
        assert!(standalone.contains("struct Foo"), "missing Foo struct");
        assert!(standalone.contains("fn new"), "missing new fn");
    }

    #[test]
    fn test_make_standalone_removes_cfg() {
        let input = "#[cfg(not(verus_keep_ghost))]\nuse something;\npub fn foo() -> i32 { 1 }\n";
        let result = make_standalone(input);
        assert!(!result.contains("#[cfg(not(verus_keep_ghost))]"), "cfg not removed");
        assert!(result.contains("pub fn foo"), "missing foo fn");
    }
}

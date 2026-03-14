//! Verus annotation stripper — converts Verus-annotated Rust to plain Rust.
//!
//! Uses ra_ap_syntax (rust-analyzer's rowan-based parser) for:
//! 1. Locating the `verus!` macro call in the syntax tree
//! 2. Validating the stripped output parses as valid Rust
//!
//! The stripping logic is a brace-depth-aware scanner that removes:
//! - `verus! { ... }` wrapper
//! - `use vstd::*` imports
//! - `pub open spec fn` / `pub closed spec fn` / `pub proof fn` (entire functions)
//! - `requires` / `ensures` / `invariant` / `decreases` clauses
//! - Named return type bindings: `-> (name: Type)` → `-> Type`
//! - Verus `assert(...)` proof assertions (distinguished from `assert!(...)`)
//! - `#[verifier::*]` attributes

use ra_ap_syntax::{Edition, SourceFile, ast, AstNode};
use ra_ap_syntax::ast::HasModuleItem;

/// Result of stripping a file.
pub struct StripResult {
    pub output: String,
    pub errors: Vec<String>,
}

/// Strip Verus annotations from a Rust source file.
pub fn strip_file(input: &str) -> StripResult {
    // Phase 1: Split at verus! macro boundary
    let (before, body, after) = split_at_verus_macro(input);

    // Phase 2: Clean preamble (remove vstd imports)
    let preamble = strip_vstd_imports(&before);

    // Phase 3: Strip Verus annotations from the macro body
    let stripped = strip_annotations(&body);

    // Phase 4: Reassemble
    let mut output = String::new();
    let trimmed_pre = preamble.trim_end();
    if !trimmed_pre.is_empty() {
        output.push_str(trimmed_pre);
        output.push('\n');
    }
    let trimmed_body = stripped.trim();
    if !trimmed_body.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(trimmed_body);
        output.push('\n');
    }
    let trimmed_after = after.trim();
    if !trimmed_after.is_empty() {
        output.push('\n');
        output.push_str(trimmed_after);
        output.push('\n');
    }

    // Phase 5: Validate
    let parse = SourceFile::parse(&output, Edition::Edition2024);
    let errors: Vec<String> = parse.errors().iter().map(|e| format!("{e}")).collect();

    StripResult { output, errors }
}

/// Use ra_ap_syntax to find the `verus!` macro call and split the file.
///
/// Returns (text_before_macro, macro_body_content, text_after_macro).
/// The body content is the text INSIDE the `{ ... }` of `verus! { ... }`.
fn split_at_verus_macro(input: &str) -> (String, String, String) {
    let parse = SourceFile::parse(input, Edition::Edition2024);
    let root = parse.tree();

    for item in root.items() {
        if let ast::Item::MacroCall(mc) = item {
            let path_text = mc
                .path()
                .map(|p| p.syntax().text().to_string())
                .unwrap_or_default();
            if path_text.trim() == "verus" {
                if let Some(tt) = mc.token_tree() {
                    let tt_text = tt.syntax().text().to_string();
                    // tt_text includes the outer { }
                    let body = if tt_text.starts_with('{') && tt_text.ends_with('}') {
                        &tt_text[1..tt_text.len() - 1]
                    } else {
                        &tt_text[..]
                    };

                    // Get text ranges
                    let mc_range = mc.syntax().text_range();
                    let before = &input[..usize::from(mc_range.start())];
                    let after = &input[usize::from(mc_range.end())..];

                    return (before.to_string(), body.to_string(), after.to_string());
                }
            }
        }
    }

    // No verus! macro found — return entire input as "before"
    (input.to_string(), String::new(), String::new())
}

/// Remove `use vstd::...` import lines.
fn strip_vstd_imports(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.starts_with("use vstd")
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ─── Core annotation stripping ──────────────────────────────────────────

/// Strip Verus annotations from the body of a `verus! { }` block.
///
/// This is a brace-depth-aware scanner that processes character by character,
/// identifying and removing Verus-specific constructs while preserving all
/// runtime Rust code.
fn strip_annotations(body: &str) -> String {
    let tokens = tokenize(body);
    let mut out = String::new();
    let mut i = 0;
    let mut brace_depth: i32 = 0;

    while i < tokens.len() {
        match &tokens[i] {
            Token::LBrace => {
                brace_depth += 1;
                out.push('{');
                i += 1;
            }
            Token::RBrace => {
                brace_depth -= 1;
                out.push('}');
                i += 1;
            }
            Token::Ident(w) => {
                // Check for spec/proof function: skip entire function
                if let Some(skip_end) = try_match_spec_or_proof_fn(&tokens, i) {
                    // Skip all tokens from i to skip_end (inclusive)
                    // Also skip any preceding blank line / comment that's a section separator
                    i = skip_end + 1;
                    // Remove trailing blank lines we may have emitted
                    trim_trailing_blank_lines(&mut out);
                    continue;
                }

                // Check for requires/ensures/invariant/decreases clause
                if is_verus_clause_keyword(w)
                    && is_clause_context(&tokens, i)
                {
                    let base_depth = brace_depth;
                    // Skip the clause, up to but not including the `{` at base_depth
                    i = skip_clause(&tokens, i, base_depth);
                    // Remove any trailing whitespace we accumulated
                    trim_trailing_whitespace(&mut out);
                    continue;
                }

                // Check for Verus assert (not assert!)
                if w == "assert" && is_verus_assert(&tokens, i) {
                    i = skip_verus_assert(&tokens, i);
                    trim_trailing_blank_lines(&mut out);
                    continue;
                }

                // Check for named return type: -> (name: Type)
                // This is handled in a fixup pass — just emit normally here
                out.push_str(w);
                i += 1;
            }
            Token::Arrow => {
                // Check for named return type: -> (name: Type)
                if let Some((replacement, skip_to)) =
                    try_strip_named_return(&tokens, i)
                {
                    out.push_str(&replacement);
                    i = skip_to;
                    continue;
                }
                out.push_str("->");
                i += 1;
            }
            Token::Whitespace(ws) => {
                out.push_str(ws);
                i += 1;
            }
            Token::Comment(c) => {
                out.push_str(c);
                i += 1;
            }
            Token::StringLit(s) => {
                out.push_str(s);
                i += 1;
            }
            Token::Punct(c) => {
                out.push(*c);
                i += 1;
            }
            Token::LParen => {
                out.push('(');
                i += 1;
            }
            Token::RParen => {
                out.push(')');
                i += 1;
            }
            Token::LBracket => {
                out.push('[');
                i += 1;
            }
            Token::RBracket => {
                out.push(']');
                i += 1;
            }
            Token::Attr(a) => {
                // Skip #[verifier::*] and #[trigger] attributes
                if a.contains("verifier::") || a.contains("trigger") {
                    i += 1;
                    // Skip following whitespace
                    while i < tokens.len() {
                        if let Token::Whitespace(_) = &tokens[i] {
                            i += 1;
                        } else {
                            break;
                        }
                    }
                    continue;
                }
                out.push_str(a);
                i += 1;
            }
        }
    }

    out
}

// ─── Tokenizer ──────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
enum Token {
    Ident(String),
    Punct(char),
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Arrow,          // ->
    Whitespace(String),
    Comment(String),
    StringLit(String),
    Attr(String),   // #[...]
    // Catch-all for numbers, operators, etc.
}

fn tokenize(input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let chars: Vec<char> = input.chars().collect();
    let mut i = 0;

    while i < chars.len() {
        let c = chars[i];

        // Whitespace
        if c.is_whitespace() {
            let start = i;
            while i < chars.len() && chars[i].is_whitespace() {
                i += 1;
            }
            tokens.push(Token::Whitespace(chars[start..i].iter().collect()));
            continue;
        }

        // Line comment
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '/' {
            let start = i;
            while i < chars.len() && chars[i] != '\n' {
                i += 1;
            }
            tokens.push(Token::Comment(chars[start..i].iter().collect()));
            continue;
        }

        // Block comment
        if c == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
            let start = i;
            i += 2;
            let mut depth = 1;
            while i < chars.len() && depth > 0 {
                if chars[i] == '/' && i + 1 < chars.len() && chars[i + 1] == '*' {
                    depth += 1;
                    i += 2;
                } else if chars[i] == '*' && i + 1 < chars.len() && chars[i + 1] == '/' {
                    depth -= 1;
                    i += 2;
                } else {
                    i += 1;
                }
            }
            tokens.push(Token::Comment(chars[start..i].iter().collect()));
            continue;
        }

        // String literal
        if c == '"' {
            let start = i;
            i += 1;
            while i < chars.len() && chars[i] != '"' {
                if chars[i] == '\\' {
                    i += 1; // skip escaped char
                }
                i += 1;
            }
            if i < chars.len() {
                i += 1; // consume closing "
            }
            tokens.push(Token::StringLit(chars[start..i].iter().collect()));
            continue;
        }

        // Raw string literal r#"..."#
        if c == 'r' && i + 1 < chars.len() && (chars[i + 1] == '"' || chars[i + 1] == '#') {
            let start = i;
            i += 1;
            let mut hashes = 0;
            while i < chars.len() && chars[i] == '#' {
                hashes += 1;
                i += 1;
            }
            if i < chars.len() && chars[i] == '"' {
                i += 1;
                // Read until "###
                'outer: while i < chars.len() {
                    if chars[i] == '"' {
                        let mut end_hashes = 0;
                        let save = i;
                        i += 1;
                        while i < chars.len() && chars[i] == '#' && end_hashes < hashes {
                            end_hashes += 1;
                            i += 1;
                        }
                        if end_hashes == hashes {
                            break 'outer;
                        }
                        i = save + 1;
                    } else {
                        i += 1;
                    }
                }
                tokens.push(Token::StringLit(chars[start..i].iter().collect()));
                continue;
            }
            // Not a raw string, fall through to ident
            i = start;
        }

        // Char literal
        if c == '\'' && i + 1 < chars.len() && !chars[i + 1].is_alphabetic() {
            let start = i;
            i += 1;
            if i < chars.len() && chars[i] == '\\' {
                i += 1;
            }
            if i < chars.len() {
                i += 1;
            }
            if i < chars.len() && chars[i] == '\'' {
                i += 1;
            }
            tokens.push(Token::StringLit(chars[start..i].iter().collect()));
            continue;
        }

        // Attribute: #[...]
        if c == '#' && i + 1 < chars.len() && chars[i + 1] == '[' {
            let start = i;
            i += 2;
            let mut depth = 1;
            while i < chars.len() && depth > 0 {
                if chars[i] == '[' {
                    depth += 1;
                } else if chars[i] == ']' {
                    depth -= 1;
                }
                i += 1;
            }
            tokens.push(Token::Attr(chars[start..i].iter().collect()));
            continue;
        }

        // Arrow: ->
        if c == '-' && i + 1 < chars.len() && chars[i + 1] == '>' {
            tokens.push(Token::Arrow);
            i += 2;
            continue;
        }

        // Braces / parens / brackets
        match c {
            '{' => { tokens.push(Token::LBrace); i += 1; continue; }
            '}' => { tokens.push(Token::RBrace); i += 1; continue; }
            '(' => { tokens.push(Token::LParen); i += 1; continue; }
            ')' => { tokens.push(Token::RParen); i += 1; continue; }
            '[' => { tokens.push(Token::LBracket); i += 1; continue; }
            ']' => { tokens.push(Token::RBracket); i += 1; continue; }
            _ => {}
        }

        // Identifier or keyword
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                i += 1;
            }
            tokens.push(Token::Ident(chars[start..i].iter().collect()));
            continue;
        }

        // Everything else (operators, numbers, etc.)
        tokens.push(Token::Punct(c));
        i += 1;
    }

    tokens
}

// ─── Pattern matching helpers ───────────────────────────────────────────

/// Check if tokens starting at `pos` match a spec or proof function definition.
/// Returns the token index of the closing `}` if matched.
fn try_match_spec_or_proof_fn(tokens: &[Token], pos: usize) -> Option<usize> {
    // Patterns:
    //   pub open spec fn ...
    //   pub closed spec fn ...
    //   pub proof fn ...
    //   proof fn ...
    let seq = collect_ident_sequence(tokens, pos);
    let seq_str: Vec<&str> = seq.iter().map(|s| s.as_str()).collect();

    let is_spec_fn = matches!(
        seq_str.as_slice(),
        ["pub", "open", "spec", "fn", ..]
            | ["pub", "closed", "spec", "fn", ..]
    );
    let is_proof_fn = matches!(
        seq_str.as_slice(),
        ["pub", "proof", "fn", ..] | ["proof", "fn", ..]
    );

    if !is_spec_fn && !is_proof_fn {
        return None;
    }

    // Find the opening { and then the matching closing }
    let mut i = pos;
    let mut brace_depth = 0i32;
    let mut found_body = false;

    while i < tokens.len() {
        match &tokens[i] {
            Token::LBrace => {
                brace_depth += 1;
                found_body = true;
            }
            Token::RBrace => {
                brace_depth -= 1;
                if found_body && brace_depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
        i += 1;
    }

    None
}

/// Collect consecutive identifier tokens (skipping whitespace) starting at pos.
fn collect_ident_sequence(tokens: &[Token], pos: usize) -> Vec<String> {
    let mut seq = Vec::new();
    let mut i = pos;
    while i < tokens.len() {
        match &tokens[i] {
            Token::Ident(w) => {
                seq.push(w.clone());
                i += 1;
            }
            Token::Whitespace(ws) if !ws.contains('\n') => {
                i += 1;
            }
            _ => break,
        }
        if seq.len() >= 5 {
            break;
        }
    }
    seq
}

fn is_verus_clause_keyword(word: &str) -> bool {
    matches!(word, "requires" | "ensures" | "recommends" | "invariant" | "decreases")
}

/// Check if a clause keyword is in a valid context (not inside an expression).
/// Heuristic: the keyword appears after a fn signature or loop header,
/// not as a variable name in an expression.
fn is_clause_context(tokens: &[Token], pos: usize) -> bool {
    // Look backwards for context: should be preceded by newline+indent or comma+newline
    // (i.e., at the start of a line, possibly indented)
    if pos == 0 {
        return true;
    }

    // Check preceding token: should be whitespace containing a newline,
    // or we're right after `)` + whitespace (fn signature end)
    let prev = pos - 1;
    match &tokens[prev] {
        Token::Whitespace(ws) => ws.contains('\n'),
        _ => false,
    }
}

/// Check if `assert` at position is a Verus proof assert (not assert!).
fn is_verus_assert(tokens: &[Token], pos: usize) -> bool {
    // Verus: assert(...)  — no ! after assert
    // Rust:  assert!(...)  — has ! after assert
    let next = next_non_ws(tokens, pos + 1);
    if let Some(idx) = next {
        matches!(&tokens[idx], Token::LParen)
    } else {
        false
    }
}

/// Skip a Verus assert(...) statement including the trailing semicolon.
fn skip_verus_assert(tokens: &[Token], pos: usize) -> usize {
    let mut i = pos;
    // Find the opening (
    while i < tokens.len() {
        if matches!(&tokens[i], Token::LParen) {
            break;
        }
        i += 1;
    }

    // Match parentheses
    let mut paren_depth = 0;
    while i < tokens.len() {
        match &tokens[i] {
            Token::LParen => paren_depth += 1,
            Token::RParen => {
                paren_depth -= 1;
                if paren_depth == 0 {
                    i += 1;
                    // Skip trailing semicolon
                    if let Some(idx) = next_non_ws(tokens, i) {
                        if matches!(&tokens[idx], Token::Punct(';')) {
                            i = idx + 1;
                        }
                    }
                    return i;
                }
            }
            _ => {}
        }
        i += 1;
    }
    i
}

/// Skip a requires/ensures/invariant/decreases clause.
/// Returns the index of the `{` token that starts the function/loop body.
fn skip_clause(tokens: &[Token], pos: usize, base_brace_depth: i32) -> usize {
    let mut i = pos;
    let mut brace_depth = base_brace_depth;

    while i < tokens.len() {
        match &tokens[i] {
            Token::LBrace => {
                if brace_depth == base_brace_depth {
                    // This { starts the function/loop body — stop skipping
                    return i;
                }
                brace_depth += 1;
                i += 1;
            }
            Token::RBrace => {
                brace_depth -= 1;
                i += 1;
            }
            _ => {
                i += 1;
            }
        }
    }
    i
}

/// Try to strip a named return type: `-> (name: Type)` → `-> Type`
fn try_strip_named_return(tokens: &[Token], arrow_pos: usize) -> Option<(String, usize)> {
    // After ->, expect whitespace then (
    let paren_idx = next_non_ws(tokens, arrow_pos + 1)?;
    if !matches!(&tokens[paren_idx], Token::LParen) {
        return None;
    }

    // After (, expect ident (the binding name)
    let name_idx = next_non_ws(tokens, paren_idx + 1)?;
    if !matches!(&tokens[name_idx], Token::Ident(_)) {
        return None;
    }

    // After ident, expect :
    let colon_idx = next_non_ws(tokens, name_idx + 1)?;
    if !matches!(&tokens[colon_idx], Token::Punct(':')) {
        return None;
    }

    // Now collect the type until the matching )
    // We need to handle nested parens/angle brackets
    let type_start = colon_idx + 1;
    let mut paren_depth = 1i32;
    let mut i = type_start;
    while i < tokens.len() && paren_depth > 0 {
        match &tokens[i] {
            Token::LParen => paren_depth += 1,
            Token::RParen => {
                paren_depth -= 1;
                if paren_depth == 0 {
                    // Collect the type text (between : and ))
                    let type_text: String = tokens[type_start..i]
                        .iter()
                        .map(token_text)
                        .collect();
                    let result = format!("-> {}", type_text.trim());
                    return Some((result, i + 1));
                }
            }
            _ => {}
        }
        i += 1;
    }
    None
}

/// Get the text representation of a token.
fn token_text(tok: &Token) -> String {
    match tok {
        Token::Ident(s)
        | Token::Whitespace(s)
        | Token::Comment(s)
        | Token::StringLit(s)
        | Token::Attr(s) => s.clone(),
        Token::Punct(c) => c.to_string(),
        Token::LBrace => "{".to_string(),
        Token::RBrace => "}".to_string(),
        Token::LParen => "(".to_string(),
        Token::RParen => ")".to_string(),
        Token::LBracket => "[".to_string(),
        Token::RBracket => "]".to_string(),
        Token::Arrow => "->".to_string(),
    }
}

/// Find the next non-whitespace token index.
fn next_non_ws(tokens: &[Token], start: usize) -> Option<usize> {
    let mut i = start;
    while i < tokens.len() {
        if !matches!(&tokens[i], Token::Whitespace(_)) {
            return Some(i);
        }
        i += 1;
    }
    None
}

/// Trim trailing blank lines from output.
fn trim_trailing_blank_lines(out: &mut String) {
    while out.ends_with("\n\n") {
        out.pop();
    }
}

/// Trim trailing whitespace (spaces/tabs, not newlines) from the last line.
fn trim_trailing_whitespace(out: &mut String) {
    while out.ends_with(' ') || out.ends_with('\t') {
        out.pop();
    }
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
    fn test_tokenize_basic() {
        let tokens = tokenize("pub fn foo() -> i32 { 42 }");
        let idents: Vec<_> = tokens
            .iter()
            .filter_map(|t| match t {
                Token::Ident(s) => Some(s.as_str()),
                _ => None,
            })
            .collect();
        assert_eq!(idents, vec!["pub", "fn", "foo", "i32", "42"]);
    }

    #[test]
    fn test_strip_spec_fn() {
        let input = r#"
pub open spec fn inv(&self) -> bool {
    &&& self.limit > 0
    &&& self.count <= self.limit
}

pub fn count_get(&self) -> u32 {
    self.count
}
"#;
        let result = strip_annotations(input);
        assert!(!result.contains("spec fn"));
        assert!(result.contains("pub fn count_get"));
        assert!(result.contains("self.count"));
    }

    #[test]
    fn test_strip_proof_fn() {
        let input = r#"
pub proof fn lemma_invariant_inductive()
    ensures
        true,
{
}

pub fn real_fn() -> u32 {
    42
}
"#;
        let result = strip_annotations(input);
        assert!(!result.contains("proof fn"));
        assert!(result.contains("pub fn real_fn"));
    }

    #[test]
    fn test_strip_requires_ensures() {
        let input = r#"
pub fn init(count: u32, limit: u32) -> Result<Self, i32>
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
        let result = strip_annotations(input);
        assert!(!result.contains("ensures"));
        assert!(!result.contains("&&&"));
        assert!(result.contains("if limit == 0"));
        assert!(result.contains("Ok(Self { count, limit })"));
    }

    #[test]
    fn test_strip_named_return_type() {
        let input = "pub fn foo() -> (result: i32) { 42 }";
        let result = strip_annotations(input);
        assert!(result.contains("-> i32"));
        assert!(!result.contains("result:"));
    }

    #[test]
    fn test_strip_named_return_complex_type() {
        let input = "pub fn foo() -> (result: Result<Self, i32>) { Ok(Self {}) }";
        let result = strip_annotations(input);
        assert!(result.contains("-> Result<Self, i32>"));
        assert!(!result.contains("(result:"));
    }

    #[test]
    fn test_preserve_runtime_code() {
        let input = r#"
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
        let result = strip_annotations(input);
        // Everything should be preserved
        assert!(result.contains("pub fn give"));
        assert!(result.contains("GiveResult::WokeThread(t)"));
        assert!(result.contains("GiveResult::Incremented"));
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
        // Check no Verus constructs remain
        assert!(!result.output.contains("vstd"));
        assert!(!result.output.contains("verus!"));
        assert!(!result.output.contains("spec fn"));
        assert!(!result.output.contains("proof fn"));
        assert!(!result.output.contains("requires"));
        assert!(!result.output.contains("ensures"));
        assert!(!result.output.contains("&&&"));
        assert!(!result.output.contains("old("));

        // Check runtime code preserved
        assert!(result.output.contains("pub struct Foo"));
        assert!(result.output.contains("pub fn new() -> Self"));
        assert!(result.output.contains("Foo { count: 0 }"));
        assert!(result.output.contains("pub fn inc(&mut self)"));
        assert!(result.output.contains("self.count = self.count + 1"));
        assert!(result.output.contains("use crate::error::*;"));
        assert!(result.output.contains("//! Module doc."));

        // Validate it parses as Rust
        assert!(result.errors.is_empty(), "Parse errors: {:?}", result.errors);
    }
}

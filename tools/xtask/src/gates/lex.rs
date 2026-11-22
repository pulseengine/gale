//! Comment and literal blanking for the spelling counters (trust-ledger,
//! kani-coverage).
//!
//! Each function returns text of the same line structure with comments and
//! literal CONTENTS replaced by spaces, so a token count over the result sees
//! code only: a mention in a comment, a doc comment or a string is not a
//! spelling. Newlines survive, so a later line-anchored match still lines up.
//!
//! These are deliberately lexers, not parsers. What they cannot see is listed by
//! the gates that use them.

/// Rust: `//` and nested `/* */` comments, `"..."` (also `b"..."` and `c"..."`:
/// the prefix letter is just an identifier character before an ordinary quote),
/// the raw forms `r"..."` / `r#"..."#` with their `br` and `cr` prefixes (the
/// only raw prefixes rustc 1.100 accepts; any other `<ident>"` is a
/// reserved-prefix error since edition 2021), and char literals (`'"'` and
/// `b'"'` must not open a string) -- lifetimes are left alone.
pub(crate) fn rust_code(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let blank = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    let mut i = 0;
    while i < c.len() {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        if ch == '/' && next == Some('/') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '/' && next == Some('*') {
            let mut depth = 1;
            i += 2;
            out.push_str("  ");
            while i < c.len() && depth > 0 {
                if c[i] == '/' && c.get(i + 1) == Some(&'*') {
                    depth += 1;
                    out.push_str("  ");
                    i += 2;
                } else if c[i] == '*' && c.get(i + 1) == Some(&'/') {
                    depth -= 1;
                    out.push_str("  ");
                    i += 2;
                } else {
                    blank(&mut out, c[i]);
                    i += 1;
                }
            }
        } else if ch == 'r' && (next == Some('"') || next == Some('#')) && !prev_is_ident_except(&c, i, &['b', 'c']) && raw_start(&c, i + 1).is_some() {
            // r"..." / r#"..."#, and br"..." / cr"..." (round 11, F1: only `b`
            // was exempt, so `cr"\"` read as an escaped quote and opened a
            // phantom string that hid the code after it)
            let hashes = raw_start(&c, i + 1).unwrap_or(0);
            out.push('r');
            for _ in 0..hashes {
                out.push('#');
            }
            out.push('"');
            i += 1 + hashes + 1;
            loop {
                if i >= c.len() {
                    break;
                }
                if c[i] == '"' && (0..hashes).all(|k| c.get(i + 1 + k) == Some(&'#')) {
                    out.push('"');
                    for _ in 0..hashes {
                        out.push('#');
                    }
                    i += 1 + hashes;
                    break;
                }
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if ch == '"' {
            out.push('"');
            i += 1;
            while i < c.len() {
                if c[i] == '\\' {
                    out.push(' ');
                    if let Some(&n) = c.get(i + 1) {
                        blank(&mut out, n);
                    }
                    i += 2;
                    continue;
                }
                if c[i] == '"' {
                    out.push('"');
                    i += 1;
                    break;
                }
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if ch == '\'' && !prev_is_ident_except(&c, i, &['b']) {
            // char literal: '\x..' / '\u{..}' / '\n' / 'c' -- otherwise a lifetime.
            if next == Some('\\') {
                // The escaped character itself (`'\''`, `'\\'`) is never the close.
                let mut j = i + 3;
                while j < c.len() && c[j] != '\'' && c[j] != '\n' {
                    j += 1;
                }
                if c.get(j) == Some(&'\'') {
                    out.push('\'');
                    for _ in i + 1..j {
                        out.push(' ');
                    }
                    out.push('\'');
                    i = j + 1;
                    continue;
                }
            } else if next.is_some() && c.get(i + 2) == Some(&'\'') {
                out.push_str("' '");
                i += 3;
                continue;
            }
            out.push('\'');
            i += 1;
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

/// Is the character before `i` an identifier character, other than a lone
/// literal-prefix letter from `prefixes` (one not itself preceded by an
/// identifier character)?
fn prev_is_ident_except(c: &[char], i: usize, prefixes: &[char]) -> bool {
    let id = |x: char| x.is_alphanumeric() || x == '_';
    i > 0 && id(c[i - 1]) && !(prefixes.contains(&c[i - 1]) && (i < 2 || !id(c[i - 2])))
}

/// At `j` (just after the `r`): `#`* then `"` -> the hash count.
fn raw_start(c: &[char], j: usize) -> Option<usize> {
    let mut k = j;
    while c.get(k) == Some(&'#') {
        k += 1;
    }
    (c.get(k) == Some(&'"')).then_some(k - j)
}

/// Rocq: nested `(* *)` comments and `"..."` strings (`""` is an escaped quote).
/// Rocq lexes strings INSIDE comments too: in `(* "*)" Admitted. *)` the
/// first `*)` is in a string, so the comment runs to the second.
pub(crate) fn rocq_code(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
    let mut out = String::with_capacity(src.len());
    let blank = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    let mut i = 0;
    let mut depth = 0usize;
    while i < c.len() {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        if ch == '(' && next == Some('*') {
            depth += 1;
            out.push_str("  ");
            i += 2;
        } else if depth > 0 && ch == '*' && next == Some(')') {
            depth -= 1;
            out.push_str("  ");
            i += 2;
        } else if depth > 0 && ch == '"' {
            // A string inside a comment: blank it whole; `""` stays inside.
            out.push(' ');
            i += 1;
            while i < c.len() {
                if c[i] == '"' && c.get(i + 1) == Some(&'"') {
                    out.push_str("  ");
                    i += 2;
                    continue;
                }
                if c[i] == '"' {
                    out.push(' ');
                    i += 1;
                    break;
                }
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if depth > 0 {
            blank(&mut out, ch);
            i += 1;
        } else if ch == '"' {
            out.push('"');
            i += 1;
            while i < c.len() {
                if c[i] == '"' && c.get(i + 1) == Some(&'"') {
                    out.push_str("  ");
                    i += 2;
                    continue;
                }
                if c[i] == '"' {
                    out.push('"');
                    i += 1;
                    break;
                }
                blank(&mut out, c[i]);
                i += 1;
            }
        } else {
            out.push(ch);
            i += 1;
        }
    }
    out
}

/// Lean 4: `--` line comments, nested `/- -/` block (and doc) comments,
/// `"..."` strings, and char literals (`'"'` must not open a string; `'` after
/// an identifier character is part of the name, as in `h'`).
///
/// NOT modelled: string interpolation. In `s!"{e}"`, `m!"..."`, `f!"..."`,
/// `throwError "..."` and any syntax built on `interpolatedStr`, the braces
/// hold a TERM -- code that may itself contain strings, comments or a `sorry`
/// (round 11, F2: `s!"{"\""}"` opened a phantom string here that hid a later
/// `sorry`). Whether a string is interpolated depends on the syntax around it,
/// which a lexer cannot know, so `lean_unmodelled` reports every unescaped `{`
/// inside a string literal and the gate REFUSES the file (fails closed).
pub(crate) fn lean_code(src: &str) -> String {
    lean_lex(src).0
}

/// The 1-based line of the first unescaped `{` inside a Lean string literal
/// (a possible interpolation the lexer cannot model), if any.
pub(crate) fn lean_unmodelled(src: &str) -> Option<usize> {
    lean_lex(src).1
}

fn lean_lex(src: &str) -> (String, Option<usize>) {
    let c: Vec<char> = src.chars().collect();
    let mut brace: Option<usize> = None;
    let mut out = String::with_capacity(src.len());
    let blank = |out: &mut String, ch: char| out.push(if ch == '\n' { '\n' } else { ' ' });
    let mut i = 0;
    let mut depth = 0usize;
    while i < c.len() {
        let ch = c[i];
        let next = c.get(i + 1).copied();
        if ch == '/' && next == Some('-') {
            depth += 1;
            out.push_str("  ");
            i += 2;
        } else if depth > 0 && ch == '-' && next == Some('/') {
            depth -= 1;
            out.push_str("  ");
            i += 2;
        } else if depth > 0 {
            blank(&mut out, ch);
            i += 1;
        } else if ch == '-' && next == Some('-') {
            while i < c.len() && c[i] != '\n' {
                i += 1;
            }
        } else if ch == '\'' && !(i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '_' || c[i - 1] == '\'' || c[i - 1] == '.')) {
            // 'c' or '\n' / '\'' / '\\' / '\x41' / '\u{41}'.
            let end = if next == Some('\\') {
                (i + 2..c.len().min(i + 14)).find(|&j| c[j] == '\'' && j > i + 2).filter(|&j| c[i + 2..j].iter().all(|x| *x != '\n'))
            } else if next.is_some_and(|n| n != '\n') && c.get(i + 2) == Some(&'\'') {
                Some(i + 2)
            } else {
                None
            };
            match end {
                Some(j) => {
                    out.push('\'');
                    for _ in i + 1..j {
                        out.push(' ');
                    }
                    out.push('\'');
                    i = j + 1;
                }
                None => {
                    out.push('\'');
                    i += 1;
                }
            }
        } else if ch == '«' && c[i + 1..].iter().take_while(|x| **x != '\n').any(|x| *x == '»') {
            // A `«...»` identifier: its contents are part of a name, never a
            // string, comment or keyword (round 7: `«zz_a"»`, `«zz/-»`, `«zz--»`
            // opened a phantom string or comment and hid a theorem; `«end»`
            // counted as a scope keyword). The contents become `_`.
            out.push('«');
            i += 1;
            while c[i] != '»' {
                out.push('_');
                i += 1;
            }
            out.push('»');
            i += 1;
        } else if ch == 'r'
            && (next == Some('"') || next == Some('#'))
            && !(i > 0 && (c[i - 1].is_alphanumeric() || "_'.!?".contains(c[i - 1])))
            && raw_start(&c, i + 1).is_some()
        {
            // Raw string r"..." / r#"..."#: no escapes, so `r"\"` is complete.
            let hashes = raw_start(&c, i + 1).unwrap_or(0);
            out.push('r');
            out.extend(std::iter::repeat_n('#', hashes));
            out.push('"');
            i += 1 + hashes + 1;
            while i < c.len() {
                if c[i] == '"' && (0..hashes).all(|k| c.get(i + 1 + k) == Some(&'#')) {
                    out.push('"');
                    out.extend(std::iter::repeat_n('#', hashes));
                    i += 1 + hashes;
                    break;
                }
                blank(&mut out, c[i]);
                i += 1;
            }
        } else if ch == '"' {
            out.push('"');
            i += 1;
            while i < c.len() {
                if c[i] == '\\' {
                    out.push(' ');
                    if let Some(&n) = c.get(i + 1) {
                        blank(&mut out, n);
                    }
                    i += 2;
                    continue;
                }
                if c[i] == '"' {
                    out.push('"');
                    i += 1;
                    break;
                }
                if c[i] == '{' && brace.is_none() {
                    brace = Some(1 + c[..i].iter().filter(|x| **x == '\n').count());
                }
                blank(&mut out, c[i]);
                i += 1;
            }
        } else {
            out.push(ch);
            i += 1;
        }
    }
    (out, brace)
}

/// Self-test plants for the lexers (run by trust-ledger's self-test): each is
/// (what, holds).
pub(crate) fn self_check() -> Vec<(String, bool)> {
    let mut v = Vec::new();
    let mut ck = |what: &str, got: bool| v.push((format!("lex: {what}"), got));
    // Round 3, finding 6: `'\''` closed at the escaped quote left `','` and a
    // phantom string from `"` that blanked the real `kani::assume` after it.
    let rs = rust_code("let _a = ['\\'','\"']; kani::assume(c); let _s = \"x\";");
    ck("PLANTED rust `'\\''` then `'\"'` does not open a phantom string (kani::assume stays code)", count_token(&rs, "assume") == 1);
    ck("rust `'\\\\'` is one char literal", count_token(&rust_code("let a = '\\\\'; assume(x); let s = \"y\";"), "assume") == 1);
    ck("rust string contents are still blanked", count_token(&rust_code("let s = \"assume\";"), "assume") == 0);
    // Finding 7: a Lean raw string `r"\"` read as an escaped quote swallowed the
    // next lines (a `sorry` theorem) into a phantom string.
    let ln = lean_code("def zz_s := r\"\\\"\ntheorem zz_bad : False := sorry\ndef zz_t := \"x\"\n");
    ck("PLANTED lean raw string `r\"\\\"` ends at its quote (the sorry after it is code)", count_token(&ln, "sorry") == 1 && count_token(&ln, "theorem") == 1);
    let ln2 = lean_code("def a := r#\"sorry \" sorry\"#\ntheorem b : True := sorry\n");
    ck("lean raw string `r#\"..\"..\"#` is blanked to its `\"#`", count_token(&ln2, "sorry") == 1);
    for (what, src) in [
        ("PLANTED G4 `«zz_a\"»` ... `«zz_b\"»`", "def «zz_a\"» : Nat := 0\ntheorem zz_nd : True := sorry\ndef «zz_b\"» : Nat := 0\n"),
        ("PLANTED G5 `«zz/-»` ... `«zz-/»`", "def «zz/-» : Nat := 0\ntheorem zz_nd : True := sorry\ndef «zz-/» : Nat := 0\n"),
        ("PLANTED G6 `«zz--»` then a theorem on the same line", "def «zz--» : Nat := 0 theorem zz_nd : True := sorry\n"),
    ] {
        let l = lean_code(src);
        ck(&format!("lean {what}: the theorem between stays code"), count_token(&l, "theorem") == 1 && count_token(&l, "sorry") == 1);
    }
    ck("lean `«end»` is a name, not the keyword `end`", count_token(&lean_code("structure S where\n  «end» : Nat\n"), "end") == 0);
    ck("lean a `'«'` char literal is not a guillemet name", count_token(&lean_code("def c : Char := '«'\ntheorem t : True := sorry\ndef d : Char := '»'\n"), "sorry") == 1);
    ck("lean `r` inside an identifier is not a raw string", count_token(&lean_code("def br := 1\ntheorem t : True := sorry\n"), "sorry") == 1);
    // Round 11, F1: raw C strings. `cr"\"` is complete (no escapes); read as
    // an escaped quote it opened a phantom string up to the `"` in the comment.
    for (what, src) in [
        ("PLANTED F1 `cr\"\\\"`", "let _p = cr\"\\\"; kani::assume(c); // \"\n"),
        ("PLANTED F1 `cr#\"\\\"#`", "let _p = cr#\"\\\"#; kani::assume(c); // \"\n"),
        ("control `br\"\\\"`", "let _p = br\"\\\"; kani::assume(c); // \"\n"),
        ("control `r\"\\\"`", "let _p = r\"\\\"; kani::assume(c); // \"\n"),
        ("`c\"\\\"\"` (an ordinary C string, escapes apply)", "let _p = c\"\\\"\"; kani::assume(c); // \"\n"),
        ("`b'\"'` (a byte char)", "let _p = b'\"'; kani::assume(c); // \"\n"),
    ] {
        ck(&format!("rust {what}: the kani::assume after it stays code"), count_token(&rust_code(src), "assume") == 1);
    }
    ck("rust `cr\"...\"` contents are blanked", count_token(&rust_code("let _p = cr\"kani::assume(x)\";\n"), "assume") == 0);
    ck("rust `xcr\"` is not a raw prefix (an identifier ending in cr)", count_token(&rust_code("let _p = xcr\"\\\"\"; kani::assume(c);\n"), "assume") == 1);
    // Round 11, F2: an interpolated Lean string is reported, never lexed.
    ck(
        "PLANTED F2 lean `s!\"{\"\\\"\"}\"` is reported as unmodelled (line 1)",
        lean_unmodelled("def zz_s : String := s!\"{\"\\\"\"}\"\ntheorem zz_bad : False := sorry\n-- \"\n") == Some(1),
    );
    ck("lean `throwError \"{sorry}\"` (no `!`) is reported too", lean_unmodelled("theorem t : True := trivial\ndef f := throwError \"{sorry}\"\n") == Some(2));
    ck("lean an escaped `\\{` in a string is not reported", lean_unmodelled("def s := \"a \\{ b\"\n").is_none());
    ck("lean a `{` in a comment or a char literal is not reported", lean_unmodelled("-- \"{\"\n/- \"{\" -/\ndef c := '{'\ndef d := { x := 1 }\n").is_none());
    v
}

/// Occurrences of `tok` as a whole identifier-ish token (no identifier
/// character on either side).
pub(crate) fn count_token(text: &str, tok: &str) -> usize {
    token_positions(text, tok).len()
}

pub(crate) fn token_positions(text: &str, tok: &str) -> Vec<usize> {
    let is_ident = |c: char| c.is_alphanumeric() || c == '_' || c == '\'';
    let mut v = Vec::new();
    let mut from = 0;
    while let Some(i) = text[from..].find(tok) {
        let at = from + i;
        let before = text[..at].chars().next_back();
        let after = text[at + tok.len()..].chars().next();
        if !before.is_some_and(is_ident) && !after.is_some_and(is_ident) {
            v.push(at);
        }
        from = at + tok.len();
    }
    v
}

/// Rocq sentences: split comment-blanked text at a run of dots followed by
/// whitespace or end of input, unless the run is exactly `..` (a notation, as
/// in `[1 .. n]`). `Nat.add` does not split; `tac...` (the `Proof with`
/// ellipsis) does.
pub(crate) fn rocq_sentences(code: &str) -> Vec<String> {
    let c: Vec<char> = code.chars().collect();
    let mut out = Vec::new();
    let mut cur = String::new();
    for i in 0..c.len() {
        cur.push(c[i]);
        let run = (0..=i).rev().take_while(|&k| c[k] == '.').count();
        if c[i] == '.' && c.get(i + 1).is_none_or(|n| n.is_whitespace()) && run != 2 {
            out.push(cur.trim().to_string());
            cur.clear();
        }
    }
    if !cur.trim().is_empty() {
        out.push(cur.trim().to_string());
    }
    out
}

/// The attribute items of every `#[cfg_attr(pred, item, item, ...)]` (and
/// `#![cfg_attr(...)]`) in `t`, which must be whitespace-tightened Rust code:
/// `#[cfg_attr(kani, kani::proof, kani::unwind(2))]` -> `kani::proof`,
/// `kani::unwind(2)`. The predicate (first argument) is dropped.
pub(crate) fn cfg_attr_items(t: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(i) = t[from..].find("cfg_attr(") {
        let at = from + i;
        from = at + "cfg_attr(".len();
        let before = t[..at].trim_end();
        if !(before.ends_with("#[") || before.ends_with("#![")) {
            continue;
        }
        let mut depth = 1usize;
        let mut args = Vec::new();
        let mut cur = String::new();
        for ch in t[from..].chars() {
            match ch {
                '(' | '[' => {
                    depth += 1;
                    cur.push(ch);
                }
                ')' | ']' => {
                    depth -= 1;
                    if depth == 0 {
                        break;
                    }
                    cur.push(ch);
                }
                ',' if depth == 1 => args.push(std::mem::take(&mut cur)),
                _ => cur.push(ch),
            }
        }
        args.push(cur);
        out.extend(args.into_iter().skip(1).map(|a| a.trim().to_string()).filter(|a| !a.is_empty()));
    }
    out
}

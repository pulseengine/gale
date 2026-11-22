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

/// Rust: `//` and nested `/* */` comments, `"..."`, `r#"..."#`, `b"..."`, char
/// literals (`'"'` must not open a string) -- lifetimes are left alone.
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
        } else if ch == 'r' && (next == Some('"') || next == Some('#')) && !prev_is_ident(&c, i) && raw_start(&c, i + 1).is_some() {
            // r"..." / r#"..."#  (also reached for br"..." via the 'b' skip below)
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
        } else if ch == '\'' && !prev_is_ident(&c, i) {
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

fn prev_is_ident(c: &[char], i: usize) -> bool {
    i > 0 && (c[i - 1].is_alphanumeric() || c[i - 1] == '_') && !(c[i - 1] == 'b' && (i < 2 || !(c[i - 2].is_alphanumeric() || c[i - 2] == '_')))
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
pub(crate) fn lean_code(src: &str) -> String {
    let c: Vec<char> = src.chars().collect();
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

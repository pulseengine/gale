//! A strict, fail-closed reader for the block-YAML subset GitHub workflows are
//! written in (kani-coverage reads workflows with it).
//!
//! Structure comes from indentation as YAML defines it, not from fixed columns:
//! a sequence item `-   run: x` opens a mapping whose keys sit at the column of
//! `run`, however many spaces follow the dash.
//!
//! Read: block mappings and sequences (also a sequence at its key's own
//! indentation), plain, single- and double-quoted scalars on one line, literal
//! (`|`, `|-`, `|+`) and folded (`>` ...) block scalars, one-line flow
//! collections (`[a, b]`, `{k: v}`), and comments.
//!
//! Refused (an error, never a guess): anchors, aliases, tags, `?` complex keys,
//! merge keys, directives and document markers, tabs in indentation, duplicate
//! keys, a plain scalar continued on the next line, a quoted scalar or flow
//! collection spanning lines, a block-scalar indentation indicator, and any
//! line at an indentation the structure does not allow.

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Style {
    Plain,
    Quoted,
    Literal,
    Folded,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Node {
    Map(Vec<(String, Node)>),
    Seq(Vec<Node>),
    Str(String, Style),
    Null,
}

impl Node {
    pub fn get(&self, key: &str) -> Option<&Node> {
        match self {
            Node::Map(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
    pub fn str(&self) -> Option<&str> {
        match self {
            Node::Str(s, _) => Some(s),
            _ => None,
        }
    }
    pub fn entries(&self) -> &[(String, Node)] {
        match self {
            Node::Map(m) => m,
            _ => &[],
        }
    }
    pub fn items(&self) -> &[Node] {
        match self {
            Node::Seq(v) => v,
            _ => &[],
        }
    }
}

struct P {
    lines: Vec<String>,
    i: usize,
}

fn indent_of(l: &str) -> usize {
    l.len() - l.trim_start_matches(' ').len()
}

/// Is `l` blank or a comment once its indentation is removed?
fn insignificant(l: &str) -> bool {
    let t = l.trim_start_matches(' ');
    t.is_empty() || t.starts_with('#')
}

/// What may follow a complete scalar or flow value on its line: nothing, or a comment.
fn tail_ok(rest: &str) -> bool {
    let t = rest.trim_start_matches(' ');
    t.is_empty() || (t.starts_with('#') && rest.starts_with(' '))
}

/// A double- or single-quoted scalar at the start of `s`: (value, bytes used).
fn quoted(s: &str) -> Result<(String, usize), String> {
    let q = s.chars().next().ok_or("empty quoted scalar")?;
    let b: Vec<char> = s.chars().collect();
    let mut out = String::new();
    let mut i = 1;
    let mut used = q.len_utf8();
    while i < b.len() {
        let c = b[i];
        if q == '\'' && c == '\'' {
            if b.get(i + 1) == Some(&'\'') {
                out.push('\'');
                i += 2;
                used += 2;
                continue;
            }
            return Ok((out, used + 1));
        }
        if q == '"' && c == '\\' {
            let n = *b.get(i + 1).ok_or("a `\\` ends a double-quoted scalar")?;
            out.push(match n {
                'n' => '\n',
                't' => '\t',
                '\\' => '\\',
                '"' => '"',
                '/' => '/',
                ' ' => ' ',
                '0' => '\0',
                o => return Err(format!("escape `\\{o}` in a double-quoted scalar is not read")),
            });
            i += 2;
            used += 1 + n.len_utf8();
            continue;
        }
        if q == '"' && c == '"' {
            return Ok((out, used + 1));
        }
        out.push(c);
        used += c.len_utf8();
        i += 1;
    }
    Err("a quoted scalar does not close on its line".into())
}

fn refuse_indicator(s: &str) -> Result<(), String> {
    match s.chars().next() {
        Some('&') => Err(format!("an anchor (`{s}`) is not read")),
        Some('*') => Err(format!("an alias (`{s}`) is not read")),
        Some('!') => Err(format!("a tag (`{s}`) is not read")),
        Some('%') | Some('@') | Some('`') => Err(format!("`{s}` starts with a reserved indicator")),
        Some('?') if s.len() == 1 || s[1..].starts_with(' ') => Err("a `?` complex key is not read".into()),
        _ => Ok(()),
    }
}

/// One flow value at `s[*pos..]`, stopping before `,`, `]` or `}` (and before
/// `:` + space inside a flow mapping key).
fn flow_value(s: &str, pos: &mut usize, in_map_key: bool) -> Result<Node, String> {
    let skip = |pos: &mut usize| {
        while s[*pos..].starts_with(' ') {
            *pos += 1;
        }
    };
    skip(pos);
    let r = &s[*pos..];
    if r.starts_with('[') {
        *pos += 1;
        let mut v = Vec::new();
        loop {
            skip(pos);
            if s[*pos..].starts_with(']') {
                *pos += 1;
                return Ok(Node::Seq(v));
            }
            v.push(flow_value(s, pos, false)?);
            skip(pos);
            if s[*pos..].starts_with(',') {
                *pos += 1;
            } else if !s[*pos..].starts_with(']') {
                return Err(format!("a flow sequence does not close on its line: `{s}`"));
            }
        }
    }
    if r.starts_with('{') {
        *pos += 1;
        let mut m: Vec<(String, Node)> = Vec::new();
        loop {
            skip(pos);
            if s[*pos..].starts_with('}') {
                *pos += 1;
                return Ok(Node::Map(m));
            }
            let k = match flow_value(s, pos, true)? {
                Node::Str(k, _) => k,
                _ => return Err(format!("a flow mapping key is not a scalar: `{s}`")),
            };
            skip(pos);
            let v = if s[*pos..].starts_with(':') {
                *pos += 1;
                flow_value(s, pos, false)?
            } else {
                Node::Null
            };
            if m.iter().any(|(x, _)| *x == k) {
                return Err(format!("duplicate key `{k}` in a flow mapping"));
            }
            m.push((k, v));
            skip(pos);
            if s[*pos..].starts_with(',') {
                *pos += 1;
            } else if !s[*pos..].starts_with('}') {
                return Err(format!("a flow mapping does not close on its line: `{s}`"));
            }
        }
    }
    if r.starts_with('"') || r.starts_with('\'') {
        let (v, used) = quoted(r)?;
        *pos += used;
        return Ok(Node::Str(v, Style::Quoted));
    }
    refuse_indicator(r)?;
    let mut end = 0;
    let b = r.as_bytes();
    while end < b.len() {
        let c = b[end];
        if c == b',' || c == b']' || c == b'}' || c == b'[' || c == b'{' {
            break;
        }
        if c == b':' && (end + 1 >= b.len() || matches!(b[end + 1], b' ' | b',' | b']' | b'}')) {
            if !in_map_key {
                return Err(format!("a `key: value` inside a flow sequence is not read: `{s}`"));
            }
            break;
        }
        if c == b'#' && end > 0 && b[end - 1] == b' ' {
            return Err("a comment inside a flow collection".into());
        }
        end += 1;
    }
    let v = r[..end].trim_end();
    *pos += end;
    if v.is_empty() {
        return Ok(Node::Null);
    }
    Ok(Node::Str(v.to_string(), Style::Plain))
}

impl P {
    /// The next significant line at or after `self.i`: (index, indent).
    fn peek(&self) -> Option<(usize, usize)> {
        (self.i..self.lines.len()).find(|&k| !insignificant(&self.lines[k])).map(|k| (k, indent_of(&self.lines[k])))
    }

    fn err<T>(&self, line: usize, what: impl std::fmt::Display) -> Result<T, String> {
        Err(format!("line {}: {what}", line + 1))
    }

    /// The block node whose first significant line is at column `col`.
    fn block(&mut self, col: usize) -> Result<Node, String> {
        let Some((k, ind)) = self.peek() else { return Ok(Node::Null) };
        if ind != col {
            return self.err(k, format!("indentation {ind}, expected {col}"));
        }
        let t = &self.lines[k][col..];
        if t == "-" || t.starts_with("- ") {
            self.seq(col)
        } else {
            self.map(col)
        }
    }

    fn seq(&mut self, col: usize) -> Result<Node, String> {
        let mut v = Vec::new();
        while let Some((k, ind)) = self.peek() {
            if ind < col {
                break;
            }
            if ind > col {
                return self.err(k, format!("indentation {ind} inside a sequence at column {col}"));
            }
            let t = self.lines[k][col..].to_string();
            if !(t == "-" || t.starts_with("- ")) {
                break; // the parent mapping's next key (a sequence at its key's indentation)
            }
            let after = &t[1..];
            let gap = after.len() - after.trim_start_matches(' ').len();
            let content = &after[gap..];
            if content.is_empty() || content.starts_with('#') {
                self.i = k + 1;
                match self.peek() {
                    Some((_, n)) if n > col => v.push(self.block(n)?),
                    _ => v.push(Node::Null),
                }
                continue;
            }
            // Blank the dash: the item's content is a node at its own column.
            let ccol = col + 1 + gap;
            let mut line = self.lines[k].clone();
            line.replace_range(col..col + 1, " ");
            self.lines[k] = line;
            self.i = k;
            if is_key_line(content) {
                v.push(self.map(ccol)?);
            } else if content == "-" || content.starts_with("- ") {
                v.push(self.seq(ccol)?);
            } else {
                self.i = k + 1;
                v.push(self.inline(k, content.to_string(), col)?);
            }
        }
        Ok(Node::Seq(v))
    }

    fn map(&mut self, col: usize) -> Result<Node, String> {
        let mut m: Vec<(String, Node)> = Vec::new();
        while let Some((k, ind)) = self.peek() {
            if ind < col {
                break;
            }
            if ind > col {
                return self.err(k, format!("indentation {ind} inside a mapping at column {col}"));
            }
            let t = self.lines[k][col..].to_string();
            if t == "-" || t.starts_with("- ") {
                if m.is_empty() {
                    return self.err(k, "a sequence item where a mapping key was expected");
                }
                break;
            }
            let (key, rest) = match split_key(&t) {
                Ok(x) => x,
                Err(e) => return self.err(k, e),
            };
            if key == "<<" {
                return self.err(k, "a merge key (`<<`) is not read");
            }
            if m.iter().any(|(x, _)| *x == key) {
                return self.err(k, format!("duplicate key `{key}`"));
            }
            self.i = k + 1;
            let r = rest.trim_start_matches(' ');
            let value = if r.is_empty() || r.starts_with('#') {
                match self.peek() {
                    Some((_, n)) if n > col => self.block(n)?,
                    Some((j, n)) if n == col && {
                        let s = &self.lines[j][col..];
                        s == "-" || s.starts_with("- ")
                    } =>
                    {
                        self.seq(col)?
                    }
                    _ => Node::Null,
                }
            } else {
                self.inline(k, r.to_string(), col)?
            };
            m.push((key, value));
        }
        Ok(Node::Map(m))
    }

    /// A value written on line `k` after its key or dash; `parent` is the
    /// column of that key or dash (block-scalar content must be deeper).
    fn inline(&mut self, k: usize, r: String, parent: usize) -> Result<Node, String> {
        let node = if r.starts_with('|') || r.starts_with('>') {
            let folded = r.starts_with('>');
            let hdr = r[1..].trim_start_matches(['-', '+']);
            if hdr.starts_with(|c: char| c.is_ascii_digit()) {
                return self.err(k, "a block-scalar indentation indicator is not read");
            }
            if !(hdr.is_empty() || (hdr.starts_with(' ') && tail_ok(hdr))) || r[1..].len() - hdr.len() > 1 {
                return self.err(k, format!("block-scalar header `{r}` is not read"));
            }
            let mut body = Vec::new();
            let mut j = self.i;
            let first = (j..self.lines.len()).find(|&x| !self.lines[x].trim().is_empty());
            let cind = first.map(|x| indent_of(&self.lines[x])).unwrap_or(0);
            if first.is_some() && cind > parent {
                while j < self.lines.len() {
                    let l = &self.lines[j];
                    if l.trim().is_empty() {
                        body.push(String::new());
                    } else if indent_of(l) >= cind {
                        body.push(l[cind..].to_string());
                    } else {
                        break;
                    }
                    j += 1;
                }
                while body.last().is_some_and(String::is_empty) {
                    body.pop();
                    j -= 1;
                }
                // Keep trailing blank lines unconsumed only if they were blank.
                while j < self.lines.len() && self.lines[j].trim().is_empty() {
                    j += 1;
                }
                self.i = j;
            }
            let mut s = body.join("\n");
            if !s.is_empty() {
                s.push('\n');
            }
            Node::Str(s, if folded { Style::Folded } else { Style::Literal })
        } else if r.starts_with('[') || r.starts_with('{') {
            let mut pos = 0;
            let n = flow_value(&r, &mut pos, false).or_else(|e| self.err(k, e))?;
            if !tail_ok(&r[pos..]) {
                return self.err(k, format!("text after a flow collection: `{}`", &r[pos..]));
            }
            n
        } else if r.starts_with('"') || r.starts_with('\'') {
            let (v, used) = quoted(&r).or_else(|e| self.err(k, e))?;
            if !tail_ok(&r[used..]) {
                return self.err(k, format!("text after a quoted scalar: `{}`", &r[used..]));
            }
            Node::Str(v, Style::Quoted)
        } else {
            if let Err(e) = refuse_indicator(&r) {
                return self.err(k, e);
            }
            let v = match r.find(" #") {
                Some(x) => r[..x].trim_end(),
                None => r.trim_end(),
            };
            if v.contains(": ") || v.ends_with(':') {
                return self.err(k, format!("`{v}`: a mapping value is not allowed here"));
            }
            Node::Str(v.to_string(), Style::Plain)
        };
        // A deeper line after the value: a continued plain scalar, a block
        // scalar's under-indented line, or a mis-indented key. None is read.
        if let Some((j, n)) = self.peek() {
            if n > parent {
                return self.err(j, "a line deeper than the value before it (a continued scalar?) is not read");
            }
        }
        Ok(node)
    }
}

/// Does `t` (a line's content) start with a mapping key?
fn is_key_line(t: &str) -> bool {
    split_key(t).is_ok()
}

/// `key: rest` -> (key, rest). Quoted keys are unquoted; a plain key ends at the
/// first `:` followed by a space or the end of the line.
fn split_key(t: &str) -> Result<(String, &str), String> {
    if t.starts_with('"') || t.starts_with('\'') {
        let (k, used) = quoted(t)?;
        let rest = t[used..].trim_start_matches(' ');
        let rest = rest.strip_prefix(':').ok_or_else(|| format!("`{t}` is not a `key: value` line"))?;
        if !(rest.is_empty() || rest.starts_with(' ')) {
            return Err(format!("`{t}` is not a `key: value` line"));
        }
        return Ok((k, rest));
    }
    refuse_indicator(t)?;
    if t.starts_with(['[', '{', '|', '>', ',', ']', '}', '#']) {
        return Err(format!("`{t}` is not a `key: value` line"));
    }
    let b = t.as_bytes();
    let i = (0..b.len()).find(|&i| b[i] == b':' && b.get(i + 1).is_none_or(|n| *n == b' ')).ok_or_else(|| format!("`{t}` is not a `key: value` line"))?;
    if t[..i].contains(" #") {
        return Err(format!("`{t}` is not a `key: value` line"));
    }
    Ok((t[..i].trim_end().to_string(), &t[i + 1..]))
}

pub fn parse(text: &str) -> Result<Node, String> {
    // YAML breaks lines at CR as well as LF (and YAML 1.1 readers, libyaml
    // among them, also at NEL, LS and PS). A lone CR inside a comment hid a
    // `continue-on-error:` from a LF-only reader (L11 round 4, K1), so any
    // break other than LF or CRLF is refused.
    let b: Vec<char> = text.chars().collect();
    for (i, c) in b.iter().enumerate() {
        let lone_cr = *c == '\r' && b.get(i + 1) != Some(&'\n');
        if lone_cr || matches!(c, '\u{85}' | '\u{2028}' | '\u{2029}') || (*c == '\u{feff}' && i > 0) {
            let line = b[..i].iter().filter(|x| **x == '\n').count() + 1;
            return Err(format!("line {line}: a line break other than LF or CRLF (U+{:04X}) is not read", *c as u32));
        }
    }
    let mut lines = Vec::new();
    for (n, l) in text.lines().enumerate() {
        let l = l.strip_suffix('\r').unwrap_or(l);
        let lead = &l[..l.len() - l.trim_start().len()];
        if lead.contains('\t') {
            return Err(format!("line {}: a tab in the indentation", n + 1));
        }
        if l.starts_with("---") || l.starts_with("...") || l.starts_with('%') {
            return Err(format!("line {}: directives and document markers are not read", n + 1));
        }
        lines.push(l.to_string());
    }
    let mut p = P { lines, i: 0 };
    let root = match p.peek() {
        None => Node::Null,
        Some((_, ind)) => p.block(ind)?,
    };
    if let Some((k, _)) = p.peek() {
        return p.err(k, "a line outside the document's structure");
    }
    Ok(root)
}

/// Self-test cases for the reader, run by kani-coverage's self-test.
pub fn self_check() -> Vec<(String, bool)> {
    let mut v = Vec::new();
    let mut ck = |what: &str, got: bool| v.push((format!("yaml: {what}"), got));
    let doc = "a: 1 # c\nb:\n  - x\n  -   k: v\n      k2: 'q''s'\n  - [p, \"r\"]\nc:\n- y\nd: |\n  line1\n    line2\n\n  line3\ne: {f: [g]}\n";
    let n = parse(doc);
    ck("a document with every read construct parses", n.is_ok());
    let n = n.unwrap_or(Node::Null);
    ck("a plain scalar loses its comment", n.get("a").and_then(Node::str) == Some("1"));
    let b = n.get("b").map(Node::items).unwrap_or(&[]);
    ck("`-   k: v` is a mapping at the column of k (dash spacing is not a column rule)",
        b.len() == 3 && b[1].get("k").and_then(Node::str) == Some("v") && b[1].get("k2").and_then(Node::str) == Some("q's"));
    ck("a sequence at its key's own indentation", n.get("c").map(Node::items).is_some_and(|i| i.len() == 1));
    ck("a literal block keeps relative indentation and blank lines",
        n.get("d").and_then(Node::str) == Some("line1\n  line2\n\nline3\n"));
    ck("a flow mapping with a flow sequence", n.get("e").and_then(|e| e.get("f")).map(Node::items).is_some_and(|i| i.len() == 1));
    for (what, bad) in [
        ("an anchor", "a: &x 1\n"),
        ("an alias", "a: *x\n"),
        ("a tag", "a: !!str 1\n"),
        ("a merge key", "<<: {a: 1}\n"),
        ("a duplicate key", "a: 1\na: 2\n"),
        ("a continued plain scalar", "a: one\n  two\n"),
        ("a quoted scalar spanning lines", "a: \"one\n  two\"\n"),
        ("a tab in the indentation", "a:\n\tb: 1\n"),
        ("a mis-indented sibling key", "a:\n    b: 1\n  c: 2\n"),
        ("a deeper key under a scalar", "- run: x\n    continue-on-error: true\n"),
        ("a document marker", "---\na: 1\n"),
        ("an indentation indicator", "a: |2\n   x\n"),
        ("a lone CR hiding a key inside a comment (K1)", "a: 1 # c\r  b: 2\n"),
        ("a NEL line break", "a: 1 # c\u{85}b: 2\n"),
        ("a LS line break", "a: 1\u{2028}b: 2\n"),
    ] {
        ck(&format!("REFUSED {what}"), parse(bad).is_err());
    }
    v
}

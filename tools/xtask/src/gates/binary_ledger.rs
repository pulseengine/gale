//! Every committed binary has a recorded reason to exist, and something still reads it.
//!
//! A census on 2026-11-11 found 60 committed binaries. Most were linked by
//! os/gust/build.rs, run by a Renode target, or read by a gate; a few were read
//! by nothing at all (two provider objects, four wasm inputs of closed upstream
//! bugs) and stayed only because nobody asked. `binaries.toml` records, per
//! binary, what it is, which file consumes it, the string that file uses to reach
//! it (`needle`, default the file name), and how to regenerate it.
//!
//! FAIL when:
//! - a tracked binary file has no ledger entry;
//! - an entry names a file that is not tracked (stale entry);
//! - an entry's consumer file is missing, or no longer contains its needle;
//! - an entry has no `sha256`, or the file's bytes no longer hash to it (gale#506).
//!
//! The hash is what makes an entry about THIS file rather than about a name.
//! Before it, 13 bytes of garbage committed as `wdg-thin-cm3.o` passed: the path
//! was tracked and the consumer still named it. The entry's freshness fields
//! (`inputs_sha256`, `toolchain`, `stale_issue`, `stale_expires`) are judged by
//! the object-freshness gate, not here.
//!
//! "Binary" means a non-empty tracked file that holds a NUL byte in its first
//! 8 KiB or is not valid UTF-8. The object-freshness gate is not a consumer:
//! it tracks a binary BECAUSE it is committed, so naming it would make every
//! entry justify itself.
//!
//! Blind spot, stated: the needle check proves the consumer still NAMES the
//! binary, not that the code path using it still runs. A build.rs branch that is
//! never taken still contains the name.

use crate::verdict::Verdict;
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "binary-ledger";
const LEDGER: &str = "binaries.toml";

#[derive(Debug, Default, Clone)]
pub struct Entry {
    pub path: String,
    pub kind: String,
    pub consumer: String,
    pub needle: Option<String>,
    pub why: String,
    /// SHA-256 of the committed file's bytes. Required on every entry.
    pub sha256: String,
    /// object-freshness only: the digest of the input set this record was made against.
    pub inputs_sha256: Option<String>,
    /// object-freshness only: the toolchain pin this record was made against.
    pub toolchain: Option<String>,
    /// object-freshness only: the issue that owns a KNOWN-STALE object.
    pub stale_issue: Option<String>,
    /// object-freshness only: the last day (YYYY-MM-DD, UTC) the staleness is accepted.
    pub stale_expires: Option<String>,
}

pub fn is_sha256_hex(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// Parse the restricted TOML subset `binaries.toml` uses: `[[binary]]` headers,
/// `key = "string"` lines, comments and blank lines. Anything else is an error,
/// so the file cannot drift into syntax this parser silently skips.
pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let mut out: Vec<Entry> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line == "[[binary]]" {
            out.push(Entry::default());
            continue;
        }
        let Some((k, v)) = line.split_once(" = ") else {
            return Err(format!("line {n}: expected `key = \"value\"`, got `{line}`"));
        };
        let v = v.trim();
        if !(v.len() >= 2 && v.starts_with('"') && v.ends_with('"')) {
            return Err(format!("line {n}: value must be a double-quoted string"));
        }
        let v = v[1..v.len() - 1].replace("\\\"", "\"");
        let Some(e) = out.last_mut() else {
            return Err(format!("line {n}: key before the first [[binary]]"));
        };
        match k.trim() {
            "path" => e.path = v,
            "kind" => e.kind = v,
            "consumer" => e.consumer = v,
            "needle" => e.needle = Some(v),
            "why" => e.why = v,
            "sha256" => e.sha256 = v,
            "inputs_sha256" => e.inputs_sha256 = Some(v),
            "toolchain" => e.toolchain = Some(v),
            "stale_issue" => e.stale_issue = Some(v),
            "stale_expires" => e.stale_expires = Some(v),
            "regenerate" => {}
            other => return Err(format!("line {n}: unknown key `{other}`")),
        }
    }
    for (i, e) in out.iter().enumerate() {
        for (field, val) in [
            ("path", &e.path),
            ("kind", &e.kind),
            ("consumer", &e.consumer),
            ("why", &e.why),
            ("sha256", &e.sha256),
        ] {
            if val.is_empty() {
                return Err(format!("entry {} ({}): missing `{field}`", i + 1, e.path));
            }
        }
        for (field, val) in [("sha256", Some(&e.sha256)), ("inputs_sha256", e.inputs_sha256.as_ref())] {
            if let Some(v) = val {
                if !is_sha256_hex(v) {
                    return Err(format!("entry {} ({}): `{field}` is not 64 lower-case hex digits", i + 1, e.path));
                }
            }
        }
    }
    Ok(out)
}

pub fn is_binary(bytes: &[u8]) -> bool {
    if bytes.is_empty() {
        return false;
    }
    let head = &bytes[..bytes.len().min(8192)];
    head.contains(&0) || std::str::from_utf8(bytes).is_err()
}

fn tracked(repo: &Path) -> Option<Vec<String>> {
    let out = Command::new("git").arg("-C").arg(repo).args(["ls-files", "-z"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        out.stdout
            .split(|b| *b == 0)
            .filter(|s| !s.is_empty())
            .map(|s| String::from_utf8_lossy(s).into_owned())
            .collect(),
    )
}

/// The decision, separated from I/O so the self-test can drive it.
pub fn judge(
    entries: &[Entry],
    tracked_binaries: &[String],
    tracked_all: &[String],
    read: &dyn Fn(&str) -> Option<String>,
    hash: &dyn Fn(&str) -> Option<String>,
) -> Vec<String> {
    let mut bad = Vec::new();
    for b in tracked_binaries {
        if !entries.iter().any(|e| &e.path == b) {
            bad.push(format!("{b}: committed binary with no entry in {LEDGER}"));
        }
    }
    let mut seen: Vec<&str> = Vec::new();
    for e in entries {
        if seen.contains(&e.path.as_str()) {
            bad.push(format!("{}: listed twice", e.path));
        }
        seen.push(&e.path);
        if !tracked_all.iter().any(|t| t == &e.path) {
            bad.push(format!("{}: in {LEDGER} but not tracked (remove the entry)", e.path));
            continue;
        }
        match hash(&e.path) {
            None => bad.push(format!("{}: cannot be read to hash", e.path)),
            Some(h) if h != e.sha256 => bad.push(format!(
                "{}: sha256 {h} does not match the recorded {} -- the committed bytes are not the ones this entry describes",
                e.path, e.sha256
            )),
            Some(_) => {}
        }
        let needle = e
            .needle
            .clone()
            .unwrap_or_else(|| e.path.rsplit('/').next().unwrap_or(&e.path).to_string());
        match read(&e.consumer) {
            None => bad.push(format!("{}: consumer {} does not exist", e.path, e.consumer)),
            Some(text) if !text.contains(&needle) => bad.push(format!(
                "{}: consumer {} no longer contains `{needle}`",
                e.path, e.consumer
            )),
            Some(_) => {}
        }
    }
    bad
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let Ok(text) = std::fs::read_to_string(repo.join(LEDGER)) else {
        return Verdict::Refused(format!("{LEDGER} is missing -- the gate cannot answer"));
    };
    let entries = match parse(&text) {
        Ok(e) => e,
        Err(m) => return Verdict::Refused(format!("{LEDGER}: {m}")),
    };
    if entries.is_empty() {
        return Verdict::Refused(format!("{LEDGER} has no entries -- the check would be vacuous"));
    }
    let Some(all) = tracked(repo) else {
        return Verdict::Refused("git ls-files failed -- the gate cannot answer".into());
    };
    let bins: Vec<String> = all
        .iter()
        .filter(|f| std::fs::read(repo.join(f)).map(|b| is_binary(&b)).unwrap_or(false))
        .cloned()
        .collect();
    let read = |p: &str| std::fs::read_to_string(repo.join(p)).ok();
    let hash = |p: &str| std::fs::read(repo.join(p)).ok().map(|b| crate::sha256::hex(&b));
    let bad = judge(&entries, &bins, &all, &read, &hash);
    let mut lines = vec![format!(
        "{} tracked binary file(s), {} ledger entr(ies)",
        bins.len(),
        entries.len()
    )];
    let mut kinds: Vec<(&str, usize)> = Vec::new();
    for e in &entries {
        match kinds.iter_mut().find(|(k, _)| *k == e.kind) {
            Some((_, n)) => *n += 1,
            None => kinds.push((&e.kind, 1)),
        }
    }
    for (k, n) in kinds {
        lines.push(format!("  {k}: {n}"));
    }
    if bad.is_empty() {
        lines.push("PASS".into());
        Verdict::Pass(lines)
    } else {
        lines.push(format!("FAIL: {} problem(s):", bad.len()));
        lines.extend(bad.into_iter().map(|b| format!("  {b}")));
        lines.push(format!("A binary nothing reads should be deleted, not ledgered; see {LEDGER}."));
        Verdict::Fail(lines)
    }
}

pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut ok = true;
    let mut ck = |what: &str, got: bool| {
        ok &= got;
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
    };
    let e = |p: &str, c: &str, n: Option<&str>| Entry {
        path: p.into(),
        kind: "k".into(),
        consumer: c.into(),
        needle: n.map(String::from),
        why: "w".into(),
        sha256: crate::sha256::hex(p.as_bytes()),
        ..Entry::default()
    };
    let files = |p: &str| -> Option<String> {
        match p {
            "build.rs" => Some("link ../../d/a.o and x/glob(\"*-thin\")".into()),
            "empty.rs" => Some(String::new()),
            _ => None,
        }
    };
    let all = vec!["d/a.o".to_string(), "d/b.o".to_string()];
    // The fixture's file CONTENT is its own path, so the recorded hash above is
    // right for every file except the one planted below.
    let hash_of = |p: &str| -> Option<String> {
        match p {
            "d/a.o" | "d/b.o" => Some(crate::sha256::hex(p.as_bytes())),
            _ => None,
        }
    };
    let j = |es: &[Entry], bins: &[&str]| {
        let b: Vec<String> = bins.iter().map(|s| s.to_string()).collect();
        judge(es, &b, &all, &files, &hash_of)
    };
    ck("listed + consumer names it: pass", j(&[e("d/a.o", "build.rs", None)], &["d/a.o"]).is_empty());
    ck("unlisted binary: fail", j(&[e("d/a.o", "build.rs", None)], &["d/a.o", "d/b.o"]).len() == 1);
    ck("consumer lacks the name: fail", j(&[e("d/b.o", "build.rs", None)], &["d/b.o"]).len() == 1);
    ck("consumer missing: fail", j(&[e("d/a.o", "gone.rs", None)], &["d/a.o"]).len() == 1);
    ck("empty consumer: fail", j(&[e("d/a.o", "empty.rs", None)], &["d/a.o"]).len() == 1);
    ck("entry for an untracked file: fail", j(&[e("d/z.o", "build.rs", None)], &[]).len() == 1);
    ck("needle overrides the file name", j(&[e("d/b.o", "build.rs", Some("glob(\"*-thin\")"))], &["d/b.o"]).is_empty());
    ck("duplicate entry: fail", j(&[e("d/a.o", "build.rs", None), e("d/a.o", "build.rs", None)], &["d/a.o"]).len() == 1);
    // gale#506: garbage committed under a ledgered name passed, because nothing
    // looked at the bytes. A wrong recorded hash is the same thing seen from the
    // ledger's side.
    let mut planted = e("d/a.o", "build.rs", None);
    planted.sha256 = crate::sha256::hex(b"13 bytes junk");
    ck("bytes that do not hash to the record: fail", j(&[planted], &["d/a.o"]).len() == 1);
    ck("sha256 implementation matches FIPS 180-4 vectors", crate::sha256::self_check());
    ck("NUL byte is binary", is_binary(b"ELF\0\x01"));
    ck("invalid UTF-8 is binary", is_binary(&[0x7f, 0xff, 0xfe]));
    ck("plain text is not binary", !is_binary(b"fn main() {}\n"));
    ck("empty file is not binary", !is_binary(b""));
    let h = crate::sha256::hex(b"");
    ck(
        "parser: well-formed",
        parse(&format!("[[binary]]\npath = \"a\"\nkind = \"k\"\nconsumer = \"c\"\nwhy = \"w\"\nsha256 = \"{h}\"\n"))
            .map(|v| v.len())
            == Ok(1),
    );
    ck(
        "parser: entry without sha256 refused",
        parse("[[binary]]\npath = \"a\"\nkind = \"k\"\nconsumer = \"c\"\nwhy = \"w\"\n").is_err(),
    );
    ck(
        "parser: malformed sha256 refused",
        parse("[[binary]]\npath = \"a\"\nkind = \"k\"\nconsumer = \"c\"\nwhy = \"w\"\nsha256 = \"abc\"\n").is_err(),
    );
    ck("parser: unknown key refused", parse("[[binary]]\npath = \"a\"\nbogus = \"x\"\n").is_err());
    ck("parser: missing field refused", parse("[[binary]]\npath = \"a\"\n").is_err());
    ck("parser: unquoted value refused", parse("[[binary]]\npath = a\n").is_err());
    if ok {
        lines.push("PASS".into());
        Verdict::Pass(lines)
    } else {
        Verdict::Fail(lines)
    }
}

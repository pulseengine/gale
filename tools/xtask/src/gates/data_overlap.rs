//! A fused core module's data segments must not overlap.
//!
//! Found the hard way (gale#266): `meld fuse --memory shared` merges memories
//! but does NOT rebase addresses, and every wit-bindgen component places
//! `.rodata` at the wasm-ld default base 1048576. Fusing three stacks all three
//! on the same address. meld exits 0 and prints "Fusion complete!"; nothing
//! downstream notices, because the dissolve gate checks undefined SYMBOLS and
//! object SIZES and nothing looks at where data lands. Segments are applied in
//! order at instantiation, so a later one silently overwrites an earlier one.
//!
//! Ported from check-data-overlap.py (gale#368), with two corrections:
//!
//! 1. The Python returned PASS when it parsed zero segments. If `wasm-tools
//!    print` ever changes shape the regex stops matching, and the gate reports
//!    "no data segments -- nothing to check" and goes green on a module that
//!    may be riddled with overlaps. Parser drift now REFUSES: if the output
//!    mentions a data segment at all but structured parsing yields none, the
//!    gate says it could not answer instead of answering "fine".
//!
//! 2. Payload length mis-counted an escaped backslash followed by hex digits.
//!    The Python substituted the hex-escape pattern first, so `\\41` (one
//!    backslash byte, then the characters '4' and '1' = 3 bytes) was scanned
//!    from the second backslash as a hex escape and counted as 2. Escapes are
//!    now consumed left to right, `\\` before hex, which is the order the WAT
//!    grammar defines them in.

use crate::verdict::Verdict;
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "data-overlap";

#[derive(Debug, Clone, PartialEq, Eq)]
struct Seg {
    idx: u32,
    mem: u32,
    lo: u64,
    hi: u64,
}

/// Byte length of a WAT data payload. `\XX` is one byte, `\\` is one byte, and
/// the named escapes are one byte each; everything else is itself.
fn payload_len(s: &str) -> u64 {
    let b = s.as_bytes();
    let mut i = 0usize;
    let mut n = 0u64;
    while i < b.len() {
        if b[i] == b'\\' && i + 1 < b.len() {
            let c = b[i + 1];
            // `\\` FIRST: an escaped backslash is not the start of a hex escape.
            if c == b'\\' || c == b'"' || c == b'\'' || c == b'n' || c == b't' || c == b'r' {
                i += 2;
            } else if i + 2 < b.len() && c.is_ascii_hexdigit() && b[i + 2].is_ascii_hexdigit() {
                i += 3;
            } else {
                i += 2;
            }
            n += 1;
        } else {
            i += 1;
            n += 1;
        }
    }
    n
}

fn num_after(s: &str, at: usize) -> Option<(u64, usize)> {
    let b = s.as_bytes();
    let mut i = at;
    while i < b.len() && b[i] == b' ' {
        i += 1;
    }
    let start = i;
    while i < b.len() && b[i].is_ascii_digit() {
        i += 1;
    }
    if i == start {
        return None;
    }
    s[start..i].parse().ok().map(|v| (v, i))
}

/// What one `(data (;N;) ...)` occurrence turned out to be.
#[derive(Debug, PartialEq, Eq)]
enum Parsed {
    /// Active: instantiated at a fixed offset, so it can collide.
    Active(Seg),
    /// Passive: copied explicitly by `memory.init`, never laid down at
    /// instantiation, so it cannot participate in an instantiation-time
    /// overlap. Counted, not gated -- and NOT silently dropped, because
    /// "I ignored it" and "there was nothing" must stay distinguishable.
    Passive,
    /// Recognised as a data segment but understood as neither. The scanner is
    /// stale; the gate must refuse rather than report clean.
    Unknown,
}

/// Scan `(data (;N;) [(memory M)] (i32.const OFF) "payload")`.
///
/// Hand-written rather than a regex dependency: the input is machine-generated
/// WAT with a fixed shape, and the self-test pins both spellings (with and
/// without the memory index, which is elided for memory 0).
fn parse_segments(wat: &str) -> Vec<Parsed> {
    let mut out: Vec<Parsed> = Vec::new();
    let mut rest = wat;
    while let Some(p) = rest.find("(data (;") {
        let after = &rest[p + "(data (;".len()..];
        let Some((idx, mut i)) = num_after(after, 0) else {
            out.push(Parsed::Unknown);
            rest = &rest[p + 8..];
            continue;
        };
        if !after[i..].starts_with(";)") {
            out.push(Parsed::Unknown);
            rest = &rest[p + 8..];
            continue;
        }
        i += 2;
        let mut mem = 0u64;
        let t = after[i..].trim_start();
        let consumed = after[i..].len() - t.len();
        i += consumed;
        if after[i..].starts_with("(memory ") {
            let Some((m, j)) = num_after(after, i + "(memory ".len()) else {
                out.push(Parsed::Unknown);
                rest = &rest[p + 8..];
                continue;
            };
            mem = m;
            i = j;
            if !after[i..].starts_with(')') {
                out.push(Parsed::Unknown);
                rest = &rest[p + 8..];
                continue;
            }
            i += 1;
            let t = after[i..].trim_start();
            i += after[i..].len() - t.len();
        }
        if !after[i..].starts_with("(i32.const ") {
            // No offset expression: a passive segment. Recognised, not gated.
            out.push(if after[i..].starts_with('"') { Parsed::Passive } else { Parsed::Unknown });
            rest = &rest[p + 8..];
            continue;
        }
        let Some((off, j)) = num_after(after, i + "(i32.const ".len()) else {
            out.push(Parsed::Unknown);
            rest = &rest[p + 8..];
            continue;
        };
        i = j;
        if !after[i..].starts_with(')') {
            out.push(Parsed::Unknown);
            rest = &rest[p + 8..];
            continue;
        }
        i += 1;
        let t = after[i..].trim_start();
        i += after[i..].len() - t.len();
        if !after[i..].starts_with('"') {
            out.push(Parsed::Unknown);
            rest = &rest[p + 8..];
            continue;
        }
        i += 1;
        // Scan to the closing quote, honouring escapes.
        let b = after.as_bytes();
        let start = i;
        while i < b.len() {
            if b[i] == b'\\' {
                i += 2;
            } else if b[i] == b'"' {
                break;
            } else {
                i += 1;
            }
        }
        if i >= b.len() {
            out.push(Parsed::Unknown);
            rest = &rest[p + 8..];
            continue;
        }
        let n = payload_len(&after[start..i]);
        out.push(Parsed::Active(Seg { idx: idx as u32, mem: mem as u32, lo: off, hi: off + n }));
        rest = &after[i..];
    }
    out
}

fn actives(v: Vec<Parsed>) -> Vec<Seg> {
    v.into_iter()
        .filter_map(|p| match p {
            Parsed::Active(s) => Some(s),
            _ => None,
        })
        .collect()
}

fn overlaps(segs: &[Seg]) -> Vec<(Seg, Seg)> {
    let mut v = Vec::new();
    for a in segs {
        for b in segs {
            if a.idx < b.idx && a.mem == b.mem && a.lo < b.hi && b.lo < a.hi {
                v.push((a.clone(), b.clone()));
            }
        }
    }
    v
}

fn check_module(path: &Path) -> Verdict {
    if !path.is_file() {
        return Verdict::Refused(format!("no such module: {}", path.display()));
    }
    let out = match Command::new("wasm-tools").arg("print").arg(path).output() {
        Ok(o) => o,
        Err(e) => return Verdict::Refused(format!("wasm-tools not runnable: {e}")),
    };
    if !out.status.success() {
        return Verdict::Refused(format!(
            "wasm-tools print failed on {}: {}",
            path.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let wat = String::from_utf8_lossy(&out.stdout).into_owned();
    let parsed = parse_segments(&wat);

    let unknown = parsed.iter().filter(|p| **p == Parsed::Unknown).count();
    let passive = parsed.iter().filter(|p| **p == Parsed::Passive).count();
    let segs: Vec<Seg> = parsed
        .into_iter()
        .filter_map(|p| match p {
            Parsed::Active(s) => Some(s),
            _ => None,
        })
        .collect();

    // The correction that matters. "I did not understand it" and "it is not
    // there" are different claims, and only one of them is a pass. The Python
    // collapsed both into "no data segments -- nothing to check", exit 0.
    if unknown > 0 {
        return Verdict::Refused(format!(
            "{unknown} data segment(s) in a shape this scanner does not understand —              wasm-tools output has drifted; the gate CANNOT answer and must not report clean"
        ));
    }

    let mut lines: Vec<String> = segs
        .iter()
        .map(|s| format!("data {}: mem {} [{} .. {}) len={}", s.idx, s.mem, s.lo, s.hi, s.hi - s.lo))
        .collect();
    if passive > 0 {
        // Recorded, never silently dropped: a passive segment is copied by an
        // explicit memory.init, so it cannot collide AT INSTANTIATION, which is
        // the only collision this gate is about.
        lines.push(format!(
            "{passive} passive segment(s): not laid down at instantiation, so out of scope here"
        ));
    }
    if segs.is_empty() {
        lines.push("no active data segments".into());
        return Verdict::Pass(lines);
    }

    let ov = overlaps(&segs);
    if ov.is_empty() {
        lines.push(format!("ok: {} active data segments, all disjoint", segs.len()));
        return Verdict::Pass(lines);
    }
    lines.push(format!(
        "FAIL: {} overlapping pair(s) — later segments overwrite earlier ones at \
         instantiation, so component state collides",
        ov.len()
    ));
    for (a, b) in &ov {
        lines.push(format!("  data {} [{}..{}) vs data {} [{}..{})", a.idx, a.lo, a.hi, b.idx, b.lo, b.hi));
    }
    lines.push(
        "Cause: --memory shared merges memories without rebasing; every component's \
         .rodata starts at the wasm-ld default base (1048576). Fix: retain \
         -C link-arg=--emit-relocs on the FINAL link (wasm-ld -r is NOT sufficient — \
         its stored values are addends, not final addresses), then fuse with \
         --address-rebase. See gale#266, meld#370."
            .into(),
    );
    Verdict::Fail(lines)
}

/// The committed fused core, the same module the shell job checks.
const FIXTURE: &str = "benches/gust/drivers/measurements/wcet-fixture/gustos.loom.wasm";

/// `--module` lets the gate be aimed at an arbitrary wasm. The CI negative
/// control needs exactly this: it plants a module with known-overlapping
/// segments and requires the gate to reject it. Without the override the gate
/// could only ever be run against the one input it already passes, which is the
/// shape of a control that cannot fail.
pub fn run(repo: &Path, target: Option<&str>) -> Verdict {
    match target {
        Some(t) => check_module(Path::new(t)),
        None => check_module(&repo.join(FIXTURE)),
    }
}

/// Matched pair, plus the payload-length cases the Python got wrong. A gate
/// whose parser is wrong reports disjoint segments that are not disjoint.
pub fn self_test(_repo: &Path, _target: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut check = |what: &str, got: bool| {
        if got {
            lines.push(format!("{what}: ok"));
        } else {
            lines.push(format!("{what}: WRONG"));
            broken = true;
        }
    };

    // Payload length. `\\41` is a backslash byte then '4' then '1' = 3.
    check("payload plain", payload_len("AAAA") == 4);
    check("payload hex escape", payload_len("\\41\\42") == 2);
    check("payload escaped backslash", payload_len("\\\\") == 1);
    check("payload backslash-then-hex", payload_len("\\\\41") == 3);
    check("payload named escape", payload_len("\\n\\t") == 2);

    // Both spellings: the memory index is elided for memory 0.
    let elided = r#"(data (;0;) (i32.const 0) "AAAAAAAA") (data (;1;) (i32.const 4) "BBBB")"#;
    let s = actives(parse_segments(elided));
    check("parse elided memory index", s.len() == 2 && s[0].mem == 0 && s[1].lo == 4);
    check("overlap detected", overlaps(&s).len() == 1);

    let explicit = r#"(data (;0;) (memory 1) (i32.const 100) "AA") (data (;1;) (memory 1) (i32.const 200) "BB")"#;
    let s2 = actives(parse_segments(explicit));
    check("parse explicit memory index", s2.len() == 2 && s2[0].mem == 1 && s2[1].lo == 200);
    check("disjoint accepted", overlaps(&s2).is_empty());

    // Different memories at the same offset do NOT overlap.
    let cross = r#"(data (;0;) (memory 0) (i32.const 0) "AAAA") (data (;1;) (memory 1) (i32.const 0) "BBBB")"#;
    check("distinct memories are not an overlap", overlaps(&actives(parse_segments(cross))).is_empty());

    // The shape the Python passed clean: a passive segment is recognised as
    // passive, not mistaken for "no data segments at all".
    let pas = parse_segments(r#"(data (;0;) "AAAA")"#);
    check("passive recognised", pas.len() == 1 && pas[0] == Parsed::Passive);
    // And a shape nothing understands is Unknown, which drives a refusal.
    let junk = parse_segments("(data (;0;) (f64.const 1) \"AA\")");
    check("unparseable is Unknown", junk.len() == 1 && junk[0] == Parsed::Unknown);

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

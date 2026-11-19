//! Every peripheral base address in the AADL target model agrees with the
//! pinned vendor-derived reference tables.
//!
//! targets/*.aadl is authoritative for gale's hardware facts (REQ-TARGET-MODEL-001),
//! and a typed-in base address can still be wrong. targets/reference/<soc>.peripherals.tsv
//! is imported by targets/reference/import.py from a pinned, hash-checked
//! stm32-data snapshot. Every one of those tables names THIS gate as the thing
//! that cross-checks them; before this module existed the command exited 2
//! (no such gate), so the citation pointed at nothing.
//!
//! What it reads: the MODELS, through the pinned spar (`varve which spar`, run
//! in the repo so varve.toml selects the layer; never a bare `spar` on PATH).
//! Not the committed spar-items fixture under tools/gust-target-gen/tests/golden:
//! that fixture is a copy of spar's output, and a gate that read the copy would
//! pass a model edit that never reached it -- the defect this gate was written
//! alongside.
//!
//! Rule, per SoC package `STM32X` with table `targets/reference/stm32x.peripherals.tsv`:
//!   * every `device` classifier (type or impl) carrying `Gust_Target_Props::Base`
//!     is checked; a device that carries NO Base and is not a board LED/button
//!     refuses the run, so a lost Base cannot silently shrink the denominator;
//!   * its reference name is the classifier upper-cased (`Adc1_Common` ->
//!     `ADC1_COMMON`), mapped through `ALIASES`; a `Console => true` device
//!     takes the UART the board table (`<soc>.board.tsv`, kind `console`) names;
//!   * the name MUST be in the reference and the base MUST equal it. A modelled
//!     peripheral missing from the reference fails. The Cortex-M core
//!     peripherals in `CORE` (PPB, listed by no SoC table) are compared against
//!     their ARCHITECTURAL address instead -- they are not skipped;
//!   * a SoC package without a table fails, and so does a table without a package.
//!
//! Two integrity checks ride along:
//!   * the committed spar-items JSON (`FIXTURE`) must be byte-equal to the
//!     pinned spar's output, so a regen that skipped the models is caught for
//!     every fact in the model, not only for bases;
//!   * every tracked targets/reference/*.tsv must match the sha256 its import
//!     script recorded in targets/reference/SHA256SUMS, so a hand edit to a
//!     table (made to agree with an equally wrong model) fails.
//!
//! Blind spots, stated: only BASES are compared (the tables hold nothing else
//! today), so a wrong register offset, bit position, length or presence flag is
//! not seen; Board_Led/Board_Button devices carry no Base and are checked by the
//! generator's --board-reference, not here; SHA256SUMS lives in the same tree,
//! so editing a table AND its recorded digest together is not detected (the
//! digest is a tripwire against a lone edit, not an authenticated anchor); and
//! the upstream reference is trusted -- a wrong row there makes a matching model
//! wrong in the same way.

use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::ffi::OsStr;
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "target-reference";

/// gale's device name -> the reference's peripheral name, where they differ.
/// Same list as gust-target-gen's reference.rs; a rename on either side fails.
const ALIASES: &[(&str, &str)] = &[("ADC", "ADC1")];

/// Cortex-M core peripherals and their ARCHITECTURAL base (ARMv7-M ARM, System
/// Control Space). No SoC table lists them, so they are compared against this
/// constant rather than exempted.
const CORE: &[(&str, u64)] = &[("MPU", 0xE000_ED90)];

/// The committed copy of spar's output that gust-target-gen's tests and the
/// generated tree are built from. It must equal what the pinned spar produces
/// from the models NOW, or every other fact in it (offsets, bits, memories) can
/// drift from the .aadl unseen -- the base comparison alone covers only bases.
const FIXTURE: &str = "tools/gust-target-gen/tests/golden/f100.items.json";

/// Digests of the reference tables as their import scripts wrote them.
const SUMS: &str = "targets/reference/SHA256SUMS";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Modelled {
    pub package: String,
    pub name: String,
    pub base: u64,
    pub console: bool,
}

/// One SoC's reference: peripheral name -> base, plus the console UART name.
#[derive(Debug, Clone, Default)]
pub struct Reference {
    pub peripherals: BTreeMap<String, u64>,
    pub console: Option<String>,
}

// ---------------------------------------------------------------- tiny JSON
// Hand-rolled for the same reason verdict.rs hand-rolls its output: the gate's
// cold build should not be longer than the check. Only what spar emits is needed.

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    Obj(Vec<(String, Json)>),
}

impl Json {
    fn get(&self, k: &str) -> Option<&Json> {
        match self {
            Json::Obj(v) => v.iter().find(|(kk, _)| kk == k).map(|(_, x)| x),
            _ => None,
        }
    }
    fn str(&self) -> Option<&str> {
        match self {
            Json::Str(s) => Some(s),
            _ => None,
        }
    }
    fn arr(&self) -> &[Json] {
        match self {
            Json::Arr(v) => v,
            _ => &[],
        }
    }
}

pub fn parse_json(s: &str) -> Result<Json, String> {
    let b = s.as_bytes();
    let mut i = 0;
    let v = value(b, &mut i)?;
    ws(b, &mut i);
    if i != b.len() {
        return Err(format!("trailing bytes at offset {i}"));
    }
    Ok(v)
}

fn ws(b: &[u8], i: &mut usize) {
    while *i < b.len() && b[*i].is_ascii_whitespace() {
        *i += 1;
    }
}

fn value(b: &[u8], i: &mut usize) -> Result<Json, String> {
    ws(b, i);
    let Some(&c) = b.get(*i) else { return Err("unexpected end of JSON".into()) };
    match c {
        b'{' => {
            *i += 1;
            let mut out = Vec::new();
            ws(b, i);
            if b.get(*i) == Some(&b'}') {
                *i += 1;
                return Ok(Json::Obj(out));
            }
            loop {
                ws(b, i);
                let Json::Str(k) = value(b, i)? else { return Err(format!("object key expected at {i}")) };
                ws(b, i);
                if b.get(*i) != Some(&b':') {
                    return Err(format!("':' expected at {i}"));
                }
                *i += 1;
                out.push((k, value(b, i)?));
                ws(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b'}') => {
                        *i += 1;
                        return Ok(Json::Obj(out));
                    }
                    _ => return Err(format!("',' or '}}' expected at {i}")),
                }
            }
        }
        b'[' => {
            *i += 1;
            let mut out = Vec::new();
            ws(b, i);
            if b.get(*i) == Some(&b']') {
                *i += 1;
                return Ok(Json::Arr(out));
            }
            loop {
                out.push(value(b, i)?);
                ws(b, i);
                match b.get(*i) {
                    Some(b',') => *i += 1,
                    Some(b']') => {
                        *i += 1;
                        return Ok(Json::Arr(out));
                    }
                    _ => return Err(format!("',' or ']' expected at {i}")),
                }
            }
        }
        b'"' => {
            *i += 1;
            let mut s = String::new();
            loop {
                let Some(&c) = b.get(*i) else { return Err("unterminated string".into()) };
                *i += 1;
                match c {
                    b'"' => return Ok(Json::Str(s)),
                    b'\\' => {
                        let Some(&e) = b.get(*i) else { return Err("bad escape".into()) };
                        *i += 1;
                        match e {
                            b'"' => s.push('"'),
                            b'\\' => s.push('\\'),
                            b'/' => s.push('/'),
                            b'b' => s.push('\u{8}'),
                            b'f' => s.push('\u{c}'),
                            b'n' => s.push('\n'),
                            b'r' => s.push('\r'),
                            b't' => s.push('\t'),
                            b'u' => {
                                let h = b.get(*i..*i + 4).ok_or("bad \\u escape")?;
                                let h = std::str::from_utf8(h).map_err(|_| "bad \\u escape")?;
                                let cp = u32::from_str_radix(h, 16).map_err(|_| "bad \\u escape")?;
                                *i += 4;
                                s.push(char::from_u32(cp).unwrap_or('\u{fffd}'));
                            }
                            _ => return Err(format!("bad escape at {i}")),
                        }
                    }
                    _ => {
                        // Copy a whole UTF-8 sequence: back up one and take the char.
                        let start = *i - 1;
                        let rest = std::str::from_utf8(&b[start..]).map_err(|_| "invalid UTF-8")?;
                        let ch = rest.chars().next().ok_or("invalid UTF-8")?;
                        s.push(ch);
                        *i = start + ch.len_utf8();
                    }
                }
            }
        }
        b't' if b[*i..].starts_with(b"true") => {
            *i += 4;
            Ok(Json::Bool(true))
        }
        b'f' if b[*i..].starts_with(b"false") => {
            *i += 5;
            Ok(Json::Bool(false))
        }
        b'n' if b[*i..].starts_with(b"null") => {
            *i += 4;
            Ok(Json::Null)
        }
        _ => {
            let start = *i;
            while *i < b.len() && matches!(b[*i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9') {
                *i += 1;
            }
            let t = std::str::from_utf8(&b[start..*i]).map_err(|_| "bad number")?;
            t.parse::<f64>().map(Json::Num).map_err(|_| format!("bad value at {start}"))
        }
    }
}


// ---------------------------------------------------------------- extraction

fn flag(c: &Json, key: &str) -> bool {
    c.get("properties").map(Json::arr).unwrap_or(&[]).iter().any(|p| {
        let pn = p.get("name").and_then(Json::str).unwrap_or("");
        pn.rsplit("::").next() == Some(key)
            && p.get("typed_value").and_then(|t| t.get("Boolean")) == Some(&Json::Bool(true))
    })
}

/// Every `device` classifier (type or impl) carrying a Base, from spar's
/// `items --format json` output. A device with no Base that is not a board LED
/// or button is an ERROR: it would otherwise drop out of the comparison and
/// shrink the denominator without a word.
pub fn modelled(items: &Json) -> Result<Vec<Modelled>, String> {
    let mut out = Vec::new();
    let Json::Arr(pkgs) = items else { return Err("spar items: top level is not an array".into()) };
    for p in pkgs {
        let pkg = p.get("name").and_then(Json::str).ok_or("spar items: package without a name")?;
        for list in ["component_types", "component_impls"] {
            for c in p.get(list).map(Json::arr).unwrap_or(&[]) {
                if c.get("category").and_then(Json::str) != Some("device") {
                    continue;
                }
                let name = c.get("name").and_then(Json::str).ok_or("spar items: component without a name")?;
                let mut base = None;
                for prop in c.get("properties").map(Json::arr).unwrap_or(&[]) {
                    let pn = prop.get("name").and_then(Json::str).unwrap_or("");
                    if pn.rsplit("::").next() != Some("Base") {
                        continue;
                    }
                    let n = prop
                        .get("typed_value")
                        .and_then(|t| t.get("Integer"))
                        .map(Json::arr)
                        .and_then(|a| a.first())
                        .and_then(|v| if let Json::Num(n) = v { Some(*n) } else { None })
                        .ok_or_else(|| format!("{pkg}::{name}: Base is not an integer"))?;
                    if n < 0.0 || n.fract() != 0.0 || n > u32::MAX as f64 {
                        return Err(format!("{pkg}::{name}: Base {n} is not a 32-bit address"));
                    }
                    base = Some(n as u64);
                }
                match base {
                    Some(base) => out.push(Modelled {
                        package: pkg.to_string(),
                        name: name.to_string(),
                        base,
                        console: flag(c, "Console"),
                    }),
                    None if flag(c, "Board_Led") || flag(c, "Board_Button") => {}
                    None => {
                        return Err(format!(
                            "{pkg}::{name}: device has no Base and is not a board LED/button -- \
                             it would silently drop out of the comparison"
                        ))
                    }
                }
            }
        }
    }
    Ok(out)
}

/// `name<TAB>0xBASE` rows; `#` lines are comments.
pub fn parse_peripherals(tsv: &str) -> Result<BTreeMap<String, u64>, String> {
    let mut out = BTreeMap::new();
    for (i, line) in tsv.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (name, base) = line
            .split_once('\t')
            .ok_or_else(|| format!("line {}: expected `name<TAB>base`", i + 1))?;
        let hex = base.trim().strip_prefix("0x").ok_or_else(|| format!("line {}: base `{base}` is not 0x-hex", i + 1))?;
        let v = u64::from_str_radix(hex, 16).map_err(|_| format!("line {}: bad base `{base}`", i + 1))?;
        out.insert(name.trim().to_string(), v);
    }
    Ok(out)
}

/// The UART the board table's `console` row names (column 2).
pub fn board_console(tsv: &str) -> Option<String> {
    tsv.lines()
        .filter(|l| !l.trim_start().starts_with('#'))
        .map(|l| l.split('\t').collect::<Vec<_>>())
        .find(|f| f.first() == Some(&"console") && f.len() >= 2)
        .map(|f| f[1].trim().to_string())
}

fn reference_name(m: &Modelled, r: &Reference) -> Option<String> {
    if m.console {
        return r.console.clone();
    }
    let up = m.name.to_ascii_uppercase();
    Some(ALIASES.iter().find(|(g, _)| *g == up).map(|(_, x)| x.to_string()).unwrap_or(up))
}

/// The decision, separated from I/O for the self-test. `refs` is keyed by the
/// lower-cased package name (the table stem). Returns (compared, problems).
pub fn judge(models: &[Modelled], refs: &BTreeMap<String, Reference>) -> (usize, Vec<String>) {
    let mut bad = Vec::new();
    let mut checked = 0;
    let pkgs: BTreeSet<String> = models.iter().map(|m| m.package.to_ascii_lowercase()).collect();
    for m in models {
        let stem = m.package.to_ascii_lowercase();
        let Some(r) = refs.get(&stem) else {
            bad.push(format!("{}::{}: package has no targets/reference/{stem}.peripherals.tsv", m.package, m.name));
            continue;
        };
        checked += 1;
        let up = m.name.to_ascii_uppercase();
        if let Some(&(_, arch)) = CORE.iter().find(|(n, _)| !m.console && *n == up) {
            if m.base != arch {
                bad.push(format!(
                    "{}::{}: model base 0x{:08X} != architectural {up} 0x{arch:08X}",
                    m.package, m.name, m.base
                ));
            }
            continue;
        }
        let Some(rn) = reference_name(m, r) else {
            bad.push(format!("{}::{}: console device, but {stem}.board.tsv names no console UART", m.package, m.name));
            continue;
        };
        match r.peripherals.get(&rn) {
            None => bad.push(format!(
                "{}::{} (base 0x{:08X}): no reference peripheral `{rn}` in {stem}.peripherals.tsv",
                m.package, m.name, m.base
            )),
            Some(&b) if b != m.base => bad.push(format!(
                "{}::{}: model base 0x{:08X} != reference {rn} 0x{b:08X}",
                m.package, m.name, m.base
            )),
            Some(_) => {}
        }
    }
    for stem in refs.keys() {
        if !pkgs.contains(stem) {
            bad.push(format!("targets/reference/{stem}.peripherals.tsv has no modelled package -- a model vanished or was renamed"));
        }
    }
    (checked, bad)
}

// ---------------------------------------------------------------- sha256
// FIPS 180-4, dependency-free for the same cold-build reason as the JSON reader.

pub fn sha256_hex(data: &[u8]) -> String {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4, 0xab1c5ed5,
        0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe, 0x9bdc06a7, 0xc19bf174,
        0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f, 0x4a7484aa, 0x5cb0a9dc, 0x76f988da,
        0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7, 0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967,
        0x27b70a85, 0x2e1b2138, 0x4d2c6dfc, 0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85,
        0xa2bfe8a1, 0xa81a664b, 0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070,
        0x19a4c116, 0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7, 0xc67178f2,
    ];
    let mut h: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab, 0x5be0cd19,
    ];
    let mut msg = data.to_vec();
    let bits = (data.len() as u64).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bits.to_be_bytes());
    for chunk in msg.chunks(64) {
        let mut w = [0u32; 64];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([chunk[4 * i], chunk[4 * i + 1], chunk[4 * i + 2], chunk[4 * i + 3]]);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16].wrapping_add(s0).wrapping_add(w[i - 7]).wrapping_add(s1);
        }
        let mut v = h;
        for i in 0..64 {
            let s1 = v[4].rotate_right(6) ^ v[4].rotate_right(11) ^ v[4].rotate_right(25);
            let ch = (v[4] & v[5]) ^ (!v[4] & v[6]);
            let t1 = v[7].wrapping_add(s1).wrapping_add(ch).wrapping_add(K[i]).wrapping_add(w[i]);
            let s0 = v[0].rotate_right(2) ^ v[0].rotate_right(13) ^ v[0].rotate_right(22);
            let maj = (v[0] & v[1]) ^ (v[0] & v[2]) ^ (v[1] & v[2]);
            let t2 = s0.wrapping_add(maj);
            v = [t1.wrapping_add(t2), v[0], v[1], v[2], v[3].wrapping_add(t1), v[4], v[5], v[6]];
        }
        for (a, b) in h.iter_mut().zip(v) {
            *a = a.wrapping_add(b);
        }
    }
    h.iter().map(|x| format!("{x:08x}")).collect()
}

/// `<hex>  <name>` lines (sha256sum format); `#` lines are comments.
pub fn parse_sums(text: &str) -> Result<BTreeMap<String, String>, String> {
    let mut out = BTreeMap::new();
    for (i, l) in text.lines().enumerate() {
        let l = l.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        let (h, n) = l.split_once(char::is_whitespace).ok_or_else(|| format!("{SUMS} line {}: malformed", i + 1))?;
        if h.len() != 64 || !h.bytes().all(|c| c.is_ascii_hexdigit()) {
            return Err(format!("{SUMS} line {}: `{h}` is not a sha256", i + 1));
        }
        out.insert(n.trim().to_string(), h.to_ascii_lowercase());
    }
    Ok(out)
}

// ---------------------------------------------------------------- I/O

fn cmd(program: &str, path: Option<&OsStr>) -> Command {
    let mut c = Command::new(program);
    if let Some(p) = path {
        c.env("PATH", p);
    }
    c
}

/// The pinned spar, and ONLY the pinned one: `varve which spar` run in the repo.
/// No PATH fallback -- a bare `spar` can be a stale build (see aadl_parse.rs).
/// `path` overrides PATH for the lookup (the self-test's stubs); None inherits.
pub fn pinned_spar(repo: &Path, path: Option<&OsStr>) -> Result<(String, String), String> {
    let out = cmd("varve", path)
        .args(["which", "spar"])
        .current_dir(repo)
        .output()
        .map_err(|e| format!("cannot run `varve which spar` ({e}); install varve -- there is no fallback to a bare spar"))?;
    if !out.status.success() {
        return Err(format!(
            "`varve which spar` failed (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let bin = String::from_utf8_lossy(&out.stdout).trim().lines().last().unwrap_or("").trim().to_string();
    if bin.is_empty() || !Path::new(&bin).is_file() {
        return Err(format!("`varve which spar` gave {bin:?}, which is not a file"));
    }
    let v = cmd(&bin, path).arg("--version").output().map_err(|e| format!("{bin}: {e}"))?;
    let first = String::from_utf8_lossy(&v.stdout).lines().next().unwrap_or("").trim().to_string();
    if !v.status.success() || !first.starts_with("spar ") {
        return Err(format!("{bin} does not answer --version as a spar (got {first:?})"));
    }
    Ok((bin, first))
}

fn git_ls(repo: &Path, spec: &[&str]) -> Option<Vec<String>> {
    let out = Command::new("git").arg("-C").arg(repo).arg("ls-files").arg("--").args(spec).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect())
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    run_in(repo, None)
}

/// The gate proper. `path` replaces PATH for varve/spar (None: inherit).
pub fn run_in(repo: &Path, path: Option<&OsStr>) -> Verdict {
    let (spar, ver) = match pinned_spar(repo, path) {
        Ok(x) => x,
        Err(e) => return Verdict::Refused(e),
    };
    let Some(mut models) = git_ls(repo, &["targets/*.aadl"]) else {
        return Verdict::Refused("git ls-files failed -- the gate cannot answer".into());
    };
    models.retain(|m| !m["targets/".len()..].contains('/'));
    if models.is_empty() {
        return Verdict::Refused("no committed targets/*.aadl -- the check would be vacuous".into());
    }
    let out = match cmd(&spar, path).args(["items", "--format", "json"]).args(&models).current_dir(repo).output() {
        Ok(o) => o,
        Err(e) => return Verdict::Refused(format!("{spar}: {e}")),
    };
    if !out.status.success() {
        return Verdict::Fail(vec![format!(
            "spar items rejected the models (exit {:?}): {}",
            out.status.code(),
            String::from_utf8_lossy(&out.stderr).trim()
        )]);
    }
    // A committed fixture that is not spar's output for these models means
    // regen.sh was skipped or bypassed.
    let fixture_fresh = match std::fs::read(repo.join(FIXTURE)) {
        Ok(committed) => committed == out.stdout,
        Err(e) => return Verdict::Refused(format!("{FIXTURE}: {e}")),
    };
    let items = match parse_json(&String::from_utf8_lossy(&out.stdout)) {
        Ok(j) => j,
        Err(e) => return Verdict::Refused(format!("spar items output is not JSON this gate reads: {e}")),
    };
    let devices = match modelled(&items) {
        Ok(d) => d,
        Err(e) => return Verdict::Refused(e),
    };

    // Table integrity: every tracked table matches the digest its import
    // script recorded, and every recorded digest names a tracked table.
    let mut bad = Vec::new();
    let Some(all_tables) = git_ls(repo, &["targets/reference/*.tsv"]) else {
        return Verdict::Refused("git ls-files failed -- the gate cannot answer".into());
    };
    let sums = match std::fs::read_to_string(repo.join(SUMS)).map_err(|e| format!("{SUMS}: {e}")).and_then(|t| parse_sums(&t)) {
        Ok(s) => s,
        Err(e) => return Verdict::Refused(e),
    };
    let mut seen = BTreeSet::new();
    for t in &all_tables {
        let short = t.trim_start_matches("targets/reference/").to_string();
        let bytes = match std::fs::read(repo.join(t)) {
            Ok(b) => b,
            Err(e) => return Verdict::Refused(format!("{t}: {e}")),
        };
        match sums.get(&short) {
            None => bad.push(format!("{t}: no digest in {SUMS} -- tables come from the import scripts, which record one")),
            Some(want) if *want != sha256_hex(&bytes) => {
                bad.push(format!("{t}: sha256 differs from {SUMS} -- the table was edited by hand; regenerate it with its import script"))
            }
            Some(_) => {}
        }
        seen.insert(short);
    }
    for n in sums.keys().filter(|n| !seen.contains(*n)) {
        bad.push(format!("{SUMS} names {n}, which is not a tracked table -- remove the stale line"));
    }

    let Some(tables) = git_ls(repo, &["targets/reference/*.peripherals.tsv"]) else {
        return Verdict::Refused("git ls-files failed -- the gate cannot answer".into());
    };
    let mut refs = BTreeMap::new();
    for t in &tables {
        let stem = t.rsplit('/').next().unwrap_or(t).trim_end_matches(".peripherals.tsv").to_string();
        let text = match std::fs::read_to_string(repo.join(t)) {
            Ok(s) => s,
            Err(e) => return Verdict::Refused(format!("{t}: {e}")),
        };
        let peripherals = match parse_peripherals(&text) {
            Ok(p) if !p.is_empty() => p,
            Ok(_) => return Verdict::Refused(format!("{t}: no rows -- the check would be vacuous")),
            Err(e) => return Verdict::Refused(format!("{t}: {e}")),
        };
        let board = std::fs::read_to_string(repo.join(format!("targets/reference/{stem}.board.tsv"))).unwrap_or_default();
        refs.insert(stem, Reference { peripherals, console: board_console(&board) });
    }
    if refs.is_empty() {
        return Verdict::Refused("no targets/reference/*.peripherals.tsv -- the check would be vacuous".into());
    }

    let (checked, problems) = judge(&devices, &refs);
    bad.extend(problems);
    if !fixture_fresh {
        bad.push(format!(
            "{FIXTURE} is not what the pinned spar emits for targets/*.aadl -- run targets/regen.sh and commit the result"
        ));
    }
    if checked == 0 {
        return Verdict::Refused("no modelled peripheral had a base to compare -- the check would be vacuous".into());
    }
    let lines_head = vec![
        format!("{ver} ({spar})"),
        format!(
            "{} model(s); {} device(s) with a Base, {checked} compared ({} against the architectural constant); \
             {} reference table(s), {} digest(s) checked",
            models.len(),
            devices.len(),
            devices.iter().filter(|d| !d.console && CORE.iter().any(|(n, _)| *n == d.name.to_ascii_uppercase())).count(),
            refs.len(),
            all_tables.len()
        ),
    ];
    let mut lines = lines_head;
    if bad.is_empty() {
        lines.push("PASS".into());
        Verdict::Pass(lines)
    } else {
        lines.push(format!("FAIL: {} problem(s):", bad.len()));
        lines.extend(bad.into_iter().map(|b| format!("  {b}")));
        lines.push("Fix the model (targets/*.aadl), or re-import the reference with targets/reference/import.py.".into());
        Verdict::Fail(lines)
    }
}

// ---------------------------------------------------------------- self-test

/// A scratch directory that removes itself.
struct Scratch(std::path::PathBuf);

impl Scratch {
    fn new() -> std::io::Result<Self> {
        let mut p = std::env::temp_dir();
        p.push(format!(
            "gale-xtask-tref-{}-{}",
            std::process::id(),
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0)
        ));
        std::fs::create_dir_all(&p)?;
        Ok(Scratch(p))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// spar-items JSON for the stub repo. `dev` is a list of extra device JSON objects.
fn stub_items(devs: &[String]) -> String {
    format!(r#"[{{"name":"STM32X","component_types":[{}],"component_impls":[]}}]"#, devs.join(","))
}

fn dev(name: &str, base: Option<u64>, flags: &[&str]) -> String {
    let mut props: Vec<String> = Vec::new();
    if let Some(b) = base {
        props.push(format!(r#"{{"name":"Gust_Target_Props::Base","typed_value":{{"Integer":[{b},null]}}}}"#));
    }
    for f in flags {
        props.push(format!(r#"{{"name":"Gust_Target_Props::{f}","typed_value":{{"Boolean":true}}}}"#));
    }
    format!(r#"{{"name":"{name}","category":"device","properties":[{}]}}"#, props.join(","))
}

fn good_devs() -> Vec<String> {
    vec![
        dev("Iwdg", Some(0x4000_3000), &[]),
        dev("Board_Console", Some(0x4000_4400), &["Console"]),
        dev("Mpu", Some(0xE000_ED90), &[]),
        dev("Led_Ld3", None, &["Board_Led"]),
    ]
}

const GOOD_TABLE: &str = "# name\tbase\nIWDG\t0x40003000\nUSART2\t0x40004400\n";
const GOOD_BOARD: &str = "# kind\tname\tport|tx\tpin|rx\tactive\nconsole\tUSART2\tA2\tA3\t-\n";

/// The stub repo: one model, one SoC's tables with recorded digests, a fixture,
/// and stub varve/spar whose behaviour is steered by files under `ctl/`.
struct Stub {
    _dir: Scratch,
    root: std::path::PathBuf,
}

impl Stub {
    fn new() -> Result<Self, String> {
        let dir = Scratch::new().map_err(|e| e.to_string())?;
        let root = dir.0.clone();
        let s = Stub { _dir: dir, root };
        for d in ["repo", "bin", "bare", "empty", "ctl"] {
            std::fs::create_dir_all(s.root.join(d)).map_err(|e| e.to_string())?;
        }
        let ctl = s.root.join("ctl");
        let spar = s.root.join("bin/spar");
        let c = ctl.display();
        let spar_sh = format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then /bin/cat '{c}/version'; exit 0; fi\n/bin/cat '{c}/items'\nexit \"$(/bin/cat '{c}/rc')\"\n"
        );
        let varve_sh = format!(
            // On failure it STILL prints a usable path, so only the exit status
            // tells the gate the pin is unresolved.
            "#!/bin/sh\n[ \"$1\" = which ] && [ \"$2\" = spar ] && [ ! -e '{c}/varve-fails' ] && {{ echo '{s}'; exit 0; }}\necho '{s}'\necho 'error: pin not installed' >&2\nexit 1\n",
            s = spar.display()
        );
        s.write_exec("bin/spar", &spar_sh)?;
        s.write_exec("bin/varve", &varve_sh)?;
        // A bare spar on a PATH that has no varve: the gate must NOT use it.
        s.write_exec("bare/spar", &spar_sh)?;
        let st = Command::new("git").args(["init", "-q"]).current_dir(s.root.join("repo")).status();
        if !st.map(|x| x.success()).unwrap_or(false) {
            return Err("git init failed in the self-test scratch repo".into());
        }
        Ok(s)
    }

    fn write(&self, rel: &str, text: &str) -> Result<(), String> {
        let p = self.root.join(rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        std::fs::write(&p, text).map_err(|e| format!("{}: {e}", p.display()))
    }

    fn write_exec(&self, rel: &str, text: &str) -> Result<(), String> {
        self.write(rel, text)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            std::fs::set_permissions(self.root.join(rel), std::fs::Permissions::from_mode(0o755)).map_err(|e| e.to_string())?;
        }
        Ok(())
    }

    fn sums(tables: &[(&str, &str)]) -> String {
        let mut s = String::from("# test\n");
        for (n, t) in tables {
            s.push_str(&format!("{}  {n}\n", sha256_hex(t.as_bytes())));
        }
        s
    }

    /// Reset to the known-good state: spar emits `items`, the fixture equals it,
    /// the tables match their digests.
    fn reset(&self) -> Result<(), String> {
        let _ = std::fs::remove_dir_all(self.root.join("repo/targets"));
        let _ = std::fs::remove_dir_all(self.root.join("repo/tools"));
        let _ = std::fs::remove_file(self.root.join("ctl/varve-fails"));
        let items = stub_items(&good_devs());
        self.write("repo/targets/stm32x.aadl", "-- self-test model; the stub spar ignores it\n")?;
        self.write("repo/targets/reference/stm32x.peripherals.tsv", GOOD_TABLE)?;
        self.write("repo/targets/reference/stm32x.board.tsv", GOOD_BOARD)?;
        self.write(
            &format!("repo/{SUMS}"),
            &Self::sums(&[("stm32x.board.tsv", GOOD_BOARD), ("stm32x.peripherals.tsv", GOOD_TABLE)]),
        )?;
        self.write(&format!("repo/{FIXTURE}"), &items)?;
        self.write("ctl/items", &items)?;
        self.write("ctl/rc", "0\n")?;
        self.write("ctl/version", "spar 0.0.0-selftest\n")
    }

    /// Set what spar emits AND make the fixture agree, so only the property under
    /// test differs from the good state.
    fn emit(&self, items: &str) -> Result<(), String> {
        self.write("ctl/items", items)?;
        self.write(&format!("repo/{FIXTURE}"), items)
    }

    fn run(&self, path_dir: &str) -> Result<Verdict, String> {
        let st = Command::new("git").args(["add", "-A"]).current_dir(self.root.join("repo")).status();
        if !st.map(|x| x.success()).unwrap_or(false) {
            return Err("git add failed in the self-test scratch repo".into());
        }
        let path = self.root.join(path_dir);
        Ok(run_in(&self.root.join("repo"), Some(path.as_os_str())))
    }
}

fn is_pass(v: &Verdict) -> bool {
    matches!(v, Verdict::Pass(_))
}
fn is_fail(v: &Verdict) -> bool {
    matches!(v, Verdict::Fail(_))
}
fn is_refused(v: &Verdict) -> bool {
    matches!(v, Verdict::Refused(_))
}

pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut ok = true;
    let mut ck = |what: &str, got: bool| {
        ok &= got;
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
    };

    // ---- extraction and parsing
    let items = r#"[{"name":"STM32X","component_types":[
        {"name":"Iwdg","category":"device","properties":[
          {"name":"Gust_Target_Props::Base","typed_value":{"Integer":[1073754112,null]}},
          {"name":"Gust_Target_Props::Present","typed_value":{"Boolean":true}}]},
        {"name":"Led_Ld3","category":"device","properties":[
          {"name":"Gust_Target_Props::Board_Led","typed_value":{"Boolean":true}}]},
        {"name":"Board_Console","category":"device","properties":[
          {"name":"Gust_Target_Props::Base","typed_value":{"Integer":[1073759232,null]}},
          {"name":"Gust_Target_Props::Console","typed_value":{"Boolean":true}}]},
        {"name":"Ram","category":"memory","properties":[
          {"name":"Gust_Target_Props::Base","typed_value":{"Integer":[536870912,null]}}]}
      ],"component_impls":[
        {"name":"Spi1.i","category":"device","properties":[
          {"name":"Gust_Target_Props::Base","typed_value":{"Integer":[1073819648,null]}}]}]}]"#;
    let got = parse_json(items).and_then(|j| modelled(&j));
    ck(
        "extraction: 3 devices with a Base (one from component_impls), LED and memory skipped, console flagged",
        got.as_ref().map(|d| {
            d.len() == 3
                && d[0] == Modelled { package: "STM32X".into(), name: "Iwdg".into(), base: 0x4000_3000, console: false }
                && d[1].console
                && d[1].base == 0x4000_4400
                && d[2] == Modelled { package: "STM32X".into(), name: "Spi1.i".into(), base: 0x4001_3000, console: false }
        }) == Ok(true),
    );
    let one = |props: &str| parse_json(&format!(r#"[{{"name":"P","component_types":[{{"name":"D","category":"device","properties":[{props}]}}]}}]"#)).and_then(|j| modelled(&j));
    ck("a non-integer Base is an error", one(r#"{"name":"X::Base","typed_value":{"String":"0x40003000"}}"#).is_err());
    ck("a fractional Base is an error", one(r#"{"name":"X::Base","typed_value":{"Integer":[1.5,null]}}"#).is_err());
    ck("a device with no Base (not LED/button) is an error", one(r#"{"name":"X::Present","typed_value":{"Boolean":true}}"#).is_err());
    ck("a board button with no Base is fine", one(r#"{"name":"X::Board_Button","typed_value":{"Boolean":true}}"#) == Ok(vec![]));
    ck("malformed JSON is an error, not an empty model", parse_json("[{\"name\":").is_err());
    ck("trailing bytes after the JSON are an error", parse_json("[] x").is_err());
    ck("escaped string parsed", parse_json("\"a\\\"b\\u0041\"") == Ok(Json::Str("a\"bA".into())));
    ck("reference row parsed", parse_peripherals("# x\nIWDG\t0x40003000\n").map(|p| p.get("IWDG") == Some(&0x4000_3000)) == Ok(true));
    ck("a reference row without a tab is an error", parse_peripherals("IWDG 0x40003000\n").is_err());
    ck("a reference row with a non-hex base is an error", parse_peripherals("IWDG\t40003000\n").is_err());
    ck("board table console row parsed", board_console("# c\nled\tld3\tC\t9\thigh\nconsole\tUSART2\tA2\tA3\t-\n") == Some("USART2".into()));
    ck("sha256(\"\") vector", sha256_hex(b"") == "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855");
    ck("sha256(\"abc\") vector", sha256_hex(b"abc") == "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
    ck(
        "sha256 of a 2-block message",
        sha256_hex(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")
            == "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
    );

    // ---- the decision
    let m = |n: &str, b: u64| Modelled { package: "STM32X".into(), name: n.into(), base: b, console: false };
    let mut refs = BTreeMap::new();
    refs.insert(
        "stm32x".to_string(),
        Reference {
            peripherals: [("IWDG".to_string(), 0x4000_3000u64), ("ADC1".into(), 0x4001_2400), ("USART2".into(), 0x4000_4400)]
                .into_iter()
                .collect(),
            console: Some("USART2".into()),
        },
    );
    let j = |ms: &[Modelled]| judge(ms, &refs);
    ck("matching base passes", j(&[m("Iwdg", 0x4000_3000)]) == (1, vec![]));
    ck("base shifted UP by 0x1000 fails", j(&[m("Iwdg", 0x4000_4000)]).1.len() == 1);
    ck("base shifted DOWN by 0x1000 fails", j(&[m("Iwdg", 0x4000_2000)]).1.len() == 1);
    ck("modelled peripheral missing from the reference fails", j(&[m("Spi9", 0x4000_3800)]).1.len() == 1);
    ck("RCC (a real SoC peripheral) is not exempt: missing from the reference fails", j(&[m("Rcc", 0x4002_1000)]).1.len() == 1);
    ck("alias ADC -> ADC1 compared", j(&[m("Adc", 0x4001_2400)]) == (1, vec![]));
    ck("alias with a wrong base fails", j(&[m("Adc", 0x4001_2800)]).1.len() == 1);
    let con = |b: u64| Modelled { console: true, ..m("Board_Console", b) };
    ck("console compared via the board table's UART", j(&[con(0x4000_4400)]) == (1, vec![]));
    ck("console at the wrong UART base fails", j(&[con(0x4001_3800)]).1.len() == 1);
    ck("MPU at its architectural address passes, and is counted", j(&[m("Mpu", 0xE000_ED90)]) == (1, vec![]));
    ck("MPU moved to 0xE000ED00 fails", j(&[m("Mpu", 0xE000_ED00)]).1.len() == 1);
    ck("an unknown PPB device is NOT exempt", j(&[m("Dwt", 0xE000_1000)]).1.len() == 1);
    let other = Modelled { package: "STM32Y".into(), ..m("Iwdg", 0x4000_3000) };
    ck("package without a reference table fails", j(&[m("Iwdg", 0x4000_3000), other]).1.len() == 1);
    ck("reference table without a package fails", j(&[]).1.len() == 1);

    // ---- run() end to end, against a stub repo with stub varve and spar
    let stub = match Stub::new() {
        Ok(s) => s,
        Err(e) => return Verdict::Refused(format!("self-test could not build its stub repo: {e}")),
    };
    type Case<'a> = (&'a str, &'a str, fn(&Verdict) -> bool, Box<dyn Fn(&Stub) -> Result<(), String> + 'a>);
    let cases: Vec<Case> = vec![
        ("positive control: the good stub repo passes", "bin", is_pass, Box::new(|_| Ok(()))),
        ("varve missing from PATH is refused", "empty", is_refused, Box::new(|_| Ok(()))),
        ("a bare spar on PATH without varve is refused, not used", "bare", is_refused, Box::new(|_| Ok(()))),
        ("`varve which spar` failing is refused", "bin", is_refused, Box::new(|s| s.write("ctl/varve-fails", ""))),
        ("a spar that does not answer --version as spar is refused", "bin", is_refused, Box::new(|s| s.write("ctl/version", "Unknown command: --version\n"))),
        ("spar items exiting non-zero fails", "bin", is_fail, Box::new(|s| s.write("ctl/rc", "1\n"))),
        ("empty spar output is refused", "bin", is_refused, Box::new(|s| s.emit(""))),
        ("spar output `[]` is refused (nothing compared)", "bin", is_refused, Box::new(|s| s.emit("[]"))),
        ("only a base-less LED is refused (nothing compared)", "bin", is_refused, Box::new(|s| s.emit(&stub_items(&[dev("Led_Ld3", None, &["Board_Led"])])))),
        ("a device that lost its Base is refused", "bin", is_refused, Box::new(|s| {
            let mut d = good_devs();
            d.push(dev("Rcc", None, &[]));
            s.emit(&stub_items(&d))
        })),
        ("a stale committed fixture fails", "bin", is_fail, Box::new(|s| s.write(&format!("repo/{FIXTURE}"), "[]"))),
        ("model IWDG 0x40004000 fails", "bin", is_fail, Box::new(|s| {
            let mut d = good_devs();
            d[0] = dev("Iwdg", Some(0x4000_4000), &[]);
            s.emit(&stub_items(&d))
        })),
        ("model Mpu 0xE000ED00 fails", "bin", is_fail, Box::new(|s| {
            let mut d = good_devs();
            d[2] = dev("Mpu", Some(0xE000_ED00), &[]);
            s.emit(&stub_items(&d))
        })),
        ("model AND table both moved to 0x40004000 fails on the table digest", "bin", is_fail, Box::new(|s| {
            let mut d = good_devs();
            d[0] = dev("Iwdg", Some(0x4000_4000), &[]);
            s.emit(&stub_items(&d))?;
            s.write("repo/targets/reference/stm32x.peripherals.tsv", &GOOD_TABLE.replace("40003000", "40004000"))
        })),
        ("a table with no recorded digest fails", "bin", is_fail, Box::new(|s| s.write("repo/targets/reference/stm32x.pins.tsv", "# x\n"))),
        ("a recorded digest naming no table fails", "bin", is_fail, Box::new(|s| {
            s.write(
                &format!("repo/{SUMS}"),
                &Stub::sums(&[("stm32x.board.tsv", GOOD_BOARD), ("stm32x.peripherals.tsv", GOOD_TABLE), ("gone.tsv", "")]),
            )
        })),
        ("a table with no rows is refused", "bin", is_refused, Box::new(|s| {
            let t = "# name\tbase\n";
            s.write("repo/targets/reference/stm32x.peripherals.tsv", t)?;
            s.write(&format!("repo/{SUMS}"), &Stub::sums(&[("stm32x.board.tsv", GOOD_BOARD), ("stm32x.peripherals.tsv", t)]))
        })),
        ("no peripheral tables at all is refused", "bin", is_refused, Box::new(|s| {
            std::fs::remove_file(s.root.join("repo/targets/reference/stm32x.peripherals.tsv")).map_err(|e| e.to_string())?;
            s.write(&format!("repo/{SUMS}"), &Stub::sums(&[("stm32x.board.tsv", GOOD_BOARD)]))
        })),
        ("a missing SHA256SUMS is refused", "bin", is_refused, Box::new(|s| {
            std::fs::remove_file(s.root.join(format!("repo/{SUMS}"))).map_err(|e| e.to_string())
        })),
        ("a missing committed fixture is refused", "bin", is_refused, Box::new(|s| {
            std::fs::remove_file(s.root.join(format!("repo/{FIXTURE}"))).map_err(|e| e.to_string())
        })),
        ("no committed model is refused", "bin", is_refused, Box::new(|s| {
            std::fs::remove_file(s.root.join("repo/targets/stm32x.aadl")).map_err(|e| e.to_string())
        })),
    ];
    for (what, path, expect, setup) in &cases {
        let got = stub.reset().and_then(|_| setup(&stub)).and_then(|_| stub.run(path));
        match got {
            Ok(v) => {
                let good = expect(&v);
                let label = if good { what.to_string() } else { format!("{what} (got: {})", v.to_string().replace('\n', " | ")) };
                ck(&label, good);
            }
            Err(e) => ck(&format!("{what} [setup error: {e}]"), false),
        }
    }

    if ok {
        lines.push("PASS".into());
        Verdict::Pass(lines)
    } else {
        lines.push("FAIL".into());
        Verdict::Fail(lines)
    }
}

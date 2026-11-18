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
//! in the repo so varve.toml selects the layer). Not the committed spar-items
//! fixture under tools/gust-target-gen/tests/golden: that fixture is a copy of
//! spar's output, and a gate that read the copy would pass a model edit that
//! never reached it -- the defect this gate was written alongside.
//!
//! Rule, per SoC package `STM32X` with table `targets/reference/stm32x.peripherals.tsv`:
//!   * every `device` classifier carrying `Gust_Target_Props::Base` is checked;
//!   * its reference name is the classifier upper-cased (`Adc1_Common` ->
//!     `ADC1_COMMON`), mapped through `ALIASES`; a `Console => true` device
//!     takes the UART the board table (`<soc>.board.tsv`, kind `console`) names;
//!   * the name MUST be in the reference and the base MUST equal it. A modelled
//!     peripheral missing from the reference fails; only the Cortex-M core
//!     peripherals in `CORE` (PPB, not listed by a SoC table) are exempt, by name;
//!   * a SoC package without a table fails, and so does a table without a package.
//!
//! It also requires the committed spar-items JSON (`FIXTURE`) to be byte-equal
//! to the pinned spar's output, so a regen that skipped the models is caught
//! for every fact in the model, not only for bases.
//!
//! Blind spots, stated: only BASES are compared (the tables hold nothing else
//! today), so a wrong register offset, bit position, length or presence flag is
//! not seen; Board_Led/Board_Button devices carry no Base and are checked by the
//! generator's --board-reference, not here; and the reference itself is trusted
//! -- a wrong row upstream makes a matching model wrong in the same way.

use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "target-reference";

/// gale's device name -> the reference's peripheral name, where they differ.
/// Same list as gust-target-gen's reference.rs; a rename on either side fails.
const ALIASES: &[(&str, &str)] = &[("ADC", "ADC1")];

/// Cortex-M core peripherals: they live in the PPB and no SoC table lists them.
/// Exempt BY NAME only, so a mis-named SoC peripheral cannot hide here.
const CORE: &[&str] = &["MPU"];

/// The committed copy of spar's output that gust-target-gen's tests and the
/// generated tree are built from. It must equal what the pinned spar produces
/// from the models NOW, or every other fact in it (offsets, bits, memories) can
/// drift from the .aadl unseen -- the base comparison alone covers only bases.
const FIXTURE: &str = "tools/gust-target-gen/tests/golden/f100.items.json";

/// One modelled device: (package, classifier, base, is_console).
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

/// Every `device` classifier (type or impl) carrying a Base, from spar's
/// `items --format json` output.
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
                let mut console = false;
                for prop in c.get("properties").map(Json::arr).unwrap_or(&[]) {
                    let pn = prop.get("name").and_then(Json::str).unwrap_or("");
                    let key = pn.rsplit("::").next().unwrap_or(pn);
                    let tv = prop.get("typed_value");
                    match key {
                        "Base" => {
                            let n = tv
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
                        "Console" => {
                            console = tv.and_then(|t| t.get("Boolean")) == Some(&Json::Bool(true));
                        }
                        _ => {}
                    }
                }
                if let Some(base) = base {
                    out.push(Modelled { package: pkg.to_string(), name: name.to_string(), base, console });
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
/// lower-cased package name (the table stem). Returns (checked, problems).
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
        if !m.console && CORE.contains(&m.name.to_ascii_uppercase().as_str()) {
            continue;
        }
        checked += 1;
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

// ---------------------------------------------------------------- I/O

/// The pinned spar, and ONLY the pinned one: `varve which spar` run in the repo.
/// No PATH fallback -- a bare `spar` can be a stale build (see aadl_parse.rs).
pub fn pinned_spar(repo: &Path) -> Result<(String, String), String> {
    let out = Command::new("varve")
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
    let v = Command::new(&bin).arg("--version").output().map_err(|e| format!("{bin}: {e}"))?;
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
    let (spar, ver) = match pinned_spar(repo) {
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
    let out = match Command::new(&spar).args(["items", "--format", "json"]).args(&models).current_dir(repo).output() {
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
    // Freshness first: a committed fixture that is not spar's output for these
    // models means regen.sh was skipped or bypassed.
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

    let (checked, mut bad) = judge(&devices, &refs);
    if !fixture_fresh {
        bad.push(format!(
            "{FIXTURE} is not what the pinned spar emits for targets/*.aadl -- run targets/regen.sh and commit the result"
        ));
    }
    if checked == 0 {
        return Verdict::Refused("no modelled peripheral had a base to compare -- the check would be vacuous".into());
    }
    let mut lines = vec![
        format!("{ver} ({spar})"),
        format!(
            "{} model(s), {} device(s) with a Base, {checked} compared against {} reference table(s); {} core-peripheral exemption(s) by name",
            models.len(),
            devices.len(),
            refs.len(),
            devices.len() - checked
        ),
    ];
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

pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut ok = true;
    let mut ck = |what: &str, got: bool| {
        ok &= got;
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
    };

    // A spar-shaped fragment: the extraction must find exactly the devices with a Base.
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
      ],"component_impls":[]}]"#;
    let got = parse_json(items).and_then(|j| modelled(&j));
    ck(
        "extraction: 2 devices with a Base, LED and memory skipped, console flagged",
        got.as_ref().map(|d| {
            d.len() == 2
                && d[0] == Modelled { package: "STM32X".into(), name: "Iwdg".into(), base: 0x4000_3000, console: false }
                && d[1].console
                && d[1].base == 0x4000_4400
        }) == Ok(true),
    );
    ck("malformed JSON is an error, not an empty model", parse_json("[{\"name\":").is_err());
    ck("escaped string parsed", parse_json(r#""a\"bA""#) == Ok(Json::Str("a\"bA".into())));

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
    // The defect this gate exists for: the G031 IWDG moved by 0x1000.
    ck("base shifted by 0x1000 fails", j(&[m("Iwdg", 0x4000_4000)]).1.len() == 1);
    ck("modelled peripheral missing from the reference fails", j(&[m("Spi9", 0x4000_3800)]).1.len() == 1);
    ck("alias ADC -> ADC1 compared", j(&[m("Adc", 0x4001_2400)]) == (1, vec![]));
    ck("alias with a wrong base fails", j(&[m("Adc", 0x4001_2800)]).1.len() == 1);
    let con = |b: u64| Modelled { console: true, ..m("Board_Console", b) };
    ck("console compared via the board table's UART", j(&[con(0x4000_4400)]) == (1, vec![]));
    ck("console at the wrong UART base fails", j(&[con(0x4001_3800)]).1.len() == 1);
    ck("core peripheral exempt by name", j(&[m("Mpu", 0xE000_ED90)]) == (0, vec![]));
    ck("an unknown PPB device is NOT exempt", j(&[m("Dwt", 0xE000_1000)]).1.len() == 1);
    let other = Modelled { package: "STM32Y".into(), ..m("Iwdg", 0x4000_3000) };
    ck("package without a reference table fails", j(&[m("Iwdg", 0x4000_3000), other]).1.len() == 1);
    ck("reference table without a package fails", j(&[]).1.len() == 1);
    ck("board table console row parsed", board_console("# c\nled\tld3\tC\t9\thigh\nconsole\tUSART2\tA2\tA3\t-\n") == Some("USART2".into()));
    ck("reference row parsed", parse_peripherals("# x\nIWDG\t0x40003000\n").map(|p| p.get("IWDG") == Some(&0x4000_3000)) == Ok(true));

    if ok {
        lines.push("PASS".into());
        Verdict::Pass(lines)
    } else {
        lines.push("FAIL".into());
        Verdict::Fail(lines)
    }
}

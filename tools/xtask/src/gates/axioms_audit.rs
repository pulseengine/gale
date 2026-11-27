//! The axiom audit from outside the file (REQ-HONESTY-AXIOMS-001,
//! docs/design/axioms-trusted-audit.md).
//!
//! L11 (rounds 1-10) forged every audit that ran INSIDE an audited file
//! (`Print Assumptions`, `#print axioms`) or derived its theorem list by lexing
//! one. This gate reads the compiled objects instead:
//!
//! - **Rocq (R1).** `bazel build //proofs:axioms_audit` writes a manifest of
//!   the 13 compiled libraries, their load paths and the pinned toolchain's
//!   `rocqchk`. The gate runs `rocqchk -o -silent <theory flags> -Q ... <lib>`
//!   per library. rocqchk re-checks the library and everything it loads in the
//!   kernel and prints the library's whole assumption context: the theory
//!   (Set predicative or impredicative, rewrite rules), every axiom -- an
//!   `Admitted` lemma is one, and so is a primitive (`PrimInt63`) -- and the
//!   constants relying on type-in-type, on unsafe (guard-unchecked)
//!   fixpoints, and inductives whose positivity is assumed. It is per LIBRARY,
//!   not per theorem: it over-approximates, which is the safe direction.
//! - **Lean.** `bazel build //proofs/lean:axioms_audit` writes a manifest of
//!   the 8 modules' `.olean` files and search path; the gate refuses a module
//!   whose root name the toolchain, Mathlib or another search-path entry also
//!   provides, then runs the trusted program `tools/xtask/axioms/AxiomAudit.lean`
//!   (it imports only `Lean`) with the toolchain's own `lean --run` and NO
//!   `LEAN_*` variable, so the audited oleans cannot shadow its `import Lean`;
//!   the search path and the module's manifest `.olean` are arguments, one
//!   `lean` process per module (in one process each module's imports were a
//!   fresh copy of Mathlib: 18.5 GB for the 8, measured). The program checks
//!   the module resolves to that `.olean`, replays its constants through the
//!   kernel and runs `collectAxioms` on every one.
//!
//! The result is held equal to `tools/xtask/axioms-audit.ledger`. Two bridge
//! checks tie it to the trust ledger's spelling counts: per Rocq file, the
//! axioms rocqchk names under the library's own module path equal its
//! `Admitted` + `Axiom` + `Parameter` + top-level `Variable` spellings; per
//! Lean module, the axiom-kind constants equal its `axiom` spellings. A
//! mismatch means one side missed something.
//!
//! Exit codes: 1 when an assumption differs from the ledger (or a library does
//! not check); 3 when the gate could not answer -- Bazel or a checker missing,
//! a manifest or a checker output that is empty, truncated or not in the
//! expected shape. A vacuous run is never a pass.

use crate::gates::trust_ledger::{self, is_artifact_id};
use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const NAME: &str = "axioms-audit";
pub const LEDGER: &str = "tools/xtask/axioms-audit.ledger";
pub const PROGRAM: &str = "tools/xtask/axioms/AxiomAudit.lean";
const ROCQ_TARGET: &str = "//proofs:axioms_audit";
const LEAN_TARGET: &str = "//proofs/lean:axioms_audit";
const ROCQ_MANIFEST: &str = "proofs/axioms_audit.manifest";
const LEAN_MANIFEST: &str = "proofs/lean/axioms_audit.manifest";
/// The theory flags the checker may be given (from each library's own
/// `extra_flags`). Anything else is refused: a flag the gate does not model
/// could change what the checker accepts.
const THEORY_FLAGS: &[&str] = &["-impredicative-set"];
/// The common axiom set the seeded ledger names once instead of per library.
const BASE_SET: &str = "rocqofrust-base";

pub const CANNOT_SEE: &[&str] = &[
    "which theorem uses which axiom (Rocq): rocqchk reports per LIBRARY, everything the library loads included -- an over-approximation; a per-theorem `Print Assumptions` driver (R2) is not built yet",
    "a Section `Hypothesis`/`Variable`: discharged at `End`, it becomes a premise, not an axiom (trust-ledger's rocq:Section-Variable spelling class is the only check)",
    "weak or vacuous statements, and premises a lemma states itself: an axiom-free theorem can still prove little",
    "the kernels themselves (Rocq's, Lean's), the VM / native compute they trust, and the Rocq plugins coqc ran (rocqchk loads none; their effect is only what reached the .vo)",
    "Lean `example` declarations (they leave no constant), and unsafe or partial constants (listed and counted as `unreplayed`, but `Environment.replay` does not send them to the kernel)",
    "the imported Lean modules (Mathlib, Lean core): loaded as compiled, not replayed -- their constants are trusted; the same for Rocq's Stdlib/Corelib objects only as far as rocqchk does not re-check them (it does re-check every library it loads)",
    "Verus: vstd's axioms are not read (trust-ledger counts Verus assumption spellings; nothing reads vstd)",
    "whether the theory flags passed to rocqchk are the ones the library NEEDS: they are the library's own Bazel `extra_flags`; rocqchk refuses an impredicative-Set library checked without the flag (exit 129), and prints the mode it ran in",
    "proof files not tracked by git, and libraries not built by the two Bazel audit targets (the gate holds the targets' sources equal to the tracked proof files)",
    "which .vo rocqchk checked: it gets the logical name and -Q paths, not a .vo path, so a same-named .vo on an earlier -Q mapping would be checked instead (the Lean side compares the resolved olean with the manifest; the Rocq side does not)",
];

// ── manifests ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, PartialEq)]
pub struct RocqLib {
    pub label: String,
    pub src: String,
    pub module: String,
    pub flags: Vec<String>,
    pub q: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct RocqManifest {
    pub checker: String,
    pub libs: Vec<RocqLib>,
}

fn fields(line: &str) -> Vec<&str> {
    line.split('\t').collect()
}

pub fn parse_rocq_manifest(text: &str) -> Result<RocqManifest, String> {
    let mut checker = None;
    let mut libs: Vec<RocqLib> = Vec::new();
    for (n, l) in text.lines().enumerate() {
        if l.is_empty() {
            continue;
        }
        let f = fields(l);
        match f.as_slice() {
            ["checker", p] if checker.is_none() => checker = Some(p.to_string()),
            ["lib", label, src, module, flags] => {
                let flags: Vec<String> = flags.split(',').filter(|x| !x.is_empty()).map(str::to_string).collect();
                if let Some(bad) = flags.iter().find(|x| !THEORY_FLAGS.contains(&x.as_str())) {
                    return Err(format!("rocq manifest line {}: {label} is built with `{bad}`, which the gate does not model (allowed: {})", n + 1, THEORY_FLAGS.join(", ")));
                }
                if libs.iter().any(|x| x.module == *module) {
                    return Err(format!("rocq manifest line {}: module {module} listed twice", n + 1));
                }
                libs.push(RocqLib { label: label.to_string(), src: src.to_string(), module: module.to_string(), flags, q: Vec::new() });
            }
            ["q", label, phys, log] => match libs.last_mut() {
                Some(lib) if lib.label == *label => lib.q.push((phys.to_string(), log.to_string())),
                _ => return Err(format!("rocq manifest line {}: a load path for {label} outside its `lib` block", n + 1)),
            },
            _ => return Err(format!("rocq manifest line {}: not a `checker`/`lib`/`q` record: {l:?}", n + 1)),
        }
    }
    let checker = checker.ok_or("rocq manifest: no `checker` line")?;
    if libs.is_empty() {
        return Err("rocq manifest lists no library -- an empty audit is not a clean one".into());
    }
    if let Some(l) = libs.iter().find(|l| l.q.is_empty()) {
        return Err(format!("rocq manifest: {} has no load path", l.module));
    }
    Ok(RocqManifest { checker, libs })
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeanMod {
    pub label: String,
    pub src: String,
    pub module: String,
    pub olean: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LeanManifest {
    pub lean: String,
    pub paths: Vec<String>,
    pub mods: Vec<LeanMod>,
}

pub fn parse_lean_manifest(text: &str) -> Result<LeanManifest, String> {
    let mut lean = None;
    let mut paths = Vec::new();
    let mut mods: Vec<LeanMod> = Vec::new();
    for (n, l) in text.lines().enumerate() {
        if l.is_empty() {
            continue;
        }
        match fields(l).as_slice() {
            ["lean", p] if lean.is_none() => lean = Some(p.to_string()),
            ["path", p] => paths.push(p.to_string()),
            ["mod", label, src, module, olean] => {
                if mods.iter().any(|m| m.module == *module) {
                    return Err(format!("lean manifest line {}: module {module} listed twice", n + 1));
                }
                mods.push(LeanMod { label: label.to_string(), src: src.to_string(), module: module.to_string(), olean: olean.to_string() })
            }
            _ => return Err(format!("lean manifest line {}: not a `lean`/`path`/`mod` record: {l:?}", n + 1)),
        }
    }
    let lean = lean.ok_or("lean manifest: no `lean` line")?;
    if mods.is_empty() {
        return Err("lean manifest lists no module -- an empty audit is not a clean one".into());
    }
    if paths.is_empty() {
        return Err("lean manifest: no LEAN_PATH entry".into());
    }
    Ok(LeanManifest { lean, paths, mods })
}

/// Roots the Lean toolchain owns whatever its `lib/lean` lists.
pub const RESERVED_LEAN_ROOTS: &[&str] = &["Lean", "Init", "Std", "Lake"];

/// The top-level module roots a search-path directory provides, the way
/// `findOLean` sees them: a subdirectory `R/` or a file `R.olean`.
pub fn dir_roots(dir: &Path) -> Result<BTreeSet<String>, String> {
    let rd = std::fs::read_dir(dir).map_err(|e| format!("cannot list the Lean search-path entry {}: {e}", dir.display()))?;
    let mut roots = BTreeSet::new();
    for e in rd {
        let e = e.map_err(|e| format!("cannot list {}: {e}", dir.display()))?;
        let name = e.file_name().to_string_lossy().into_owned();
        if e.path().is_dir() {
            roots.insert(name);
        } else if let Some(stem) = name.strip_suffix(".olean") {
            roots.insert(stem.to_string());
        }
    }
    Ok(roots)
}

/// Every audited module must be the only provider of its root name: not a
/// toolchain root (`toolchain` = the roots of the pinned `lib/lean`, plus
/// RESERVED_LEAN_ROOTS), not a root any OTHER search-path entry provides
/// (`path_roots[i]` = the roots of `lm.paths[i]`, Mathlib's lib dir among
/// them), and its manifest `.olean` must be `<entry>/<A/B/C>.olean` for one
/// entry. `findOLean` resolves by root, first entry wins: a shared root means
/// another library's object would be audited in the module's place (F2), and
/// a `Lean` module would shadow the trusted program's own import (F1). The
/// trusted program re-checks the resolved path at run time. Any violation is
/// an input the gate cannot audit soundly, so it is a refusal (exit 3), like
/// an unmodelled Rocq theory flag.
pub fn lean_root_conflicts(lm: &LeanManifest, toolchain: &BTreeSet<String>, path_roots: &[BTreeSet<String>]) -> Vec<String> {
    let mut fails = Vec::new();
    for m in &lm.mods {
        let root = m.module.split('.').next().unwrap_or("");
        if RESERVED_LEAN_ROOTS.contains(&root) || toolchain.contains(root) {
            fails.push(format!("lean module {}: its root `{root}` belongs to the Lean toolchain -- refusing to audit it", m.module));
            continue;
        }
        let rel: PathBuf = m.module.split('.').collect::<PathBuf>().with_extension("olean");
        let Some(own) = lm.paths.iter().position(|p| Path::new(p).join(&rel) == Path::new(&m.olean)) else {
            fails.push(format!("lean module {}: its olean {} is not <entry>/{} for any search-path entry", m.module, m.olean, rel.display()));
            continue;
        };
        for (j, roots) in path_roots.iter().enumerate() {
            if j != own && roots.contains(root) {
                fails.push(format!(
                    "lean module {}: its root `{root}` is also provided by search-path entry {} -- `findOLean` would resolve {} there or here by order, refusing",
                    m.module, lm.paths.get(j).map(String::as_str).unwrap_or("?"), m.module
                ));
            }
        }
    }
    fails
}

// ── checker outputs ────────────────────────────────────────────────────────

/// What rocqchk -o says about one library.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RocqReport {
    pub impredicative: bool,
    pub rewrite_rules: bool,
    pub axioms: BTreeSet<String>,
    /// (kind, name): kind is type-in-type | unsafe-fixpoint | positivity.
    pub bypass: BTreeSet<(String, String)>,
}

const ROCQ_SECTIONS: &[(&str, &str)] = &[
    ("Axioms:", "axiom"),
    ("Constants/Inductives relying on type-in-type:", "type-in-type"),
    ("Constants/Inductives relying on unsafe (co)fixpoints:", "unsafe-fixpoint"),
    ("Inductives whose positivity is assumed:", "positivity"),
];

/// Parse `rocqchk -o -silent` output (Rocq 9.0.1, measured in the spike). The
/// shape is exact: the `CONTEXT SUMMARY` header, the two theory lines and the
/// four lists, each exactly once. Anything else -- a missing or repeated
/// section, an unknown line, an empty output -- is an error (Refused), never
/// read as "no axioms".
pub fn parse_rocqchk(text: &str) -> Result<RocqReport, String> {
    let mut r = RocqReport::default();
    let mut lines = text.lines().map(str::trim_end).filter(|l| !l.trim().is_empty()).peekable();
    if lines.next() != Some("CONTEXT SUMMARY") || lines.next() != Some("===============") {
        return Err("no `CONTEXT SUMMARY` header (rocqchk did not finish, or this is not rocqchk -o output)".into());
    }
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let mut current: Option<&str> = None;
    for l in lines {
        if let Some(rest) = l.strip_prefix("* ") {
            current = None;
            let key = match rest {
                "Theory: Set is predicative" => "set",
                "Theory: Set is impredicative" => {
                    r.impredicative = true;
                    "set"
                }
                "Theory: Rewrite rules are not allowed" => "rewrite",
                "Theory: Rewrite rules are allowed" => {
                    r.rewrite_rules = true;
                    "rewrite"
                }
                _ => {
                    let Some((head, kind)) = ROCQ_SECTIONS.iter().find(|(h, _)| rest.starts_with(h)) else {
                        return Err(format!("unknown rocqchk section: {l:?}"));
                    };
                    let tail = rest[head.len()..].trim();
                    match tail {
                        "<none>" => {}
                        "" => current = Some(kind),
                        _ => return Err(format!("unexpected text after `{head}`: {tail:?}")),
                    }
                    kind
                }
            };
            if !seen.insert(key) {
                return Err(format!("rocqchk section repeated: {l:?}"));
            }
            continue;
        }
        let Some(name) = l.strip_prefix("    ") else {
            return Err(format!("unexpected rocqchk line: {l:?}"));
        };
        let name = name.trim();
        if name.is_empty() || name.contains(char::is_whitespace) {
            return Err(format!("not a qualified name: {l:?}"));
        }
        match current {
            Some("axiom") => {
                r.axioms.insert(name.to_string());
            }
            Some(kind) => {
                r.bypass.insert((kind.to_string(), name.to_string()));
            }
            None => return Err(format!("a name outside any list: {l:?}")),
        }
    }
    for key in ["set", "rewrite", "axiom", "type-in-type", "unsafe-fixpoint", "positivity"] {
        if !seen.contains(key) {
            return Err(format!("rocqchk output lacks the `{key}` section (truncated?)"));
        }
    }
    Ok(r)
}

fn unhex(s: &str) -> Result<String, String> {
    if s.len() % 2 != 0 || !s.bytes().all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase()) {
        return Err(format!("not lowercase hex: {s:?}"));
    }
    let bytes: Vec<u8> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap_or(0)).collect();
    String::from_utf8(bytes).map_err(|_| format!("hex {s:?} is not UTF-8"))
}

/// A private Lean name embeds the path the file was compiled at
/// (`_private.«bazel-out».«darwin_arm64-fastbuild».bin.proofs.lean.x_src.M.0.foo`
/// under rules_lean, measured): it differs per host. Keep the part after the
/// first `.0.` as `private:foo`.
pub fn normalize_lean_name(n: &str) -> String {
    match n.strip_prefix("_private.").and_then(|r| r.split_once(".0.")) {
        Some((_, rest)) => format!("private:{rest}"),
        None => n.to_string(),
    }
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct LeanReport {
    pub constants: usize,
    pub theorems: usize,
    /// unsafe or partial: listed, not replayed through the kernel.
    pub unreplayed: usize,
    /// constants of kind `axiom` declared in the module itself.
    pub declared_axioms: usize,
    /// axiom -> how many of the module's constants depend on it.
    pub axioms: BTreeMap<String, usize>,
}

pub enum LeanRun {
    Ok(BTreeMap<String, LeanReport>),
    /// The kernel rejected a replayed constant (exit 1).
    Rejected(String),
}

/// Parse the trusted program's output. `mods` is the list it was asked for, in
/// order; `nonce` the value it was given. Refused (Err) on any shape problem:
/// a missing `begin`/`end`, another nonce, a module out of order or not
/// asked for, a count that disagrees, bad hex, a module-system olean (whose
/// private part this program does not read).
pub fn parse_lean_output(text: &str, nonce: &str, mods: &[String]) -> Result<LeanRun, String> {
    let mut lines = text.lines().filter(|l| !l.is_empty());
    match lines.next().map(fields).as_deref() {
        Some(["begin", n]) if *n == nonce => {}
        _ => return Err("the Lean audit printed no `begin <nonce>` line".into()),
    }
    let mut out: BTreeMap<String, LeanReport> = BTreeMap::new();
    let mut expect: BTreeMap<String, usize> = BTreeMap::new();
    let mut order = Vec::new();
    let mut records = 0usize;
    for l in lines {
        match fields(l).as_slice() {
            ["module", m, is_module, nconst, _nimports] => {
                let m = unhex(m)?;
                if *is_module != "false" {
                    return Err(format!("{m} is a module-system olean (isModule = {is_module}): its private part is not read -- refusing"));
                }
                let n: usize = nconst.parse().map_err(|_| format!("bad constant count {nconst:?}"))?;
                if out.contains_key(&m) {
                    return Err(format!("module {m} reported twice"));
                }
                order.push(m.clone());
                expect.insert(m.clone(), n);
                out.insert(m, LeanReport::default());
            }
            ["const", m, name, kind, unsafe_, partial, axs] => {
                let m = unhex(m)?;
                let _ = unhex(name)?;
                let Some(r) = out.get_mut(&m) else { return Err(format!("a constant of {m} before its `module` record")) };
                if order.last() != Some(&m) {
                    return Err(format!("a constant of {m} after another module's records"));
                }
                r.constants += 1;
                match *kind {
                    "theorem" => r.theorems += 1,
                    "axiom" => r.declared_axioms += 1,
                    "def" | "opaque" | "quot" | "inductive" | "ctor" | "rec" => {}
                    k => return Err(format!("unknown constant kind {k:?}")),
                }
                let flag = |s: &str| match s {
                    "true" => Ok(true),
                    "false" => Ok(false),
                    o => Err(format!("not a bool: {o:?}")),
                };
                if flag(unsafe_)? || flag(partial)? {
                    r.unreplayed += 1;
                }
                for a in axs.split(',').filter(|a| !a.is_empty()) {
                    *r.axioms.entry(normalize_lean_name(&unhex(a)?)).or_default() += 1;
                }
                records += 1;
            }
            ["end", n, total] if *n == nonce => {
                let total: usize = total.parse().map_err(|_| format!("bad total {total:?}"))?;
                if total != records {
                    return Err(format!("`end` says {total} constants, {records} were printed"));
                }
                if order != mods {
                    return Err(format!("modules reported {order:?}, asked for {mods:?}"));
                }
                for (m, r) in &out {
                    if expect.get(m) != Some(&r.constants) {
                        return Err(format!("{m}: the olean holds {} constants, {} were reported", expect.get(m).unwrap_or(&0), r.constants));
                    }
                }
                return Ok(LeanRun::Ok(out));
            }
            ["refuse", n, msg] if *n == nonce => return Err(format!("the Lean audit could not read its input: {}", unhex(msg)?)),
            ["reject", n, msg] if *n == nonce => return Ok(LeanRun::Rejected(unhex(msg)?)),
            _ => return Err(format!("unexpected Lean audit line: {l:?}")),
        }
    }
    Err("the Lean audit output has no `end <nonce>` line (truncated)".into())
}

/// The nonce of the `i`-th per-module run: the run's base nonce and the
/// module's position, so one module's transcript cannot stand in for
/// another's (nor a stale run's for this one).
pub fn lean_module_nonce(base: &str, i: usize) -> String {
    format!("{base}m{i}")
}

/// Judge ONE per-module run of the trusted program (one process per module:
/// see the header of PROGRAM for the measured reason). `code` is the exit
/// code (None = killed by a signal, e.g. the OOM killer), `text` its stdout.
/// Ok(Ok) only for exit 0 with a complete transcript that carries this run's
/// nonce and reports exactly `module`; Ok(Rejected) only for exit 1 with a
/// `reject` record; anything else -- a killed or truncated run included -- is
/// refused (Err).
pub fn judge_lean_module(code: Option<i32>, text: &str, nonce: &str, module: &str) -> Result<LeanRun, String> {
    match (code, parse_lean_output(text, nonce, &[module.to_string()])) {
        (Some(0), Ok(LeanRun::Ok(r))) if r.len() == 1 && r.contains_key(module) => Ok(LeanRun::Ok(r)),
        (Some(1), Ok(LeanRun::Rejected(m))) => Ok(LeanRun::Rejected(m)),
        (c, Err(e)) => Err(format!("{module}: {e} (exit {c:?})")),
        (c, _) => Err(format!("{module}: the Lean audit exited {c:?} with output that does not match that exit")),
    }
}

// ── the ledger ─────────────────────────────────────────────────────────────

/// One fact, as the ledger states it and as the run observes it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Fact {
    RocqSet { set: String, axiom: String },
    RocqTheory { lib: String, item: String },
    RocqUses { lib: String, set: String },
    RocqAxiom { lib: String, name: String },
    RocqBypass { lib: String, kind: String, name: String },
    LeanMod { module: String, constants: usize, theorems: usize, unreplayed: usize },
    LeanAxiom { module: String, axiom: String, n: usize },
}

impl Fact {
    pub fn render(&self) -> String {
        match self {
            Fact::RocqSet { set, axiom } => format!("rocq-set {set} {axiom}"),
            Fact::RocqTheory { lib, item } => format!("rocq-lib {lib} theory {item}"),
            Fact::RocqUses { lib, set } => format!("rocq-lib {lib} uses {set}"),
            Fact::RocqAxiom { lib, name } => format!("rocq-lib {lib} axiom {name}"),
            Fact::RocqBypass { lib, kind, name } => format!("rocq-lib {lib} bypass {kind} {name}"),
            Fact::LeanMod { module, constants, theorems, unreplayed } => {
                format!("lean-mod {module} constants {constants} theorems {theorems} unreplayed {unreplayed}")
            }
            Fact::LeanAxiom { module, axiom, n } => format!("lean-axiom {module} {axiom} {n}"),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Ledger {
    pub rows: Vec<(Fact, String)>,
}

fn num(s: &str, ln: usize) -> Result<usize, String> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err(format!("line {ln}: {s:?} is not a count"));
    }
    s.parse().map_err(|_| format!("line {ln}: {s:?} is not a count"))
}

pub fn parse_ledger(text: &str) -> Result<Ledger, String> {
    let (mut mode, mut baseline) = (None, None);
    let mut rows: Vec<(Fact, String)> = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let ln = i + 1;
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let f: Vec<&str> = line.split_whitespace().collect();
        if f[0] == "mode" || f[0] == "baseline" {
            let v = line[f[0].len()..].trim().strip_prefix('=').map(str::trim).ok_or(format!("line {ln}: `{} = ...`", f[0]))?;
            let slot = if f[0] == "mode" { &mut mode } else { &mut baseline };
            if slot.is_some() {
                return Err(format!("line {ln}: a second `{}` line", f[0]));
            }
            *slot = Some(v.to_string());
            continue;
        }
        if f[0] == "admit" {
            return Err(format!("line {ln}: `admit` rows belong to shrink-only mode, which this gate does not implement yet"));
        }
        let id = *f.last().unwrap_or(&"");
        if !is_artifact_id(id) {
            return Err(format!("line {ln}: '{id}' is not an artifact ID -- every row names what justifies it"));
        }
        let fact = match &f[..f.len() - 1] {
            ["rocq-set", set, axiom] => Fact::RocqSet { set: set.to_string(), axiom: axiom.to_string() },
            ["rocq-lib", lib, "theory", item] => {
                if !["set-predicative", "set-impredicative", "rewrite-rules-disallowed", "rewrite-rules-allowed"].contains(item) {
                    return Err(format!("line {ln}: unknown theory item '{item}'"));
                }
                Fact::RocqTheory { lib: lib.to_string(), item: item.to_string() }
            }
            ["rocq-lib", lib, "uses", set] => Fact::RocqUses { lib: lib.to_string(), set: set.to_string() },
            ["rocq-lib", lib, "axiom", name] => Fact::RocqAxiom { lib: lib.to_string(), name: name.to_string() },
            ["rocq-lib", lib, "bypass", kind, name] => {
                if !["type-in-type", "unsafe-fixpoint", "positivity"].contains(kind) {
                    return Err(format!("line {ln}: unknown bypass kind '{kind}'"));
                }
                Fact::RocqBypass { lib: lib.to_string(), kind: kind.to_string(), name: name.to_string() }
            }
            ["lean-mod", m, "constants", c, "theorems", t, "unreplayed", u] => {
                Fact::LeanMod { module: m.to_string(), constants: num(c, ln)?, theorems: num(t, ln)?, unreplayed: num(u, ln)? }
            }
            ["lean-axiom", m, a, n] => {
                let n = num(n, ln)?;
                if n == 0 {
                    return Err(format!("line {ln}: a zero row is a removed row -- delete it"));
                }
                Fact::LeanAxiom { module: m.to_string(), axiom: a.to_string(), n }
            }
            _ => return Err(format!("line {ln}: not a ledger row: {line:?}")),
        };
        rows.push((fact, id.to_string()));
    }
    match (mode.as_deref(), baseline.as_deref()) {
        (Some("equality"), Some("none")) => {}
        (Some("shrink-only"), _) => {
            return Err("mode = shrink-only: this gate implements equality only so far (the ratchet after the v0.11.0 tag is a later step); refusing to judge growth".into())
        }
        (None, _) => return Err("the ledger has no `mode =` line".into()),
        (_, None) => return Err("the ledger has no `baseline =` line".into()),
        (m, b) => return Err(format!("mode = {} with baseline = {}: only `equality` with `none` is valid now", m.unwrap_or(""), b.unwrap_or(""))),
    }
    // Duplicates: the same fact twice, or two lean-mod/lean-axiom rows for one key.
    let mut seen = BTreeSet::new();
    for (f, _) in &rows {
        let key = match f {
            Fact::LeanMod { module, .. } => format!("lean-mod {module}"),
            Fact::LeanAxiom { module, axiom, .. } => format!("lean-axiom {module} {axiom}"),
            other => other.render(),
        };
        if !seen.insert(key.clone()) {
            return Err(format!("duplicate row: {key}"));
        }
    }
    Ok(Ledger { rows })
}

/// Observed results, before they are folded into ledger facts.
#[derive(Debug, Clone, Default)]
pub struct Observed {
    pub rocq: BTreeMap<String, RocqReport>,
    pub lean: BTreeMap<String, LeanReport>,
    /// The Lean replay was rejected by the kernel: no Lean figures exist, and
    /// that one failure is the finding (the per-module comparison is skipped
    /// rather than reporting every module as lost).
    pub lean_rejected: bool,
}

fn theory_facts(lib: &str, r: &RocqReport) -> Vec<Fact> {
    vec![
        Fact::RocqTheory { lib: lib.into(), item: if r.impredicative { "set-impredicative" } else { "set-predicative" }.into() },
        Fact::RocqTheory { lib: lib.into(), item: if r.rewrite_rules { "rewrite-rules-allowed" } else { "rewrite-rules-disallowed" }.into() },
    ]
}

/// Render the observation as ledger facts. The base set is the intersection
/// of the axiom sets of every library that loads RocqOfRust (a library whose
/// axioms name `RocqOfRust.`); each such library `uses` it, and the rest of
/// its axioms are its own rows.
pub fn facts_of(o: &Observed) -> Vec<Fact> {
    let mut facts = Vec::new();
    let users: Vec<&String> = o.rocq.iter().filter(|(_, r)| r.axioms.iter().any(|a| a.starts_with("RocqOfRust."))).map(|(k, _)| k).collect();
    let base: BTreeSet<String> = users
        .iter()
        .map(|k| o.rocq[*k].axioms.clone())
        .reduce(|a, b| a.intersection(&b).cloned().collect())
        .unwrap_or_default();
    for a in &base {
        facts.push(Fact::RocqSet { set: BASE_SET.into(), axiom: a.clone() });
    }
    for (lib, r) in &o.rocq {
        facts.extend(theory_facts(lib, r));
        let uses = !base.is_empty() && users.contains(&lib);
        if uses {
            facts.push(Fact::RocqUses { lib: lib.clone(), set: BASE_SET.into() });
        }
        for a in &r.axioms {
            if !(uses && base.contains(a)) {
                facts.push(Fact::RocqAxiom { lib: lib.clone(), name: a.clone() });
            }
        }
        for (kind, name) in &r.bypass {
            facts.push(Fact::RocqBypass { lib: lib.clone(), kind: kind.clone(), name: name.clone() });
        }
    }
    for (m, r) in &o.lean {
        facts.push(Fact::LeanMod { module: m.clone(), constants: r.constants, theorems: r.theorems, unreplayed: r.unreplayed });
        for (a, n) in &r.axioms {
            facts.push(Fact::LeanAxiom { module: m.clone(), axiom: a.clone(), n: *n });
        }
    }
    facts.sort();
    facts
}

/// Compare the ledger with the observation, library by library (Rocq: the
/// expanded axiom set, theory and bypass rows; Lean: per module). Every
/// difference is named. `ids` are the artifact IDs that exist.
pub fn compare(ledger: &Ledger, o: &Observed, ids: &BTreeSet<String>) -> Vec<String> {
    let mut fails = Vec::new();
    for (f, id) in &ledger.rows {
        if !ids.contains(id) {
            fails.push(format!("{LEDGER}: `{}` names {id} -- no such artifact in artifacts/", f.render()));
        }
    }
    let mut sets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
    let mut used_sets = BTreeSet::new();
    let mut want: BTreeMap<String, (BTreeSet<String>, BTreeSet<String>, BTreeSet<(String, String)>)> = BTreeMap::new();
    let mut lean_want: BTreeMap<String, (Option<(usize, usize, usize)>, BTreeMap<String, usize>)> = BTreeMap::new();
    for (f, _) in &ledger.rows {
        if let Fact::RocqSet { set, axiom } = f {
            sets.entry(set.clone()).or_default().insert(axiom.clone());
        }
    }
    // Sorted, so every `uses` row is expanded before any own `axiom` row is
    // checked against it, whatever order the file has.
    let mut ordered = ledger.rows.clone();
    ordered.sort();
    for (f, _) in &ordered {
        match f {
            Fact::RocqSet { .. } => {}
            Fact::RocqTheory { lib, item } => {
                want.entry(lib.clone()).or_default().0.insert(item.clone());
            }
            Fact::RocqUses { lib, set } => match sets.get(set) {
                Some(s) => {
                    used_sets.insert(set.clone());
                    want.entry(lib.clone()).or_default().1.extend(s.iter().cloned());
                }
                None => fails.push(format!("{LEDGER}: {lib} uses set {set}, which has no `rocq-set` rows")),
            },
            Fact::RocqAxiom { lib, name } => {
                let w = want.entry(lib.clone()).or_default();
                if !w.1.insert(name.clone()) {
                    fails.push(format!("{LEDGER}: {lib} lists {name} both in a set it uses and as its own row"));
                }
            }
            Fact::RocqBypass { lib, kind, name } => {
                want.entry(lib.clone()).or_default().2.insert((kind.clone(), name.clone()));
            }
            Fact::LeanMod { module, constants, theorems, unreplayed } => {
                lean_want.entry(module.clone()).or_default().0 = Some((*constants, *theorems, *unreplayed));
            }
            Fact::LeanAxiom { module, axiom, n } => {
                lean_want.entry(module.clone()).or_default().1.insert(axiom.clone(), *n);
            }
        }
    }
    for s in sets.keys() {
        if !used_sets.contains(s) {
            fails.push(format!("{LEDGER}: rocq-set {s} is used by no library -- a stale set"));
        }
    }
    let libs: BTreeSet<&String> = want.keys().chain(o.rocq.keys()).collect();
    for lib in libs {
        let Some(r) = o.rocq.get(lib) else {
            fails.push(format!("{lib}: in the ledger, not audited (a stale row, or a library the audit target lost)"));
            continue;
        };
        let Some((theory, axioms, bypass)) = want.get(lib) else {
            fails.push(format!("{lib}: audited, but the ledger has no row for it"));
            continue;
        };
        let seen_theory: BTreeSet<String> = theory_facts(lib, r)
            .into_iter()
            .filter_map(|f| if let Fact::RocqTheory { item, .. } = f { Some(item) } else { None })
            .collect();
        if &seen_theory != theory {
            fails.push(format!("{lib}: theory {:?}, ledger {:?}", seen_theory, theory));
        }
        for a in r.axioms.difference(axioms) {
            fails.push(format!("{lib}: rocqchk reports axiom {a}, not in the ledger"));
        }
        for a in axioms.difference(&r.axioms) {
            fails.push(format!("{lib}: the ledger lists axiom {a}, rocqchk no longer reports it (an unrecorded shrink)"));
        }
        for (k, n) in r.bypass.difference(bypass) {
            fails.push(format!("{lib}: rocqchk reports a kernel-check bypass ({k}) at {n}, not in the ledger"));
        }
        for (k, n) in bypass.difference(&r.bypass) {
            fails.push(format!("{lib}: the ledger lists a {k} bypass at {n}, rocqchk no longer reports it"));
        }
    }
    let mods: BTreeSet<&String> = if o.lean_rejected { BTreeSet::new() } else { lean_want.keys().chain(o.lean.keys()).collect() };
    for m in mods {
        let Some(r) = o.lean.get(m) else {
            fails.push(format!("lean {m}: in the ledger, not audited"));
            continue;
        };
        let Some((counts, axioms)) = lean_want.get(m) else {
            fails.push(format!("lean {m}: audited, but the ledger has no row for it"));
            continue;
        };
        match counts {
            None => fails.push(format!("lean {m}: no `lean-mod` row")),
            Some((c, t, u)) => {
                if (*c, *t, *u) != (r.constants, r.theorems, r.unreplayed) {
                    fails.push(format!(
                        "lean {m}: constants {} theorems {} unreplayed {}, ledger {c} {t} {u}",
                        r.constants, r.theorems, r.unreplayed
                    ));
                }
            }
        }
        let keys: BTreeSet<&String> = axioms.keys().chain(r.axioms.keys()).collect();
        for a in keys {
            let (seen, rec) = (r.axioms.get(a).copied().unwrap_or(0), axioms.get(a).copied().unwrap_or(0));
            if seen != rec {
                fails.push(format!("lean {m}: {seen} constant(s) depend on axiom {a}, ledger {rec}"));
            }
        }
    }
    fails
}

/// Bridge: per Rocq library, the axioms under its own module path equal its
/// assumption spellings (trust-ledger's Admitted + Axiom + Parameter +
/// top-level Variable); per Lean module, the axiom-kind constants equal its
/// `axiom` spellings. `texts` maps a source path to its text.
pub fn bridge(rm: &RocqManifest, lm: &LeanManifest, o: &Observed, texts: &BTreeMap<String, String>) -> Vec<String> {
    let mut fails = Vec::new();
    for lib in &rm.libs {
        let Some(r) = o.rocq.get(&lib.module) else { continue };
        let own = r.axioms.iter().filter(|a| a.starts_with(&format!("{}.", lib.module))).count();
        let Some(text) = texts.get(&lib.src) else {
            fails.push(format!("{}: cannot read {} for the bridge check", lib.module, lib.src));
            continue;
        };
        let c = trust_ledger::count_file(&lib.src, text).counts;
        let spelled: usize = ["rocq:Admitted", "rocq:Axiom", "rocq:Parameter", "rocq:Variable"].iter().map(|k| c.get(k).copied().unwrap_or(0)).sum();
        if own != spelled {
            fails.push(format!(
                "bridge {}: rocqchk names {own} axiom(s) under {}.*, the trust ledger counts {spelled} Admitted/Axiom/Parameter/Variable spelling(s) in {} -- one side missed something",
                lib.module, lib.module, lib.src
            ));
        }
    }
    for m in &lm.mods {
        let Some(r) = o.lean.get(&m.module) else { continue };
        let Some(text) = texts.get(&m.src) else {
            fails.push(format!("lean {}: cannot read {} for the bridge check", m.module, m.src));
            continue;
        };
        let spelled = trust_ledger::count_file(&m.src, text).counts.get("lean:axiom").copied().unwrap_or(0);
        if r.declared_axioms != spelled {
            fails.push(format!(
                "bridge lean {}: {} axiom constant(s) in the olean, {spelled} `axiom` spelling(s) in {} -- one side missed something",
                m.module, r.declared_axioms, m.src
            ));
        }
    }
    fails
}

/// The audited sources must be exactly the tracked proof files: every tracked
/// `.v` / `.lean` outside the gate's own directory.
pub fn coverage(rm: &RocqManifest, lm: &LeanManifest, tracked: &[String]) -> Vec<String> {
    let mut fails = Vec::new();
    for (ext, listed, target) in [
        (".v", rm.libs.iter().map(|l| l.src.clone()).collect::<BTreeSet<_>>(), ROCQ_TARGET),
        (".lean", lm.mods.iter().map(|m| m.src.clone()).collect::<BTreeSet<_>>(), LEAN_TARGET),
    ] {
        let want: BTreeSet<String> = tracked.iter().filter(|p| p.ends_with(ext) && !p.starts_with("tools/xtask/")).cloned().collect();
        for p in want.difference(&listed) {
            fails.push(format!("{p}: a tracked proof file {target} does not audit"));
        }
        for p in listed.difference(&want) {
            fails.push(format!("{p}: audited by {target} but not a tracked file"));
        }
    }
    fails
}

// ── running ────────────────────────────────────────────────────────────────

fn tail(s: &[u8], n: usize) -> String {
    let t = String::from_utf8_lossy(s);
    let lines: Vec<&str> = t.lines().collect();
    lines[lines.len().saturating_sub(n)..].join(" | ")
}

fn bazel(repo: &Path, args: &[&str]) -> Result<String, String> {
    let out = Command::new("bazel")
        .current_dir(repo)
        .args(args)
        .output()
        .map_err(|e| format!("cannot run bazel: {e}"))?;
    if !out.status.success() {
        return Err(format!("bazel {} failed ({}): {}", args.join(" "), out.status, tail(&out.stderr, 15)));
    }
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

/// Build the two audit targets, then run every checker. Err = Refused.
/// Ok((manifests, observation, checker failures)).
fn observe(repo: &Path) -> Result<(RocqManifest, LeanManifest, Observed, Vec<String>), String> {
    bazel(repo, &["build", "--remote_download_outputs=all", ROCQ_TARGET, LEAN_TARGET])?;
    let exec = PathBuf::from(bazel(repo, &["info", "execution_root"])?);
    let bin = PathBuf::from(bazel(repo, &["info", "bazel-bin"])?);
    let read = |p: &str| std::fs::read_to_string(bin.join(p)).map_err(|e| format!("cannot read the manifest {p}: {e}"));
    let rm = parse_rocq_manifest(&read(ROCQ_MANIFEST)?)?;
    let lm = parse_lean_manifest(&read(LEAN_MANIFEST)?)?;
    let mut o = Observed::default();
    let mut fails = Vec::new();
    for lib in &rm.libs {
        let mut args: Vec<String> = vec!["-o".into(), "-silent".into()];
        args.extend(lib.flags.iter().cloned());
        for (p, l) in &lib.q {
            args.extend(["-Q".to_string(), p.clone(), l.clone()]);
        }
        args.push(lib.module.clone());
        // Progress on stderr, live: the first Linux runs (gale#558) lost the
        // runner or timed out inside this loop with nothing in the log.
        eprintln!("axioms-audit: rocqchk {} ({} -Q paths) ...", lib.module, lib.q.len());
        let t0 = std::time::Instant::now();
        let out = Command::new(exec.join(&rm.checker))
            .current_dir(&exec)
            .args(&args)
            .output()
            .map_err(|e| format!("cannot run {}: {e}", rm.checker))?;
        eprintln!("axioms-audit: rocqchk {} done in {:.1}s ({})", lib.module, t0.elapsed().as_secs_f64(), out.status);
        if !out.status.success() {
            fails.push(format!("{}: rocqchk did not accept the library ({}): {}", lib.module, out.status, tail(&[out.stdout.as_slice(), out.stderr.as_slice()].concat(), 6)));
            continue;
        }
        // rocqchk 9.0.1 prints the summary on STDERR (measured); with -silent
        // stdout is empty, and anything there is a shape the gate does not know.
        if !out.stdout.iter().all(u8::is_ascii_whitespace) {
            return Err(format!("{}: rocqchk wrote to stdout, which -silent leaves empty: {}", lib.module, tail(&out.stdout, 4)));
        }
        let r = parse_rocqchk(&String::from_utf8_lossy(&out.stderr)).map_err(|e| format!("{}: {e}", lib.module))?;
        o.rocq.insert(lib.module.clone(), r);
    }
    let nonce = format!(
        "{:x}{:x}",
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0),
        std::process::id()
    );
    // Before running anything: no audited module may share a root with the
    // toolchain, Mathlib, or another search-path entry.
    let lean_bin = exec.join(&lm.lean);
    let sysroot = lean_bin.parent().and_then(Path::parent).map(|p| p.join("lib").join("lean")).ok_or(format!("{}: no toolchain prefix", lm.lean))?;
    let toolchain = dir_roots(&sysroot)?;
    let path_roots = lm.paths.iter().map(|p| dir_roots(&exec.join(p))).collect::<Result<Vec<_>, _>>()?;
    let conflicts = lean_root_conflicts(&lm, &toolchain, &path_roots);
    if !conflicts.is_empty() {
        return Err(conflicts.join("; "));
    }
    // One `lean` process per module, sequentially: in one process every
    // module's `importModules` kept its own copy of Mathlib (measured 18.5 GB
    // peak footprint for the 8 vs 2.0 GB for one; the hosted runner was
    // OOM-killed). Each run has its own nonce and must report exactly its
    // module; a killed or truncated run is refused like any other bad shape.
    let mut lean: BTreeMap<String, LeanReport> = BTreeMap::new();
    for (i, m) in lm.mods.iter().enumerate() {
        let mnonce = lean_module_nonce(&nonce, i);
        // F1: the trusted program is elaborated with NO `LEAN_*` variable, so its
        // `import Lean` resolves in the toolchain's lib/lean only; the audit search
        // path reaches it as arguments, installed at run time (toolchain first).
        let mut cmd = Command::new(&lean_bin);
        for (k, _) in std::env::vars_os() {
            if k.to_string_lossy().starts_with("LEAN") {
                cmd.env_remove(k);
            }
        }
        eprintln!("axioms-audit: lean --run {} {} ({}/{}) ...", PROGRAM, m.module, i + 1, lm.mods.len());
        let t0 = std::time::Instant::now();
        let out = cmd
            .current_dir(&exec)
            .arg("--run")
            .arg(repo.join(PROGRAM))
            .arg(&mnonce)
            .args(lm.paths.iter().map(|p| format!("--path={}", exec.join(p).display())))
            .arg(format!("{}={}", m.module, exec.join(&m.olean).display()))
            .output()
            .map_err(|e| format!("cannot run {}: {e}", lm.lean))?;
        eprintln!("axioms-audit: lean {} done in {:.1}s ({})", m.module, t0.elapsed().as_secs_f64(), out.status);
        let text = String::from_utf8_lossy(&out.stdout);
        match judge_lean_module(out.status.code(), &text, &mnonce, &m.module) {
            Ok(LeanRun::Ok(r)) => {
                for (k, v) in r {
                    if lean.insert(k.clone(), v).is_some() {
                        return Err(format!("lean module {k} reported by two runs"));
                    }
                }
            }
            Ok(LeanRun::Rejected(msg)) => {
                // Measured: a `native_decide` auxiliary cannot be replayed (the kernel
                // needs compiled code for `Lean.reduceBool` that replay does not
                // load), so the measured native_decide plant (oracle P4) was rejected here, not
                // ledgered: the replay's kernel check is the mechanism. As before
                // the split, the first rejection ends the Lean half with no figures.
                o.lean_rejected = true;
                lean.clear();
                fails.push(format!("lean: the kernel rejected a replayed constant (a `native_decide` auxiliary is rejected this way): {msg}"));
                break;
            }
            Err(e) => return Err(format!("{e}; stderr: {}", tail(&out.stderr, 6))),
        }
    }
    o.lean = lean;
    Ok((rm, lm, o, fails))
}

fn tracked(repo: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("git").arg("-C").arg(repo).args(["ls-files", "-z", "--", "*.v", "*.lean"]).output().map_err(|e| format!("git ls-files: {e}"))?;
    if !out.status.success() {
        return Err("git ls-files failed".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout).split('\0').filter(|p| !p.is_empty()).map(str::to_string).collect())
}

fn source_texts(repo: &Path, rm: &RocqManifest, lm: &LeanManifest) -> BTreeMap<String, String> {
    rm.libs
        .iter()
        .map(|l| &l.src)
        .chain(lm.mods.iter().map(|m| &m.src))
        .filter_map(|p| std::fs::read_to_string(repo.join(p)).ok().map(|t| (p.clone(), t)))
        .collect()
}

fn summary(o: &Observed) -> Vec<String> {
    let mut v = vec![format!("Rocq: {} libraries checked by rocqchk -o (per library, everything loaded included)", o.rocq.len())];
    for (lib, r) in &o.rocq {
        v.push(format!(
            "  {lib:<32} {:>4} axioms  Set {}  bypasses {}",
            r.axioms.len(),
            if r.impredicative { "impredicative" } else { "predicative" },
            r.bypass.len()
        ));
    }
    v.push(format!("Lean: {} modules replayed through the kernel, axioms collected per constant", o.lean.len()));
    for (m, r) in &o.lean {
        let axs: Vec<String> = r.axioms.iter().map(|(a, n)| format!("{a}x{n}")).collect();
        v.push(format!("  {m:<32} {:>4} constants ({} theorems, {} unreplayed)  {}", r.constants, r.theorems, r.unreplayed, axs.join(" ")));
    }
    v.push("cannot see:".into());
    v.extend(CANNOT_SEE.iter().map(|s| format!("  - {s}")));
    v
}

/// Every ledger row against the rows the run observed (IDs left out), as a
/// diff: `- ` only in the ledger, `+ ` only observed. Printed on every failure
/// in full, so a run on another platform (CI is Linux, the ledger was seeded
/// on darwin) shows the whole difference in one log.
pub fn row_diff(ledger: &Ledger, o: &Observed) -> Vec<String> {
    let want: BTreeSet<String> = ledger.rows.iter().map(|(f, _)| f.render()).collect();
    let seen: BTreeSet<String> = facts_of(o).iter().map(Fact::render).collect();
    let mut v = vec![format!("--- {LEDGER} (rows, IDs omitted)"), "+++ observed by this run".to_string()];
    v.extend(want.difference(&seen).map(|r| format!("- {r}")));
    v.extend(seen.difference(&want).map(|r| format!("+ {r}")));
    if o.lean_rejected {
        v.push("(the Lean replay was rejected: no Lean rows were observed, so every Lean row shows as `-`)".into());
    }
    v
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let tracked = match tracked(repo) {
        Ok(t) => t,
        Err(e) => return Verdict::Refused(e),
    };
    let ledger = match std::fs::read_to_string(repo.join(LEDGER)) {
        Ok(t) => match parse_ledger(&t) {
            Ok(l) => l,
            Err(e) => return Verdict::Fail(vec![format!("{LEDGER}: {e}")]),
        },
        Err(e) => return Verdict::Refused(format!("cannot read {LEDGER}: {e}")),
    };
    let ids = match trust_ledger::artifact_ids(repo) {
        Ok(i) => i,
        Err(e) => return Verdict::Refused(e),
    };
    let (rm, lm, o, mut fails) = match observe(repo) {
        Ok(x) => x,
        Err(e) => return Verdict::Refused(e),
    };
    let mut lines = summary(&o);
    fails.extend(coverage(&rm, &lm, &tracked));
    fails.extend(bridge(&rm, &lm, &o, &source_texts(repo, &rm, &lm)));
    fails.extend(compare(&ledger, &o, &ids));
    if fails.is_empty() {
        lines.push(format!("the compiled objects match {LEDGER} ({} rows)", ledger.rows.len()));
        Verdict::Pass(lines)
    } else {
        lines.extend(fails);
        lines.extend(row_diff(&ledger, &o));
        lines.push("regenerate rows with `cargo xtask axioms-audit --write --id <ARTIFACT-ID>`".into());
        Verdict::Fail(lines)
    }
}

/// `xtask axioms-audit [--write --id ID]`: print the observed rows, or rewrite
/// the ledger's rows (the header is kept; unchanged rows keep their ID).
pub fn cmd(repo: &Path, args: &[String]) -> i32 {
    use crate::verdict::{EXIT_PASS, EXIT_REFUSED, EXIT_USAGE};
    let (mut write, mut id) = (false, None::<String>);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--write" => write = true,
            "--id" => id = it.next().cloned(),
            o => {
                eprintln!("xtask: unknown argument '{o}' to axioms-audit (--write --id ID)");
                return EXIT_USAGE;
            }
        }
    }
    if write && !id.as_deref().is_some_and(is_artifact_id) {
        eprintln!("xtask: --write needs --id <ARTIFACT-ID>");
        return EXIT_USAGE;
    }
    let (_, _, o, fails) = match observe(repo) {
        Ok(x) => x,
        Err(e) => {
            eprintln!("xtask: {e}");
            return EXIT_REFUSED;
        }
    };
    if !fails.is_empty() {
        for f in fails {
            eprintln!("xtask: {f}");
        }
        return EXIT_REFUSED;
    }
    let old_text = std::fs::read_to_string(repo.join(LEDGER)).ok();
    let old = old_text.as_deref().and_then(|t| parse_ledger(t).ok());
    let id = id.unwrap_or_else(|| "UNJUSTIFIED-0".into());
    let rows: String = facts_of(&o)
        .into_iter()
        .map(|f| {
            let keep = old.as_ref().and_then(|l| l.rows.iter().find(|(g, _)| *g == f)).map(|(_, i)| i.clone());
            format!("{} {}\n", f.render(), keep.unwrap_or_else(|| id.clone()))
        })
        .collect();
    if !write {
        print!("{rows}");
        return EXIT_PASS;
    }
    let header: String = old_text
        .as_deref()
        .unwrap_or("mode = equality\nbaseline = none\n")
        .lines()
        .take_while(|l| l.trim().is_empty() || l.trim_start().starts_with('#') || l.trim_start().starts_with("mode") || l.trim_start().starts_with("baseline"))
        .map(|l| format!("{l}\n"))
        .collect();
    if let Err(e) = std::fs::write(repo.join(LEDGER), format!("{header}{rows}")) {
        eprintln!("xtask: cannot write {LEDGER}: {e}");
        return EXIT_REFUSED;
    }
    println!("wrote {LEDGER}");
    EXIT_PASS
}

// ── self-test ──────────────────────────────────────────────────────────────

/// A rocqchk -o -silent output in the shape Rocq 9.0.1 prints (recorded in
/// the spike, shortened).
const ROCQ_SAMPLE: &str = "\nCONTEXT SUMMARY\n===============\n\n* Theory: Set is impredicative\n  \n* Theory: Rewrite rules are not allowed\n  \n\
* Axioms:\n    RocqOfRust.M.run\n    Corelib.Strings.PrimString.string\n    proofs.poll_proofs.p1\n  \n\
* Constants/Inductives relying on type-in-type: <none>\n  \n* Constants/Inductives relying on unsafe (co)fixpoints: <none>\n  \n\
* Inductives whose positivity is assumed: <none>\n  \n";

fn hexs(s: &str) -> String {
    s.bytes().map(|b| format!("{b:02x}")).collect()
}

// rivet: verifies VER-HONESTY-AXIOMS-GATE-001
/// Pure: recorded outputs and planted variants; no Bazel, no checker.
pub fn self_test(repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: &str, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };

    // -- rocqchk output ------------------------------------------------------
    let r = parse_rocqchk(ROCQ_SAMPLE);
    ck("rocqchk: the recorded shape parses (3 axioms, impredicative, no bypass)",
        r.as_ref().is_ok_and(|r| r.axioms.len() == 3 && r.impredicative && !r.rewrite_rules && r.bypass.is_empty()));
    let guard = ROCQ_SAMPLE.replace("relying on unsafe (co)fixpoints: <none>", "relying on unsafe (co)fixpoints:\n    T.G.loop");
    ck("rocqchk: an unsafe (guard-unchecked) fixpoint is a bypass row (recorded: `Unset Guard Checking` + Fixpoint)",
        parse_rocqchk(&guard).is_ok_and(|r| r.bypass.contains(&("unsafe-fixpoint".to_string(), "T.G.loop".to_string()))));
    for (what, bad) in [
        ("VACUOUS an empty output", String::new()),
        ("VACUOUS no CONTEXT SUMMARY header", ROCQ_SAMPLE.replace("CONTEXT SUMMARY\n===============\n", "")),
        ("VACUOUS a truncated output (the positivity section missing)", ROCQ_SAMPLE.replace("* Inductives whose positivity is assumed: <none>\n", "")),
        ("an unknown section", format!("{ROCQ_SAMPLE}* Something new: <none>\n")),
        ("a repeated section", format!("{ROCQ_SAMPLE}* Axioms: <none>\n")),
        ("a name with a space", ROCQ_SAMPLE.replace("RocqOfRust.M.run", "RocqOfRust.M run")),
        ("text after a list header", ROCQ_SAMPLE.replace("* Axioms:\n", "* Axioms: maybe\n")),
    ] {
        ck(&format!("rocqchk: {what} is refused, never read as no axioms"), parse_rocqchk(&bad).is_err());
    }

    // -- manifests -----------------------------------------------------------
    let rman = "checker\tbin/rocqchk\nlib\t@@//proofs:a\tproofs/a.v\tproofs.a\t-impredicative-set\nq\t@@//proofs:a\tx/Stdlib\tStdlib\n\
                lib\t@@//proofs:b\tproofs/b.v\tproofs.b\t\nq\t@@//proofs:b\tx/Stdlib\tStdlib\n";
    ck("manifest: the rocq shape parses (2 libs, flags kept)",
        parse_rocq_manifest(rman).is_ok_and(|m| m.libs.len() == 2 && m.libs[0].flags == ["-impredicative-set"] && m.libs[1].flags.is_empty()));
    ck("VACUOUS manifest: no library is refused", parse_rocq_manifest("checker\tbin/rocqchk\n").is_err());
    ck("VACUOUS manifest: empty is refused", parse_rocq_manifest("").is_err());
    ck("manifest: a flag the gate does not model (`-type-in-type`) is refused", parse_rocq_manifest(&rman.replace("-impredicative-set", "-type-in-type")).is_err());
    ck("manifest: an unknown record is refused", parse_rocq_manifest(&format!("{rman}x\ty\n")).is_err());
    let lman = "lean\tbin/lean\npath\tmathlib/lib\npath\tout/a\nmod\t@@//proofs/lean:a\tproofs/lean/A.lean\tA\tout/a/A.olean\n";
    ck("manifest: the lean shape parses", parse_lean_manifest(lman).is_ok_and(|m| m.mods.len() == 1 && m.paths.len() == 2));
    ck("VACUOUS manifest: a lean manifest with no module is refused", parse_lean_manifest("lean\tbin/lean\npath\tp\n").is_err());
    ck("VACUOUS manifest: a lean manifest with no LEAN_PATH is refused", parse_lean_manifest("lean\tbin/lean\nmod\tl\ts\tA\to\n").is_err());

    // -- roots: what findOLean would resolve (F1 / F2) ---------------------------
    let set = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<BTreeSet<String>>();
    let tc = set(&["Init", "Lake", "LakeMain", "Lean", "Leanc", "Std"]);
    let conflicts = |module: &str, olean: &str, own_roots: &[&str]| {
        parse_lean_manifest(&format!("lean\tbin/lean\npath\tmathlib/lib\npath\tout/a/\nmod\t@@//proofs/lean:x\tproofs/lean/X.lean\t{module}\t{olean}\n"))
            .map(|m| lean_root_conflicts(&m, &tc, &[set(&["Aesop", "Batteries", "Mathlib", "Qq"]), set(own_roots)]))
    };
    ck("roots: a module that alone provides its root (A in out/a/) passes", conflicts("A", "out/a/A.olean", &["A"]).is_ok_and(|f| f.is_empty()));
    ck("roots: a nested module (A.B at out/a/A/B.olean) passes", conflicts("A.B", "out/a/A/B.olean", &["A"]).is_ok_and(|f| f.is_empty()));
    ck("PLANTED F1 roots: an audited module named `Lean` (it would shadow the trusted program's `import Lean`) is refused",
        conflicts("Lean", "out/a/Lean.olean", &["Lean"]).is_ok_and(|f| f.iter().any(|x| x.contains("toolchain"))));
    ck("PLANTED F1 roots: a module under the `Init` root is refused", conflicts("Init.Fake", "out/a/Init/Fake.olean", &["Init"]).is_ok_and(|f| !f.is_empty()));
    ck("PLANTED F1 roots: a root the pinned lib/lean lists (`LakeMain`) is refused", conflicts("LakeMain", "out/a/LakeMain.olean", &["LakeMain"]).is_ok_and(|f| !f.is_empty()));
    ck("PLANTED F2 roots: an audited `Batteries` (a Mathlib root: Mathlib's olean would be audited in its place) is refused",
        conflicts("Batteries", "out/a/Batteries.olean", &["Batteries"]).is_ok_and(|f| f.iter().any(|x| x.contains("mathlib/lib"))));
    ck("PLANTED F2 roots: a manifest olean that is not <entry>/<module>.olean is refused", conflicts("A", "elsewhere/A.olean", &["A"]).is_ok_and(|f| !f.is_empty()));
    ck(&format!("the trusted program {PROGRAM} sets its search path itself and never calls initSearchPath (which reads LEAN_PATH)"),
        std::fs::read_to_string(repo.join(PROGRAM)).is_ok_and(|t| t.contains("searchPathRef.set") && !t.contains("initSearchPath") && t.contains("realPath")));

    // -- the Lean program's output ---------------------------------------------
    let mods = vec!["A".to_string()];
    let lean_out = |consts: &[(&str, &str, &[&str])], end: Option<usize>| {
        let mut s = format!("begin\tN1\nmodule\t{}\tfalse\t{}\t2\n", hexs("A"), consts.len());
        for (name, kind, axs) in consts {
            let a: Vec<String> = axs.iter().map(|x| hexs(x)).collect();
            s.push_str(&format!("const\t{}\t{}\t{kind}\tfalse\tfalse\t{}\n", hexs("A"), hexs(name), a.join(",")));
        }
        if let Some(n) = end {
            s.push_str(&format!("end\tN1\t{n}\n"));
        }
        s
    };
    let good: &[(&str, &str, &[&str])] = &[("t1", "theorem", &["propext"]), ("t2", "theorem", &["propext", "Quot.sound"]), ("d", "def", &[])];
    let parsed = parse_lean_output(&lean_out(good, Some(3)), "N1", &mods);
    let la = match &parsed {
        Ok(LeanRun::Ok(m)) => m.get("A").cloned(),
        _ => None,
    };
    ck("lean: the output parses (3 constants, 2 theorems, propext x2, Quot.sound x1)",
        la.as_ref().is_some_and(|r| r.constants == 3 && r.theorems == 2 && r.axioms.get("propext") == Some(&2) && r.axioms.get("Quot.sound") == Some(&1)));
    ck("VACUOUS lean: a truncated output (no `end`) is refused", parse_lean_output(&lean_out(good, None), "N1", &mods).is_err());
    ck("lean: `end` with the wrong count is refused", parse_lean_output(&lean_out(good, Some(4)), "N1", &mods).is_err());
    ck("lean: another nonce (a stale or mixed-up transcript) is refused", parse_lean_output(&lean_out(good, Some(3)), "N2", &mods).is_err());
    ck("VACUOUS lean: a module asked for but not reported is refused",
        parse_lean_output(&lean_out(good, Some(3)), "N1", &["A".to_string(), "B".to_string()]).is_err());
    ck("lean: a module-system olean (private part unread) is refused",
        parse_lean_output(&lean_out(good, Some(3)).replace("\tfalse\t3\t2", "\ttrue\t3\t2"), "N1", &mods).is_err());
    ck("lean: the olean's constant count disagreeing with the records is refused",
        parse_lean_output(&lean_out(good, Some(3)).replace("\tfalse\t3\t2", "\tfalse\t4\t2"), "N1", &mods).is_err());
    ck("VACUOUS lean: a `refuse` record (wrong search path: olean not found) is refused",
        parse_lean_output(&format!("begin\tN1\nrefuse\tN1\t{}\n", hexs("findOLean A: unknown module prefix")), "N1", &mods).is_err());
    ck("lean: a `reject` record (the kernel refused a constant) is a failure, not a refusal",
        matches!(parse_lean_output(&format!("begin\tN1\nreject\tN1\t{}\n", hexs("replay A: bad")), "N1", &mods), Ok(LeanRun::Rejected(_))));
    ck("lean: uppercase or odd hex is refused", parse_lean_output(&lean_out(good, Some(3)).replace(&hexs("t1"), "7"), "N1", &mods).is_err());

    // -- one process per module: judging each run ---------------------------------
    let n0 = lean_module_nonce("N", 0);
    let n1 = lean_module_nonce("N", 1);
    let one = |nonce: &str, module: &str| lean_out(good, Some(3)).replace("N1", nonce).replace(&hexs("A"), &hexs(module));
    ck("per-module: exit 0 with this run's nonce and module passes",
        matches!(judge_lean_module(Some(0), &one(&n0, "A"), &n0, "A"), Ok(LeanRun::Ok(m)) if m.len() == 1 && m.contains_key("A")));
    ck("per-module: the nonces of two runs differ", n0 != n1);
    ck("PLANTED per-module: another module's run (its nonce) as this module's transcript is refused",
        judge_lean_module(Some(0), &one(&n1, "A"), &n0, "A").is_err());
    ck("PLANTED per-module: a transcript reporting another module (B for A) is refused",
        judge_lean_module(Some(0), &one(&n0, "B"), &n0, "A").is_err());
    ck("PLANTED per-module: a transcript reporting this module AND another is refused",
        judge_lean_module(Some(0), &format!("{}{}", one(&n0, "A").replace(&format!("end\t{n0}\t3\n"), ""), one(&n0, "B").replace(&format!("begin\t{n0}\n"), "").replace("\t3\n", "\t6\n")), &n0, "A").is_err());
    {
        let full = one(&n0, "A");
        let cut = &full[..full.rfind("end\t").unwrap_or(0)];
        ck("VACUOUS per-module: a run killed by a signal (OOM: no exit code, transcript cut) is refused",
            judge_lean_module(None, cut, &n0, "A").is_err());
        ck("VACUOUS per-module: a killed run whose transcript happens to be complete is still refused",
            judge_lean_module(None, &full, &n0, "A").is_err());
        ck("VACUOUS per-module: exit 0 with a truncated transcript is refused", judge_lean_module(Some(0), cut, &n0, "A").is_err());
        ck("per-module: exit 3 with a complete transcript (shape and exit disagree) is refused", judge_lean_module(Some(3), &full, &n0, "A").is_err());
    }
    ck("per-module: exit 1 with a `reject` record is the kernel's rejection",
        matches!(judge_lean_module(Some(1), &format!("begin\t{n0}\nreject\t{n0}\t{}\n", hexs("replay A: bad")), &n0, "A"), Ok(LeanRun::Rejected(_))));
    ck("per-module: a `reject` record with exit 0 is refused", judge_lean_module(Some(0), &format!("begin\t{n0}\nreject\t{n0}\t{}\n", hexs("x")), &n0, "A").is_err());
    ck(&format!("the trusted program {PROGRAM} refuses anything but exactly one module per run"),
        std::fs::read_to_string(repo.join(PROGRAM)).is_ok_and(|t| t.contains("if mods.length != 1 then")));
    ck("lean: a private name loses its host-specific path",
        normalize_lean_name("_private.«bazel-out».«k8-fastbuild».bin.proofs.lean.x_src.M.0.foo") == "private:foo" && normalize_lean_name("propext") == "propext");

    // -- the ledger and the comparison -----------------------------------------
    let mut o = Observed::default();
    let base = |extra: &[&str]| {
        let mut r = RocqReport { impredicative: true, ..Default::default() };
        for a in ["RocqOfRust.M.run", "Corelib.Strings.PrimString.string"].iter().chain(extra) {
            r.axioms.insert(a.to_string());
        }
        r
    };
    o.rocq.insert("proofs.a".into(), base(&[]));
    o.rocq.insert("proofs.b".into(), base(&["proofs.b.adm"]));
    o.rocq.insert("proofs.e".into(), RocqReport::default());
    if let Some(r) = la.clone() {
        o.lean.insert("A".into(), r);
    }
    let mut ids = BTreeSet::new();
    ids.insert("VER-X-001".to_string());
    let render = |o: &Observed| -> String {
        format!("mode = equality\nbaseline = none\n{}", facts_of(o).iter().map(|f| format!("{} VER-X-001\n", f.render())).collect::<String>())
    };
    let text = render(&o);
    let ledger = parse_ledger(&text);
    ck("ledger: rendered rows parse; the shared set is named once (2 rows), b keeps its own Admitted row",
        ledger.is_ok() && text.matches("rocq-set rocqofrust-base ").count() == 2 && text.contains("rocq-lib proofs.b axiom proofs.b.adm VER-X-001")
            && text.contains("rocq-lib proofs.e theory set-predicative") && !text.contains("rocq-lib proofs.e uses"));
    let ledger = ledger.unwrap_or(Ledger { rows: Vec::new() });
    ck("compare: the unchanged observation passes", compare(&ledger, &o, &ids).is_empty());
    let planted = |f: &dyn Fn(&mut Observed)| {
        let mut p = o.clone();
        f(&mut p);
        compare(&ledger, &p, &ids)
    };
    ck("PLANTED classical: executor-like library gains Stdlib.Logic.Classical_Prop.classic -> fails naming it",
        planted(&|p| {
            p.rocq.get_mut("proofs.e").map(|r| r.axioms.insert("Stdlib.Logic.Classical_Prop.classic".into()));
        })
        .iter()
        .any(|f| f.contains("Classical_Prop.classic")));
    ck("PLANTED an axiom removed from the shared set's library is an unrecorded shrink",
        !planted(&|p| {
            p.rocq.get_mut("proofs.a").map(|r| r.axioms.remove("RocqOfRust.M.run"));
        })
        .is_empty());
    ck("PLANTED Set turned impredicative on the Stdlib-only library fails",
        !planted(&|p| {
            p.rocq.get_mut("proofs.e").map(|r| r.impredicative = true);
        })
        .is_empty());
    ck("PLANTED a guard bypass (unsafe fixpoint) fails",
        !planted(&|p| {
            p.rocq.get_mut("proofs.e").map(|r| r.bypass.insert(("unsafe-fixpoint".into(), "proofs.e.loop".into())));
        })
        .is_empty());
    ck("PLANTED native_decide: a Lean constant now depends on Lean.ofReduceBool -> fails naming it",
        planted(&|p| {
            p.lean.get_mut("A").map(|r| r.axioms.insert("Lean.ofReduceBool".into(), 1));
        })
        .iter()
        .any(|f| f.contains("Lean.ofReduceBool")));
    ck("PLANTED a Lean theorem added (constants and theorems change) fails",
        !planted(&|p| {
            p.lean.get_mut("A").map(|r| {
                r.constants += 1;
                r.theorems += 1
            });
        })
        .is_empty());
    ck("a kernel-rejected Lean replay (native_decide, measured) is reported once, not as 8 lost modules",
        planted(&|p| {
            p.lean.clear();
            p.lean_rejected = true;
        })
        .iter()
        .all(|f| !f.starts_with("lean ")));
    ck("the failure diff names exactly the planted row (+ classic on proofs.e) and nothing else",
        {
            let mut p = o.clone();
            p.rocq.get_mut("proofs.e").map(|r| r.axioms.insert("Stdlib.Logic.Classical_Prop.classic".into()));
            let d = row_diff(&ledger, &p);
            d.len() == 3 && d[2] == "+ rocq-lib proofs.e axiom Stdlib.Logic.Classical_Prop.classic"
        });
    ck("PLANTED a library the audit lost fails (the ledger has rows for it)", !planted(&|p| { p.rocq.remove("proofs.b"); }).is_empty());
    ck("PLANTED a new audited library with no ledger row fails",
        !planted(&|p| {
            p.rocq.insert("proofs.z".into(), RocqReport::default());
        })
        .is_empty());
    let stale = parse_ledger(&format!("{text}rocq-set other X.y VER-X-001\n"));
    ck("ledger: a set no library uses is stale", stale.is_ok_and(|l| !compare(&l, &o, &ids).is_empty()));
    let dup_in_set = parse_ledger(&format!("{text}rocq-lib proofs.a axiom RocqOfRust.M.run VER-X-001\n"));
    ck("ledger: an axiom both in a used set and as an own row fails", dup_in_set.is_ok_and(|l| !compare(&l, &o, &ids).is_empty()));
    let unknown_id = parse_ledger(&text.replacen("VER-X-001", "VER-NOPE-001", 1));
    ck("ledger: a row naming an artifact that does not exist fails", unknown_id.is_ok_and(|l| !compare(&l, &o, &ids).is_empty()));
    for (what, bad) in [
        ("a duplicate row", format!("{text}rocq-lib proofs.e theory set-predicative VER-X-001\n")),
        ("a row without an artifact ID", format!("{text}rocq-lib proofs.e axiom X.y\n")),
        ("an unknown theory item", format!("{text}rocq-lib proofs.e theory type-in-type VER-X-001\n")),
        ("a zero lean-axiom row", format!("{text}lean-axiom A Classical.choice 0 VER-X-001\n")),
        ("an `admit` row (shrink-only is not implemented)", format!("{text}admit x y +1 @v0.11.0 #1 r\n")),
        ("mode = shrink-only", text.replace("mode = equality\nbaseline = none", &format!("mode = shrink-only\nbaseline = {}", "a".repeat(40)))),
        ("no mode line", text.replace("mode = equality\n", "")),
    ] {
        ck(&format!("ledger: {what} is rejected"), parse_ledger(&bad).is_err());
    }

    // -- bridge and coverage -----------------------------------------------------
    let rm = parse_rocq_manifest(rman).unwrap_or(RocqManifest { checker: String::new(), libs: Vec::new() });
    let lm = parse_lean_manifest(lman).unwrap_or(LeanManifest { lean: String::new(), paths: Vec::new(), mods: Vec::new() });
    let mut bo = Observed::default();
    bo.rocq.insert("proofs.a".into(), base(&["proofs.a.l1"]));
    bo.rocq.insert("proofs.b".into(), base(&[]));
    if let Some(r) = la.clone() {
        bo.lean.insert("A".into(), r);
    }
    let texts = |a: &str| -> BTreeMap<String, String> {
        [("proofs/a.v", a), ("proofs/b.v", "Lemma x : True. Proof. exact I. Qed.\n"), ("proofs/lean/A.lean", "theorem t1 : True := trivial\n")]
            .iter()
            .map(|(p, t)| (p.to_string(), t.to_string()))
            .collect()
    };
    ck("bridge: one Admitted lemma = one own axiom passes", bridge(&rm, &lm, &bo, &texts("Lemma l1 : False. Proof. Admitted.\n")).is_empty());
    ck("PLANTED bridge: an own axiom the spelling lexer does not see (0 spellings) fails",
        !bridge(&rm, &lm, &bo, &texts("Lemma l1 : False. Proof. exact I. Qed.\n")).is_empty());
    ck("PLANTED bridge: a Lean axiom constant with no `axiom` spelling fails",
        {
            let mut b2 = bo.clone();
            b2.lean.get_mut("A").map(|r| r.declared_axioms = 1);
            !bridge(&rm, &lm, &b2, &texts("Lemma l1 : False. Proof. Admitted.\n")).is_empty()
        });
    let tr = |v: &[&str]| v.iter().map(|s| s.to_string()).collect::<Vec<_>>();
    ck("coverage: the tracked proof files equal the audited ones", coverage(&rm, &lm, &tr(&["proofs/a.v", "proofs/b.v", "proofs/lean/A.lean", "tools/xtask/axioms/AxiomAudit.lean"])).is_empty());
    ck("PLANTED coverage: a tracked .v the target does not audit fails", !coverage(&rm, &lm, &tr(&["proofs/a.v", "proofs/b.v", "proofs/c.v", "proofs/lean/A.lean"])).is_empty());
    ck("PLANTED coverage: a tracked .lean the target does not audit fails", !coverage(&rm, &lm, &tr(&["proofs/a.v", "proofs/b.v", "proofs/lean/A.lean", "proofs/lean/B.lean"])).is_empty());

    // -- recorded real outputs (tools/xtask/fixtures/axioms, spike 2026-11-25) ------
    let fx = |f: &str| std::fs::read_to_string(repo.join("tools/xtask/fixtures/axioms").join(f));
    ck("RECORDED rocqchk -o stderr of proofs.mutex_proofs parses: impredicative, 92 axioms, no bypass",
        fx("rocqchk-mutex_proofs.stderr").map_err(|e| e.to_string()).and_then(|t| parse_rocqchk(&t)).is_ok_and(|r| r.impredicative && r.axioms.len() == 92 && r.bypass.is_empty()));
    ck("RECORDED rocqchk -o stderr of proofs.executor_proofs parses: predicative, 0 axioms",
        fx("rocqchk-executor_proofs.stderr").map_err(|e| e.to_string()).and_then(|t| parse_rocqchk(&t)).is_ok_and(|r| !r.impredicative && r.axioms.is_empty()));
    let sys = fx("lean-SysTick-n0nce.stdout").map_err(|e| e.to_string());
    ck("RECORDED the Lean audit of SysTick parses: 29 constants, 25 theorems",
        sys.clone().and_then(|t| parse_lean_output(&t, "n0nce", &["SysTick".to_string()])).is_ok_and(|r| {
            matches!(r, LeanRun::Ok(m) if m.get("SysTick").is_some_and(|s| s.constants == 29 && s.theorems == 25))
        }));
    ck("RECORDED ... cut before its `end` line is refused (truncated)",
        sys.is_ok_and(|t| parse_lean_output(&t[..t.rfind("end\t").unwrap_or(0)], "n0nce", &["SysTick".to_string()]).is_err()));

    // -- the real tree (no Bazel) ---------------------------------------------------
    match std::fs::read_to_string(repo.join(LEDGER)).map_err(|e| e.to_string()).and_then(|t| parse_ledger(&t)) {
        Ok(l) => ck(&format!("the committed {LEDGER} parses ({} rows)", l.rows.len()), !l.rows.is_empty()),
        Err(e) => ck(&format!("the committed {LEDGER} parses: {e}"), false),
    }
    ck(&format!("the trusted program {PROGRAM} exists and imports only Lean"),
        std::fs::read_to_string(repo.join(PROGRAM))
            .is_ok_and(|t| t.lines().filter(|l| l.starts_with("import ")).collect::<Vec<_>>() == ["import Lean"]));

    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

//! No image built for an ARMv6-M board carries code ARMv6-M cannot execute.
//!
//! The NUCLEO-G031K8 is a Cortex-M0+ (ARMv6-M, `thumbv6m-none-eabi`). Many gust
//! bins link a committed dissolved object (`*-cm3.o`) that synth produced for
//! ARMv7-M. The linker takes it without complaint and tags the output v6S-M, so
//! a G031 image can be full of Thumb-2 (`stmdb sp!`, movw/movt, ...) that an M0+
//! answers with UsageFault-less HardFault. Before this gate, the only guards were
//! one `compile_error!` (gust_wdg_silicon) and the hand-written `not_on` lists in
//! `xtask silicon`; a mnemonic blacklist scan missed 24 of 28 v7-only encodings.
//!
//! This gate decides by architecture facts, two independent legs per image:
//!
//! 1. BUILD ATTRIBUTES of every input the linker placed into the image, read
//!    from the linker map (`-Map`) with `llvm-readobj --arch-specific`. An input
//!    passes only if its `Tag_CPU_arch` is v6-M (11) or v6S-M (12) and its
//!    `Tag_CPU_arch_profile`, when present, is 'M'. Anything else, v7 and above
//!    (including v8-M.baseline, a strict superset of v6-M), the A-profile
//!    architectures, or an input with no attribute at all, fails. The output's
//!    own attributes are NOT used: the linker writes v6S-M over a v7 input.
//!
//! 2. A POSITIVE WHITELIST DECODE of every executable section. A first halfword
//!    whose top five bits are 0b11101/0b11110/0b11111 begins a 32-bit
//!    instruction; ARMv6-M has exactly six of those (BL, MSR, MRS, DMB, DSB,
//!    ISB), and MSR/MRS only with the v6-M special registers (BASEPRI and
//!    FAULTMASK are v7-M). Every other 32-bit encoding fails. The 16-bit space is
//!    whitelisted too, because ARMv7-M added 16-bit encodings the M0+ also
//!    traps on: CBZ/CBNZ, IT, and the unallocated hints. No mnemonic is matched
//!    anywhere; the decision is on encoding bits.
//!
//! LITERAL POOLS are data inside `.text`, and a pool word can look like a 32-bit
//! prefix (0xF000E800 does). They are separated by the AAELF32 mapping symbols:
//! `$d` starts data, `$t` starts Thumb code, `$a` starts ARM code (itself a
//! failure: an M-profile core has no ARM state). An STT_FUNC symbol restarts
//! code at its entry, so an object that emits no mapping symbols (synth's
//! do not) is still decoded rather than inheriting a neighbour's `$d`. Bytes no
//! symbol classifies are decoded as code: fail closed, never "assume data".
//!
//! What this cannot see: a `$d` deliberately placed over real code (the
//! toolchain is trusted to emit mapping symbols honestly); UNPREDICTABLE
//! register combinations inside an otherwise ARMv6-M encoding; code reached
//! from outside the executable sections (e.g. copied to RAM at run time). And
//! it is a static check: it says nothing about whether a clean image runs.
//!
//! The ledger (armv6m-isa.ledger) names the bins KNOWN to link ARMv7-M objects
//! (synth#1301). They are still REJECTED, and listed as such; the gate fails if
//! any other image is rejected, or if a ledgered bin stops being rejected
//! (delete its line), so the list only shrinks.

use crate::verdict::Verdict;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::Command;

pub const NAME: &str = "armv6m-isa";
const LEDGER: &str = "tools/xtask/armv6m-isa.ledger";
const CRATE_DIR: &str = "os/gust";
const TARGET: &str = "thumbv6m-none-eabi";
const BOARD_FEATURE: &str = "target-g031k8";

// --------------------------------------------------------------------------
// ELF32 little-endian, just enough: section headers and the symbol table.
// --------------------------------------------------------------------------

const ET_REL: u16 = 1;
const EM_ARM: u16 = 40;
const SHT_PROGBITS: u32 = 1;
const SHT_SYMTAB: u32 = 2;
const SHF_EXECINSTR: u32 = 4;
const STT_FUNC: u8 = 2;

struct Section {
    name: String,
    kind: u32,
    flags: u32,
    addr: u32,
    off: u32,
    size: u32,
}

struct Symbol {
    name: String,
    value: u32,
    kind: u8,
    shndx: u16,
}

struct Elf {
    etype: u16,
    machine: u16,
    sections: Vec<Section>,
    symbols: Vec<Symbol>,
    data: Vec<u8>,
}

fn u16_at(d: &[u8], o: usize) -> Result<u16, String> {
    d.get(o..o + 2).map(|b| u16::from_le_bytes([b[0], b[1]])).ok_or_else(|| format!("truncated at {o:#x}"))
}

fn u32_at(d: &[u8], o: usize) -> Result<u32, String> {
    d.get(o..o + 4)
        .map(|b| u32::from_le_bytes([b[0], b[1], b[2], b[3]]))
        .ok_or_else(|| format!("truncated at {o:#x}"))
}

fn cstr(d: &[u8], o: usize) -> String {
    let s = d.get(o..).unwrap_or(&[]);
    let n = s.iter().position(|&b| b == 0).unwrap_or(s.len());
    String::from_utf8_lossy(&s[..n]).into_owned()
}

fn parse_elf(data: Vec<u8>) -> Result<Elf, String> {
    if data.len() < 52 || &data[..4] != b"\x7fELF" {
        return Err("not an ELF file".into());
    }
    if data[4] != 1 || data[5] != 1 {
        return Err("not a 32-bit little-endian ELF".into());
    }
    let etype = u16_at(&data, 16)?;
    let machine = u16_at(&data, 18)?;
    let shoff = u32_at(&data, 32)? as usize;
    let shentsize = u16_at(&data, 46)? as usize;
    let shnum = u16_at(&data, 48)? as usize;
    let shstrndx = u16_at(&data, 50)? as usize;
    if shentsize != 40 {
        return Err(format!("section header size {shentsize}, expected 40"));
    }
    let mut raw = Vec::with_capacity(shnum);
    for i in 0..shnum {
        let b = shoff + i * 40;
        raw.push((
            u32_at(&data, b)?,      // name
            u32_at(&data, b + 4)?,  // type
            u32_at(&data, b + 8)?,  // flags
            u32_at(&data, b + 12)?, // addr
            u32_at(&data, b + 16)?, // offset
            u32_at(&data, b + 20)?, // size
            u32_at(&data, b + 24)?, // link
        ));
    }
    let strbase = raw.get(shstrndx).map(|r| r.4 as usize).ok_or("bad e_shstrndx")?;
    let sections: Vec<Section> = raw
        .iter()
        .map(|r| Section {
            name: cstr(&data, strbase + r.0 as usize),
            kind: r.1,
            flags: r.2,
            addr: r.3,
            off: r.4,
            size: r.5,
        })
        .collect();
    let mut symbols = Vec::new();
    for r in raw.iter().filter(|r| r.1 == SHT_SYMTAB) {
        let strtab = raw.get(r.6 as usize).map(|s| s.4 as usize).ok_or("bad symtab link")?;
        let n = r.5 as usize / 16;
        for i in 0..n {
            let b = r.4 as usize + i * 16;
            let info = *data.get(b + 12).ok_or("truncated symtab")?;
            symbols.push(Symbol {
                name: cstr(&data, strtab + u32_at(&data, b)? as usize),
                value: u32_at(&data, b + 4)?,
                kind: info & 0xf,
                shndx: u16_at(&data, b + 14)?,
            });
        }
    }
    Ok(Elf { etype, machine, sections, symbols, data })
}

// --------------------------------------------------------------------------
// Leg 2: the ARMv6-M encoding whitelist.
// --------------------------------------------------------------------------

/// The first halfword of a 32-bit Thumb instruction: top five bits 0b11101,
/// 0b11110 or 0b11111. The encoding rule, not a list of instructions.
fn is_32bit(h: u16) -> bool {
    matches!(h >> 11, 0b11101 | 0b11110 | 0b11111)
}

/// ARMv6-M MSR/MRS SYSm values: APSR/IAPSR/EAPSR/xPSR, IPSR/EPSR/IEPSR, MSP,
/// PSP, PRIMASK, CONTROL. BASEPRI (17, 18) and FAULTMASK (19) are ARMv7-M.
fn v6m_sysm(s: u16) -> bool {
    matches!(s, 0..=3 | 5..=9 | 16 | 20)
}

/// None if ARMv6-M executes this 32-bit encoding; otherwise why not.
fn reject32(h1: u16, h2: u16) -> Option<&'static str> {
    // BL: 11110 S imm10 : 11 J1 1 J2 imm11
    if h1 & 0xF800 == 0xF000 && h2 & 0xD000 == 0xD000 {
        return None;
    }
    // MSR: 11110 0111 000 Rn : 10 0 0 1000 SYSm
    if h1 & 0xFFF0 == 0xF380 && h2 & 0xFF00 == 0x8800 {
        return if v6m_sysm(h2 & 0xFF) { None } else { Some("MSR to a special register ARMv6-M lacks") };
    }
    // MRS: 11110 0111 110 1111 : 10 0 0 Rd SYSm
    if h1 == 0xF3EF && h2 & 0xF000 == 0x8000 {
        return if v6m_sysm(h2 & 0xFF) { None } else { Some("MRS from a special register ARMv6-M lacks") };
    }
    // DSB / DMB / ISB: 11110 0111 011 1111 : 10 0 0 1111 01xx option
    if h1 == 0xF3BF && matches!(h2 & 0xFFF0, 0x8F40 | 0x8F50 | 0x8F60) {
        return None;
    }
    Some("32-bit encoding outside ARMv6-M's {BL, MSR, MRS, DMB, DSB, ISB}")
}

/// None if ARMv6-M executes this 16-bit encoding; otherwise why not.
fn reject16(h: u16) -> Option<&'static str> {
    match h >> 12 {
        // shift/add/sub/mov/cmp, data processing, special data + BX/BLX, LDR
        // literal, load/store, ADR/ADD SP, LDM/STM, B<cond>/UDF/SVC, B.
        0x0..=0xA | 0xC | 0xD | 0xE => None,
        0xB => {
            if h & 0xFF00 == 0xB000 {
                None // ADD/SUB SP, #imm7
            } else if h & 0x0500 == 0x0100 {
                Some("CBZ/CBNZ (ARMv7-M)")
            } else if h & 0xFF00 == 0xB200 {
                None // SXTH, SXTB, UXTH, UXTB
            } else if h & 0xFE00 == 0xB400 || h & 0xFE00 == 0xBC00 {
                None // PUSH, POP
            } else if h & 0xFFE0 == 0xB660 {
                None // CPS
            } else if h & 0xFF00 == 0xBA00 {
                if (h >> 6) & 3 == 2 { Some("undefined in ARMv6-M (1011 1010 10)") } else { None } // REV, REV16, REVSH
            } else if h & 0xFF00 == 0xBE00 {
                None // BKPT
            } else if h & 0xFF00 == 0xBF00 {
                if h & 0xF != 0 {
                    Some("IT (ARMv7-M)")
                } else if (h >> 4) & 0xF <= 4 {
                    None // NOP, YIELD, WFE, WFI, SEV
                } else {
                    Some("hint ARMv6-M does not allocate")
                }
            } else {
                Some("undefined in ARMv6-M (miscellaneous 16-bit space)")
            }
        }
        _ => Some("32-bit prefix decoded as 16-bit (internal error)"),
    }
}

/// Walk `code` (halfword-aligned Thumb) and report every encoding ARMv6-M
/// cannot execute, at `base + offset`.
fn decode(code: &[u8], base: u32, out: &mut Vec<String>) {
    let mut i = 0usize;
    while i + 2 <= code.len() {
        let h1 = u16::from_le_bytes([code[i], code[i + 1]]);
        if is_32bit(h1) {
            if i + 4 > code.len() {
                out.push(format!("{:#010x}: {h1:04x} ....  32-bit prefix with no second halfword", base + i as u32));
                return;
            }
            let h2 = u16::from_le_bytes([code[i + 2], code[i + 3]]);
            if let Some(why) = reject32(h1, h2) {
                out.push(format!("{:#010x}: {h1:04x} {h2:04x}  {why}", base + i as u32));
            }
            i += 4;
        } else {
            if let Some(why) = reject16(h1) {
                out.push(format!("{:#010x}: {h1:04x}       {why}", base + i as u32));
            }
            i += 2;
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum State {
    Thumb,
    Data,
    Arm,
}

/// Split one executable section into (start, end, state) runs, by mapping
/// symbols, with STT_FUNC entries restarting Thumb code. Offsets are
/// section-relative.
fn runs(elf: &Elf, idx: usize) -> Vec<(u32, u32, State)> {
    let sec = &elf.sections[idx];
    let rel = elf.etype == ET_REL;
    let mut marks: Vec<(u32, u8, State)> = Vec::new();
    for s in elf.symbols.iter().filter(|s| s.shndx as usize == idx) {
        let at = if rel { s.value } else { s.value.wrapping_sub(sec.addr) };
        let base = s.name.split('.').next().unwrap_or("");
        // At one offset a function entry is applied first, so a mapping
        // symbol at the same place has the last word.
        let m = match base {
            "$t" => Some((1, State::Thumb)),
            "$d" => Some((1, State::Data)),
            "$a" => Some((1, State::Arm)),
            _ if s.kind == STT_FUNC => Some((0, State::Thumb)),
            _ => None,
        };
        if let Some((prio, st)) = m {
            let at = if st == State::Thumb && prio == 0 { at & !1 } else { at };
            if at < sec.size {
                marks.push((at, prio, st));
            }
        }
    }
    marks.sort_by_key(|m| (m.0, m.1));
    let mut out = Vec::new();
    let (mut cur, mut st) = (0u32, State::Thumb); // unclassified bytes are code: fail closed
    for (at, _, s) in marks {
        if at > cur {
            out.push((cur, at, st));
            cur = at;
        }
        st = s;
    }
    if cur < sec.size {
        out.push((cur, sec.size, st));
    }
    out
}

/// Leg 2 over every executable section of `elf`.
fn decode_elf(elf: &Elf) -> Vec<String> {
    let mut bad = Vec::new();
    for (idx, sec) in elf.sections.iter().enumerate() {
        if sec.kind != SHT_PROGBITS || sec.flags & SHF_EXECINSTR == 0 || sec.size == 0 {
            continue;
        }
        let Some(bytes) = elf.data.get(sec.off as usize..(sec.off + sec.size) as usize) else {
            bad.push(format!("{}: section extends past the file", sec.name));
            continue;
        };
        let base = if elf.etype == ET_REL { 0 } else { sec.addr };
        for (a, b, st) in runs(elf, idx) {
            match st {
                State::Data => {}
                State::Arm => bad.push(format!(
                    "{:#010x}: ARM-state code ($a) in {}: an M-profile core has no ARM state",
                    base + a,
                    sec.name
                )),
                State::Thumb => {
                    let start = (a + 1) & !1;
                    if start < b {
                        decode(&bytes[start as usize..b as usize], base + start, &mut bad);
                    }
                }
            }
        }
    }
    bad
}

// --------------------------------------------------------------------------
// Leg 1: the build attributes of every input.
// --------------------------------------------------------------------------

/// Tag_CPU_arch values ARMv6-M can execute: v6-M (11) and v6S-M (12).
fn arch_ok(arch: Option<u64>, profile: Option<u64>) -> Result<(), String> {
    let name = |a: u64| match a {
        0 => "pre-v4",
        1 => "v4",
        2 => "v4T",
        3 => "v5T",
        4 => "v5TE",
        5 => "v5TEJ",
        6 => "v6",
        7 => "v6KZ",
        8 => "v6T2",
        9 => "v6K",
        10 => "v7",
        11 => "v6-M",
        12 => "v6S-M",
        13 => "v7E-M",
        14 => "v8-A",
        15 => "v8-R",
        16 => "v8-M.baseline",
        17 => "v8-M.mainline",
        18 => "v8.1-A",
        19 => "v8.2-A",
        20 => "v8.3-A",
        21 => "v8.1-M.mainline",
        22 => "v9-A",
        _ => "unknown",
    };
    match arch {
        None => Err("no Tag_CPU_arch: architecture unknown".into()),
        Some(a @ (11 | 12)) => match profile {
            None | Some(77) => Ok(()),
            Some(p) => Err(format!("Tag_CPU_arch {} but Tag_CPU_arch_profile {p} (not 'M')", name(a))),
        },
        Some(a) => Err(format!("Tag_CPU_arch {a} ({}): not ARMv6-M", name(a))),
    }
}

/// `llvm-readobj --arch-specific` output -> (file label, CPU_arch, profile).
/// An archive prints one `File: ar(member)` block per member, and the label is
/// the spelling the linker map uses.
fn parse_attrs(text: &str) -> BTreeMap<String, (Option<u64>, Option<u64>)> {
    let mut out: BTreeMap<String, (Option<u64>, Option<u64>)> = BTreeMap::new();
    let mut cur: Option<String> = None;
    let mut pending: Option<u64> = None;
    for line in text.lines() {
        let t = line.trim();
        if let Some(f) = t.strip_prefix("File: ") {
            cur = Some(f.to_string());
            out.entry(f.to_string()).or_insert((None, None));
            pending = None;
        } else if let Some(v) = t.strip_prefix("Tag: ") {
            pending = v.trim().parse().ok();
        } else if let Some(v) = t.strip_prefix("Value: ") {
            if let (Some(f), Some(tag)) = (&cur, pending.take()) {
                let val: Option<u64> = v.trim().parse().ok();
                let e = out.get_mut(f).unwrap();
                match tag {
                    6 => e.0 = val,
                    7 => e.1 = val,
                    _ => {}
                }
            }
        }
    }
    out
}

/// `llvm-readobj` from the rustup `llvm-tools` component of the toolchain that
/// builds the image, or from `$LLVM_READOBJ`, or from PATH.
fn readobj() -> Result<PathBuf, String> {
    if let Some(p) = std::env::var_os("LLVM_READOBJ") {
        return Ok(PathBuf::from(p));
    }
    let run = |args: &[&str]| -> Option<String> {
        let o = Command::new("rustc").args(args).output().ok()?;
        o.status.success().then(|| String::from_utf8_lossy(&o.stdout).trim().to_string())
    };
    if let (Some(sysroot), Some(vv)) = (run(&["--print", "sysroot"]), run(&["-vV"])) {
        if let Some(host) = vv.lines().find_map(|l| l.strip_prefix("host: ")) {
            let p = Path::new(&sysroot).join("lib/rustlib").join(host).join("bin/llvm-readobj");
            if p.exists() {
                return Ok(p);
            }
        }
    }
    for c in ["llvm-readobj", "llvm-readelf"] {
        if Command::new(c).arg("--version").output().map(|o| o.status.success()).unwrap_or(false) {
            return Ok(PathBuf::from(c));
        }
    }
    Err("no llvm-readobj: `rustup component add llvm-tools`, or set LLVM_READOBJ".into())
}

/// Read the attributes of every input label (`path` or `archive(member)`).
/// Each archive or object is passed once; a label readobj did not report is
/// an error, never a pass.
fn attrs_of(ro: &Path, labels: &BTreeSet<String>) -> Result<BTreeMap<String, (Option<u64>, Option<u64>)>, String> {
    let mut files: BTreeSet<String> = BTreeSet::new();
    for l in labels {
        let file = match l.find('(') {
            Some(p) if l.ends_with(')') => &l[..p],
            _ => l.as_str(),
        };
        if !Path::new(file).exists() {
            return Err(format!("linker input {file} no longer exists (built without -C save-temps?)"));
        }
        files.insert(file.to_string());
    }
    let mut all = BTreeMap::new();
    for f in &files {
        let o = Command::new(ro).arg("--arch-specific").arg(f).output().map_err(|e| format!("{}: {e}", ro.display()))?;
        let text = String::from_utf8_lossy(&o.stdout);
        all.extend(parse_attrs(&text));
    }
    for l in labels {
        if !all.contains_key(l) {
            return Err(format!("llvm-readobj reported nothing for linker input {l}"));
        }
    }
    Ok(all)
}

/// Every input file the linker placed a section from, from an lld `-Map`.
/// The header fixes the columns: an input-section line has its text starting
/// at the `In` column and reads `<file>:(<section>)`.
fn map_inputs(map: &str) -> Result<BTreeSet<String>, String> {
    let mut lines = map.lines();
    let header = lines.next().ok_or("empty linker map")?;
    let in_col = header.find(" In ").map(|p| p + 1).ok_or("linker map header has no `In` column")?;
    let mut out = BTreeSet::new();
    for l in lines {
        if l.len() <= in_col || l.as_bytes()[in_col] == b' ' || l.as_bytes()[in_col - 1] != b' ' {
            continue;
        }
        let rest = &l[in_col..];
        let Some(p) = rest.rfind(":(") else { continue };
        let file = &rest[..p];
        if file.starts_with('<') {
            continue; // <internal>: linker-synthesised, no input file
        }
        out.insert(file.to_string());
    }
    if out.is_empty() {
        return Err("linker map names no input files".into());
    }
    Ok(out)
}

// --------------------------------------------------------------------------
// One ELF, both legs.
// --------------------------------------------------------------------------

struct Report {
    inputs: usize,
    attr_bad: Vec<String>,
    code_bad: Vec<String>,
}

impl Report {
    fn clean(&self) -> bool {
        self.attr_bad.is_empty() && self.code_bad.is_empty()
    }
    fn summary(&self) -> String {
        let mut s = format!(
            "{} of {} input(s) not ARMv6-M, {} encoding(s) ARMv6-M cannot execute",
            self.attr_bad.len(),
            self.inputs,
            self.code_bad.len()
        );
        for a in self.attr_bad.iter().take(3) {
            s.push_str(&format!("\n      input  {a}"));
        }
        for c in self.code_bad.iter().take(3) {
            s.push_str(&format!("\n      code   {c}"));
        }
        s
    }
}

/// A relocatable object is its own (only) input; a linked image needs the map
/// that lists its inputs. `Err` = could not check (not a pass).
fn check_elf(ro: &Path, path: &Path, map: Option<&Path>) -> Result<Report, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let elf = parse_elf(data).map_err(|e| format!("{}: {e}", path.display()))?;
    if elf.machine != EM_ARM {
        return Err(format!("{}: e_machine {} is not ARM", path.display(), elf.machine));
    }
    let labels: BTreeSet<String> = if elf.etype == ET_REL {
        [path.to_string_lossy().into_owned()].into()
    } else {
        let Some(m) = map else {
            return Err(format!("{}: a linked image needs its linker map to see its inputs", path.display()));
        };
        let t = std::fs::read_to_string(m).map_err(|e| format!("{}: {e}", m.display()))?;
        map_inputs(&t)?
    };
    let attrs = attrs_of(ro, &labels)?;
    let mut attr_bad = Vec::new();
    for l in &labels {
        let (a, p) = attrs[l];
        if let Err(why) = arch_ok(a, p) {
            let short = l.rsplit('/').next().unwrap_or(l);
            attr_bad.push(format!("{short}: {why}"));
        }
    }
    Ok(Report { inputs: labels.len(), attr_bad, code_bad: decode_elf(&elf) })
}

// --------------------------------------------------------------------------
// The gate: build every gust bin for the G031K8, check every image.
// --------------------------------------------------------------------------

fn bins(repo: &Path) -> Result<Vec<String>, String> {
    let dir = repo.join(CRATE_DIR);
    let mut v = Vec::new();
    if dir.join("src/main.rs").exists() {
        v.push("gust".to_string());
    }
    let bin = dir.join("src/bin");
    let rd = std::fs::read_dir(&bin).map_err(|e| format!("{}: {e}", bin.display()))?;
    for e in rd.flatten() {
        let p = e.path();
        if p.extension().is_some_and(|x| x == "rs") {
            v.push(p.file_stem().unwrap().to_string_lossy().into_owned());
        } else if p.join("main.rs").exists() {
            v.push(p.file_name().unwrap().to_string_lossy().into_owned());
        }
    }
    v.sort();
    Ok(v)
}

fn ledger(repo: &Path) -> Result<BTreeSet<String>, String> {
    let t = std::fs::read_to_string(repo.join(LEDGER)).map_err(|e| format!("{LEDGER}: {e}"))?;
    Ok(t.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(String::from)
        .collect())
}

/// The ratchet: rejected images must equal the ledger.
fn compare(rejected: &BTreeSet<String>, built: &BTreeSet<String>, led: &BTreeSet<String>) -> Vec<String> {
    let mut f = Vec::new();
    for r in rejected.difference(led) {
        f.push(format!("FAIL: {r} contains code ARMv6-M cannot execute and is not on the ledger"));
    }
    for l in led.difference(rejected) {
        if built.contains(l) {
            f.push(format!("FAIL: {l} is on the ledger but its image is now clean -- delete its ledger line"));
        } else {
            f.push(format!("FAIL: {l} is on the ledger but no longer builds for {TARGET} -- delete its ledger line"));
        }
    }
    f
}

fn first_error(stderr: &str) -> String {
    stderr
        .lines()
        .find(|l| l.starts_with("error"))
        .unwrap_or("build failed")
        .chars()
        .take(140)
        .collect()
}

pub fn run(repo: &Path, module: Option<&str>) -> Verdict {
    let ro = match readobj() {
        Ok(r) => r,
        Err(e) => return Verdict::Refused(e),
    };
    // --module PATH: one ELF, strict. An image needs PATH.map beside it.
    if let Some(m) = module {
        let p = PathBuf::from(m);
        let map = PathBuf::from(format!("{m}.map"));
        return match check_elf(&ro, &p, map.exists().then_some(map.as_path())) {
            Err(e) => Verdict::Refused(e),
            Ok(r) if r.clean() => Verdict::Pass(vec![format!("CLEAN {m}: {}", r.summary())]),
            Ok(r) => {
                let mut l = vec![format!(
                    "REJECT {m}: {} of {} input(s) not ARMv6-M, {} encoding(s) ARMv6-M cannot execute",
                    r.attr_bad.len(),
                    r.inputs,
                    r.code_bad.len()
                )];
                l.extend(r.attr_bad.iter().map(|a| format!("  input {a}")));
                l.extend(r.code_bad.iter().map(|c| format!("  code  {c}")));
                Verdict::Fail(l)
            }
        };
    }

    let names = match bins(repo) {
        Ok(b) => b,
        Err(e) => return Verdict::Refused(e),
    };
    let led = match ledger(repo) {
        Ok(l) => l,
        Err(e) => return Verdict::Refused(e),
    };
    // A dedicated target dir: the map and the image must come from the same
    // link, and a plain `cargo build` into a shared dir would replace the image
    // and leave the map describing a different one.
    let tdir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("target"))
        .join("armv6m-isa");
    let maps = tdir.join("maps");
    if let Err(e) = std::fs::create_dir_all(&maps) {
        return Verdict::Refused(format!("{}: {e}", maps.display()));
    }
    let mut lines = Vec::new();
    let (mut built, mut rejected, mut clean) = (BTreeSet::new(), BTreeSet::new(), Vec::new());
    let mut no_image = Vec::new();
    for b in &names {
        let map = maps.join(format!("{b}.map"));
        let o = Command::new("cargo")
            .current_dir(repo.join(CRATE_DIR))
            .env("CARGO_TARGET_DIR", &tdir)
            .args(["rustc", "--release", "--target", TARGET, "--no-default-features", "--features", BOARD_FEATURE, "--bin", b, "--"])
            .arg(format!("-Clink-arg=-Map={}", map.display()))
            .arg("-Csave-temps")
            .output();
        let o = match o {
            Ok(o) => o,
            Err(e) => return Verdict::Refused(format!("cargo: {e}")),
        };
        if !o.status.success() {
            no_image.push(format!("{b}: {}", first_error(&String::from_utf8_lossy(&o.stderr))));
            continue;
        }
        let img = tdir.join(TARGET).join("release").join(b);
        if !map.exists() {
            return Verdict::Refused(format!("{b}: built, but the linker wrote no map at {}", map.display()));
        }
        let r = match check_elf(&ro, &img, Some(&map)) {
            Ok(r) => r,
            Err(e) => return Verdict::Refused(e),
        };
        built.insert(b.clone());
        if r.clean() {
            clean.push(b.clone());
            eprintln!("CLEAN   {b}");
        } else {
            eprintln!("REJECT  {b}");
            lines.push(format!("REJECT {b}{}: {}", if led.contains(b) { " (ledgered)" } else { "" }, r.summary()));
            rejected.insert(b.clone());
        }
    }
    if built.is_empty() {
        return Verdict::Refused(format!("no bin built for {TARGET}: nothing was checked"));
    }
    let mut head = vec![
        format!(
            "{} bin(s); {} built for {TARGET} ({BOARD_FEATURE}); {} rejected, {} clean; {} produce no image",
            names.len(),
            built.len(),
            rejected.len(),
            clean.len(),
            no_image.len()
        ),
        format!("clean: {}", clean.join(", ")),
    ];
    head.extend(no_image.iter().map(|n| format!("no image  {n}")));
    head.append(&mut lines);
    let fails = compare(&rejected, &built, &led);
    if fails.is_empty() {
        head.push(format!(
            "rejected set equals the ledger ({}): every other ARMv6-M image is clean, and the list only shrinks",
            led.len()
        ));
        Verdict::Pass(head)
    } else {
        head.extend(fails);
        Verdict::Fail(head)
    }
}

// --------------------------------------------------------------------------
// Self-test: planted objects, written as real ELF files and read back through
// the same parser and the same llvm-readobj the gate uses.
// --------------------------------------------------------------------------

/// The 28 ARMv7-M-only encodings (assembled with -mcpu=cortex-m3, halfwords in
/// stream order). 24 are 32-bit; IT, CBZ, CBNZ are the 16-bit ones.
pub(crate) const V7_ONLY: &[(&str, &[u16])] = &[
    ("stmdb sp!, {r4-r8, lr}", &[0xe92d, 0x41f0]),
    ("ldmia.w sp!, {r4-r8, pc}", &[0xe8bd, 0x81f0]),
    ("movw r0, #0x1234", &[0xf241, 0x2034]),
    ("movt r0, #0x5678", &[0xf2c5, 0x6078]),
    ("mls r0, r1, r2, r3", &[0xfb01, 0x3012]),
    ("umull r0, r1, r2, r3", &[0xfba2, 0x0103]),
    ("clz r0, r1", &[0xfab1, 0xf081]),
    ("ldrd r0, r1, [r2]", &[0xe9d2, 0x0100]),
    ("addw r0, r1, #0xfff", &[0xf601, 0x70ff]),
    ("bic.w r0, r1, #0xff00", &[0xf421, 0x407f]),
    ("eor.w r0, r1, r2", &[0xea81, 0x0002]),
    ("uxtb.w r0, r1, ror #8", &[0xfa5f, 0xf091]),
    ("ldrh.w r0, [r1, #2]", &[0xf8b1, 0x0002]),
    ("sbfx r0, r1, #3, #4", &[0xf341, 0x00c3]),
    ("msr BASEPRI, r0", &[0xf380, 0x8811]),
    ("ldrex r0, [r1]", &[0xe851, 0x0f00]),
    ("bne.w", &[0xf47f, 0xaffe]),
    ("sdiv r0, r1, r2", &[0xfb91, 0xf0f2]),
    ("udiv r0, r1, r2", &[0xfbb1, 0xf0f2]),
    ("it eq", &[0xbf08]),
    ("cbz r0, +2", &[0xb100]),
    ("cbnz r0, +2", &[0xb900]),
    ("tbb [r0, r1]", &[0xe8d0, 0xf001]),
    ("b.w", &[0xf7ff, 0xbffe]),
    ("ubfx r0, r1, #3, #4", &[0xf3c1, 0x00c3]),
    ("strd r0, r1, [r2]", &[0xe9c2, 0x0100]),
    ("mrs r0, BASEPRI", &[0xf3ef, 0x8011]),
    ("strex r0, r1, [r2]", &[0xe842, 0x1000]),
];

/// ARMv6-M code (assembled with -mcpu=cortex-m0): every 32-bit encoding v6-M
/// has, and a 16-bit sample of each class. Ends at 0x52; a literal pool follows.
const V6_CODE: &[u16] = &[
    0xb5f0, 0x2001, 0x1840, 0x4a15, 0x4b12, // push, movs, adds, ldr =, ldr lit
    0xf7ff, 0xfffe, // bl
    0xf3ef, 0x8010, 0xf380, 0x8810, // mrs/msr PRIMASK
    0xf3ef, 0x8114, 0xf381, 0x8814, // mrs/msr CONTROL
    0xf3bf, 0x8f5f, 0xf3bf, 0x8f4f, 0xf3bf, 0x8f6f, // dmb, dsb, isb
    0xb672, 0xb662, 0xba08, 0xb248, 0xb288, 0xc806, 0xc006, // cpsid/ie, rev, sxtb, uxth, ldm, stm
    0xbf30, 0xbf20, 0xbf40, 0x46c0, 0xbf10, 0xdf00, 0xbe00, 0xde00, // wfi wfe sev nop yield svc bkpt udf
    0xd0fe, 0xe7d9, 0x4780, 0x4770, 0xbdf0, 0x46c0, // beq, b, blx, bx, pop, pad
];

/// A literal pool whose words decode as v7 32-bit instructions if read as code:
/// 0xF000E800 (halfword e800 is a 32-bit prefix), udiv, stmdb.
const POOL: &[u32] = &[0xf000_e800, 0xfbb1_f0f2, 0xe92d_41f0];

/// A `.ARM.attributes` section: Tag_CPU_arch / Tag_CPU_arch_profile as given.
fn attributes(arch: Option<u8>, profile: Option<u8>) -> Vec<u8> {
    let mut a = Vec::new();
    if let Some(x) = arch {
        a.extend([6, x]);
    }
    if let Some(p) = profile {
        a.extend([7, p]);
    }
    a.extend([9, 1]); // Tag_THUMB_ISA_use
    let sub = 4 + 6 + 1 + 4 + a.len();
    let mut s = vec![b'A'];
    s.extend((sub as u32).to_le_bytes());
    s.extend(b"aeabi\0");
    s.push(1); // Tag_File
    s.extend(((1 + 4 + a.len()) as u32).to_le_bytes());
    s.extend(a);
    s
}

/// A minimal ELF32 ET_REL: .text, .ARM.attributes, .symtab, .strtab, .shstrtab.
/// `syms` are (name, value, STT type) in .text.
fn write_object(text: &[u8], attrs: &[u8], syms: &[(&str, u32, u8)]) -> Vec<u8> {
    let shstr = b"\0.text\0.ARM.attributes\0.symtab\0.strtab\0.shstrtab\0";
    let (n_text, n_attr, n_sym, n_str, n_shstr) = (1u32, 7u32, 23u32, 31u32, 39u32);
    let mut strtab = vec![0u8];
    let mut symtab = vec![0u8; 16]; // null symbol
    for (name, value, kind) in syms {
        let off = strtab.len() as u32;
        strtab.extend(name.as_bytes());
        strtab.push(0);
        symtab.extend(off.to_le_bytes());
        symtab.extend(value.to_le_bytes());
        symtab.extend(0u32.to_le_bytes());
        symtab.push(*kind); // STB_LOCAL
        symtab.push(0);
        symtab.extend(1u16.to_le_bytes()); // .text
    }
    fn place(b: &mut Vec<u8>, d: &[u8]) -> u32 {
        while b.len() % 4 != 0 {
            b.push(0);
        }
        let o = b.len() as u32;
        b.extend(d);
        o
    }
    let mut body = vec![0u8; 52];
    let o_text = place(&mut body, text);
    let o_attr = place(&mut body, attrs);
    let o_sym = place(&mut body, &symtab);
    let o_str = place(&mut body, &strtab);
    let o_shstr = place(&mut body, shstr);
    while body.len() % 4 != 0 {
        body.push(0);
    }
    let shoff = body.len() as u32;
    let sh = |name: u32, kind: u32, flags: u32, off: u32, size: u32, link: u32, info: u32, align: u32, ent: u32| {
        let mut v = Vec::new();
        for x in [name, kind, flags, 0, off, size, link, info, align, ent] {
            v.extend(x.to_le_bytes());
        }
        v
    };
    body.extend(vec![0u8; 40]);
    body.extend(sh(n_text, SHT_PROGBITS, 2 | SHF_EXECINSTR, o_text, text.len() as u32, 0, 0, 4, 0));
    body.extend(sh(n_attr, 0x7000_0003, 0, o_attr, attrs.len() as u32, 0, 0, 1, 0));
    body.extend(sh(n_sym, SHT_SYMTAB, 0, o_sym, symtab.len() as u32, 4, syms.len() as u32 + 1, 4, 16));
    body.extend(sh(n_str, 3, 0, o_str, strtab.len() as u32, 0, 0, 1, 0));
    body.extend(sh(n_shstr, 3, 0, o_shstr, shstr.len() as u32, 0, 0, 1, 0));
    // ELF header
    body[..4].copy_from_slice(b"\x7fELF");
    body[4] = 1; // ELFCLASS32
    body[5] = 1; // little-endian
    body[6] = 1; // EV_CURRENT
    body[16..18].copy_from_slice(&ET_REL.to_le_bytes());
    body[18..20].copy_from_slice(&EM_ARM.to_le_bytes());
    body[20..24].copy_from_slice(&1u32.to_le_bytes());
    body[32..36].copy_from_slice(&shoff.to_le_bytes());
    body[36..40].copy_from_slice(&0x0500_0000u32.to_le_bytes()); // EABI5
    body[40..42].copy_from_slice(&52u16.to_le_bytes());
    body[46..48].copy_from_slice(&40u16.to_le_bytes());
    body[48..50].copy_from_slice(&6u16.to_le_bytes());
    body[50..52].copy_from_slice(&5u16.to_le_bytes());
    body
}

fn halfwords(h: &[u16]) -> Vec<u8> {
    h.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The clean object: v6-M code, then a `$d`-marked literal pool, inside one
/// STT_FUNC (as the assembler lays it out).
fn clean_text() -> (Vec<u8>, u32) {
    let mut t = halfwords(V6_CODE);
    let pool_at = t.len() as u32;
    for w in POOL {
        t.extend(w.to_le_bytes());
    }
    (t, pool_at)
}

pub fn self_test(_repo: &Path, _t: Option<&str>) -> Verdict {
    let mut lines = Vec::new();
    let mut broken = false;
    let mut ck = |what: String, got: bool| {
        lines.push(format!("{what}: {}", if got { "ok" } else { "WRONG" }));
        if !got {
            broken = true;
        }
    };
    let ro = match readobj() {
        Ok(r) => r,
        Err(e) => return Verdict::Refused(e),
    };
    let dir = std::env::temp_dir().join(format!("gale-armv6m-isa-selftest-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Verdict::Refused(format!("{}: {e}", dir.display()));
    }
    let write = |name: &str, bytes: Vec<u8>| -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).expect("write self-test object");
        p
    };
    let v6s_m = attributes(Some(12), Some(b'M'));
    let check = |p: &Path| check_elf(&ro, p, None);

    // 1. The clean object passes BOTH legs, pool and all.
    let (t, pool_at) = clean_text();
    let syms = [("g", 1u32, STT_FUNC), ("$t", 0, 0), ("$d", pool_at, 0)];
    let clean = write("clean.o", write_object(&t, &v6s_m, &syms));
    match check(&clean) {
        Ok(r) => {
            ck(format!("clean v6-M object (BL, MSR, MRS, DMB, DSB, ISB + 16-bit sample) passes the decode leg [{}]", r.code_bad.join("; ")), r.code_bad.is_empty());
            ck(format!("clean v6S-M object passes the attribute leg [{}]", r.attr_bad.join("; ")), r.attr_bad.is_empty() && r.inputs == 1);
        }
        Err(e) => ck(format!("clean object could be checked ({e})"), false),
    }

    // 2. Each ARMv7-M-only encoding, alone in a v6S-M-tagged object, is caught
    //    by the DECODE leg (attributes cannot help: they say v6S-M).
    let mut caught = 0;
    for (asm, hw) in V7_ONLY {
        let mut code = halfwords(&[0x46c0]); // nop before
        code.extend(halfwords(hw));
        code.extend(halfwords(&[0x46c0, 0x4770])); // nop; bx lr
        let p = write("v7-one.o", write_object(&code, &v6s_m, &[("f", 1, STT_FUNC), ("$t", 0, 0)]));
        let ok = match check(&p) {
            Ok(r) => r.attr_bad.is_empty() && r.code_bad.len() == 1 && r.code_bad[0].starts_with("0x00000002"),
            Err(_) => false,
        };
        if ok {
            caught += 1;
        } else {
            ck(format!("decode leg rejects `{asm}` at its own address"), false);
        }
    }
    ck(format!("decode leg rejects all {} ARMv7-M-only encodings ({caught} caught)", V7_ONLY.len()), caught == V7_ONLY.len());

    // 3. The ATTRIBUTE leg alone: v6-M code tagged with a non-v6-M arch, or
    //    with no arch at all, is rejected though every instruction decodes.
    for (what, arch, prof) in [
        ("v7 (10)", Some(10u8), Some(b'M')),
        ("v7E-M (13)", Some(13), Some(b'M')),
        ("v8-M.baseline (16)", Some(16), Some(b'M')),
        ("v6S-M with profile 'A'", Some(12), Some(b'A')),
        ("no Tag_CPU_arch", None, None),
    ] {
        let p = write("attr.o", write_object(&t, &attributes(arch, prof), &syms));
        let ok = matches!(check(&p), Ok(r) if r.code_bad.is_empty() && r.attr_bad.len() == 1);
        ck(format!("attribute leg rejects clean code tagged {what}"), ok);
    }
    let p = write("attr-v6m.o", write_object(&t, &attributes(Some(11), Some(b'M')), &syms));
    ck("attribute leg accepts v6-M (11)".into(), matches!(check(&p), Ok(r) if r.clean()));

    // 4. Literal pools: the same pool with no `$d` is decoded, and its words
    //    are v7 encodings -- so the mapping symbol is what makes the clean
    //    object clean, and dropping it fails closed.
    let p = write("nodollar.o", write_object(&t, &v6s_m, &[("g", 1, STT_FUNC), ("$t", 0, 0)]));
    ck("an unmarked literal pool is decoded (fail closed), not assumed data".into(),
        matches!(check(&p), Ok(r) if !r.code_bad.is_empty()));
    // A function with no mapping symbol after a `$d` is code again (synth
    // emits no mapping symbols).
    let mut t2 = t.clone();
    let f2 = t2.len() as u32;
    t2.extend(halfwords(&[0xe92d, 0x41f0])); // stmdb, after the pool
    let p = write("fnreset.o", write_object(&t2, &v6s_m, &[("g", 1, STT_FUNC), ("$t", 0, 0), ("$d", pool_at, 0), ("h", f2 | 1, STT_FUNC)]));
    ck("an STT_FUNC entry after a `$d` restarts code (no-mapping-symbol objects)".into(),
        matches!(check(&p), Ok(r) if r.code_bad.len() == 1));
    // `$a` is ARM state: no M-profile core has it.
    let p = write("arm.o", write_object(&t, &v6s_m, &[("$a", 0, 0), ("$d", pool_at, 0)]));
    ck("`$a` ARM-state code is rejected".into(), matches!(check(&p), Ok(r) if !r.code_bad.is_empty()));
    // A 32-bit prefix cut off by a pool boundary.
    ck("a truncated 32-bit prefix is rejected".into(), {
        let mut v = Vec::new();
        decode(&halfwords(&[0xf7ff]), 0, &mut v);
        v.len() == 1
    });

    // 5. The linker-map reader finds plain and archive-member inputs, and
    //    skips symbol lines and <internal>.
    let map = "     VMA      LMA     Size Align Out     In      Symbol\n\
               \x20      0        0       c0     4 .vector_table\n\
               \x20      0        0       c0     4         /t/rel/deps/gust-1.rcgu.o:(.vector_table)\n\
               \x20      0        0        0     1                 __RESET_VECTOR\n\
               \x20     c0       c0       10     4 .text\n\
               \x20     c0       c0        8     4         /d/wdg-thin-cm3.o:(.text)\n\
               \x20     c8       c8        8     4         /s/libcompiler_builtins.rlib(cb.cgu.017.rcgu.o):(.text.memcpy)\n\
               \x20     d0       d0        4     4         <internal>:(.ARM.exidx)\n";
    let got = map_inputs(map).unwrap_or_default();
    ck("map reader lists 3 inputs (object, archive member, committed .o)".into(),
        got.len() == 3 && got.contains("/s/libcompiler_builtins.rlib(cb.cgu.017.rcgu.o)") && got.contains("/d/wdg-thin-cm3.o"));

    // 6. The ratchet.
    let s = |v: &[&str]| v.iter().map(|x| x.to_string()).collect::<BTreeSet<String>>();
    ck("rejected == ledger passes".into(), compare(&s(&["a"]), &s(&["a", "b"]), &s(&["a"])).is_empty());
    ck("a rejected image off the ledger fails".into(), compare(&s(&["a", "b"]), &s(&["a", "b"]), &s(&["a"])).len() == 1);
    ck("a ledgered image that is now clean fails".into(), compare(&s(&[]), &s(&["a"]), &s(&["a"])).len() == 1);
    ck("a ledgered bin that no longer builds fails".into(), compare(&s(&[]), &s(&[]), &s(&["a"])).len() == 1);

    let _ = std::fs::remove_dir_all(&dir);
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

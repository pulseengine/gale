//! No image built for an ARMv6-M board carries code ARMv6-M cannot execute.
//!
//! The NUCLEO-G031K8 is a Cortex-M0+ (ARMv6-M, `thumbv6m-none-eabi`). Many gust
//! bins link a committed dissolved object (`*-cm3.o`) that synth produced for
//! ARMv7-M. The linker takes it without complaint and tags the output v6S-M, so
//! a G031 image can be full of Thumb-2 (`stmdb sp!`, movw/movt, ...) that an M0+
//! answers with a HardFault. Before this gate, the only guards were one
//! `compile_error!` (gust_wdg_silicon) and the hand-written `not_on` lists in
//! `xtask silicon`; a mnemonic blacklist scan missed 24 of 28 v7-only encodings.
//!
//! WHAT IS BUILT: every os/gust bin with the board feature alone, plus every
//! (bin, extra features) pair `xtask silicon`'s matrix names (read from that
//! table, not copied), each linked with `-Map` and `-C save-temps`.
//!
//! Each image is decided by architecture facts, two independent legs:
//!
//! 1. BUILD ATTRIBUTES of every input the linker placed into the image (plain
//!    objects and archive members alike), read from the linker map with
//!    `llvm-readobj --arch-specific`. An input passes only if its
//!    `Tag_CPU_arch` is v6-M (11) or v6S-M (12) and its `Tag_CPU_arch_profile`,
//!    when present, is 'M'. Anything else, v7 and above (including
//!    v8-M.baseline, a strict superset of v6-M), the older A-profile
//!    architectures, or no tag at all, fails. The output's own attributes are
//!    NOT used: the linker writes v6S-M over a v7 input. Relative paths in the
//!    map are resolved against the link's working directory (os/gust).
//!
//! 2. A POSITIVE WHITELIST DECODE of every executable section, whatever its
//!    name. A first halfword whose top five bits are 0b11101/0b11110/0b11111
//!    begins a 32-bit instruction; ARMv6-M has exactly six of those (BL, MSR,
//!    MRS, DMB, DSB, ISB), MSR/MRS only with the v6-M special registers (not
//!    BASEPRI, BASEPRI_MAX, FAULTMASK, nor reserved SYSm values). Every other
//!    32-bit encoding fails, BLX-immediate and CLREX included. The 16-bit space
//!    is whitelisted too, because ARMv7-M added 16-bit encodings the M0+ also
//!    traps on: CBZ/CBNZ, IT, the unallocated hints; and CPS is accepted only as
//!    `cpsie i`/`cpsid i` (v6-M has no F bit). No mnemonic is matched anywhere.
//!
//! LITERAL POOLS are data inside `.text`, and a pool word can look like a 32-bit
//! prefix (0xF000E800 does). They are separated by the AAELF32 mapping symbols:
//! `$d` starts data, `$t` starts Thumb code, `$a` starts ARM code (itself a
//! failure: an M-profile core has no ARM state). Code restarts at every
//! STT_FUNC entry and, in a linked image, at the start of every input section
//! the map names. So an input that carries no mapping symbols (synth's do not)
//! never inherits the `$d` its neighbour ended on. Bytes no symbol classifies
//! are decoded as code: fail closed, never "assume data".
//!
//! What this cannot see:
//! - a `$d` deliberately placed over real code (the toolchain is trusted to
//!   emit mapping symbols honestly);
//! - in an input with NO mapping symbols, a data halfword in 0xF000-0xF7FF
//!   placed directly before a real 0xFxxx instruction: the pair decodes as a BL
//!   and swallows the real instruction's first halfword, so what follows is
//!   decoded off by one halfword and an ARMv7-M instruction there can be missed;
//! - UNPREDICTABLE register combinations inside an otherwise ARMv6-M encoding;
//! - code reached from outside the executable sections (e.g. copied to RAM);
//! - whether a clean image actually runs: this is a static check.
//!
//! THE LEDGER (armv6m-isa.ledger) is compared for equality, both directions:
//! - `reject <variant>: <input>` per offending input of each image KNOWN to
//!   link ARMv7-M code (synth#1301). Code violations are attributed to the
//!   input section that holds them. A ledgered image that gains a new
//!   offending input fails, and so does a line that no longer applies.
//! - `nobuild <variant>: <reason>` per variant that produces no image for the
//!   board, with a substring of its first compiler error. A clean image that
//!   stops building fails; it is not quietly "one fewer clean image".
//! The list only shrinks.

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
/// PSP, PRIMASK, CONTROL. BASEPRI (17), BASEPRI_MAX (18) and FAULTMASK (19)
/// are ARMv7-M; 4 and the rest are reserved.
fn v6m_sysm(s: u16) -> bool {
    matches!(s, 0..=3 | 5..=9 | 16 | 20)
}

/// None if ARMv6-M executes this 32-bit encoding; otherwise why not.
fn reject32(h1: u16, h2: u16) -> Option<&'static str> {
    // BL: 11110 S imm10 : 11 J1 1 J2 imm11 (BLX-immediate has bit 12 clear)
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
            } else if h == 0xB662 || h == 0xB672 {
                None // CPSIE i / CPSID i: v6-M CPS has only the I bit
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
/// cannot execute, as (address, text).
fn decode(code: &[u8], base: u32, out: &mut Vec<(u32, String)>) {
    let mut i = 0usize;
    while i + 2 <= code.len() {
        let at = base + i as u32;
        let h1 = u16::from_le_bytes([code[i], code[i + 1]]);
        if is_32bit(h1) {
            if i + 4 > code.len() {
                out.push((at, format!("{at:#010x}: {h1:04x} ....  32-bit prefix with no second halfword")));
                return;
            }
            let h2 = u16::from_le_bytes([code[i + 2], code[i + 3]]);
            if let Some(why) = reject32(h1, h2) {
                out.push((at, format!("{at:#010x}: {h1:04x} {h2:04x}  {why}")));
            }
            i += 4;
        } else {
            if let Some(why) = reject16(h1) {
                out.push((at, format!("{at:#010x}: {h1:04x}       {why}")));
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
/// symbols. Thumb code restarts at every STT_FUNC entry and at every address
/// in `starts` (a linked image's input-section starts, absolute). Offsets are
/// section-relative.
fn runs(elf: &Elf, idx: usize, starts: &[u32]) -> Vec<(u32, u32, State)> {
    let sec = &elf.sections[idx];
    let rel = elf.etype == ET_REL;
    let off = |v: u32| if rel { v } else { v.wrapping_sub(sec.addr) };
    // At one offset a restart is applied first, so a mapping symbol at the
    // same place has the last word.
    let mut marks: Vec<(u32, u8, State)> = Vec::new();
    for s in elf.symbols.iter().filter(|s| s.shndx as usize == idx) {
        let base = s.name.split('.').next().unwrap_or("");
        let m = match base {
            "$t" => Some((off(s.value), 1, State::Thumb)),
            "$d" => Some((off(s.value), 1, State::Data)),
            "$a" => Some((off(s.value), 1, State::Arm)),
            _ if s.kind == STT_FUNC => Some((off(s.value & !1), 0, State::Thumb)),
            _ => None,
        };
        if let Some(m) = m {
            marks.push(m);
        }
    }
    if !rel {
        for &a in starts {
            marks.push((off(a), 0, State::Thumb));
        }
    }
    marks.retain(|m| m.0 < sec.size);
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

/// Leg 2 over every executable section of `elf`, whatever its name.
fn decode_elf(elf: &Elf, starts: &[u32]) -> Vec<(u32, String)> {
    let mut bad = Vec::new();
    for (idx, sec) in elf.sections.iter().enumerate() {
        if sec.kind != SHT_PROGBITS || sec.flags & SHF_EXECINSTR == 0 || sec.size == 0 {
            continue;
        }
        let base = if elf.etype == ET_REL { 0 } else { sec.addr };
        let Some(bytes) = elf.data.get(sec.off as usize..(sec.off + sec.size) as usize) else {
            bad.push((base, format!("{}: section extends past the file", sec.name)));
            continue;
        };
        for (a, b, st) in runs(elf, idx, starts) {
            match st {
                State::Data => {}
                State::Arm => bad.push((
                    base + a,
                    format!("{:#010x}: ARM-state code ($a) in {}: an M-profile core has no ARM state", base + a, sec.name),
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

type Attrs = BTreeMap<String, (Option<u64>, Option<u64>)>;

/// `llvm-readobj --arch-specific` output -> (file label, CPU_arch, profile).
/// An archive prints one `File: ar(member)` block per member.
fn parse_attrs(text: &str) -> Attrs {
    let mut out: Attrs = BTreeMap::new();
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

/// Split `ar(member)` into (file, Some(member)); a plain path is (path, None).
fn split_label(l: &str) -> (&str, Option<&str>) {
    match l.find('(') {
        Some(p) if l.ends_with(')') => (&l[..p], Some(&l[p + 1..l.len() - 1])),
        _ => (l, None),
    }
}

/// A label resolved against the directory the linker ran in.
fn resolve(label: &str, cwd: &Path) -> String {
    let (file, member) = split_label(label);
    let f = if Path::new(file).is_relative() { cwd.join(file) } else { PathBuf::from(file) };
    match member {
        Some(m) => format!("{}({m})", f.display()),
        None => f.display().to_string(),
    }
}

/// The short, stable name of an input for the ledger: `file.o` or `lib.a(m.o)`.
fn input_name(label: &str) -> String {
    let (file, member) = split_label(label);
    let base = file.rsplit('/').next().unwrap_or(file);
    match member {
        Some(m) => format!("{base}({m})"),
        None => base.to_string(),
    }
}

/// Read the attributes of every (resolved) input label. Each archive or
/// object is passed once; a label readobj did not report is an error.
fn attrs_of(ro: &Path, labels: &BTreeSet<String>) -> Result<Attrs, String> {
    let mut files: BTreeSet<String> = BTreeSet::new();
    for l in labels {
        let (file, _) = split_label(l);
        if !Path::new(file).exists() {
            return Err(format!("linker input {file} no longer exists (built without -C save-temps?)"));
        }
        files.insert(file.to_string());
    }
    let mut all = BTreeMap::new();
    for f in &files {
        let o = Command::new(ro).arg("--arch-specific").arg(f).output().map_err(|e| format!("{}: {e}", ro.display()))?;
        all.extend(parse_attrs(&String::from_utf8_lossy(&o.stdout)));
    }
    for l in labels {
        if !all.contains_key(l) {
            return Err(format!("llvm-readobj reported nothing for linker input {l}"));
        }
    }
    Ok(all)
}

/// What an lld `-Map` says: every input file it placed a section from, and
/// each input section's (vma, size, input label).
struct LinkMap {
    inputs: BTreeSet<String>,
    sections: Vec<(u32, u32, String)>,
}

/// The header fixes the columns: an input-section line has its text starting
/// at the `In` column and reads `<file>:(<section>)`; its first and third
/// fields are the VMA and size in hex.
fn parse_map(map: &str) -> Result<LinkMap, String> {
    let mut lines = map.lines();
    let header = lines.next().ok_or("empty linker map")?;
    let in_col = header.find(" In ").map(|p| p + 1).ok_or("linker map header has no `In` column")?;
    let mut lm = LinkMap { inputs: BTreeSet::new(), sections: Vec::new() };
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
        let f: Vec<&str> = l[..in_col].split_whitespace().collect();
        let (Some(vma), Some(size)) = (
            f.first().and_then(|v| u32::from_str_radix(v, 16).ok()),
            f.get(2).and_then(|v| u32::from_str_radix(v, 16).ok()),
        ) else {
            return Err(format!("linker map line without a VMA and size: {l}"));
        };
        lm.inputs.insert(file.to_string());
        lm.sections.push((vma, size, file.to_string()));
    }
    if lm.inputs.is_empty() {
        return Err("linker map names no input files".into());
    }
    Ok(lm)
}

// --------------------------------------------------------------------------
// One ELF, both legs.
// --------------------------------------------------------------------------

struct Report {
    inputs: usize,
    attr_bad: Vec<String>,
    code_bad: Vec<String>,
    /// Every input that is not ARMv6-M or holds code ARMv6-M cannot execute.
    offenders: BTreeSet<String>,
}

impl Report {
    fn clean(&self) -> bool {
        self.attr_bad.is_empty() && self.code_bad.is_empty()
    }
    fn counts(&self) -> String {
        format!(
            "{} of {} input(s) not ARMv6-M, {} encoding(s) ARMv6-M cannot execute; offending: {}",
            self.attr_bad.len(),
            self.inputs,
            self.code_bad.len(),
            self.offenders.iter().cloned().collect::<Vec<_>>().join(", ")
        )
    }
}

/// A relocatable object is its own (only) input; a linked image needs the map
/// that lists its inputs, resolved against `cwd` (where the linker ran).
/// `Err` = could not check (not a pass).
fn check_elf(ro: &Path, path: &Path, map: Option<&Path>, cwd: &Path) -> Result<Report, String> {
    let data = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let elf = parse_elf(data).map_err(|e| format!("{}: {e}", path.display()))?;
    if elf.machine != EM_ARM {
        return Err(format!("{}: e_machine {} is not ARM", path.display(), elf.machine));
    }
    let lm = if elf.etype == ET_REL {
        let l = path.to_string_lossy().into_owned();
        LinkMap { inputs: [l.clone()].into(), sections: vec![(0, u32::MAX, l)] }
    } else {
        let Some(m) = map else {
            return Err(format!("{}: a linked image needs its linker map to see its inputs", path.display()));
        };
        let t = std::fs::read_to_string(m).map_err(|e| format!("{}: {e}", m.display()))?;
        parse_map(&t)?
    };
    let resolved: BTreeMap<String, String> = lm.inputs.iter().map(|l| (l.clone(), resolve(l, cwd))).collect();
    let attrs = attrs_of(ro, &resolved.values().cloned().collect())?;
    let mut attr_bad = Vec::new();
    let mut offenders = BTreeSet::new();
    for (l, r) in &resolved {
        let (a, p) = attrs[r];
        if let Err(why) = arch_ok(a, p) {
            attr_bad.push(format!("{}: {why}", input_name(l)));
            offenders.insert(input_name(l));
        }
    }
    let starts: Vec<u32> = lm.sections.iter().map(|s| s.0).collect();
    let code = decode_elf(&elf, &starts);
    for (at, _) in &code {
        let holder = lm
            .sections
            .iter()
            .filter(|(v, s, _)| *at >= *v && (*at - *v) < *s)
            .map(|(_, _, l)| input_name(l))
            .next()
            .unwrap_or_else(|| "<no input section>".into());
        offenders.insert(holder);
    }
    Ok(Report {
        inputs: lm.inputs.len(),
        attr_bad,
        code_bad: code.into_iter().map(|(_, s)| s).collect(),
        offenders,
    })
}

/// One built variant's outcome.
enum Outcome {
    NoImage(String),
    Clean,
    Reject(Report),
    Refused(String),
}

/// Decide one variant from its build result. A successful build with no map,
/// or an image that cannot be read, is REFUSED -- never "no image".
fn judge(ro: &Path, built: bool, stderr: &str, img: &Path, map: &Path, cwd: &Path) -> Outcome {
    if !built {
        return Outcome::NoImage(first_error(stderr));
    }
    if !map.exists() {
        return Outcome::Refused(format!("built, but the linker wrote no map at {}", map.display()));
    }
    match check_elf(ro, img, Some(map), cwd) {
        Err(e) => Outcome::Refused(e),
        Ok(r) if r.clean() => Outcome::Clean,
        Ok(r) => Outcome::Reject(r),
    }
}

// --------------------------------------------------------------------------
// The gate: build every variant for the G031K8, check every image.
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

/// (variant name, bin, extra features): every bin alone, plus every pair the
/// silicon matrix names with extra features. A matrix bin that does not exist
/// is an error, not a skip.
fn variants(bins: &[String], silicon: &[(&str, &[&str])]) -> Result<Vec<(String, String, Vec<String>)>, String> {
    let mut v: Vec<(String, String, Vec<String>)> = bins.iter().map(|b| (b.clone(), b.clone(), Vec::new())).collect();
    for (b, extra) in silicon {
        if !bins.iter().any(|x| x == b) {
            return Err(format!("xtask silicon's matrix names bin {b}, which os/gust does not have"));
        }
        if extra.is_empty() {
            continue;
        }
        let name = format!("{b}+{}", extra.join("+"));
        if !v.iter().any(|x| x.0 == name) {
            v.push((name, b.to_string(), extra.iter().map(|s| s.to_string()).collect()));
        }
    }
    Ok(v)
}

#[derive(Default)]
struct Ledger {
    reject: BTreeSet<(String, String)>,
    nobuild: BTreeMap<String, String>,
}

fn parse_ledger(t: &str) -> Result<Ledger, String> {
    let mut led = Ledger::default();
    for (n, raw) in t.lines().enumerate() {
        let l = raw.split('#').next().unwrap_or("").trim();
        if l.is_empty() {
            continue;
        }
        let bad = || format!("{LEDGER}:{}: expected `reject <variant>: <input>` or `nobuild <variant>: <reason>`", n + 1);
        let (kind, rest) = l.split_once(char::is_whitespace).ok_or_else(bad)?;
        let (v, x) = rest.trim().split_once(": ").ok_or_else(bad)?;
        let (v, x) = (v.trim().to_string(), x.trim().to_string());
        match kind {
            "reject" => {
                led.reject.insert((v, x));
            }
            "nobuild" => {
                if led.nobuild.insert(v.clone(), x).is_some() {
                    return Err(format!("{LEDGER}:{}: {v} listed twice as nobuild", n + 1));
                }
            }
            _ => return Err(bad()),
        }
    }
    Ok(led)
}

/// The ratchet, both directions, both lists.
fn compare(reject: &BTreeSet<(String, String)>, nobuild: &BTreeMap<String, String>, led: &Ledger) -> Vec<String> {
    let mut f = Vec::new();
    for (v, i) in reject.difference(&led.reject) {
        f.push(format!("FAIL: {v} links {i}, which ARMv6-M cannot execute, and `reject {v}: {i}` is not on the ledger"));
    }
    for (v, i) in led.reject.difference(reject) {
        f.push(format!("FAIL: `reject {v}: {i}` is on the ledger but no longer applies -- delete its line"));
    }
    for (v, why) in nobuild {
        match led.nobuild.get(v) {
            None => f.push(format!("FAIL: {v} no longer builds for {TARGET} and is not ledgered as nobuild: {why}")),
            Some(sub) if !why.contains(sub.as_str()) => {
                f.push(format!("FAIL: {v} fails to build for a different reason than ledgered (`{sub}`): {why}"))
            }
            Some(_) => {}
        }
    }
    for v in led.nobuild.keys() {
        if !nobuild.contains_key(v) {
            f.push(format!("FAIL: `nobuild {v}` is on the ledger but it builds now -- delete its line"));
        }
    }
    f
}

/// Remove ANSI escape sequences (CSI `ESC [ ... final`, and two-byte `ESC x`).
/// The gate runs cargo with `--color never`, but CI exports
/// CARGO_TERM_COLOR=always, and a coloured `error` line does not start with
/// "error" -- every nobuild reason then mismatched its ledgered substring.
fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut it = s.chars().peekable();
    while let Some(c) = it.next() {
        if c != '\x1b' {
            out.push(c);
            continue;
        }
        if it.next() == Some('[') {
            for d in it.by_ref() {
                if ('\x40'..='\x7e').contains(&d) {
                    break;
                }
            }
        }
    }
    out
}

/// The first compiler error line, colour-stripped. If there is none, the last
/// stderr lines, so a mismatch against the ledger is diagnosable.
fn first_error(stderr: &str) -> String {
    let plain = strip_ansi(stderr);
    if let Some(l) = plain.lines().find(|l| l.trim_start().starts_with("error")) {
        return l.trim_start().chars().take(160).collect();
    }
    let tail: Vec<&str> = plain.lines().filter(|l| !l.trim().is_empty()).collect();
    let tail = tail[tail.len().saturating_sub(4)..].join(" | ");
    format!("build failed, no error line; stderr ends: {}", tail.chars().take(400).collect::<String>())
}

pub fn run(repo: &Path, module: Option<&str>) -> Verdict {
    let ro = match readobj() {
        Ok(r) => r,
        Err(e) => return Verdict::Refused(e),
    };
    // --module PATH: one ELF, strict. An image needs PATH.map beside it;
    // relative paths in that map resolve against the current directory.
    if let Some(m) = module {
        let p = PathBuf::from(m);
        let map = PathBuf::from(format!("{m}.map"));
        let cwd = std::env::current_dir().unwrap_or_default();
        return match check_elf(&ro, &p, map.exists().then_some(map.as_path()), &cwd) {
            Err(e) => Verdict::Refused(e),
            Ok(r) if r.clean() => Verdict::Pass(vec![format!("CLEAN {m}: {}", r.counts())]),
            Ok(r) => {
                let mut l = vec![format!("REJECT {m}: {}", r.counts())];
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
    let vars = match variants(&names, &crate::silicon::variants()) {
        Ok(v) => v,
        Err(e) => return Verdict::Refused(e),
    };
    let led = match std::fs::read_to_string(repo.join(LEDGER)).map_err(|e| format!("{LEDGER}: {e}")).and_then(|t| parse_ledger(&t)) {
        Ok(l) => l,
        Err(e) => return Verdict::Refused(e),
    };
    let crate_dir = repo.join(CRATE_DIR);
    let tdir = std::env::var_os("CARGO_TARGET_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| repo.join("target"))
        .join("armv6m-isa");
    let mut lines = Vec::new();
    let mut clean = Vec::new();
    let mut reject: BTreeSet<(String, String)> = BTreeSet::new();
    let mut nobuild: BTreeMap<String, String> = BTreeMap::new();
    let mut nrej = 0;
    for (name, bin, extra) in &vars {
        // A dedicated target dir per feature set: the map and the image must
        // come from the same link, and two feature sets of one bin share an
        // output path.
        let vdir = tdir.join(if extra.is_empty() { "default".to_string() } else { extra.join("+") });
        let maps = vdir.join("maps");
        if let Err(e) = std::fs::create_dir_all(&maps) {
            return Verdict::Refused(format!("{}: {e}", maps.display()));
        }
        let map = maps.join(format!("{bin}.map"));
        let mut feats = vec![BOARD_FEATURE.to_string()];
        feats.extend(extra.iter().cloned());
        let o = match Command::new("cargo")
            .current_dir(&crate_dir)
            .env("CARGO_TARGET_DIR", &vdir)
            .env_remove("CARGO_TERM_COLOR")
            .args(["rustc", "--locked", "--color", "never", "--release", "--target", TARGET, "--no-default-features", "--features"])
            .arg(feats.join(","))
            .args(["--bin", bin, "--"])
            .arg(format!("-Clink-arg=-Map={}", map.display()))
            .arg("-Csave-temps")
            .output()
        {
            Ok(o) => o,
            Err(e) => return Verdict::Refused(format!("cargo: {e}")),
        };
        let img = vdir.join(TARGET).join("release").join(bin);
        match judge(&ro, o.status.success(), &String::from_utf8_lossy(&o.stderr), &img, &map, &crate_dir) {
            Outcome::Refused(e) => return Verdict::Refused(format!("{name}: {e}")),
            Outcome::NoImage(why) => {
                eprintln!("NOIMAGE {name}");
                nobuild.insert(name.clone(), why);
            }
            Outcome::Clean => {
                eprintln!("CLEAN   {name}");
                clean.push(name.clone());
            }
            Outcome::Reject(r) => {
                eprintln!("REJECT  {name}");
                nrej += 1;
                lines.push(format!("REJECT {name}: {}", r.counts()));
                for a in r.attr_bad.iter().take(3) {
                    lines.push(format!("    input  {a}"));
                }
                for c in r.code_bad.iter().take(3) {
                    lines.push(format!("    code   {c}"));
                }
                for i in r.offenders {
                    reject.insert((name.clone(), i));
                }
            }
        }
    }
    if clean.is_empty() && nrej == 0 {
        return Verdict::Refused(format!("no variant built for {TARGET}: nothing was checked"));
    }
    let mut head = vec![
        format!(
            "{} variant(s) ({} bin(s) + {} silicon feature set(s)); {} rejected, {} clean, {} produce no image",
            vars.len(),
            names.len(),
            vars.len() - names.len(),
            nrej,
            clean.len(),
            nobuild.len()
        ),
        format!("clean: {}", clean.join(", ")),
    ];
    head.extend(nobuild.iter().map(|(v, w)| format!("no image  {v}: {w}")));
    head.append(&mut lines);
    let fails = compare(&reject, &nobuild, &led);
    if fails.is_empty() {
        head.push(format!(
            "ledger exact ({} reject pair(s), {} nobuild): every other image is clean, and the lists only shrink",
            led.reject.len(),
            led.nobuild.len()
        ));
        Verdict::Pass(head)
    } else {
        head.extend(fails);
        Verdict::Fail(head)
    }
}

// --------------------------------------------------------------------------
// Self-test: planted objects, archives and linked images, written as real ELF
// files and read back through the same parser and the same llvm-readobj the
// gate uses.
// --------------------------------------------------------------------------

/// The 28 ARMv7-M-only encodings (assembled with -mcpu=cortex-m3, halfwords in
/// stream order). 25 are 32-bit; IT, CBZ, CBNZ are the 3 16-bit ones.
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

/// Encodings at the edges of the whitelist that ARMv6-M does NOT execute.
const EDGE_REJECT: &[(&str, &[u16])] = &[
    ("cbz, i=1 (next to ADD/SUB SP)", &[0xb300]),
    ("cbnz, i=1", &[0xbb00]),
    ("it ne", &[0xbf18]),
    ("hint 5 (unallocated)", &[0xbf50]),
    ("1011 1010 10 (between REV16 and REVSH)", &[0xba80]),
    ("1011 1000 (undefined)", &[0xb800]),
    ("setend", &[0xb650]),
    ("cpsie f (no F bit on v6-M)", &[0xb661]),
    ("cpsid f", &[0xb671]),
    ("cpsie if", &[0xb663]),
    ("blx imm (BL with bit 12 clear)", &[0xf000, 0xc000]),
    ("clrex", &[0xf3bf, 0x8f2f]),
    ("msr FAULTMASK, r0", &[0xf380, 0x8813]),
    ("msr BASEPRI_MAX, r0", &[0xf380, 0x8812]),
    ("msr SYSm 4 (reserved)", &[0xf380, 0x8804]),
    ("msr with mask bits (v7-M APSR_g)", &[0xf380, 0x8400]),
    ("mrs r0, FAULTMASK", &[0xf3ef, 0x8013]),
    ("mrs r0, BASEPRI_MAX", &[0xf3ef, 0x8012]),
    ("mrs r0, SYSm 4 (reserved)", &[0xf3ef, 0x8004]),
    ("mrs r0, SYSm 21 (reserved)", &[0xf3ef, 0x8015]),
    ("dsb with bit 7 set (0x8fc?)", &[0xf3bf, 0x8fcf]),
    ("barrier-space 0x8f7f (v7 SB/undef)", &[0xf3bf, 0x8f7f]),
    ("32-bit prefix 11101 (e800)", &[0xe800, 0x0000]),
];

/// Encodings at the edges of the whitelist that ARMv6-M DOES execute.
const EDGE_ACCEPT: &[(&str, &[u16])] = &[
    ("add sp, #508", &[0xb07f]),
    ("sub sp, #508", &[0xb0ff]),
    ("sxth / sxtb / uxth / uxtb", &[0xb208, 0xb248, 0xb288, 0xb2c8]),
    ("push {r0} / push {lr} / pop {r0} / pop {pc}", &[0xb401, 0xb500, 0xbc01, 0xbd00]),
    ("cpsie i / cpsid i", &[0xb662, 0xb672]),
    ("rev / rev16 / revsh", &[0xba08, 0xba48, 0xbac8]),
    ("bkpt #255", &[0xbeff]),
    ("nop / yield / wfe / wfi / sev", &[0xbf00, 0xbf10, 0xbf20, 0xbf30, 0xbf40]),
    ("b (unconditional, max back)", &[0xe7ff]),
    ("udf / svc", &[0xdeff, 0xdfff]),
    ("bl, J1=J2=1 / J1=J2=0", &[0xf7ff, 0xfffe, 0xf400, 0xd000]),
    ("dmb / dsb / isb, option sy and 0", &[0xf3bf, 0x8f5f, 0xf3bf, 0x8f4f, 0xf3bf, 0x8f6f, 0xf3bf, 0x8f40]),
    ("msr APSR / IAPSR / EAPSR / xPSR", &[0xf380, 0x8800, 0xf380, 0x8801, 0xf380, 0x8802, 0xf380, 0x8803]),
    ("msr IPSR / EPSR / IEPSR / MSP / PSP", &[0xf380, 0x8805, 0xf380, 0x8806, 0xf380, 0x8807, 0xf380, 0x8808, 0xf380, 0x8809]),
    ("msr PRIMASK / CONTROL", &[0xf380, 0x8810, 0xf381, 0x8814]),
    ("mrs r0, APSR / xPSR / IPSR / MSP / PSP", &[0xf3ef, 0x8000, 0xf3ef, 0x8003, 0xf3ef, 0x8005, 0xf3ef, 0x8008, 0xf3ef, 0x8009]),
    ("mrs r0, PRIMASK / CONTROL", &[0xf3ef, 0x8010, 0xf3ef, 0x8014]),
    ("a 16-bit sample of every other class", &[0x0000, 0x1840, 0x2001, 0x4000, 0x4400, 0x4780, 0x4800, 0x5000, 0x6000, 0x7000, 0x8000, 0x9000, 0xa000, 0xa800, 0xc000, 0xc800, 0xd0fe]),
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

/// One section for `write_elf`.
struct WSec {
    name: &'static str,
    kind: u32,
    flags: u32,
    addr: u32,
    data: Vec<u8>,
}

fn exec_sec(name: &'static str, addr: u32, data: Vec<u8>) -> WSec {
    WSec { name, kind: SHT_PROGBITS, flags: 2 | SHF_EXECINSTR, addr, data }
}

fn attr_sec(arch: Option<u8>, profile: Option<u8>) -> WSec {
    WSec { name: ".ARM.attributes", kind: 0x7000_0003, flags: 0, addr: 0, data: attributes(arch, profile) }
}

/// A minimal ELF32: the given sections (indices 1..), then .symtab, .strtab,
/// .shstrtab. `syms` are (name, value, STT type, section index).
fn write_elf(etype: u16, secs: &[WSec], syms: &[(&str, u32, u8, u16)]) -> Vec<u8> {
    fn place(b: &mut Vec<u8>, d: &[u8]) -> u32 {
        while b.len() % 4 != 0 {
            b.push(0);
        }
        let o = b.len() as u32;
        b.extend(d);
        o
    }
    let mut shstr = vec![0u8];
    let mut name_of = |n: &str| {
        let o = shstr.len() as u32;
        shstr.extend(n.as_bytes());
        shstr.push(0);
        o
    };
    let sec_names: Vec<u32> = secs.iter().map(|s| name_of(s.name)).collect();
    let (n_sym, n_str, n_shstr) = (name_of(".symtab"), name_of(".strtab"), name_of(".shstrtab"));
    let mut strtab = vec![0u8];
    let mut symtab = vec![0u8; 16];
    for (name, value, kind, shndx) in syms {
        let off = strtab.len() as u32;
        strtab.extend(name.as_bytes());
        strtab.push(0);
        symtab.extend(off.to_le_bytes());
        symtab.extend(value.to_le_bytes());
        symtab.extend(0u32.to_le_bytes());
        symtab.push(*kind); // STB_LOCAL
        symtab.push(0);
        symtab.extend(shndx.to_le_bytes());
    }
    let mut body = vec![0u8; 52];
    let offs: Vec<u32> = secs.iter().map(|s| place(&mut body, &s.data)).collect();
    let o_sym = place(&mut body, &symtab);
    let o_str = place(&mut body, &strtab);
    let o_shstr = place(&mut body, &shstr);
    while body.len() % 4 != 0 {
        body.push(0);
    }
    let shoff = body.len() as u32;
    let sh = |v: [u32; 10]| v.iter().flat_map(|x| x.to_le_bytes()).collect::<Vec<u8>>();
    body.extend(vec![0u8; 40]);
    for (i, s) in secs.iter().enumerate() {
        body.extend(sh([sec_names[i], s.kind, s.flags, s.addr, offs[i], s.data.len() as u32, 0, 0, 4, 0]));
    }
    let n = secs.len() as u32;
    body.extend(sh([n_sym, SHT_SYMTAB, 0, 0, o_sym, symtab.len() as u32, n + 2, syms.len() as u32 + 1, 4, 16]));
    body.extend(sh([n_str, 3, 0, 0, o_str, strtab.len() as u32, 0, 0, 1, 0]));
    body.extend(sh([n_shstr, 3, 0, 0, o_shstr, shstr.len() as u32, 0, 0, 1, 0]));
    body[..4].copy_from_slice(b"\x7fELF");
    body[4] = 1; // ELFCLASS32
    body[5] = 1; // little-endian
    body[6] = 1; // EV_CURRENT
    body[16..18].copy_from_slice(&etype.to_le_bytes());
    body[18..20].copy_from_slice(&EM_ARM.to_le_bytes());
    body[20..24].copy_from_slice(&1u32.to_le_bytes());
    body[32..36].copy_from_slice(&shoff.to_le_bytes());
    body[36..40].copy_from_slice(&0x0500_0000u32.to_le_bytes()); // EABI5
    body[40..42].copy_from_slice(&52u16.to_le_bytes());
    body[46..48].copy_from_slice(&40u16.to_le_bytes());
    body[48..50].copy_from_slice(&((n + 4) as u16).to_le_bytes());
    body[50..52].copy_from_slice(&((n + 3) as u16).to_le_bytes());
    body
}

/// A relocatable object: .text then .ARM.attributes; `syms` are in .text.
fn write_object(text: &[u8], attrs: (Option<u8>, Option<u8>), syms: &[(&str, u32, u8)]) -> Vec<u8> {
    let s: Vec<(&str, u32, u8, u16)> = syms.iter().map(|(n, v, k)| (*n, *v, *k, 1)).collect();
    write_elf(ET_REL, &[exec_sec(".text", 0, text.to_vec()), attr_sec(attrs.0, attrs.1)], &s)
}

/// A GNU `ar` archive (short member names).
fn write_ar(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut a = b"!<arch>\n".to_vec();
    for (name, data) in members {
        let hdr = format!("{:<16}{:<12}{:<6}{:<6}{:<8}{:<10}`\n", format!("{name}/"), 0, 0, 0, 644, data.len());
        a.extend(hdr.as_bytes());
        a.extend(data);
        if a.len() % 2 == 1 {
            a.push(b'\n');
        }
    }
    a
}

/// An lld-style map: header, then (vma, size, "file:(section)") input lines.
fn write_map(lines: &[(u32, u32, String)]) -> String {
    let mut m = String::from("     VMA      LMA     Size Align Out     In      Symbol\n");
    m.push_str(&format!("{:>8x} {:>8x} {:>8x} {:>5} .text\n", 0x0800_0100u32, 0x0800_0100u32, 0x100, 4));
    for (v, s, l) in lines {
        m.push_str(&format!("{v:>8x} {v:>8x} {s:>8x} {:>5}         {l}\n", 4));
        m.push_str(&format!("{v:>8x} {v:>8x} {:>8x} {:>5}                 some_symbol\n", 0, 1));
    }
    m
}

fn halfwords(h: &[u16]) -> Vec<u8> {
    h.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The clean code: v6-M instructions, then a literal pool, (bytes, pool offset).
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
    let _ = std::fs::remove_dir_all(&dir);
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Verdict::Refused(format!("{}: {e}", dir.display()));
    }
    let write = |name: &str, bytes: &[u8]| -> PathBuf {
        let p = dir.join(name);
        std::fs::write(&p, bytes).expect("write self-test file");
        p
    };
    const V6SM: (Option<u8>, Option<u8>) = (Some(12), Some(b'M'));
    let check = |p: &Path| check_elf(&ro, p, None, &dir);
    let dec = |h: &[u16]| {
        let mut v = Vec::new();
        decode(&halfwords(h), 0, &mut v);
        v.len()
    };

    // 1. The clean object passes BOTH legs, pool and all.
    let (t, pool_at) = clean_text();
    let syms = [("g", 1u32, STT_FUNC), ("$t", 0, 0), ("$d", pool_at, 0)];
    let clean = write("clean.o", &write_object(&t, V6SM, &syms));
    match check(&clean) {
        Ok(r) => {
            ck(format!("clean v6-M object passes the decode leg [{}]", r.code_bad.join("; ")), r.code_bad.is_empty());
            ck(format!("clean v6S-M object passes the attribute leg [{}]", r.attr_bad.join("; ")), r.attr_bad.is_empty() && r.inputs == 1);
        }
        Err(e) => ck(format!("clean object could be checked ({e})"), false),
    }

    // 2. Each ARMv7-M-only encoding, alone in a v6S-M-tagged object, is caught
    //    by the DECODE leg (attributes cannot help: they say v6S-M).
    let mut caught = 0;
    for (asm, hw) in V7_ONLY {
        let mut code = halfwords(&[0x46c0]);
        code.extend(halfwords(hw));
        code.extend(halfwords(&[0x46c0, 0x4770]));
        let p = write("v7-one.o", &write_object(&code, V6SM, &[("f", 1, STT_FUNC), ("$t", 0, 0)]));
        let ok = matches!(check(&p), Ok(r) if r.attr_bad.is_empty() && r.code_bad.len() == 1 && r.code_bad[0].starts_with("0x00000002"));
        if ok {
            caught += 1;
        } else {
            ck(format!("decode leg rejects `{asm}` at its own address"), false);
        }
    }
    ck(format!("decode leg rejects all {} ARMv7-M-only encodings ({caught} caught)", V7_ONLY.len()), caught == V7_ONLY.len());

    // 3. The whitelist's edges, one encoding per boundary, both sides.
    for (what, hw) in EDGE_REJECT {
        ck(format!("edge: rejects {what}"), dec(hw) == 1);
    }
    // CBZ/CBNZ with i=1 must be recognised as CBZ/CBNZ, not merely land in
    // the undefined bucket by luck of the decode order.
    ck("edge: cbz/cbnz i=1 are classified as CBZ/CBNZ".into(),
        reject16(0xb300) == Some("CBZ/CBNZ (ARMv7-M)") && reject16(0xbb00) == Some("CBZ/CBNZ (ARMv7-M)"));
    for (what, hw) in EDGE_ACCEPT {
        ck(format!("edge: accepts {what}"), dec(hw) == 0);
    }

    // 4. The ATTRIBUTE leg alone: v6-M code tagged anything but v6-M/v6S-M
    //    with profile M (or absent), is rejected though every instruction decodes.
    for (what, arch, prof) in [
        ("v7 (10)", Some(10u8), Some(b'M')),
        ("v7E-M (13)", Some(13), Some(b'M')),
        ("v8-M.baseline (16)", Some(16), Some(b'M')),
        ("v8-M.mainline (17)", Some(17), Some(b'M')),
        ("v6K (9)", Some(9), None),
        ("v6T2 (8)", Some(8), None),
        ("v6S-M with profile 'A'", Some(12), Some(b'A')),
        ("v6S-M with profile 'R'", Some(12), Some(b'R')),
        ("v6-M with profile 'S'", Some(11), Some(b'S')),
        ("no Tag_CPU_arch", None, None),
    ] {
        let p = write("attr.o", &write_object(&t, (arch, prof), &syms));
        ck(format!("attribute leg rejects clean code tagged {what}"),
            matches!(check(&p), Ok(r) if r.code_bad.is_empty() && r.attr_bad.len() == 1));
    }
    for (what, arch, prof) in [("v6-M (11), profile M", Some(11u8), Some(b'M')), ("v6S-M (12), no profile", Some(12), None)] {
        let p = write("attr-ok.o", &write_object(&t, (arch, prof), &syms));
        ck(format!("attribute leg accepts {what}"), matches!(check(&p), Ok(r) if r.clean()));
    }

    // 5. Literal pools and mapping symbols in an object.
    let p = write("nodollar.o", &write_object(&t, V6SM, &[("g", 1, STT_FUNC), ("$t", 0, 0)]));
    ck("an unmarked literal pool is decoded (fail closed), not assumed data".into(),
        matches!(check(&p), Ok(r) if !r.code_bad.is_empty()));
    let mut t2 = t.clone();
    let f2 = t2.len() as u32;
    t2.extend(halfwords(&[0xe92d, 0x41f0]));
    let p = write("fnreset.o", &write_object(&t2, V6SM, &[("g", 1, STT_FUNC), ("$t", 0, 0), ("$d", pool_at, 0), ("h", f2 | 1, STT_FUNC)]));
    ck("an STT_FUNC entry after a `$d` restarts code".into(), matches!(check(&p), Ok(r) if r.code_bad.len() == 1));
    let p = write("arm.o", &write_object(&t, V6SM, &[("$a", 0, 0), ("$d", pool_at, 0)]));
    ck("`$a` ARM-state code is rejected".into(), matches!(check(&p), Ok(r) if !r.code_bad.is_empty()));
    ck("a truncated 32-bit prefix is rejected".into(), dec(&[0xf7ff]) == 1);

    // 6. LINKED IMAGES (ET_EXEC, non-zero sh_addr, a second executable section
    //    not named .text) with a map naming plain objects and archive members.
    //    Each leg must reject on its own here, not only on objects.
    let good = write("good.o", &write_object(&t, V6SM, &syms));
    let bad7 = write("bad7.o", &write_object(&t, (Some(10), Some(b'M')), &syms));
    let lib = write(
        "lib.a",
        &write_ar(&[("m6.o", write_object(&t, V6SM, &syms)), ("m7.o", write_object(&t, (Some(13), Some(b'M')), &syms))]),
    );
    let (gs, bs, ls) = (good.display().to_string(), bad7.display().to_string(), lib.display().to_string());
    let (text_at, ram_at) = (0x0800_0100u32, 0x2000_0000u32);
    let alen = t.len() as u32;
    let stmdb = halfwords(&[0xe92d, 0x41f0, 0x4770]);
    // An image: .text = clean A (pool at end) + `second` (an input with no
    // mapping symbol and an untyped entry symbol), .ramfunc = `ram`.
    let image = |second: &[u8], ram: &[u8]| -> Vec<u8> {
        let mut text = t.clone();
        text.extend(second);
        write_elf(
            2,
            &[exec_sec(".text", text_at, text), exec_sec(".ramfunc", ram_at, ram.to_vec()), attr_sec(Some(12), Some(b'M'))],
            &[
                ("$t", text_at, 0, 1),
                ("$d", text_at + pool_at, 0, 1),
                ("entry_b", text_at + alen, 0, 1), // STT_NOTYPE: no restart from the symbol
                ("$t", ram_at, 0, 2),
            ],
        )
    };
    let clean_b = halfwords(&[0x46c0, 0x4770]);
    let map_with = |extra: &[(u32, u32, String)]| {
        let mut l = vec![
            (text_at, alen, format!("{gs}:(.text)")),
            (text_at + alen, 4, format!("{ls}(m6.o):(.text)")),
            (ram_at, 6, format!("{gs}:(.ramfunc)")),
        ];
        l.extend(extra.iter().cloned());
        write_map(&l)
    };
    let linked = |name: &str, img: Vec<u8>, map: String| -> Result<Report, String> {
        let p = write(name, &img);
        let m = write(&format!("{name}.map"), map.as_bytes());
        check_elf(&ro, &p, Some(&m), &dir)
    };
    let r = linked("img-clean", image(&clean_b, &clean_b), map_with(&[]));
    ck(format!("linked: clean image (object + archive member, two exec sections) passes [{:?}]",
        r.as_ref().map(|r| r.counts()).err()), matches!(&r, Ok(r) if r.clean() && r.inputs == 2));
    let r = linked("img-ram", image(&clean_b, &stmdb), map_with(&[]));
    ck("linked: decode leg rejects v7 code in a second exec section not named .text".into(),
        matches!(&r, Ok(r) if r.attr_bad.is_empty() && r.code_bad.len() == 1 && r.offenders.contains("good.o")));
    let r = linked("img-nbr", image(&stmdb, &clean_b), map_with(&[]));
    ck("linked: an input after a neighbour's `$d` is decoded from its own start (attributed to it)".into(),
        matches!(&r, Ok(r) if r.code_bad.len() == 1 && r.offenders.len() == 1 && r.offenders.contains("lib.a(m6.o)")));
    let r = linked("img-attr", image(&clean_b, &clean_b), map_with(&[(0x0800_2000, 8, format!("{bs}:(.rodata)"))]));
    ck("linked: attribute leg rejects a v7-tagged plain input".into(),
        matches!(&r, Ok(r) if r.code_bad.is_empty() && r.attr_bad.len() == 1 && r.offenders.contains("bad7.o")));
    let r = linked("img-ar", image(&clean_b, &clean_b), map_with(&[(0x0800_2000, 8, format!("{ls}(m7.o):(.rodata)"))]));
    ck("linked: attribute leg rejects a v7E-M archive member".into(),
        matches!(&r, Ok(r) if r.code_bad.is_empty() && r.attr_bad.len() == 1 && r.offenders.contains("lib.a(m7.o)")));
    // Relative map paths resolve against the link's directory, not ours.
    let rel = write_map(&[(text_at, alen, "good.o:(.text)".into()), (text_at + alen, 4, "lib.a(m6.o):(.text)".into()), (ram_at, 6, "good.o:(.ramfunc)".into())]);
    let r = linked("img-rel", image(&clean_b, &clean_b), rel);
    ck("linked: relative map paths resolve against the link's working directory".into(), matches!(&r, Ok(r) if r.clean()));

    // 7. One variant's outcome: a corrupt image or a missing map is REFUSED,
    //    never folded into "no image"; a failed build is "no image".
    let img = write("img-good", &image(&clean_b, &clean_b));
    let map = write("img-good.map", map_with(&[]).as_bytes());
    ck("judge: a good build is judged clean".into(), matches!(judge(&ro, true, "", &img, &map, &dir), Outcome::Clean));
    ck("judge: a built variant with no map is refused".into(),
        matches!(judge(&ro, true, "", &img, &dir.join("absent.map"), &dir), Outcome::Refused(_)));
    let junk = write("junk", b"not an elf");
    ck("judge: an unreadable image is refused".into(), matches!(judge(&ro, true, "", &junk, &map, &dir), Outcome::Refused(_)));
    // CI exports CARGO_TERM_COLOR=always: a coloured error line must still be
    // found and still match its ledgered substring.
    let coloured = "\x1b[0m\x1b[1m\x1b[32m   Compiling\x1b[0m gust v0.1.0\n\x1b[0m\x1b[1m\x1b[38;5;9merror[E0599]\x1b[0m\x1b[0m\x1b[1m: no method named `fetch_add` found for struct `Atomic<T>` in the current scope\x1b[0m\n";
    let got = first_error(coloured);
    ck(format!("ANSI-coloured stderr yields the plain error line [{got}]"),
        got.starts_with("error[E0599]: no method named `fetch_add`") && !got.contains('\x1b'));
    let cled = parse_ledger("nobuild gust: no method named `fetch_add`\n").unwrap_or_default();
    ck("ANSI-coloured nobuild reason still matches its ledgered substring".into(),
        compare(&BTreeSet::new(), &[("gust".to_string(), got.clone())].into_iter().collect(), &cled).is_empty());
    let none = first_error("warning: a\n\nnote: b\nlinker exited 1\n");
    ck(format!("no error line: the message carries the stderr tail [{none}]"),
        none.contains("linker exited 1") && none.contains("note: b"));
    ck("judge: a failed build is no image, with its first error".into(),
        matches!(judge(&ro, false, "warning: x\nerror: boom\n", &img, &map, &dir), Outcome::NoImage(w) if w == "error: boom"));

    // 8. The map reader on lld's real layout (columns from the header).
    let real = "     VMA      LMA     Size Align Out     In      Symbol\n\
                \x20      0        0       c0     4 .vector_table\n\
                \x20      0        0       c0     4         /t/rel/deps/gust-1.rcgu.o:(.vector_table)\n\
                \x20      0        0        0     1                 __RESET_VECTOR\n\
                \x20     c0       c0       10     4 .text\n\
                \x20     c0       c0        8     4         /d/wdg-thin-cm3.o:(.text)\n\
                \x20     c8       c8        8     4         /s/libcompiler_builtins.rlib(cb.cgu.017.rcgu.o):(.text.memcpy)\n\
                \x20     d0       d0        4     4         <internal>:(.ARM.exidx)\n";
    let got = parse_map(real).map(|m| (m.inputs, m.sections)).unwrap_or_default();
    ck("map reader: 3 inputs (object, archive member, committed .o) with VMA and size".into(),
        got.0.len() == 3 && got.0.contains("/s/libcompiler_builtins.rlib(cb.cgu.017.rcgu.o)") && got.1.contains(&(0xc8, 8, "/s/libcompiler_builtins.rlib(cb.cgu.017.rcgu.o)".into())));

    // 9. What is built: every bin, plus the silicon matrix's feature sets.
    let names: Vec<String> = ["gust_iso_unpriv_probe", "gust_priv_region_probe", "gust_hm_probe"].iter().map(|s| s.to_string()).collect();
    let sil = crate::silicon::variants();
    let mine: Vec<(&str, &[&str])> = sil.iter().filter(|(b, _)| names.iter().any(|n| n == b)).cloned().collect();
    let v = variants(&names, &mine).unwrap_or_default();
    let has = |n: &str, f: &[&str]| v.iter().any(|x| x.0 == n && x.2 == f);
    ck("variants: every bin alone".into(), names.iter().all(|n| has(n, &[])));
    ck("variants: gust_iso_unpriv_probe+drop-priv, read from xtask silicon's matrix".into(), has("gust_iso_unpriv_probe+drop-priv", &["drop-priv"]));
    ck("variants: gust_priv_region_probe+priv-private and +priv-shared".into(),
        has("gust_priv_region_probe+priv-private", &["priv-private"]) && has("gust_priv_region_probe+priv-shared", &["priv-shared"]));
    ck("variants: a matrix bin os/gust lacks is an error".into(), variants(&names, &[("nope", &["x"][..])]).is_err());

    // 10. The ledger, both lists, both directions.
    let led = parse_ledger("reject a: x.o\nreject a: y.o  # two inputs\nnobuild n: fetch_add\n").unwrap_or_default();
    let rj = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<BTreeSet<_>>();
    let nb = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<BTreeMap<_, _>>();
    let ok_r = rj(&[("a", "x.o"), ("a", "y.o")]);
    let ok_n = nb(&[("n", "error[E0599]: no method named `fetch_add`")]);
    ck("ledger: exact match passes".into(), compare(&ok_r, &ok_n, &led).is_empty());
    ck("ledger: a ledgered image that gains a new offending input fails".into(),
        compare(&rj(&[("a", "x.o"), ("a", "y.o"), ("a", "z.o")]), &ok_n, &led).len() == 1);
    ck("ledger: a rejected image off the ledger fails".into(), compare(&rj(&[("a", "x.o"), ("a", "y.o"), ("b", "x.o")]), &ok_n, &led).len() == 1);
    ck("ledger: a reject line that no longer applies fails".into(), compare(&rj(&[("a", "x.o")]), &ok_n, &led).len() == 1);
    ck("ledger: a clean image that stops building fails".into(),
        compare(&ok_r, &nb(&[("n", "error[E0599]: no method named `fetch_add`"), ("c", "error: x")]), &led).len() == 1);
    ck("ledger: a nobuild that builds now fails".into(), compare(&ok_r, &nb(&[]), &led).len() == 1);
    ck("ledger: a nobuild failing for another reason fails".into(), compare(&ok_r, &nb(&[("n", "error: other")]), &led).len() == 1);
    ck("ledger: a malformed line is an error".into(), parse_ledger("a: b\n").is_err());

    let _ = std::fs::remove_dir_all(&dir);
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

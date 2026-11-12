//! Every peripheral address the code uses comes from the AADL target model.
//!
//! The two-axes rework (docs/architecture/three-modes-two-axes.md) makes SoC and
//! board facts DATA: targets/*.aadl is authoritative and gust-target-gen writes
//! targets/generated/gust_target_<soc>.rs. A peripheral address typed into a
//! binary or driver bypasses that, and drifts from it silently.
//!
//! The check: every hex literal in the STM32 peripheral range (0x4000_0000 to
//! 0x5FFF_FFFF) in the CODE (comments are skipped) of os/gust/src, drivers/*/src
//! and ports/ must fall in a 1 KiB peripheral block some generated constant also
//! falls in. A literal outside every modelled block fails unless it is on
//! `LEDGER`, and the ledger is two-sided: an entry whose literal is gone fails
//! until it is removed, so the list only shrinks as the models grow.
//!
//! Blind spots, stated: a literal passes if its block is modelled for ANY target,
//! because a binary does not declare its chip; an accidental cross-chip match is
//! pinned in `PINNED` once found, but nothing finds a new one. An address built arithmetically (`BASE + 0x400`) or
//! written in decimal is not a literal this sees; architecture addresses
//! (0xE000_xxxx, the PPB) are out of range by design, they belong to
//! ports/arch; and a literal inside a modelled block passes even if it names the
//! wrong register in it.

use crate::verdict::Verdict;
use std::collections::BTreeSet;
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "peripheral-addresses";

const LO: u64 = 0x4000_0000;
const HI: u64 = 0x5FFF_FFFF;
const SCAN: &[&str] = &["os/gust/src", "drivers", "ports"];
const GENERATED: &str = "targets/generated";

/// (file, literal, why it is not (yet) modelled). Remove an entry when its
/// literal is modelled or gone; the gate fails on a stale entry.
const LEDGER: &[(&str, u32, &str)] = &[
    ("os/gust/src/bin/gust_priv_region_probe.rs", 0x5157_2A10, "a magic value, not an address"),
    ("os/gust/src/bin/gust_two_tenant.rs", 0x51AE_0010, "a step marker, not an address"),
    ("os/gust/src/bin/gust_two_tenant.rs", 0x51AE_0011, "a step marker, not an address"),
    ("os/gust/src/bin/gust_two_tenant.rs", 0x51AE_0012, "a step marker, not an address"),
    ("os/gust/src/bin/mpu_spike_renode.rs", 0x51AE_0001, "a step marker, not an address"),
    ("os/gust/src/bin/mpu_spike_renode.rs", 0x51AE_0002, "a step marker, not an address"),
    ("os/gust/src/bin/mpu_spike_renode.rs", 0x51AE_0003, "a step marker, not an address"),
];

/// Literals that DO fall in a modelled block, but only by accident: the block is
/// modelled for a different chip than the one the code means. The gate matches a
/// block on ANY target (a binary does not declare its chip), so these are pinned
/// by hand: each is valid while its literal exists, and fails once it is gone.
const PINNED: &[(&str, u32, &str)] = &[(
    "os/gust/src/bin/gust_can.rs",
    0x4000_6400,
    "F1-style bxCAN window of the Renode gate .repl; the block is modelled only as the G474's FDCAN1, a different controller, and the F100 has no CAN",
)];

/// Hex literals (`0x` + 8 digits, `_` allowed) in a line's code, i.e. before `//`.
pub fn literals(line: &str) -> Vec<u64> {
    let code = line.split("//").next().unwrap_or("");
    let b = code.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i + 2 < b.len() {
        let boundary = i == 0 || !(b[i - 1].is_ascii_alphanumeric() || b[i - 1] == b'_');
        if boundary && b[i] == b'0' && (b[i + 1] == b'x' || b[i + 1] == b'X') {
            let mut j = i + 2;
            let mut digits = String::new();
            while j < b.len() && (b[j].is_ascii_hexdigit() || b[j] == b'_') {
                if b[j] != b'_' {
                    digits.push(b[j] as char);
                }
                j += 1;
            }
            let ends = j >= b.len() || !(b[j].is_ascii_alphanumeric());
            if digits.len() == 8 && ends {
                if let Ok(v) = u64::from_str_radix(&digits, 16) {
                    out.push(v);
                }
            }
            i = j;
        } else {
            i += 1;
        }
    }
    out
}

fn in_range(v: u64) -> bool {
    (LO..=HI).contains(&v)
}

/// The decision, separated from I/O for the self-test. `code` is (file, line no, text).
pub fn judge(
    modelled: &BTreeSet<u64>,
    code: &[(String, usize, String)],
    ledger: &[(&str, u32, &str)],
    pinned: &[(&str, u32, &str)],
) -> Vec<String> {
    let mut bad = Vec::new();
    let mut found: BTreeSet<(String, u64)> = BTreeSet::new();
    let mut present: BTreeSet<(String, u64)> = BTreeSet::new();
    for (f, n, text) in code {
        for v in literals(text).into_iter().filter(|v| in_range(*v)) {
            present.insert((f.clone(), v));
            if modelled.contains(&(v >> 10)) {
                continue;
            }
            found.insert((f.clone(), v));
            if !ledger.iter().any(|(lf, lv, _)| *lf == f && u64::from(*lv) == v) {
                bad.push(format!("{f}:{n}: 0x{v:08X} is in no modelled peripheral block and not on the ledger"));
            }
        }
    }
    for (lf, lv, _) in ledger {
        if !found.contains(&(lf.to_string(), u64::from(*lv))) {
            bad.push(format!("ledger entry {lf} 0x{lv:08X} no longer matches an unmodelled literal -- remove it"));
        }
    }
    for (pf, pv, _) in pinned {
        if !present.contains(&(pf.to_string(), u64::from(*pv))) {
            bad.push(format!("pinned entry {pf} 0x{pv:08X} no longer matches a literal -- remove it"));
        }
    }
    bad
}

fn tracked(repo: &Path, paths: &[&str]) -> Option<Vec<String>> {
    let out = Command::new("git").arg("-C").arg(repo).arg("ls-files").arg("--").args(paths).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).lines().map(String::from).collect())
}

pub fn run(repo: &Path, _t: Option<&str>) -> Verdict {
    let Some(gen_files) = tracked(repo, &[GENERATED]) else {
        return Verdict::Refused("git ls-files failed -- the gate cannot answer".into());
    };
    let mut modelled = BTreeSet::new();
    for g in gen_files.iter().filter(|g| g.ends_with(".rs")) {
        let Ok(t) = std::fs::read_to_string(repo.join(g)) else { continue };
        for line in t.lines() {
            for v in literals(line).into_iter().filter(|v| in_range(*v)) {
                modelled.insert(v >> 10);
            }
        }
    }
    if modelled.is_empty() {
        return Verdict::Refused(format!("no peripheral constants in {GENERATED} -- the check would be vacuous"));
    }
    let Some(files) = tracked(repo, SCAN) else {
        return Verdict::Refused("git ls-files failed -- the gate cannot answer".into());
    };
    let mut code = Vec::new();
    for f in files.iter().filter(|f| f.ends_with(".rs")) {
        let Ok(t) = std::fs::read_to_string(repo.join(f)) else { continue };
        for (i, line) in t.lines().enumerate() {
            code.push((f.clone(), i + 1, line.to_string()));
        }
    }
    let bad = judge(&modelled, &code, LEDGER, PINNED);
    let mut lines = vec![format!(
        "{} modelled 1 KiB peripheral block(s); {} source line(s) scanned; {} ledgered, {} pinned literal(s)",
        modelled.len(),
        code.len(),
        LEDGER.len(),
        PINNED.len()
    )];
    if bad.is_empty() {
        lines.push("PASS".into());
        Verdict::Pass(lines)
    } else {
        lines.push(format!("FAIL: {} problem(s):", bad.len()));
        lines.extend(bad.into_iter().map(|b| format!("  {b}")));
        lines.push("Model the peripheral in targets/*.aadl and use the generated constant.".into());
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
    let modelled: BTreeSet<u64> = [0x4001_3800u64 >> 10].into_iter().collect();
    let c = |s: &str| vec![("a.rs".to_string(), 1usize, s.to_string())];
    ck("underscored literal parsed", literals("const X: u32 = 0x4001_3800;") == vec![0x4001_3800]);
    ck("literal in a comment ignored", literals("let x = 1; // 0x4000_5400").is_empty());
    ck("short hex not an address", literals("let m = 0x80;").is_empty());
    ck("identifier tail not a literal", literals("let a0x40005400 = 1;").is_empty());
    ck("modelled block passes", judge(&modelled, &c("const U: u32 = 0x4001_3804;"), &[], &[]).is_empty());
    ck("unmodelled literal fails", judge(&modelled, &c("const I: u32 = 0x4000_5400;"), &[], &[]).len() == 1);
    ck("ledgered literal passes", judge(&modelled, &c("const I: u32 = 0x4000_5400;"), &[("a.rs", 0x4000_5400, "w")], &[]).is_empty());
    ck("stale ledger entry fails", judge(&modelled, &c("let x = 0;"), &[("a.rs", 0x4000_5400, "w")], &[]).len() == 1);
    ck("pinned entry with its literal passes", judge(&modelled, &c("const U: u32 = 0x4001_3800;"), &[], &[("a.rs", 0x4001_3800, "w")]).is_empty());
    ck("pinned entry without its literal fails", judge(&modelled, &c("let x = 0;"), &[], &[("a.rs", 0x4001_3800, "w")]).len() == 1);
    ck("out-of-range literal ignored", judge(&modelled, &c("const S: u32 = 0xE000_ED90;"), &[], &[]).is_empty());
    if ok {
        lines.push("PASS".into());
        Verdict::Pass(lines)
    } else {
        Verdict::Fail(lines)
    }
}

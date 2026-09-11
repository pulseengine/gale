//! VER-DRV-GRAPH-001 — no raw `env` import survives in the composed graph.
//!
//! The rule, deliberately narrow: no core module of a committed composed/fused
//! wasm may import from module `env`. A WIT-lowered import carries a namespaced
//! module name (`gust:hal/mmio@0.1.0`); a raw `extern "C"` seam lowers to `env`.
//!
//! It does NOT classify by symbol shape. An earlier sketch keyed on the
//! underscore-vs-hyphen split (`poll_task` raw, `poll-task` WIT-lowered). A
//! hyphen does prove WIT-lowering -- it cannot occur in a C identifier -- but
//! its ABSENCE proves nothing: `read32`, `write32`, `poll` are WIT field names
//! with no hyphen. A one-way signal is not a rule, so this reads the import's
//! MODULE name, which is unambiguous.
//!
//! Ported from check-graph-env.py (gale#368). One correction: the Python
//! printed `SKIP <path> (not present)` for a listed artifact that was missing,
//! and the sweep still passed. The census could not catch it either -- it flags
//! committed files ABSENT from the list, not listed files absent from disk -- so
//! deleting a composed artifact silently retired its coverage while the gate
//! stayed green. A missing listed artifact is now a REFUSAL: the gate cannot
//! check what it claims to cover.

use crate::verdict::Verdict;
use std::path::Path;
use std::process::Command;

pub const NAME: &str = "graph-env";

/// Composed / fused artifacts, relative to the GUST BENCH ROOT (benches/gust) so
/// the sweep and the census cover the same tree. An earlier draft rooted the
/// sweep at drivers/ and reached ../wasm-kernel/* from outside the censused
/// area: those two were checked, but a NEW artifact beside them would not have
/// been noticed -- precisely how dma-own escaped the sibling gate (gale#316).
const COMPOSED: &[&str] = &[
    "drivers/measurements/wcet-fixture/gustos.loom.wasm",
    "drivers/measurements/wcet-fixture/gustos.loom.named.wasm",
    "drivers/os-node/repro-757/loom.wasm",
    "wasm-kernel/fused.wasm",
    "wasm-kernel/gust_kernel.wasm",
];

/// Committed .wasm that are NOT composed graphs, each with its reason. Ledgered
/// so the census cannot go vacuous. Empty today; the reason field is mandatory
/// so a future entry cannot be added as a bare path.
const NOT_COMPOSED: &[(&str, &str)] = &[];

/// Scan `(import "MODULE" "FIELD"` out of printed WAT.
fn parse_imports(wat: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = wat;
    while let Some(p) = rest.find("(import \"") {
        let after = &rest[p + "(import \"".len()..];
        let Some(q1) = after.find('"') else { break };
        let module = after[..q1].to_string();
        let tail = &after[q1 + 1..];
        let t = tail.trim_start();
        if !t.starts_with('"') {
            rest = tail;
            continue;
        }
        let f = &t[1..];
        let Some(q2) = f.find('"') else { break };
        out.push((module, f[..q2].to_string()));
        rest = &f[q2 + 1..];
    }
    out.sort();
    out.dedup();
    out
}

/// A WIT-lowered import names its interface: `ns:pkg/iface@version`.
fn wit_shaped(module: &str) -> bool {
    module.contains(':') && module.contains('/')
}

fn imports_of(wasm: &Path) -> Result<Vec<(String, String)>, String> {
    let out = Command::new("wasm-tools")
        .arg("print")
        .arg(wasm)
        .output()
        .map_err(|e| format!("wasm-tools not runnable: {e}"))?;
    if !out.status.success() {
        return Err(format!(
            "wasm-tools print failed on {}: {}",
            wasm.display(),
            String::from_utf8_lossy(&out.stderr).chars().take(400).collect::<String>()
        ));
    }
    Ok(parse_imports(&String::from_utf8_lossy(&out.stdout)))
}

fn git_wasm(bench: &Path) -> Result<Vec<String>, String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(bench)
        .args(["ls-files", "*.wasm"])
        .output()
        .map_err(|e| format!("git: {e}"))?;
    if !out.status.success() {
        return Err("git ls-files failed".into());
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect())
}

pub fn run(repo: &Path, _target: Option<&str>) -> Verdict {
    let bench = repo.join("benches/gust");
    if Command::new("wasm-tools").arg("--version").output().is_err() {
        return Verdict::Refused("wasm-tools not on PATH".into());
    }

    let mut lines = Vec::new();
    let mut noted = Vec::new();
    let mut failed = false;

    for rel in COMPOSED {
        let w = bench.join(rel);
        if !w.is_file() {
            // The correction. The Python SKIPped here and still passed.
            return Verdict::Refused(format!(
                "listed composed artifact is missing: {rel} — the gate cannot check what \
                 it claims to cover; remove it from COMPOSED deliberately or restore it"
            ));
        }
        let imps = match imports_of(&w) {
            Ok(i) => i,
            Err(e) => return Verdict::Refused(e),
        };
        let env: Vec<&(String, String)> = imps.iter().filter(|(m, _)| m == "env").collect();
        if env.is_empty() {
            let shown = if imps.is_empty() {
                "(none)".to_string()
            } else {
                imps.iter().map(|(m, f)| format!("{m} {f}")).collect::<Vec<_>>().join(", ")
            };
            lines.push(format!("ok   {rel}: 0 env, {} import(s): {shown}", imps.len()));
        } else {
            let names: Vec<String> = env
                .iter()
                .map(|(_, f)| if f.is_empty() { "<anon>".into() } else { f.clone() })
                .collect();
            lines.push(format!(
                "FAIL {rel}: {} raw env import(s): {}",
                env.len(),
                names.join(", ")
            ));
            failed = true;
        }
        for (m, f) in imps.iter().filter(|(m, _)| m != "env" && !wit_shaped(m)) {
            noted.push(format!("{rel}: ({m:?}, {f:?})"));
        }
    }

    if !noted.is_empty() {
        lines.push(
            "noted — imports that are neither `env` nor WIT-shaped. Outside this \
             requirement's literal rule, listed so nobody assumes they were approved:"
                .into(),
        );
        lines.extend(noted.iter().map(|n| format!("  {n}")));
    }

    // Census over COMMITTED files only -- git, not a filesystem glob. A glob
    // would sweep local build output in and fail on a clean checkout that
    // happens to have built, which is a gate that cries wolf.
    let found = match git_wasm(&bench) {
        Ok(f) => f,
        Err(e) => return Verdict::Refused(e),
    };
    let ledgered: Vec<&str> = NOT_COMPOSED.iter().map(|(p, _)| *p).collect();
    let stray: Vec<&String> = found
        .iter()
        .filter(|f| !COMPOSED.contains(&f.as_str()) && !ledgered.contains(&f.as_str()))
        .collect();
    if !stray.is_empty() {
        lines.push(format!(
            "FAIL: {} committed .wasm neither swept nor ledgered:",
            stray.len()
        ));
        lines.extend(stray.iter().map(|s| format!("  {s}")));
        lines.push(
            "Add it to COMPOSED if it is a composed graph, or to NOT_COMPOSED with a reason."
                .into(),
        );
        failed = true;
    }

    lines.push(format!("swept {} composed artifact(s)", COMPOSED.len()));
    if failed {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

/// The gate must be OBSERVED to fail on a raw env import, and OBSERVED not to
/// flag a WIT-typed one. Either half misbehaving means it cannot distinguish.
pub fn self_test(_repo: &Path, _target: Option<&str>) -> Verdict {
    if Command::new("wasm-tools").arg("--version").output().is_err() {
        return Verdict::Refused("wasm-tools not on PATH".into());
    }
    let dir = std::env::temp_dir().join(format!("gale-xtask-graphenv-{}", std::process::id()));
    if let Err(e) = std::fs::create_dir_all(&dir) {
        return Verdict::Refused(format!("scratch: {e}"));
    }
    let cases: [(&str, &str, bool); 2] = [
        (
            "raw env",
            r#"(module (import "env" "mmio_read32" (func (param i32) (result i32))))"#,
            true,
        ),
        (
            "WIT-typed",
            r#"(module (import "gust:hal/mmio@0.1.0" "read32" (func (param i32) (result i32))))"#,
            false,
        ),
    ];
    let mut lines = Vec::new();
    let mut broken = false;
    for (name, src, must_hit) in cases {
        let wat = dir.join(format!("{}.wat", name.replace(' ', "-")));
        let wasm = dir.join(format!("{}.wasm", name.replace(' ', "-")));
        if let Err(e) = std::fs::write(&wat, src) {
            let _ = std::fs::remove_dir_all(&dir);
            return Verdict::Refused(format!("write: {e}"));
        }
        let parsed = Command::new("wasm-tools")
            .arg("parse")
            .arg(&wat)
            .arg("-o")
            .arg(&wasm)
            .output();
        match parsed {
            Ok(o) if o.status.success() => {}
            Ok(o) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Verdict::Refused(format!(
                    "wasm-tools parse failed for {name}: {}",
                    String::from_utf8_lossy(&o.stderr).trim()
                ));
            }
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Verdict::Refused(format!("wasm-tools parse: {e}"));
            }
        }
        let hit = match imports_of(&wasm) {
            Ok(i) => i.iter().any(|(m, _)| m == "env"),
            Err(e) => {
                let _ = std::fs::remove_dir_all(&dir);
                return Verdict::Refused(e);
            }
        };
        if hit == must_hit {
            lines.push(format!("ok   {name} control behaves: env-detected={hit}"));
        } else if must_hit {
            lines.push(format!("MISSED: {name} control was not caught"));
            broken = true;
        } else {
            lines.push(format!("BROKEN: {name} control was flagged"));
            broken = true;
        }
    }
    // The correction's own control: a missing listed artifact must REFUSE.
    lines.push("missing-artifact path refuses (see run(): SKIP was the Python's hole)".into());
    let _ = std::fs::remove_dir_all(&dir);
    if broken {
        Verdict::Fail(lines)
    } else {
        Verdict::Pass(lines)
    }
}

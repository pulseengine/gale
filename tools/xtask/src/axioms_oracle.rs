//! `xtask axioms-oracle --tree DIR`: the axiom audit's oracle
//! (VER-HONESTY-AXIOMS-GATE-001, docs/design/axioms-trusted-audit.md).
//!
//! Applies each planted input and its control to a CLEAN git checkout `DIR`,
//! runs that checkout's own gates (`cargo run` of DIR's xtask), and holds every
//! exit code to the table below. GREEN only if every row holds. It restores
//! every file it touched (`git checkout --`) after each row, and refuses a
//! checkout with uncommitted changes to the files it plants into.
//!
//! On origin/main the axiom audit does not exist (exit 2), so the oracle is
//! RED; it also asserts that the gates main already has stay GREEN on the
//! classical, Section-Hypothesis and native_decide plants -- the gap this
//! lane closes. On the lane it is GREEN.
//!
//! Plants (expected exits on the lane):
//!   P1 classical  executor_proofs.v: `Require Import Stdlib.Logic.Classical` +
//!                 a lemma using `classic`, against a seeded ledger in which the
//!                 control (the same statement, proved by `tauto`) is green:
//!                 axioms-audit 1 (names Classical_Prop.classic); control 0.
//!   P3 Section    mutex_proofs.v: `Section ... Hypothesis bad : False ... End`:
//!                 trust-ledger 1 (rocq:Section-Variable); axioms-audit 0 -- a
//!                 discharged hypothesis is a premise, not an axiom (a stated
//!                 blind spot). Control (a Section with only Let): both 0.
//!   P4 native     Atomic.lean: a theorem `by native_decide` against a ledger
//!                 seeded with the same theorem `by decide`: axioms-audit 1
//!                 (measured: the kernel replay REJECTS the `_nativeDecide`
//!                 auxiliary -- replay has no compiled code for
//!                 Lean.reduceBool -- so this plant is not ledgered), trust-ledger 1;
//!                 control 0 / 0.
//!   P6 vacuous    proofs/BUILD.bazel audits no library: axioms-audit 3 (refused).
//!   C0 clean      the unmodified tree: axioms-audit 0, its self-test 0.
//!
//! What it cannot see: whether the plants are the right plants (they are the
//! plan's, section 3); runs of other commits. The CI job runs
//! the gate and its self-test on every PR (not required); it does not run
//! this oracle.

use std::path::Path;
use std::process::Command;

const ID: &str = "VER-HONESTY-AXIOMS-GATE-001";
const EXEC: &str = "proofs/executor_proofs.v";
const MUTEX: &str = "proofs/mutex_proofs.v";
const ATOMIC: &str = "proofs/lean/Atomic.lean";
const BUILD: &str = "proofs/BUILD.bazel";
const LEDGER: &str = "tools/xtask/axioms-audit.ledger";

const P1_CONTROL: &str = "\nLemma zz_plant_em (P : Prop) : ~ ~ (P \\/ ~ P).\nProof. tauto. Qed.\n";
const P1_PLANT: &str = "\nRequire Import Stdlib.Logic.Classical.\nLemma zz_plant_em (P : Prop) : ~ ~ (P \\/ ~ P).\nProof. intros H. apply H. apply classic. Qed.\n";
const P3_PLANT: &str = "\nSection ZzPlant.\nHypothesis zz_bad : False.\nLemma zz_plant_absurd : 1 = 2.\nProof. destruct zz_bad. Qed.\nEnd ZzPlant.\n";
const P3_CONTROL: &str = "\nSection ZzPlant.\nLet zz_k := 1.\nLemma zz_plant_absurd : zz_k = 1.\nProof. reflexivity. Qed.\nEnd ZzPlant.\n";
const P4_CONTROL: &str = "\ntheorem zz_plant_arith : 2 + 2 = 4 := by decide\n";
const P4_PLANT: &str = "\ntheorem zz_plant_arith : 2 + 2 = 4 := by native_decide\n";

struct Run {
    code: i32,
    out: String,
}

fn gate(tree: &Path, args: &[&str]) -> Run {
    let o = Command::new("cargo")
        .current_dir(tree)
        .env("CARGO_BUILD_JOBS", std::env::var("CARGO_BUILD_JOBS").unwrap_or_else(|_| "2".into()))
        .args(["run", "--quiet", "--locked", "--manifest-path", "tools/xtask/Cargo.toml", "--"])
        .args(args)
        .output();
    match o {
        Ok(o) => Run { code: o.status.code().unwrap_or(-1), out: format!("{}{}", String::from_utf8_lossy(&o.stdout), String::from_utf8_lossy(&o.stderr)) },
        Err(e) => Run { code: -1, out: format!("cannot run cargo: {e}") },
    }
}

fn append(tree: &Path, file: &str, text: &str) -> Result<(), String> {
    let p = tree.join(file);
    let old = std::fs::read_to_string(&p).map_err(|e| format!("{file}: {e}"))?;
    std::fs::write(&p, format!("{old}{text}")).map_err(|e| format!("{file}: {e}"))
}

fn replace(tree: &Path, file: &str, from: &str, to: &str) -> Result<(), String> {
    let p = tree.join(file);
    let old = std::fs::read_to_string(&p).map_err(|e| format!("{file}: {e}"))?;
    if !old.contains(from) {
        return Err(format!("{file}: the text to replace is not there"));
    }
    std::fs::write(&p, old.replacen(from, to, 1)).map_err(|e| format!("{file}: {e}"))
}

fn restore(tree: &Path) -> Result<(), String> {
    let o = Command::new("git").arg("-C").arg(tree).args(["checkout", "--", EXEC, MUTEX, ATOMIC, BUILD]).output().map_err(|e| e.to_string())?;
    if !o.status.success() {
        return Err(format!("git checkout failed: {}", String::from_utf8_lossy(&o.stderr)));
    }
    if tree.join(LEDGER).exists() {
        // The ledger is tracked on the lane; on main it does not exist.
        let _ = Command::new("git").arg("-C").arg(tree).args(["checkout", "--", LEDGER]).output();
    }
    Ok(())
}

pub fn main(args: &[String]) -> i32 {
    let tree = match args {
        [flag, dir] if flag == "--tree" => std::path::PathBuf::from(dir),
        _ => {
            eprintln!("usage: xtask axioms-oracle --tree <clean git checkout>");
            return crate::verdict::EXIT_USAGE;
        }
    };
    let st = Command::new("git").arg("-C").arg(&tree).args(["status", "--porcelain", "--", EXEC, MUTEX, ATOMIC, BUILD, LEDGER]).output();
    match st {
        Ok(o) if o.status.success() && o.stdout.is_empty() => {}
        _ => {
            eprintln!("axioms-oracle: {} is not a clean git checkout of the planted files -- refusing", tree.display());
            return crate::verdict::EXIT_REFUSED;
        }
    }
    let lane = gate(&tree, &["check", "axioms-audit", "--self-test"]).code != 2;
    println!("axioms-oracle on {} ({})", tree.display(), if lane { "the gate exists: lane expectations" } else { "no axioms-audit gate: main expectations" });
    let mut rows: Vec<(String, bool, String)> = Vec::new();
    let mut row = |what: &str, ok: bool, detail: String| {
        println!("  {} {what} {detail}", if ok { "ok  " } else { "MISS" });
        rows.push((what.to_string(), ok, detail));
    };
    let step = |name: &str, f: &mut dyn FnMut() -> Result<(), String>| {
        let r = f();
        let _ = restore(&tree);
        if let Err(e) = r {
            println!("  MISS {name}: could not apply: {e}");
            false
        } else {
            true
        }
    };
    let ex = |r: &Run| format!("(exit {})", r.code);

    // C0: the clean tree.
    let c0 = gate(&tree, &["check", "axioms-audit"]);
    let c0s = gate(&tree, &["check", "axioms-audit", "--self-test"]);
    let mut applied = true;
    if lane {
        row("C0 axioms-audit on the clean tree is 0", c0.code == 0, ex(&c0));
        row("C0 axioms-audit --self-test is 0", c0s.code == 0, ex(&c0s));
    } else {
        row("C0 axioms-audit exists", c0.code != 2, ex(&c0));
    }

    // P1: classical import.
    applied &= step("P1", &mut || {
        append(&tree, EXEC, P1_CONTROL)?;
        if lane {
            let w = gate(&tree, &["axioms-audit", "--write", "--id", ID]);
            let g = gate(&tree, &["check", "axioms-audit"]);
            row("P1 control (tauto) after seeding: axioms-audit 0", w.code == 0 && g.code == 0, format!("(write {}, gate {})", w.code, g.code));
        }
        replace(&tree, EXEC, P1_CONTROL, P1_PLANT)?;
        let g = gate(&tree, &["check", "axioms-audit"]);
        let t = gate(&tree, &["check", "trust-ledger"]);
        row("P1 plant (classic): axioms-audit 1 naming Classical_Prop.classic", g.code == 1 && g.out.contains("Classical_Prop.classic"), ex(&g));
        row("P1 plant: trust-ledger stays 0 (no spelling changes)", t.code == 0, ex(&t));
        Ok(())
    });

    // P3: Section Hypothesis False.
    applied &= step("P3", &mut || {
        append(&tree, MUTEX, P3_PLANT)?;
        let t = gate(&tree, &["check", "trust-ledger"]);
        if lane {
            let g = gate(&tree, &["check", "axioms-audit"]);
            row("P3 plant (Section Hypothesis False): trust-ledger 1 (rocq:Section-Variable)", t.code == 1 && t.out.contains("rocq:Section-Variable"), ex(&t));
            row("P3 plant: axioms-audit 0 (a discharged premise is not an axiom: stated blind spot)", g.code == 0, ex(&g));
        } else {
            row("P3 plant: trust-ledger catches it", t.code == 1, ex(&t));
        }
        replace(&tree, MUTEX, P3_PLANT, P3_CONTROL)?;
        let t = gate(&tree, &["check", "trust-ledger"]);
        row("P3 control (Section with only Let): trust-ledger 0", t.code == 0, ex(&t));
        if lane {
            let g = gate(&tree, &["check", "axioms-audit"]);
            row("P3 control: axioms-audit 0", g.code == 0, ex(&g));
        }
        Ok(())
    });

    // P4: native_decide.
    applied &= step("P4", &mut || {
        append(&tree, ATOMIC, P4_CONTROL)?;
        if lane {
            let w = gate(&tree, &["axioms-audit", "--write", "--id", ID]);
            let g = gate(&tree, &["check", "axioms-audit"]);
            let t = gate(&tree, &["check", "trust-ledger"]);
            row("P4 control (decide) after seeding: axioms-audit 0, trust-ledger 0", w.code == 0 && g.code == 0 && t.code == 0, format!("(write {}, gate {}, trust-ledger {})", w.code, g.code, t.code));
        }
        replace(&tree, ATOMIC, P4_CONTROL, P4_PLANT)?;
        let g = gate(&tree, &["check", "axioms-audit"]);
        let t = gate(&tree, &["check", "trust-ledger"]);
        row(
            "P4 plant (native_decide): axioms-audit 1, naming Lean.ofReduceBool or the rejected `_nativeDecide` auxiliary",
            g.code == 1 && (g.out.contains("Lean.ofReduceBool") || g.out.contains("_nativeDecide")),
            ex(&g),
        );
        if lane {
            row("P4 plant: trust-ledger 1 (lean:native_decide)", t.code == 1 && t.out.contains("lean:native_decide"), ex(&t));
        } else {
            row("P4 plant: trust-ledger catches it", t.code == 1, ex(&t));
        }
        Ok(())
    });

    // P6: a vacuous run (the Rocq target audits nothing).
    applied &= step("P6", &mut || {
        let build = std::fs::read_to_string(tree.join(BUILD)).map_err(|e| e.to_string())?;
        let Some(start) = build.find("rocq_axiom_audit(") else {
            let g = gate(&tree, &["check", "axioms-audit"]);
            row("P6 vacuous: axioms-audit refuses (3)", g.code == 3, format!("{} (no audit target in this tree)", ex(&g)));
            return Ok(());
        };
        let libs = build[start..].find("libs = [").map(|i| start + i).ok_or("no libs list")?;
        let end = build[libs..].find(']').map(|i| libs + i + 1).ok_or("no end of libs")?;
        std::fs::write(tree.join(BUILD), format!("{}libs = []{}", &build[..libs], &build[end..])).map_err(|e| e.to_string())?;
        let g = gate(&tree, &["check", "axioms-audit"]);
        row("P6 vacuous (no library audited): axioms-audit refuses (3)", g.code == 3, ex(&g));
        Ok(())
    });

    let missed: Vec<&String> = rows.iter().filter(|r| !r.1).map(|r| &r.0).collect();
    if !applied {
        println!("REFUSED: a plant could not be applied");
        return crate::verdict::EXIT_REFUSED;
    }
    if lane && missed.is_empty() {
        println!("GREEN: every plant and control holds ({} rows)", rows.len());
        crate::verdict::EXIT_PASS
    } else {
        println!("RED: {} of {} rows do not hold", missed.len(), rows.len());
        if !lane {
            let kept: Vec<&(String, bool, String)> = rows.iter().filter(|r| r.0.contains("trust-ledger")).collect();
            let existing_green = kept.iter().filter(|r| r.0.contains("plant")).all(|r| r.2.contains("exit 0"));
            println!("main: the existing trust-ledger stays green on the P1/P3/P4 plants: {}", if existing_green { "yes (the gap)" } else { "NO" });
        }
        crate::verdict::EXIT_FAIL
    }
}

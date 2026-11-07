//! gale's verdict-bearing gates, in one compiled language (gale#368).
//!
//! Conforms to the PulseEngine CLI baseline: `--version`/`-V` print
//! `xtask <semver>` and exit 0, `--help` exits 0, an unknown flag or command
//! exits 2 with usage on stderr, and structured output is `--format json`.

mod gates;
mod silicon;
mod verdict;

use std::path::PathBuf;
use std::process::{exit, Command};
use verdict::{Verdict, EXIT_PASS, EXIT_USAGE};

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn usage() -> String {
    let mut s = String::from(
        "xtask — gale's verdict-bearing gates\n\n\
         USAGE:\n  \
           cargo xtask check <gate> [--self-test] [--module PATH] [--format json]\n  \
           cargo xtask check            list the gates\n  \
           cargo xtask silicon ...      gust firmware on physical boards (see `xtask silicon`)\n  \
           cargo xtask trusted-base [--out PATH | --write-doc]\n  \
                                        the trusted-base inventory as JSON (gale#467)\n\n\
         EXIT CODES:\n  \
           0  the property holds\n  \
           1  the property does NOT hold\n  \
           2  usage error\n  \
           3  the gate could not run (missing tool or input) — NOT a pass\n\n\
         GATES:\n",
    );
    for g in gates::GATES {
        s.push_str(&format!("  {:<16} {}\n", g.name, g.about));
    }
    s
}

/// Repo root via git, so the gates work from any subdirectory.
fn repo_root() -> Option<PathBuf> {
    let out = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8(out.stdout).ok()?;
    Some(PathBuf::from(s.trim()))
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();

    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("xtask {VERSION}");
        exit(EXIT_PASS);
    }
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{}", usage());
        exit(EXIT_PASS);
    }

    if args[0] == "silicon" {
        let Some(root) = repo_root() else {
            eprintln!("xtask: not a git repository");
            exit(EXIT_USAGE);
        };
        exit(silicon::main(&root, &args[1..]));
    }
    if args[0] == "trusted-base" {
        let Some(root) = repo_root() else {
            eprintln!("xtask: not a git repository");
            exit(EXIT_USAGE);
        };
        exit(trusted_base_cmd(&root, &args[1..]));
    }
    if args[0] != "check" {
        eprintln!("xtask: unknown command '{}'\n\n{}", args[0], usage());
        exit(EXIT_USAGE);
    }

    let mut json = false;
    let mut self_test = false;
    let mut gate_name: Option<String> = None;
    let mut target: Option<String> = None;
    let mut rest = args[1..].iter();
    while let Some(a) = rest.next() {
        match a.as_str() {
            "--self-test" => self_test = true,
            "--module" => match rest.next() {
                Some(v) => target = Some(v.clone()),
                None => {
                    eprintln!("xtask: --module needs a path");
                    exit(EXIT_USAGE);
                }
            },
            "--format" => match rest.next().map(String::as_str) {
                Some("json") => json = true,
                Some(other) => {
                    eprintln!("xtask: unknown --format '{other}' (only 'json')");
                    exit(EXIT_USAGE);
                }
                None => {
                    eprintln!("xtask: --format needs a value");
                    exit(EXIT_USAGE);
                }
            },
            s if s.starts_with('-') => {
                eprintln!("xtask: unknown flag '{s}'\n\n{}", usage());
                exit(EXIT_USAGE);
            }
            s => gate_name = Some(s.to_string()),
        }
    }

    let Some(name) = gate_name else {
        print!("{}", usage());
        exit(EXIT_PASS);
    };
    let Some(gate) = gates::find(&name) else {
        eprintln!("xtask: no such gate '{name}'\n\n{}", usage());
        exit(EXIT_USAGE);
    };
    let Some(root) = repo_root() else {
        eprintln!("xtask: not a git repository");
        exit(EXIT_USAGE);
    };

    let v: Verdict = if self_test {
        (gate.self_test)(&root, target.as_deref())
    } else {
        (gate.run)(&root, target.as_deref())
    };

    if json {
        println!("{}", v.to_json(gate.name));
    } else {
        let what = if self_test { "self-test" } else { "gate" };
        println!("{} {}:", gate.name, what);
        println!("{v}");
    }
    exit(v.code());
}

/// `xtask trusted-base`: print (or `--out PATH`) the inventory JSON, or
/// `--write-doc` to re-render the honesty doc's block. Exit 3 when the tree
/// cannot be measured -- never an empty document.
fn trusted_base_cmd(root: &std::path::Path, args: &[String]) -> i32 {
    use gates::trusted_base as tb;
    let mut out: Option<String> = None;
    let mut doc = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--write-doc" => doc = true,
            "--out" => match it.next() {
                Some(p) => out = Some(p.clone()),
                None => {
                    eprintln!("xtask: --out needs a path");
                    return EXIT_USAGE;
                }
            },
            other => {
                eprintln!("xtask: unknown argument '{other}' to trusted-base\n\n{}", usage());
                return EXIT_USAGE;
            }
        }
    }
    if doc && out.is_some() {
        eprintln!("xtask: --write-doc and --out are separate actions");
        return EXIT_USAGE;
    }
    if doc {
        return match tb::write_doc(root) {
            Ok(m) => {
                println!("{m}");
                EXIT_PASS
            }
            Err(e) => {
                eprintln!("xtask: {e}");
                verdict::EXIT_REFUSED
            }
        };
    }
    let inv = match tb::inventory(root) {
        Ok(i) => i,
        Err(e) => {
            eprintln!("xtask: {e}");
            return verdict::EXIT_REFUSED;
        }
    };
    let commit = match tb::commit(root) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("xtask: {e}");
            return verdict::EXIT_REFUSED;
        }
    };
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let json = tb::to_json(&inv, &commit, &tb::utc(now));
    match out {
        Some(p) => match std::fs::write(&p, &json) {
            Ok(()) => {
                println!("wrote {p}");
                EXIT_PASS
            }
            Err(e) => {
                eprintln!("xtask: cannot write {p}: {e}");
                verdict::EXIT_REFUSED
            }
        },
        None => {
            print!("{json}");
            EXIT_PASS
        }
    }
}

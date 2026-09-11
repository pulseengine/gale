//! gale's verdict-bearing gates, in one compiled language (gale#368).
//!
//! Conforms to the PulseEngine CLI baseline: `--version`/`-V` print
//! `xtask <semver>` and exit 0, `--help` exits 0, an unknown flag or command
//! exits 2 with usage on stderr, and structured output is `--format json`.

mod gates;
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
           cargo xtask check            list the gates\n\n\
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

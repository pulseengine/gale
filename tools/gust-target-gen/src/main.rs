//! gust-target-gen — generate per-target constants, linker memory maps, and
//! gust:hal WIT worlds from a spar/AADL hardware model.
//!
//!   gust-target-gen --items <spar-items.json> --board <Pkg::Type.impl> --out <dir> [--reference <tsv> [--board-reference <tsv>]]
//!
//! Reads `spar items --format json` output, resolves the named board system
//! implementation, and writes three DO-NOT-EDIT artifacts into <dir>:
//!   gust_target_<stem>.rs   memory-<stem>.x   world-<stem>.wit
//! where <stem> is the board's package name lowercased (e.g. stm32f100).
//! Build-time only — nothing here runs on the target.
//!
//! User-facing failures print `error: ...` on stderr and exit with a code from
//! `USAGE` below; a panic (exit 101) means a bug in the tool.

mod emit_ld;
mod emit_rs;
mod emit_wit;
mod model;
mod reference;

use std::path::PathBuf;
use std::process::exit;

/// The input content was rejected: malformed JSON/TSV, a board or classifier
/// the items do not contain, a model missing a required property or device,
/// or a disagreement with `--reference`.
const EXIT_INPUT: i32 = 1;
/// Bad command line: unknown flag, missing flag or value, stray argument.
const EXIT_USAGE: i32 = 2;
/// A file could not be read, or the output could not be created or written.
const EXIT_IO: i32 = 3;

const USAGE: &str = "\
Generate per-target Rust consts, a linker memory map and a WIT world from spar AADL items.

Usage: gust-target-gen --items <spar-items.json> --board <Pkg::Type.impl> --out <dir>
                       [--reference <peripherals.tsv> [--board-reference <board.tsv>]]

Options:
  --items <file>            `spar items --format json` output
  --board <Pkg::Type.impl>  board system implementation to resolve
  --out <dir>               directory to write the generated files into
  --reference <tsv>         cross-check SoC device bases before writing
  --board-reference <tsv>   also cross-check board pins (needs --reference)
  -h, --help                print this help and exit
  -V, --version             print the version and exit

Exit codes:
  0  success
  1  input rejected (malformed items/TSV, unknown board, invalid model, reference mismatch)
  2  usage error (unknown flag, missing flag or value)
  3  I/O error (cannot read an input, cannot create or write the output)
";

/// Print `error: <msg>` and exit with `code`; usage errors also print usage.
fn fail(code: i32, msg: &str) -> ! {
    eprintln!("error: {msg}");
    if code == EXIT_USAGE {
        eprintln!();
        eprint!("{USAGE}");
    }
    exit(code)
}

#[derive(Default)]
struct Args {
    items: Option<String>,
    board: Option<String>,
    out: Option<String>,
    reference: Option<String>,
    board_reference: Option<String>,
}

fn parse_args() -> Args {
    let mut a = Args::default();
    let mut it = std::env::args().skip(1);
    while let Some(flag) = it.next() {
        let slot = match flag.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                exit(0);
            }
            "-V" | "--version" => {
                println!("gust-target-gen {}", env!("CARGO_PKG_VERSION"));
                exit(0);
            }
            "--items" => &mut a.items,
            "--board" => &mut a.board,
            "--out" => &mut a.out,
            "--reference" => &mut a.reference,
            "--board-reference" => &mut a.board_reference,
            other if other.starts_with('-') => fail(EXIT_USAGE, &format!("unknown flag `{other}`")),
            other => fail(EXIT_USAGE, &format!("unexpected argument `{other}`")),
        };
        let Some(value) = it.next() else {
            fail(EXIT_USAGE, &format!("`{flag}` needs a value"));
        };
        if slot.replace(value).is_some() {
            fail(EXIT_USAGE, &format!("`{flag}` given more than once"));
        }
    }
    a
}

fn read(flag: &str, path: &str) -> String {
    std::fs::read_to_string(path)
        .unwrap_or_else(|e| fail(EXIT_IO, &format!("cannot read {flag} `{path}`: {e}")))
}

fn main() {
    let args = parse_args();
    let required = |v: Option<String>, flag: &str, what: &str| {
        v.unwrap_or_else(|| fail(EXIT_USAGE, &format!("{flag} {what} is required")))
    };
    let items = required(args.items, "--items", "<spar-items.json>");
    let board = required(args.board, "--board", "<Pkg::Type.impl>");
    let out = required(args.out, "--out", "<dir>");
    if args.board_reference.is_some() && args.reference.is_none() {
        fail(EXIT_USAGE, "--board-reference needs --reference");
    }

    let json = read("--items", &items);
    let target = model::parse_items(&json, &board)
        .unwrap_or_else(|e| fail(EXIT_INPUT, &format!("--items `{items}`: {e}")));
    let stem = board.split("::").next().unwrap_or(&board).to_lowercase();

    // Cross-check against the pinned vendor-derived reference before writing anything:
    // a model that disagrees with it must not regenerate quietly.
    if let Some(r) = args.reference {
        let tsv = read("--reference", &r);
        let reference = reference::parse(&tsv)
            .unwrap_or_else(|e| fail(EXIT_INPUT, &format!("--reference `{r}`: {e}")));
        let mut bad = reference::check(&target, &reference);
        if let Some(b) = args.board_reference {
            let tsv = read("--board-reference", &b);
            bad.extend(reference::check_board(&target, &tsv, &reference));
        }
        if !bad.is_empty() {
            for b in &bad {
                eprintln!("gust-target-gen: REFERENCE MISMATCH {b}");
            }
            fail(EXIT_INPUT, &format!("{} reference mismatch(es) against `{r}`", bad.len()));
        }
        eprintln!("gust-target-gen: {stem}: every SoC device (and board pin, when given) agrees with {r}");
    }

    // Emit everything before writing anything, so a rejected model leaves no partial output.
    let model_err = |e: String| fail(EXIT_INPUT, &format!("board `{board}`: {e}"));
    let files = [
        (format!("gust_target_{stem}.rs"), emit_rs::emit_rs(&target).unwrap_or_else(model_err)),
        (format!("memory-{stem}.x"), emit_ld::emit_ld(&target)),
        (format!("world-{stem}.wit"), emit_wit::emit_wit(&target).unwrap_or_else(model_err)),
    ];

    let out = PathBuf::from(&out);
    if let Err(e) = std::fs::create_dir_all(&out) {
        fail(EXIT_IO, &format!("cannot create --out dir `{}`: {e}", out.display()));
    }
    for (name, contents) in files {
        let p = out.join(&name);
        if let Err(e) = std::fs::write(&p, contents) {
            fail(EXIT_IO, &format!("cannot write `{}`: {e}", p.display()));
        }
        eprintln!("gust-target-gen: wrote {}", p.display());
    }
}
